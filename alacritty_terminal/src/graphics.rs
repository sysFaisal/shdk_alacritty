use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

#[cfg(target_family = "unix")]
extern crate libc;

use base64::engine::general_purpose::STANDARD as Base64;
use base64::Engine;
use log::debug;

use crate::index::{Column, Line, Point};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    TransmitAndDisplay,
    Transmit,
    Query,
    Put,
    Delete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Rgba, // 32
    Rgb,  // 24
    Png,  // 100
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Medium {
    Direct,
    File,
    TempFile,
    SharedMemory,
}

#[derive(Debug, Clone, Default)]
pub struct GraphicsCommand {
    pub action: Option<Action>,
    pub delete_action: Option<char>,
    pub id: u32,
    pub placement_id: u32,
    pub format: Option<Format>,
    pub medium: Option<Medium>,
    pub width: u32,
    pub height: u32,
    pub columns: u16,
    pub lines: u16,
    pub x_offset: u32,
    pub y_offset: u32,
    pub cell_x_offset: u16,
    pub cell_y_offset: u16,
    pub z_index: i32,
    pub more: bool,
    pub quiet: u8,
    pub cursor_movement: bool, // true = do not move cursor (C=1)
    pub unicode_placement: bool, // U=1 for unicode placeholder mode
    pub data_sz: u32,          // S= total size hint (used with t=s)
    pub data_offset: u32,      // O= offset into file/shm
    pub src_width: u32,        // w= crop width
    pub src_height: u32,       // h= crop height
    pub payload: Vec<u8>,
}

impl GraphicsCommand {
    pub fn parse(bytes: &[u8]) -> Option<Self> {
        let semicolon = bytes.iter().position(|&b| b == b';');
        let (control, payload) = match semicolon {
            Some(idx) => (&bytes[..idx], &bytes[idx + 1..]),
            None => (bytes, &[][..]),
        };

        let mut cmd = GraphicsCommand {
            action: None,
            delete_action: None,
            id: 0,
            placement_id: 0,
            format: None,
            medium: None,
            width: 0,
            height: 0,
            columns: 0,
            lines: 0,
            x_offset: 0,
            y_offset: 0,
            cell_x_offset: 0,
            cell_y_offset: 0,
            z_index: 0,
            more: false,
            quiet: 0,
            cursor_movement: false,
            unicode_placement: false,
            data_sz: 0,
            data_offset: 0,
            src_width: 0,
            src_height: 0,
            payload: payload.to_vec(),
        };

        if !control.is_empty() {
            let control_str = std::str::from_utf8(control).ok()?;
            for part in control_str.split(',') {
                if part.is_empty() {
                    continue;
                }
                let mut kv = part.splitn(2, '=');
                let key = kv.next()?.trim();
                let val = kv.next().unwrap_or("").trim();

                match key {
                    "a" => {
                        cmd.action = match val {
                            "t" | "T" => Some(Action::TransmitAndDisplay),
                            "q" => Some(Action::Query),
                            "p" => Some(Action::Put),
                            "d" => Some(Action::Delete),
                            "f" => Some(Action::Transmit),
                            _ => Some(Action::TransmitAndDisplay),
                        };
                    },
                    "d" => {
                        cmd.delete_action = val.chars().next();
                    },
                    "i" => cmd.id = val.parse().unwrap_or(0),
                    "p" => cmd.placement_id = val.parse().unwrap_or(0),
                    "f" => {
                        cmd.format = match val.parse::<u32>().unwrap_or(32) {
                            24 => Some(Format::Rgb),
                            100 => Some(Format::Png),
                            _ => Some(Format::Rgba),
                        };
                    },
                    "t" => {
                        cmd.medium = match val {
                            "f" => Some(Medium::File),
                            "t" => Some(Medium::TempFile),
                            "s" => Some(Medium::SharedMemory),
                            _ => Some(Medium::Direct),
                        };
                    },
                    "s" => cmd.width = val.parse().unwrap_or(0),
                    "v" => cmd.height = val.parse().unwrap_or(0),
                    "w" => cmd.src_width = val.parse().unwrap_or(0),
                    "h" => cmd.src_height = val.parse().unwrap_or(0),
                    "S" => cmd.data_sz = val.parse().unwrap_or(0),
                    "O" => cmd.data_offset = val.parse().unwrap_or(0),
                    "c" => cmd.columns = val.parse().unwrap_or(0),
                    "r" => cmd.lines = val.parse().unwrap_or(0),
                    "x" => cmd.x_offset = val.parse().unwrap_or(0),
                    "y" => cmd.y_offset = val.parse().unwrap_or(0),
                    "X" => cmd.cell_x_offset = val.parse().unwrap_or(0),
                    "Y" => cmd.cell_y_offset = val.parse().unwrap_or(0),
                    "z" => cmd.z_index = val.parse().unwrap_or(0),
                    "m" => cmd.more = val == "1",
                    "q" => cmd.quiet = val.parse().unwrap_or(0),
                    "C" => cmd.cursor_movement = val == "1",
                    "U" => cmd.unicode_placement = val == "1",
                    _ => {},
                }
            }
        }

        Some(cmd)
    }
}

