use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::{fmt, ptr};

use ahash::RandomState;
use crossfont::Metrics;
use glutin::context::{ContextApi, GlContext, PossiblyCurrentContext};
use glutin::display::{GetGlDisplay, GlDisplay};
use log::{LevelFilter, debug, info};
use unicode_width::UnicodeWidthChar;

use alacritty_terminal::graphics::{GraphicsState, PlaceholderTile};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Point;
use alacritty_terminal::term::cell::Flags;

use crate::config::debug::RendererPreference;
use crate::display::SizeInfo;
use crate::display::color::Rgb;
use crate::display::content::RenderableCell;
use crate::display::cursor::CursorTrail;
use crate::gl;
use crate::renderer::rects::{RectRenderer, RenderRect};
use crate::renderer::shader::{ShaderError, ShaderProgram};

pub mod platform;
pub mod rects;
mod shader;
mod text;

pub use text::{GlyphCache, LoaderApi};

use shader::ShaderVersion;
use text::{Gles2Renderer, Glsl3Renderer, TextRenderer};

/// Whether the OpenGL functions have been loaded.
pub static GL_FUNS_LOADED: AtomicBool = AtomicBool::new(false);

#[derive(Debug)]
pub enum Error {
    /// Shader error.
    Shader(ShaderError),

    /// Other error.
    Other(String),
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Shader(err) => err.source(),
            Error::Other(_) => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Shader(err) => {
                write!(f, "There was an error initializing the shaders: {err}")
            },
            Error::Other(err) => {
                write!(f, "{err}")
            },
        }
    }
}

impl From<ShaderError> for Error {
    fn from(val: ShaderError) -> Self {
        Error::Shader(val)
    }
}

impl From<String> for Error {
    fn from(val: String) -> Self {
        Error::Other(val)
    }
}

#[derive(Debug)]
enum TextRendererProvider {
    Gles2(Gles2Renderer),
    Glsl3(Glsl3Renderer),
}

#[derive(Debug)]
pub struct Renderer {
    text_renderer: TextRendererProvider,
    rect_renderer: RectRenderer,
    trail_renderer: TrailRenderer,
    image_renderer: ImageRenderer,
    robustness: bool,
}

/// Wrapper around gl::GetString with error checking and reporting.
fn gl_get_string(
    string_id: gl::types::GLenum,
    description: &str,
) -> Result<Cow<'static, str>, Error> {
    unsafe {
        let string_ptr = gl::GetString(string_id);
        match gl::GetError() {
            gl::NO_ERROR if !string_ptr.is_null() => {
                Ok(CStr::from_ptr(string_ptr as *const _).to_string_lossy())
            },
            gl::INVALID_ENUM => {
                Err(format!("OpenGL error requesting {description}: invalid enum").into())
            },
            error_id => Err(format!("OpenGL error {error_id} requesting {description}").into()),
        }
    }
}

