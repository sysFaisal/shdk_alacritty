//! Convert a cursor into rectangles and track Kitty-compatible trail geometry.

use std::time::Instant;

use alacritty_terminal::vte::ansi::CursorShape;

use crate::display::SizeInfo;
use crate::display::color::Rgb;
use crate::display::content::RenderableCursor;
use crate::renderer::rects::RenderRect;

/// Trait for conversion into the iterator.
pub trait IntoRects {
    /// Consume the cursor for an iterator of rects.
    fn rects(self, size_info: &SizeInfo, thickness: f32) -> CursorRects;
}

/// Geometry consumed directly by the GPU trail shader.
#[derive(Clone, Copy, Debug)]
pub struct CursorTrail {
    pub corners: [[f32; 2]; 4],
    pub cursor_edges: [f32; 4], // left, right, top, bottom in NDC.
    pub color: Rgb,
    pub opacity: f32,
}

#[derive(Debug, Default)]
pub struct CursorTrailState {
    corners: Option<[[f32; 2]; 4]>,
    edges: Option<[f32; 4]>,
    updated_at: Option<Instant>,
    needs_render: bool,
    prev_damage_rect: Option<(i32, i32, i32, i32)>,
    clearing_frames: u8,
}

impl CursorTrailState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// A live trail needs another compositor frame even when terminal content is unchanged.
    pub fn is_animating(&self) -> bool {
        self.needs_render || self.clearing_frames > 0
    }

    /// Track and union damage rectangles across frames to avoid frozen remnants on Wayland.
    pub fn damage_rect(
        &mut self,
        current: Option<(i32, i32, i32, i32)>,
    ) -> Option<(i32, i32, i32, i32)> {
        let result = match (self.prev_damage_rect, current) {
            (Some(prev), Some(curr)) => Some(union_rect(prev, curr)),
            (Some(prev), None) => Some(prev),
            (None, Some(curr)) => Some(curr),
            (None, None) => None,
        };

        if current.is_some() {
            self.prev_damage_rect = current;
        } else if self.clearing_frames > 0 {
            self.clearing_frames -= 1;
            if self.clearing_frames == 0 {
                self.prev_damage_rect = None;
            }
        } else {
            self.prev_damage_rect = None;
        }

        result
    }

    /// Port of Kitty's `update_cursor_trail`: four independently eased corners.
    pub fn update(
        &mut self,
        cursor: RenderableCursor,
        size: &SizeInfo,
        thickness: f32,
        now: Instant,
        fast_decay_ms: u32,
        slow_decay_ms: u32,
        threshold: (u32, u32),
    ) -> Option<CursorTrail> {
        let Some(edges) = cursor_edges(cursor, size, thickness) else {
            if self.needs_render {
                self.needs_render = false;
                if self.clearing_frames == 0 {
                    self.clearing_frames = 3;
                }
            }
            return None;
        };
        let Some(previous_edges) = self.edges.replace(edges) else {
            self.corners = Some(corners(edges));
            self.updated_at = Some(now);
            return None;
        };

        let mut trail_corners = self.corners.unwrap_or_else(|| corners(previous_edges));
        // Reset the integration clock on new movement so idle time does not fast-forward easing.
        let moved = edges != previous_edges;
        let dt = if moved && !self.needs_render {
            0.016
        } else {
            self.updated_at.map_or(0.016, |at| now.saturating_duration_since(at).as_secs_f32())
        };
        // Clamp dt to prevent visual glitches on frame spikes or background tab switches.
        let dt = dt.clamp(0.001, 0.05);
        self.updated_at = Some(now);

        let cell_x = 2. * size.cell_width() / size.width();
        let cell_y = 2. * size.cell_height() / size.height();
        let dx = ((trail_corners[0][0] - edges[1]) / cell_x).round().abs() as u32;
        let dy = ((trail_corners[0][1] - edges[2]) / cell_y).round().abs() as u32;
        if !self.needs_render && dx <= threshold.0 && dy <= threshold.1 {
            self.corners = Some(corners(edges));
            return None;
        }

        // Kitty varies each corner's decay from slow to fast by its direction dot product.
        let target = corners(edges);
        let center = [(edges[0] + edges[1]) * 0.5, (edges[2] + edges[3]) * 0.5];
        let diagonal = ((edges[1] - edges[0]).powi(2) + (edges[2] - edges[3]).powi(2)).sqrt() * 0.5;
        let mut delta = [[0.; 2]; 4];
        let mut dots = [0.; 4];
        let mut min_dot = f32::INFINITY;
        let mut max_dot = f32::NEG_INFINITY;
        for i in 0..4 {
            delta[i] = [target[i][0] - trail_corners[i][0], target[i][1] - trail_corners[i][1]];
            let length = (delta[i][0].powi(2) + delta[i][1].powi(2)).sqrt();
            if length > 1e-6 && diagonal > 0. {
                dots[i] = (delta[i][0] * (target[i][0] - center[0])
                    + delta[i][1] * (target[i][1] - center[1]))
                    / diagonal
                    / length;
                min_dot = min_dot.min(dots[i]);
                max_dot = max_dot.max(dots[i]);
            }
        }
        let fast = fast_decay_ms.max(1) as f32 / 1000.;
        let slow = slow_decay_ms.max(fast_decay_ms).max(1) as f32 / 1000.;
        for i in 0..4 {
            if delta[i] == [0., 0.] {
                continue;
            }
            let decay = if min_dot == max_dot {
                slow
            } else {
                slow + (fast - slow) * (dots[i] - min_dot) / (max_dot - min_dot)
            };
            let step = 1. - (-10. * dt / decay).exp2();
            trail_corners[i][0] += delta[i][0] * step;
            trail_corners[i][1] += delta[i][1] * step;
        }
        let dx_threshold = 1. / size.width();
        let dy_threshold = 1. / size.height();
        let was_rendering = self.needs_render;
        self.needs_render = (0..4).any(|i| {
            (target[i][0] - trail_corners[i][0]).abs() >= dx_threshold
                || (target[i][1] - trail_corners[i][1]).abs() >= dy_threshold
        });
        if !self.needs_render {
            trail_corners = target;
            if was_rendering && self.clearing_frames == 0 {
                self.clearing_frames = 3;
            }
        }
        self.corners = Some(trail_corners);
        self.needs_render.then_some(CursorTrail {
            corners: trail_corners,
            cursor_edges: edges,
            color: cursor.color(),
            opacity: 1.,
        })
    }
}