/// Decoded image stored in memory.
#[derive(Clone, Debug)]
pub struct StoredImage {
    pub id: u32,
    pub width: u32,
    pub height: u32,
    pub pixels: Arc<[u8]>,
    pub version: u64,
}

/// Visible or placed instance of an image anchored to terminal grid line/column.
#[derive(Clone, Debug)]
pub struct ImagePlacement {
    pub image_id: u32,
    pub placement_id: u32,
    pub line: Line,
    pub column: Column,
    pub columns: usize,
    pub lines: usize,
    pub z_index: i32,
    pub src_x: u32,
    pub src_y: u32,
    pub src_w: u32,
    pub src_h: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VirtualPlacement {
    pub columns: usize,
    pub lines: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlaceholderTile {
    pub point: Point<usize>,
    pub image_id: u32,
    pub row: usize,
    pub col: usize,
}

/// State tracking images, placements, and chunked transfer buffers.
#[derive(Default, Debug, Clone)]
pub struct GraphicsState {
    images: HashMap<u32, StoredImage>,
    placements: Vec<ImagePlacement>,
    virtual_placements: HashMap<u32, VirtualPlacement>,
    chunk_buffer: Option<GraphicsCommand>,
    next_auto_id: u32,
    next_image_version: u64,
}

impl GraphicsState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn images(&self) -> &HashMap<u32, StoredImage> {
        &self.images
    }

    pub fn placements(&self) -> &[ImagePlacement] {
        &self.placements
    }

    pub fn placements_mut(&mut self) -> &mut Vec<ImagePlacement> {
        &mut self.placements
    }

    pub fn virtual_placements(&self) -> &HashMap<u32, VirtualPlacement> {
        &self.virtual_placements
    }

    /// Scroll placements when grid scrolls.
    pub fn scroll(&mut self, lines: usize) {
        for placement in &mut self.placements {
            placement.line -= lines;
        }
        // Keep placements that haven't scrolled infinitely far into history (-50,000 lines).
        self.placements.retain(|p| p.line.0 > -50000);
    }

    /// Process a parsed command. Returns an optional PTY response string.
    pub fn process_command(
        &mut self,
        cmd: GraphicsCommand,
        cursor: Point,
        cell_width: f32,
        cell_height: f32,
    ) -> (Option<String>, Option<(usize, usize, bool)>) {
        // Handle chunking
        if cmd.more {
            if let Some(buf) = &mut self.chunk_buffer {
                buf.payload.extend_from_slice(&cmd.payload);
            } else {
                self.chunk_buffer = Some(cmd);
            }
            return (None, None);
        }

        let full_cmd = if let Some(mut buf) = self.chunk_buffer.take() {
            buf.payload.extend_from_slice(&cmd.payload);
            buf.more = false;
            buf
        } else {
            cmd
        };

        let action = full_cmd.action.unwrap_or(Action::TransmitAndDisplay);

        match action {
            Action::Query => {
                let id = full_cmd.id;
                let response = format!("\x1b_Gi={id};OK\x1b\\");
                (Some(response), None)
            },
            Action::Delete => {
                if let Some(d) = full_cmd.delete_action {
                    match d {
                        'a' | 'A' => {
                            self.images.clear();
                            self.placements.clear();
                            self.virtual_placements.clear();
                        },
                        'i' | 'I' if full_cmd.id > 0 => {
                            self.images.remove(&full_cmd.id);
                            self.placements.retain(|p| p.image_id != full_cmd.id);
                            self.virtual_placements.remove(&full_cmd.id);
                        },
                        'p' | 'P' if full_cmd.id > 0 => {
                            self.placements.retain(|p| {
                                !(p.image_id == full_cmd.id
                                    && (full_cmd.placement_id == 0
                                        || p.placement_id == full_cmd.placement_id))
                            });
                        },
                        _ => {},
                    }
                } else if full_cmd.id > 0 {
                    self.images.remove(&full_cmd.id);
                    self.placements.retain(|p| p.image_id != full_cmd.id);
                    self.virtual_placements.remove(&full_cmd.id);
                } else {
                    self.images.clear();
                    self.placements.clear();
                    self.virtual_placements.clear();
                }
                (None, None)
            },
            Action::Put => {
                let id = full_cmd.id;
                if let Some(image) = self.images.get(&id) {
                    let src_w = if full_cmd.src_width > 0 {
                        full_cmd.src_width
                    } else {
                        image.width.saturating_sub(full_cmd.x_offset).max(1)
                    };
                    let src_h = if full_cmd.src_height > 0 {
                        full_cmd.src_height
                    } else {
                        image.height.saturating_sub(full_cmd.y_offset).max(1)
                    };
                    let (cols, lines) = compute_dimensions(
                        src_w,
                        src_h,
                        full_cmd.columns,
                        full_cmd.lines,
                        cell_width,
                        cell_height,
                    );
                    if full_cmd.unicode_placement {
                        self.virtual_placements.insert(
                            id,
                            VirtualPlacement { columns: cols, lines },
                        );
                    } else {
                        self.placements.push(ImagePlacement {
                            image_id: id,
                            placement_id: full_cmd.placement_id,
                            line: cursor.line,
                            column: cursor.column,
                            columns: cols,
                            lines,
                            z_index: full_cmd.z_index,
                            src_x: full_cmd.x_offset,
                            src_y: full_cmd.y_offset,
                            src_w: full_cmd.src_width,
                            src_h: full_cmd.src_height,
                        });
                    }
                    let reply = if full_cmd.quiet == 0 {
                        Some(format!("\x1b_Gi={id};OK\x1b\\"))
                    } else {
                        None
                    };
                    let cursor_advance = (!full_cmd.cursor_movement && !full_cmd.unicode_placement).then_some((cols, lines, false));
                    (reply, cursor_advance)
                } else {
                    let reply = if full_cmd.quiet < 2 {
                        Some(format!("\x1b_Gi={id};ENOENT\x1b\\"))
                    } else {
                        None
                    };
                    (reply, None)
                }
            },
            Action::Transmit | Action::TransmitAndDisplay => {
                let (width, height, pixels) = match decode_payload(&full_cmd) {
                    Ok(res) => res,
                    Err(e) => {
                        debug!("Kitty graphics decode error: {e}");
                        let id = full_cmd.id;
                        let reply = if id > 0 && full_cmd.quiet < 2 {
                            Some(format!("\x1b_Gi={id};EINVAL: {e}\x1b\\"))
                        } else {
                            None
                        };
                        return (reply, None);
                    },
                };

                let image_id = if full_cmd.id == 0 {
                    self.next_auto_id += 1;
                    self.next_auto_id
                } else {
                    full_cmd.id
                };

                self.next_image_version = self.next_image_version.wrapping_add(1);
                self.images.insert(
                    image_id,
                    StoredImage {
                        id: image_id,
                        width,
                        height,
                        pixels: Arc::from(pixels),
                        version: self.next_image_version,
                    },
                );

                let cursor_advance = if action == Action::TransmitAndDisplay {
                    let src_w = if full_cmd.src_width > 0 {
                        full_cmd.src_width
                    } else {
                        width.saturating_sub(full_cmd.x_offset).max(1)
                    };
                    let src_h = if full_cmd.src_height > 0 {
                        full_cmd.src_height
                    } else {
                        height.saturating_sub(full_cmd.y_offset).max(1)
                    };
                    let (cols, lines) = compute_dimensions(
                        src_w,
                        src_h,
                        full_cmd.columns,
                        full_cmd.lines,
                        cell_width,
                        cell_height,
                    );
                    
                    if !full_cmd.unicode_placement {
                        self.placements.push(ImagePlacement {
                            image_id,
                            placement_id: full_cmd.placement_id,
                            line: cursor.line,
                            column: cursor.column,
                            columns: cols,
                            lines,
                            z_index: full_cmd.z_index,
                            src_x: full_cmd.x_offset,
                            src_y: full_cmd.y_offset,
                            src_w: full_cmd.src_width,
                            src_h: full_cmd.src_height,
                        });
                        (!full_cmd.cursor_movement).then_some((cols, lines, false))
                    } else {
                        self.virtual_placements.insert(
                            image_id,
                            VirtualPlacement { columns: cols, lines },
                        );
                        None // Don't move cursor for U=1
                    }
                } else {
                    None
                };

                let reply = if full_cmd.quiet == 0 {
                    Some(format!("\x1b_Gi={image_id};OK\x1b\\"))
                } else {
                    None
                };

                (reply, cursor_advance)
            },
        }
    }
}

fn compute_dimensions(
    pixel_w: u32,
    pixel_h: u32,
    req_cols: u16,
    req_lines: u16,
    cell_w: f32,
    cell_h: f32,
) -> (usize, usize) {
    let cw = cell_w.max(1.0);
    let ch = cell_h.max(1.0);

    match (req_cols, req_lines) {
        (c, r) if c > 0 && r > 0 => (c as usize, r as usize),
        (c, 0) if c > 0 => {
            let width_px = c as f32 * cw;
            let aspect = pixel_h as f32 / pixel_w.max(1) as f32;
            let height_px = width_px * aspect;
            let lines = (height_px / ch).ceil().max(1.0) as usize;
            (c as usize, lines)
        },
        (0, r) if r > 0 => {
            let height_px = r as f32 * ch;
            let aspect = pixel_w as f32 / pixel_h.max(1) as f32;
            let width_px = height_px * aspect;
            let cols = (width_px / cw).ceil().max(1.0) as usize;
            (cols, r as usize)
        },
        _ => {
            let cols = (pixel_w as f32 / cw).ceil().max(1.0) as usize;
            let lines = (pixel_h as f32 / ch).ceil().max(1.0) as usize;
            (cols, lines)
        },
    }
}

fn decode_payload(cmd: &GraphicsCommand) -> Result<(u32, u32, Vec<u8>), &'static str> {
    let medium = cmd.medium.unwrap_or(Medium::Direct);
    let raw_bytes = match medium {
        Medium::Direct => {
            // Strip whitespace/newlines from base64
            let filtered: Vec<u8> = cmd
                .payload
                .iter()
                .copied()
                .filter(|b| !b.is_ascii_whitespace())
                .collect();
            Base64.decode(&filtered).map_err(|_| "Base64 decode failed")?
        },
        Medium::File | Medium::TempFile => {
            let filtered: Vec<u8> = cmd
                .payload
                .iter()
                .copied()
                .filter(|b| !b.is_ascii_whitespace())
                .collect();
            let path_bytes = Base64.decode(&filtered).unwrap_or_else(|_| cmd.payload.clone());
            let path_str = std::str::from_utf8(&path_bytes).map_err(|_| "Invalid file path")?;
            let path = path_str.trim();
            let data = std::fs::read(path).map_err(|_| "File read failed")?;
            if medium == Medium::TempFile {
                let _ = std::fs::remove_file(path);
            }
            data
        },
        Medium::SharedMemory => {
            let filtered: Vec<u8> = cmd
                .payload
                .iter()
                .copied()
                .filter(|b| !b.is_ascii_whitespace())
                .collect();
            let name_bytes = Base64.decode(&filtered).map_err(|_| "Base64 decode failed for SHM name")?;
            let name_str = std::str::from_utf8(&name_bytes).map_err(|_| "Invalid SHM name")?;
            let name = name_str.trim();
            read_shm(name, cmd.data_sz as usize)?
        },
    };