impl Renderer {
    /// Create a new renderer.
    ///
    /// This will automatically pick between the GLES2 and GLSL3 renderer based on the GPU's
    /// supported OpenGL version.
    pub fn new(
        context: &PossiblyCurrentContext,
        renderer_preference: Option<RendererPreference>,
    ) -> Result<Self, Error> {
        // We need to load OpenGL functions once per instance, but only after we make our context
        // current due to WGL limitations.
        if !GL_FUNS_LOADED.swap(true, Ordering::Relaxed) {
            let gl_display = context.display();
            gl::load_with(|symbol| {
                let symbol = CString::new(symbol).unwrap();
                gl_display.get_proc_address(symbol.as_c_str()).cast()
            });
        }

        let shader_version = gl_get_string(gl::SHADING_LANGUAGE_VERSION, "shader version")?;
        let gl_version = gl_get_string(gl::VERSION, "OpenGL version")?;
        let renderer = gl_get_string(gl::RENDERER, "renderer version")?;

        info!("Running on {renderer}");
        info!("OpenGL version {gl_version}, shader_version {shader_version}");

        // Check if robustness is supported.
        let robustness = Self::supports_robustness();

        let is_gles_context = matches!(context.context_api(), ContextApi::Gles(_));

        // Use the config option to enforce a particular renderer configuration.
        let (use_glsl3, allow_dsb) = match renderer_preference {
            Some(RendererPreference::Glsl3) => (true, true),
            Some(RendererPreference::Gles2) => (false, true),
            Some(RendererPreference::Gles2Pure) => (false, false),
            None => (shader_version.as_ref() >= "3.3" && !is_gles_context, true),
        };

        let (text_renderer, rect_renderer, trail_renderer, image_renderer) = if use_glsl3 {
            let text_renderer = TextRendererProvider::Glsl3(Glsl3Renderer::new()?);
            let rect_renderer = RectRenderer::new(ShaderVersion::Glsl3)?;
            let trail_renderer = TrailRenderer::new(ShaderVersion::Glsl3)?;
            let image_renderer = ImageRenderer::new(ShaderVersion::Glsl3)?;
            (text_renderer, rect_renderer, trail_renderer, image_renderer)
        } else {
            let text_renderer =
                TextRendererProvider::Gles2(Gles2Renderer::new(allow_dsb, is_gles_context)?);
            let rect_renderer = RectRenderer::new(ShaderVersion::Gles2)?;
            let trail_renderer = TrailRenderer::new(ShaderVersion::Gles2)?;
            let image_renderer = ImageRenderer::new(ShaderVersion::Gles2)?;
            (text_renderer, rect_renderer, trail_renderer, image_renderer)
        };

        // Enable debug logging for OpenGL as well.
        if log::max_level() >= LevelFilter::Debug && GlExtensions::contains("GL_KHR_debug") {
            debug!("Enabled debug logging for OpenGL");
            unsafe {
                gl::Enable(gl::DEBUG_OUTPUT);
                gl::Enable(gl::DEBUG_OUTPUT_SYNCHRONOUS);
                gl::DebugMessageCallback(Some(gl_debug_log), ptr::null_mut());
            }
        }

        Ok(Self { text_renderer, rect_renderer, trail_renderer, image_renderer, robustness })
    }

    #[inline]
    fn restore_text_blend_mode(&self) {
        unsafe {
            match &self.text_renderer {
                TextRendererProvider::Glsl3(_) => {
                    gl::BlendFunc(gl::SRC1_COLOR, gl::ONE_MINUS_SRC1_COLOR);
                },
                TextRendererProvider::Gles2(renderer) => {
                    if renderer.dual_source_blending() {
                        gl::BlendFunc(gl::SRC1_COLOR, gl::ONE_MINUS_SRC1_COLOR);
                    } else {
                        gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
                    }
                },
            }
        }
    }

    pub fn draw_images(
        &mut self,
        size_info: &SizeInfo,
        graphics: &GraphicsState,
        display_offset: usize,
        tiles: &[PlaceholderTile],
        z_pass: ImageZPass,
    ) {
        if graphics.placements().is_empty() && tiles.is_empty() && graphics.images().is_empty() {
            return;
        }

        unsafe {
            gl::Viewport(0, 0, size_info.width() as i32, size_info.height() as i32);
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
        }

        self.image_renderer.draw(size_info, graphics, display_offset, tiles, z_pass);

        self.restore_text_blend_mode();
        self.set_viewport(size_info);

        match &mut self.text_renderer {
            TextRendererProvider::Gles2(renderer) => renderer.reset_active_tex(),
            TextRendererProvider::Glsl3(renderer) => renderer.reset_active_tex(),
        }
    }

    pub fn draw_cells<I: Iterator<Item = RenderableCell>>(
        &mut self,
        size_info: &SizeInfo,
        glyph_cache: &mut GlyphCache,
        cells: I,
    ) {
        match &mut self.text_renderer {
            TextRendererProvider::Gles2(renderer) => {
                renderer.draw_cells(size_info, glyph_cache, cells)
            },
            TextRendererProvider::Glsl3(renderer) => {
                renderer.draw_cells(size_info, glyph_cache, cells)
            },
        }
    }