fn union_rect(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> (i32, i32, i32, i32) {
    let left = a.0.min(b.0);
    let top = a.1.min(b.1);
    let right = (a.0 + a.2).max(b.0 + b.2);
    let bottom = (a.1 + a.3).max(b.1 + b.3);
    (left, top, right - left, bottom - top)
}

fn corners(edges: [f32; 4]) -> [[f32; 2]; 4] {
    [[edges[1], edges[2]], [edges[1], edges[3]], [edges[0], edges[3]], [edges[0], edges[2]]]
}

#[cfg(test)]
mod tests {
    use super::corners;

    #[test]
    fn trail_corner_order_matches_kitty_quad() {
        assert_eq!(
            corners([-0.5, 0.5, 0.25, -0.25]),
            [[0.5, 0.25], [0.5, -0.25], [-0.5, -0.25], [-0.5, 0.25]],
        );
    }
}

fn cursor_edges(cursor: RenderableCursor, size: &SizeInfo, thickness: f32) -> Option<[f32; 4]> {
    let point = cursor.point();
    let x = point.column.0 as f32 * size.cell_width() + size.padding_x();
    let y = point.line as f32 * size.cell_height() + size.padding_y();
    let mut width = size.cell_width() * cursor.width().get() as f32;
    let mut height = size.cell_height();
    let stroke = (thickness * size.cell_width()).round().max(1.);
    match cursor.shape() {
        CursorShape::Block | CursorShape::HollowBlock => {},
        CursorShape::Beam => width = stroke,
        CursorShape::Underline => height = stroke,
        CursorShape::Hidden => return None,
    }
    let top =
        if cursor.shape() == CursorShape::Underline { y + size.cell_height() - height } else { y };
    Some([
        x / size.width() * 2. - 1.,
        (x + width) / size.width() * 2. - 1.,
        1. - top / size.height() * 2.,
        1. - (top + height) / size.height() * 2.,
    ])
}

impl IntoRects for RenderableCursor {
    fn rects(self, size_info: &SizeInfo, thickness: f32) -> CursorRects {
        let point = self.point();
        let x = point.column.0 as f32 * size_info.cell_width() + size_info.padding_x();
        let y = point.line as f32 * size_info.cell_height() + size_info.padding_y();
        let mut width = size_info.cell_width();
        let height = size_info.cell_height();
        let thickness = (thickness * width).round().max(1.);
        width *= self.width().get() as f32;
        match self.shape() {
            CursorShape::Beam => beam(x, y, height, thickness, self.color()),
            CursorShape::Underline => underline(x, y, width, height, thickness, self.color()),
            CursorShape::HollowBlock => hollow(x, y, width, height, thickness, self.color()),
            _ => CursorRects::default(),
        }
    }
}

#[derive(Default)]
pub struct CursorRects {
    rects: [Option<RenderRect>; 4],
    index: usize,
}
impl From<RenderRect> for CursorRects {
    fn from(rect: RenderRect) -> Self {
        Self { rects: [Some(rect), None, None, None], index: 0 }
    }
}
impl Iterator for CursorRects {
    type Item = RenderRect;
    fn next(&mut self) -> Option<Self::Item> {
        let rect = self.rects.get_mut(self.index)?;
        self.index += 1;
        rect.take()
    }
}
fn beam(x: f32, y: f32, height: f32, thickness: f32, color: Rgb) -> CursorRects {
    RenderRect::new(x, y, thickness, height, color, 1.).into()
}
fn underline(x: f32, y: f32, width: f32, height: f32, thickness: f32, color: Rgb) -> CursorRects {
    RenderRect::new(x, y + height - thickness, width, thickness, color, 1.).into()
}
fn hollow(x: f32, y: f32, width: f32, height: f32, thickness: f32, color: Rgb) -> CursorRects {
    let vertical_y = y + thickness;
    let vertical_height = height - 2. * thickness;
    CursorRects {
        rects: [
            Some(RenderRect::new(x, y, width, thickness, color, 1.)),
            Some(RenderRect::new(x, y + height - thickness, width, thickness, color, 1.)),
            Some(RenderRect::new(x, vertical_y, thickness, vertical_height, color, 1.)),
            Some(RenderRect::new(
                x + width - thickness,
                vertical_y,
                thickness,
                vertical_height,
                color,
                1.,
            )),
        ],
        index: 0,
    }
}