    let format = cmd.format.unwrap_or(Format::Rgba);
    match format {
        Format::Png => decode_png(&raw_bytes),
        Format::Rgba => {
            let w = cmd.width;
            let h = cmd.height;
            if w == 0 || h == 0 {
                return Err("Missing width or height for raw RGBA");
            }
            let expected_len = (w * h * 4) as usize;
            if raw_bytes.len() < expected_len {
                return Err("RGBA payload truncated");
            }
            Ok((w, h, raw_bytes[..expected_len].to_vec()))
        },
        Format::Rgb => {
            let w = cmd.width;
            let h = cmd.height;
            if w == 0 || h == 0 {
                return Err("Missing width or height for raw RGB");
            }
            let expected_len = (w * h * 3) as usize;
            if raw_bytes.len() < expected_len {
                return Err("RGB payload truncated");
            }
            let mut rgba = Vec::with_capacity((w * h * 4) as usize);
            for chunk in raw_bytes[..expected_len].chunks_exact(3) {
                rgba.push(chunk[0]);
                rgba.push(chunk[1]);
                rgba.push(chunk[2]);
                rgba.push(255);
            }
            Ok((w, h, rgba))
        },
    }
}

fn read_shm(name: &str, hint_size: usize) -> Result<Vec<u8>, &'static str> {
    use std::ffi::CString;
    let cname = CString::new(name).map_err(|_| "Invalid SHM name")?;
    unsafe {
        let fd = libc::shm_open(cname.as_ptr(), libc::O_RDONLY, 0);
        if fd < 0 {
            return Err("shm_open failed");
        }

        // Get actual size via fstat
        let mut stat: libc::stat = std::mem::zeroed();
        if libc::fstat(fd, &mut stat) < 0 {
            libc::close(fd);
            return Err("fstat on SHM failed");
        }
        let size = if stat.st_size > 0 {
            stat.st_size as usize
        } else if hint_size > 0 {
            hint_size
        } else {
            libc::close(fd);
            return Err("SHM has zero size");
        };

        let ptr = libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            0,
        );
        libc::close(fd);

        if ptr == libc::MAP_FAILED {
            return Err("mmap of SHM failed");
        }

        let data = std::slice::from_raw_parts(ptr as *const u8, size).to_vec();
        libc::munmap(ptr, size);
        // Unlink so yazi knows we consumed it
        libc::shm_unlink(cname.as_ptr());
        Ok(data)
    }
}