    /// Draw a string in a variable location. Used for printing the render timer, warnings and
    /// errors.
    pub fn draw_string(
        &mut self,
        point: Point<usize>,
        fg: Rgb,
        bg: Rgb,
        string_chars: impl Iterator<Item = char>,
        size_info: &SizeInfo,
        glyph_cache: &mut GlyphCache,
    ) {
        let mut wide_char_spacer = false;
        let cells = string_chars.enumerate().filter_map(|(i, character)| {
            let flags = if wide_char_spacer {
                wide_char_spacer = false;
                return None;
            } else if character.width() == Some(2) {
                // The spacer is always following the wide char.
                wide_char_spacer = true;
                Flags::WIDE_CHAR
            } else {
                Flags::empty()
            };

            Some(RenderableCell {
                point: Point::new(point.line, point.column + i),
                character,
                extra: None,
                flags,
                bg_alpha: 1.0,
                fg,
                bg,
                underline: fg,
            })
        });

        self.draw_cells(size_info, glyph_cache, cells);
    }

    pub fn with_loader<F, T>(&mut self, func: F) -> T
    where
        F: FnOnce(LoaderApi<'_>) -> T,
    {
        match &mut self.text_renderer {
            TextRendererProvider::Gles2(renderer) => renderer.with_loader(func),
            TextRendererProvider::Glsl3(renderer) => renderer.with_loader(func),
        }
    }

    /// Draw all rectangles simultaneously to prevent excessive program swaps.
    pub fn draw_rects(&mut self, size_info: &SizeInfo, metrics: &Metrics, rects: Vec<RenderRect>) {
        if rects.is_empty() {
            return;
        }

        // Prepare rect rendering state.
        unsafe {
            // Remove padding from viewport.
            gl::Viewport(0, 0, size_info.width() as i32, size_info.height() as i32);
            gl::BlendFuncSeparate(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA, gl::SRC_ALPHA, gl::ONE);
        }

        self.rect_renderer.draw(size_info, metrics, rects);

        // Activate regular state again.
        unsafe {
            // Reset blending strategy.
            gl::BlendFunc(gl::SRC1_COLOR, gl::ONE_MINUS_SRC1_COLOR);

            // Restore viewport with padding.
            self.set_viewport(size_info);
        }
    }

    /// Draw Kitty's cursor trail as a single GPU-warped quad.
    pub fn draw_cursor_trail(&self, size_info: &SizeInfo, trail: CursorTrail) {
        unsafe {
            gl::Viewport(0, 0, size_info.width() as i32, size_info.height() as i32);
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::ONE, gl::ONE_MINUS_SRC_ALPHA);
        }
        self.trail_renderer.draw(trail);
        self.restore_text_blend_mode();
        self.set_viewport(size_info);
    }

    /// Fill the window with `color` and `alpha`.
    pub fn clear(&self, color: Rgb, alpha: f32) {
        unsafe {
            gl::ClearColor(
                (f32::from(color.r) / 255.0).min(1.0) * alpha,
                (f32::from(color.g) / 255.0).min(1.0) * alpha,
                (f32::from(color.b) / 255.0).min(1.0) * alpha,
                alpha,
            );
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
    }

    /// Get the context reset status.
    pub fn was_context_reset(&self) -> bool {
        // If robustness is not supported, don't use its functions.
        if !self.robustness {
            return false;
        }

        let status = unsafe { gl::GetGraphicsResetStatus() };
        if status == gl::NO_ERROR {
            false
        } else {
            let reason = match status {
                gl::GUILTY_CONTEXT_RESET_KHR => "guilty",
                gl::INNOCENT_CONTEXT_RESET_KHR => "innocent",
                gl::UNKNOWN_CONTEXT_RESET_KHR => "unknown",
                _ => "invalid",
            };

            info!("GPU reset ({reason})");

            true
        }
    }

    fn supports_robustness() -> bool {
        let mut notification_strategy = 0;
        if GlExtensions::contains("GL_KHR_robustness") {
            unsafe {
                gl::GetIntegerv(gl::RESET_NOTIFICATION_STRATEGY_KHR, &mut notification_strategy);
            }
        } else {
            notification_strategy = gl::NO_RESET_NOTIFICATION_KHR as gl::types::GLint;
        }

        if notification_strategy == gl::LOSE_CONTEXT_ON_RESET_KHR as gl::types::GLint {
            info!("GPU reset notifications are enabled");
            true
        } else {
            info!("GPU reset notifications are disabled");
            false
        }
    }

    pub fn finish(&self) {
        unsafe {
            gl::Finish();
        }
    }

    /// Set the viewport for cell rendering.
    #[inline]
    pub fn set_viewport(&self, size: &SizeInfo) {
        unsafe {
            gl::Viewport(
                size.padding_x() as i32,
                size.padding_y() as i32,
                size.width() as i32 - 2 * size.padding_x() as i32,
                size.height() as i32 - 2 * size.padding_y() as i32,
            );
        }
    }

    /// Resize the renderer.
    pub fn resize(&self, size_info: &SizeInfo) {
        self.set_viewport(size_info);
        match &self.text_renderer {
            TextRendererProvider::Gles2(renderer) => renderer.resize(size_info),
            TextRendererProvider::Glsl3(renderer) => renderer.resize(size_info),
        }
    }
}

const TRAIL_SHADER_V: &str = include_str!("../../res/trail.v.glsl");
const TRAIL_SHADER_F: &str = include_str!("../../res/trail.f.glsl");

#[derive(Debug)]
struct TrailRenderer {
    program: ShaderProgram,
    vao: gl::types::GLuint,
    vbo: gl::types::GLuint,
    cursor_edges: gl::types::GLint,
    trail_color: gl::types::GLint,
    trail_opacity: gl::types::GLint,
}

impl TrailRenderer {
    fn new(version: ShaderVersion) -> Result<Self, ShaderError> {
        let program = ShaderProgram::new(version, None, TRAIL_SHADER_V, TRAIL_SHADER_F)?;
        let cursor_edges = program.get_uniform_location(c"cursorEdges")?;
        let trail_color = program.get_uniform_location(c"trailColor")?;
        let trail_opacity = program.get_uniform_location(c"trailOpacity")?;
        let (mut vao, mut vbo) = (0, 0);
        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::BindVertexArray(vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (4 * 2 * std::mem::size_of::<f32>()) as isize,
                std::ptr::null(),
                gl::DYNAMIC_DRAW,
            );
            gl::VertexAttribPointer(0, 2, gl::FLOAT, gl::FALSE, 0, std::ptr::null());
            gl::EnableVertexAttribArray(0);
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::BindVertexArray(0);
        }
        Ok(Self { program, vao, vbo, cursor_edges, trail_color, trail_opacity })
    }

    fn draw(&self, trail: CursorTrail) {
        let (r, g, b) = trail.color.as_tuple();
        unsafe {
            while gl::GetError() != gl::NO_ERROR {}
            gl::UseProgram(self.program.id());

            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);
            gl::BufferSubData(
                gl::ARRAY_BUFFER,
                0,
                (4 * 2 * std::mem::size_of::<f32>()) as isize,
                trail.corners.as_ptr().cast(),
            );
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);

            gl::Uniform4fv(self.cursor_edges, 1, trail.cursor_edges.as_ptr());
            gl::Uniform3f(
                self.trail_color,
                f32::from(r) / 255.,
                f32::from(g) / 255.,
                f32::from(b) / 255.,
            );
            gl::Uniform1f(self.trail_opacity, trail.opacity);

            gl::BindVertexArray(self.vao);
            gl::DrawArrays(gl::TRIANGLE_FAN, 0, 4);
            gl::BindVertexArray(0);
            gl::UseProgram(0);
        }
    }
}