fn decode_png(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>), &'static str> {
    let decoder = png::Decoder::new(Cursor::new(bytes));
    let mut reader = decoder.read_info().map_err(|_| "PNG header decode failed")?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|_| "PNG frame decode failed")?;
    let width = info.width;
    let height = info.height;

    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => {
            let mut out = Vec::with_capacity((width * height * 4) as usize);
            for chunk in buf[..info.buffer_size()].chunks_exact(3) {
                out.push(chunk[0]);
                out.push(chunk[1]);
                out.push(chunk[2]);
                out.push(255);
            }
            out
        },
        png::ColorType::GrayscaleAlpha => {
            let mut out = Vec::with_capacity((width * height * 4) as usize);
            for chunk in buf[..info.buffer_size()].chunks_exact(2) {
                out.push(chunk[0]);
                out.push(chunk[0]);
                out.push(chunk[0]);
                out.push(chunk[1]);
            }
            out
        },
        png::ColorType::Grayscale => {
            let mut out = Vec::with_capacity((width * height * 4) as usize);
            for &gray in &buf[..info.buffer_size()] {
                out.push(gray);
                out.push(gray);
                out.push(gray);
                out.push(255);
            }
            out
        },
        png::ColorType::Indexed => return Err("Indexed PNG not supported"),
    };

    Ok((width, height, rgba))
}