impl Drop for TrailRenderer {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteBuffers(1, &self.vbo);
            gl::DeleteVertexArrays(1, &self.vao);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageZPass {
    UnderText,
    OverText,
}

#[inline]
fn append_quad(
    buf: &mut Vec<f32>,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    u0: f32,
    v0: f32,
    u1: f32,
    v1: f32,
) {
    buf.extend_from_slice(&[
        left, top, u0, v0,
        left, bottom, u0, v1,
        right, bottom, u1, v1,
        left, top, u0, v0,
        right, bottom, u1, v1,
        right, top, u1, v0,
    ]);
}

const IMAGE_SHADER_V: &str = include_str!("../../res/image.v.glsl");
const IMAGE_SHADER_F: &str = include_str!("../../res/image.f.glsl");

#[derive(Debug)]
struct ImageRenderer {
    program: ShaderProgram,
    vao: gl::types::GLuint,
    vbo: gl::types::GLuint,
    textures: HashMap<u32, (gl::types::GLuint, u64)>,
    batch_buffers: HashMap<gl::types::GLuint, Vec<f32>>,
}

impl ImageRenderer {
    fn new(version: ShaderVersion) -> Result<Self, ShaderError> {
        let program = ShaderProgram::new(version, None, IMAGE_SHADER_V, IMAGE_SHADER_F)?;
        let (mut vao, mut vbo) = (0, 0);
        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::BindVertexArray(vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::VertexAttribPointer(
                0,
                2,
                gl::FLOAT,
                gl::FALSE,
                (4 * std::mem::size_of::<f32>()) as i32,
                std::ptr::null(),
            );
            gl::EnableVertexAttribArray(0);

            gl::VertexAttribPointer(
                1,
                2,
                gl::FLOAT,
                gl::FALSE,
                (4 * std::mem::size_of::<f32>()) as i32,
                (2 * std::mem::size_of::<f32>()) as *const _,
            );
            gl::EnableVertexAttribArray(1);

            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::BindVertexArray(0);
        }

        Ok(Self {
            program,
            vao,
            vbo,
            textures: HashMap::new(),
            batch_buffers: HashMap::new(),
        })
    }