#[derive(Debug, PartialEq, Eq)]
pub enum GraphicsStreamItem<'a> {
    Text(&'a [u8]),
    Graphics(Vec<u8>),
}

/// Stream interceptor that extracts `\x1b_G ... (\x1b\ | \x07)` from incoming PTY bytes.
#[derive(Default)]
pub struct GraphicsStreamParser {
    state: StreamState,
    buffer: Vec<u8>,
}

#[derive(Default, PartialEq, Eq)]
enum StreamState {
    #[default]
    Ground,
    Esc,
    ApcStart,
    KittyGraphics,
    KittyGraphicsEsc,
}

impl GraphicsStreamParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Advance stream parser with incoming chunk.
    pub fn advance<'a, F>(&mut self, bytes: &'a [u8], mut callback: F)
    where
        F: FnMut(GraphicsStreamItem<'a>),
    {
        let mut normal_start = 0;
        let mut i = 0;

        while i < bytes.len() {
            let b = bytes[i];
            match self.state {
                StreamState::Ground => {
                    if b == 0x1b {
                        if i > normal_start {
                            callback(GraphicsStreamItem::Text(&bytes[normal_start..i]));
                        }
                        self.state = StreamState::Esc;
                    }
                },
                StreamState::Esc => {
                    if b == b'_' {
                        self.state = StreamState::ApcStart;
                    } else {
                        callback(GraphicsStreamItem::Text(&[0x1b]));
                        normal_start = i;
                        self.state = StreamState::Ground;
                        continue;
                    }
                },
                StreamState::ApcStart => {
                    if b == b'G' {
                        self.state = StreamState::KittyGraphics;
                        self.buffer.clear();
                    } else {
                        callback(GraphicsStreamItem::Text(&[0x1b, b'_']));
                        normal_start = i;
                        self.state = StreamState::Ground;
                        continue;
                    }
                },
                StreamState::KittyGraphics => {
                    if b == 0x07 {
                        let payload = std::mem::take(&mut self.buffer);
                        callback(GraphicsStreamItem::Graphics(payload));
                        self.state = StreamState::Ground;
                        normal_start = i + 1;
                    } else if b == 0x1b {
                        self.state = StreamState::KittyGraphicsEsc;
                    } else {
                        self.buffer.push(b);
                    }
                },
                StreamState::KittyGraphicsEsc => {
                    if b == b'\\' {
                        let payload = std::mem::take(&mut self.buffer);
                        callback(GraphicsStreamItem::Graphics(payload));
                        self.state = StreamState::Ground;
                        normal_start = i + 1;
                    } else {
                        self.buffer.push(0x1b);
                        self.buffer.push(b);
                        self.state = StreamState::KittyGraphics;
                    }
                },
            }
            i += 1;
        }

        if self.state == StreamState::Ground && normal_start < bytes.len() {
            callback(GraphicsStreamItem::Text(&bytes[normal_start..]));
        }
    }
}

/// Decode combining diacritics into 0-based offset for Kitty Unicode Placeholders (U+10EEEE).
/// Follows the canonical mapping defined in Kitty's rowcolumn-diacritics.c / rowcolumn-diacritics.txt.
pub fn diacritic_to_num(code: u32) -> Option<usize> {
    let num = match code {
        0x305..=0x306 => code - 0x305 + 1,
        0x30d..=0x30f => code - 0x30d + 2,
        0x310..=0x311 => code - 0x310 + 4,
        0x312..=0x313 => code - 0x312 + 5,
        0x33d..=0x340 => code - 0x33d + 6,
        0x346..=0x347 => code - 0x346 + 9,
        0x34a..=0x34d => code - 0x34a + 10,
        0x350..=0x353 => code - 0x350 + 13,
        0x357..=0x358 => code - 0x357 + 16,
        0x35b..=0x35c => code - 0x35b + 17,
        0x363..=0x370 => code - 0x363 + 18,
        0x483..=0x488 => code - 0x483 + 31,
        0x592..=0x596 => code - 0x592 + 36,
        0x597..=0x59a => code - 0x597 + 40,
        0x59c..=0x5a2 => code - 0x59c + 43,
        0x5a8..=0x5aa => code - 0x5a8 + 49,
        0x5ab..=0x5ad => code - 0x5ab + 51,
        0x5af..=0x5b0 => code - 0x5af + 53,
        0x5c4..=0x5c5 => code - 0x5c4 + 54,
        0x610..=0x618 => code - 0x610 + 55,
        0x657..=0x65c => code - 0x657 + 63,
        0x65d..=0x65f => code - 0x65d + 68,
        0x6d6..=0x6dd => code - 0x6d6 + 70,
        0x6df..=0x6e3 => code - 0x6df + 77,
        0x6e4..=0x6e5 => code - 0x6e4 + 81,
        0x6e7..=0x6e9 => code - 0x6e7 + 82,
        0x6eb..=0x6ed => code - 0x6eb + 84,
        0x730..=0x731 => code - 0x730 + 86,
        0x732..=0x734 => code - 0x732 + 87,
        0x735..=0x737 => code - 0x735 + 89,
        0x73a..=0x73b => code - 0x73a + 91,
        0x73d..=0x73e => code - 0x73d + 92,
        0x73f..=0x742 => code - 0x73f + 93,
        0x743..=0x744 => code - 0x743 + 96,
        0x745..=0x746 => code - 0x745 + 97,
        0x747..=0x748 => code - 0x747 + 98,
        0x749..=0x74b => code - 0x749 + 99,
        0x7eb..=0x7f2 => code - 0x7eb + 101,
        0x7f3..=0x7f4 => code - 0x7f3 + 108,
        0x816..=0x81a => code - 0x816 + 109,
        0x81b..=0x824 => code - 0x81b + 113,
        0x825..=0x828 => code - 0x825 + 122,
        0x829..=0x82e => code - 0x829 + 125,
        0x951..=0x952 => code - 0x951 + 130,
        0x953..=0x955 => code - 0x953 + 131,
        0xf82..=0xf84 => code - 0xf82 + 133,
        0xf86..=0xf88 => code - 0xf86 + 135,
        0x135d..=0x1360 => code - 0x135d + 137,
        0x17dd..=0x17de => code - 0x17dd + 140,
        0x193a..=0x193b => code - 0x193a + 141,
        0x1a17..=0x1a18 => code - 0x1a17 + 142,
        0x1a75..=0x1a7d => code - 0x1a75 + 143,
        0x1b6b..=0x1b6c => code - 0x1b6b + 151,
        0x1b6d..=0x1b74 => code - 0x1b6d + 152,
        0x1cd0..=0x1cd3 => code - 0x1cd0 + 159,
        0x1cda..=0x1cdc => code - 0x1cda + 162,
        0x1ce0..=0x1ce1 => code - 0x1ce0 + 164,
        0x1dc0..=0x1dc2 => code - 0x1dc0 + 165,
        0x1dc3..=0x1dca => code - 0x1dc3 + 167,
        0x1dcb..=0x1dcd => code - 0x1dcb + 174,
        0x1dd1..=0x1de7 => code - 0x1dd1 + 176,
        0x1dfe..=0x1dff => code - 0x1dfe + 198,
        0x20d0..=0x20d2 => code - 0x20d0 + 199,
        0x20d4..=0x20d8 => code - 0x20d4 + 201,
        0x20db..=0x20dd => code - 0x20db + 205,
        0x20e1..=0x20e2 => code - 0x20e1 + 207,
        0x20e7..=0x20e8 => code - 0x20e7 + 208,
        0x20e9..=0x20ea => code - 0x20e9 + 209,
        0x20f0..=0x20f1 => code - 0x20f0 + 210,
        0x2cef..=0x2cf2 => code - 0x2cef + 211,
        0x2de0..=0x2e00 => code - 0x2de0 + 214,
        0xa66f..=0xa670 => code - 0xa66f + 246,
        0xa67c..=0xa67e => code - 0xa67c + 247,
        0xa6f0..=0xa6f2 => code - 0xa6f0 + 249,
        0xa8e0..=0xa8f2 => code - 0xa8e0 + 251,
        0xaab0..=0xaab1 => code - 0xaab0 + 269,
        0xaab2..=0xaab4 => code - 0xaab2 + 270,
        0xaab7..=0xaab9 => code - 0xaab7 + 272,
        0xaabe..=0xaac0 => code - 0xaabe + 274,
        0xaac1..=0xaac2 => code - 0xaac1 + 276,
        0xfe20..=0xfe27 => code - 0xfe20 + 277,
        0x10a0f..=0x10a10 => code - 0x10a0f + 284,
        0x10a38..=0x10a39 => code - 0x10a38 + 285,
        0x1d185..=0x1d18a => code - 0x1d185 + 286,
        0x1d1aa..=0x1d1ae => code - 0x1d1aa + 291,
        0x1d242..=0x1d245 => code - 0x1d242 + 295,
        _ => return None,
    };
    if num > 0 { Some((num - 1) as usize) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kitty_command() {
        let cmd = GraphicsCommand::parse(b"a=T,f=100,s=20,v=30,c=2,r=3;AAAA").unwrap();
        assert_eq!(cmd.action, Some(Action::TransmitAndDisplay));
        assert_eq!(cmd.format, Some(Format::Png));
        assert_eq!(cmd.width, 20);
        assert_eq!(cmd.height, 30);
        assert_eq!(cmd.columns, 2);
        assert_eq!(cmd.lines, 3);
        assert_eq!(cmd.payload, b"AAAA");
    }

    #[test]
    fn test_stream_interceptor() {
        let mut parser = GraphicsStreamParser::new();
        let mut text = Vec::new();
        let mut graphics = Vec::new();

        let input = b"Hello\x1b_Ga=q,i=42;AAAA\x1b\\World";
        parser.advance(input, |item| match item {
            GraphicsStreamItem::Text(t) => text.extend_from_slice(t),
            GraphicsStreamItem::Graphics(g) => graphics.push(g),
        });

        assert_eq!(text, b"HelloWorld");
        assert_eq!(graphics.len(), 1);
        assert_eq!(graphics[0], b"a=q,i=42;AAAA");
    }

    #[test]
    fn test_diacritic_to_num() {
        assert_eq!(diacritic_to_num(0x305), Some(0)); // 1st diacritic is 0-based 0
        assert_eq!(diacritic_to_num(0x30d), Some(1));
        assert_eq!(diacritic_to_num(0x30e), Some(2));
        assert_eq!(diacritic_to_num(0x310), Some(3));
        assert_eq!(diacritic_to_num(0x1d245), Some(297));
        assert_eq!(diacritic_to_num(0x41), None); // Not a diacritic
    }

    #[test]
    fn test_virtual_placement_parsing() {
        let cmd = GraphicsCommand::parse(b"a=T,f=100,s=20,v=30,c=10,r=5,U=1;AAAA").unwrap();
        assert_eq!(cmd.action, Some(Action::TransmitAndDisplay));
        assert!(cmd.unicode_placement);
        assert_eq!(cmd.columns, 10);
        assert_eq!(cmd.lines, 5);
    }

    #[test]
    fn test_image_versioning_and_deletion() {
        let mut state = GraphicsState::new();
        // 1x1 RGBA red pixel base64: /wAA/w== -> [255, 0, 0, 255]
        let cmd1 = GraphicsCommand::parse(b"a=t,f=32,s=1,v=1,i=1,q=0;/wAA/w==").unwrap();
        let (rep1, _) = state.process_command(cmd1, Point::default(), 10.0, 20.0);
        assert_eq!(rep1, Some("\x1b_Gi=1;OK\x1b\\".to_string()));
        let ver1 = state.images().get(&1).unwrap().version;
        assert_eq!(ver1, 1);

        // Transmit again with same id=1 (new image content: 4 zero bytes RGBA)
        let cmd2 = GraphicsCommand::parse(b"a=t,f=32,s=1,v=1,i=1,q=0;AAAAAA==").unwrap();
        let _ = state.process_command(cmd2, Point::default(), 10.0, 20.0);
        let ver2 = state.images().get(&1).unwrap().version;
        assert_eq!(ver2, 2);
        assert!(ver2 > ver1);

        // Put virtual placement
        let cmd_put = GraphicsCommand::parse(b"a=p,i=1,c=5,r=5,U=1").unwrap();
        let _ = state.process_command(cmd_put, Point::default(), 10.0, 20.0);
        assert!(state.virtual_placements().contains_key(&1));

        // Delete all
        let cmd_del = GraphicsCommand::parse(b"a=d,d=a").unwrap();
        let _ = state.process_command(cmd_del, Point::default(), 10.0, 20.0);
        assert!(state.images().is_empty());
        assert!(state.placements().is_empty());
        assert!(state.virtual_placements().is_empty());
    }
}