    fn draw(
        &mut self,
        size_info: &SizeInfo,
        graphics: &GraphicsState,
        display_offset: usize,
        tiles: &[PlaceholderTile],
        z_pass: ImageZPass,
    ) {
        if graphics.placements().is_empty() && tiles.is_empty() && graphics.images().is_empty() {
            return;
        }

        unsafe {
            // 1. Remove deleted textures
            self.textures.retain(|id, (tex, _)| {
                if !graphics.images().contains_key(id) {
                    gl::DeleteTextures(1, tex);
                    false
                } else {
                    true
                }
            });

            // 2. Identify visible images in this pass and upload only their textures (Lazy VRAM Upload)
            let mut needed_images = HashSet::new();

            // Direct placements matching z_pass
            for placement in graphics.placements() {
                let matches_z = match z_pass {
                    ImageZPass::UnderText => placement.z_index < 0,
                    ImageZPass::OverText => placement.z_index >= 0,
                };
                if !matches_z {
                    continue;
                }

                let viewport_line = placement.line.0 + display_offset as i32;
                if viewport_line + (placement.lines as i32) > 0
                    && (viewport_line as usize) < size_info.screen_lines()
                {
                    needed_images.insert(placement.image_id);
                }
            }

            // Placeholder tiles only render on OverText pass
            if z_pass == ImageZPass::OverText {
                for tile in tiles {
                    needed_images.insert(tile.image_id);
                }
            }

            if needed_images.is_empty() {
                return;
            }

            for id in &needed_images {
                let Some(img) = graphics.images().get(id) else { continue };
                match self.textures.get_mut(id) {
                    Some((tex, version)) => {
                        if *version != img.version {
                            gl::BindTexture(gl::TEXTURE_2D, *tex);
                            gl::TexImage2D(
                                gl::TEXTURE_2D,
                                0,
                                gl::RGBA as i32,
                                img.width as i32,
                                img.height as i32,
                                0,
                                gl::RGBA,
                                gl::UNSIGNED_BYTE,
                                img.pixels.as_ptr().cast(),
                            );
                            gl::BindTexture(gl::TEXTURE_2D, 0);
                            *version = img.version;
                        }
                    },
                    None => {
                        let mut tex = 0;
                        gl::GenTextures(1, &mut tex);
                        gl::BindTexture(gl::TEXTURE_2D, tex);
                        gl::TexParameteri(
                            gl::TEXTURE_2D,
                            gl::TEXTURE_MIN_FILTER,
                            gl::LINEAR as i32,
                        );
                        gl::TexParameteri(
                            gl::TEXTURE_2D,
                            gl::TEXTURE_MAG_FILTER,
                            gl::LINEAR as i32,
                        );
                        gl::TexParameteri(
                            gl::TEXTURE_2D,
                            gl::TEXTURE_WRAP_S,
                            gl::CLAMP_TO_EDGE as i32,
                        );
                        gl::TexParameteri(
                            gl::TEXTURE_2D,
                            gl::TEXTURE_WRAP_T,
                            gl::CLAMP_TO_EDGE as i32,
                        );
                        gl::TexImage2D(
                            gl::TEXTURE_2D,
                            0,
                            gl::RGBA as i32,
                            img.width as i32,
                            img.height as i32,
                            0,
                            gl::RGBA,
                            gl::UNSIGNED_BYTE,
                            img.pixels.as_ptr().cast(),
                        );
                        gl::BindTexture(gl::TEXTURE_2D, 0);
                        self.textures.insert(*id, (tex, img.version));
                    },
                }
            }

            // 3. Clear batch buffers
            for buf in self.batch_buffers.values_mut() {
                buf.clear();
            }

            // 4. Batch direct placements
            for placement in graphics.placements() {
                let matches_z = match z_pass {
                    ImageZPass::UnderText => placement.z_index < 0,
                    ImageZPass::OverText => placement.z_index >= 0,
                };
                if !matches_z {
                    continue;
                }

                let Some(&(tex, _)) = self.textures.get(&placement.image_id) else { continue };
                let Some(img) = graphics.images().get(&placement.image_id) else { continue };

                let viewport_line = placement.line.0 + display_offset as i32;
                if viewport_line + (placement.lines as i32) <= 0
                    || viewport_line as usize >= size_info.screen_lines()
                {
                    continue;
                }

                let x = size_info.padding_x() + placement.column.0 as f32 * size_info.cell_width();
                let y = size_info.padding_y() + viewport_line as f32 * size_info.cell_height();
                let w = placement.columns as f32 * size_info.cell_width();
                let h = placement.lines as f32 * size_info.cell_height();

                let left_ndc = (x / size_info.width()) * 2.0 - 1.0;
                let right_ndc = ((x + w) / size_info.width()) * 2.0 - 1.0;
                let top_ndc = 1.0 - (y / size_info.height()) * 2.0;
                let bottom_ndc = 1.0 - ((y + h) / size_info.height()) * 2.0;

                let src_w = if placement.src_w > 0 {
                    placement.src_w
                } else {
                    img.width.saturating_sub(placement.src_x).max(1)
                };
                let src_h = if placement.src_h > 0 {
                    placement.src_h
                } else {
                    img.height.saturating_sub(placement.src_y).max(1)
                };

                let u0 = placement.src_x as f32 / img.width.max(1) as f32;
                let v0 = placement.src_y as f32 / img.height.max(1) as f32;
                let u1 = (placement.src_x + src_w) as f32 / img.width.max(1) as f32;
                let v1 = (placement.src_y + src_h) as f32 / img.height.max(1) as f32;

                let buf = self.batch_buffers.entry(tex).or_default();
                append_quad(buf, left_ndc, top_ndc, right_ndc, bottom_ndc, u0, v0, u1, v1);
            }

            // 5. Batch Unicode placeholder tiles (U=1) (only on OverText pass)
            if z_pass == ImageZPass::OverText {
                for tile in tiles {
                    let Some(&(tex, _)) = self.textures.get(&tile.image_id) else { continue };
                    let vp = graphics.virtual_placements().get(&tile.image_id);
                    let total_cols = vp.map_or(1, |v| v.columns.max(1));
                    let total_lines = vp.map_or(1, |v| v.lines.max(1));

                    let u0 = (tile.col as f32) / (total_cols as f32);
                    let u1 = ((tile.col + 1) as f32) / (total_cols as f32);
                    let v0 = (tile.row as f32) / (total_lines as f32);
                    let v1 = ((tile.row + 1) as f32) / (total_lines as f32);

                    let x = size_info.padding_x()
                        + tile.point.column.0 as f32 * size_info.cell_width();
                    let y = size_info.padding_y()
                        + tile.point.line as f32 * size_info.cell_height();
                    let w = size_info.cell_width();
                    let h = size_info.cell_height();

                    let left_ndc = (x / size_info.width()) * 2.0 - 1.0;
                    let right_ndc = ((x + w) / size_info.width()) * 2.0 - 1.0;
                    let top_ndc = 1.0 - (y / size_info.height()) * 2.0;
                    let bottom_ndc = 1.0 - ((y + h) / size_info.height()) * 2.0;

                    let buf = self.batch_buffers.entry(tex).or_default();
                    append_quad(buf, left_ndc, top_ndc, right_ndc, bottom_ndc, u0, v0, u1, v1);
                }
            }

            // 6. Execute batched draw: 1 glDrawArrays per unique texture!
            while gl::GetError() != gl::NO_ERROR {}
            gl::UseProgram(self.program.id());
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            gl::BindVertexArray(self.vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.vbo);

            for (&tex, vertices) in &self.batch_buffers {
                if vertices.is_empty() {
                    continue;
                }
                gl::BindTexture(gl::TEXTURE_2D, tex);
                gl::BufferData(
                    gl::ARRAY_BUFFER,
                    (vertices.len() * std::mem::size_of::<f32>()) as isize,
                    vertices.as_ptr().cast(),
                    gl::DYNAMIC_DRAW,
                );
                let vertex_count = (vertices.len() / 4) as i32;
                gl::DrawArrays(gl::TRIANGLES, 0, vertex_count);
            }

            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_2D, 0);
            gl::BindVertexArray(0);
            gl::UseProgram(0);
        }
    }
}

impl Drop for ImageRenderer {
    fn drop(&mut self) {
        unsafe {
            for (_, (tex, _)) in &self.textures {
                gl::DeleteTextures(1, tex);
            }
            gl::DeleteBuffers(1, &self.vbo);
            gl::DeleteVertexArrays(1, &self.vao);
        }
    }
}

struct GlExtensions;

impl GlExtensions {
    /// Check if the given `extension` is supported.
    ///
    /// This function will lazily load OpenGL extensions.
    fn contains(extension: &str) -> bool {
        static OPENGL_EXTENSIONS: OnceLock<HashSet<&'static str, RandomState>> = OnceLock::new();

        OPENGL_EXTENSIONS.get_or_init(Self::load_extensions).contains(extension)
    }

    /// Load available OpenGL extensions.
    fn load_extensions() -> HashSet<&'static str, RandomState> {
        unsafe {
            let extensions = gl::GetString(gl::EXTENSIONS);

            if extensions.is_null() {
                let mut extensions_number = 0;
                gl::GetIntegerv(gl::NUM_EXTENSIONS, &mut extensions_number);

                (0..extensions_number as gl::types::GLuint)
                    .flat_map(|i| {
                        let extension = CStr::from_ptr(gl::GetStringi(gl::EXTENSIONS, i) as *mut _);
                        extension.to_str()
                    })
                    .collect()
            } else {
                match CStr::from_ptr(extensions as *mut _).to_str() {
                    Ok(ext) => ext.split_whitespace().collect(),
                    Err(_) => Default::default(),
                }
            }
        }
    }
}

extern "system" fn gl_debug_log(
    _: gl::types::GLenum,
    _: gl::types::GLenum,
    _: gl::types::GLuint,
    _: gl::types::GLenum,
    _: gl::types::GLsizei,
    msg: *const gl::types::GLchar,
    _: *mut std::os::raw::c_void,
) {
    let msg = unsafe { CStr::from_ptr(msg).to_string_lossy() };
    debug!("[gl_render] {msg}");
}
