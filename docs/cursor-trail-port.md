# Cursor Trail Port

## Goal

Port Kitty's GPU cursor-trail behavior to Alacritty without Pixman, CPU framebuffer compositing, or per-frame texture uploads.

## Rendering design

- CPU tracks cursor target, interpolated trail position, movement timestamp, interpolation progress,
  opacity, and redraw deadline.
- GPU renders the trail through an OpenGL shader.
- The existing rectangle path continues to render the active cursor.
- Block cursor trails do not use the rectangle path; they remain disabled until the dedicated
  Kitty-style shader is integrated, preventing a duplicate cursor overlay.
- GLES2 and GLSL3 paths must have equivalent behavior.
- Trail damage remains local; full-window damage is not acceptable for normal movement.

## Kitty reference

The indexed Kitty implementation is in `kitty/cursor_trail.c`, `kitty/shaders.c`, and `kitty/shaders/trail.slang`.
The main state transitions are `update_cursor_trail_target`, `update_cursor_trail_corners`,
`update_cursor_trail_opacity`, and `update_cursor_trail_needs_render`.
Alacritty mirrors these with one interpolated cursor geometry: movement uses Kitty's exponential
ease-out step `1 - exp2(-10 * dt / decay)`, rather than drawing a row of fixed historical cells.

## Alacritty targets

- `alacritty/src/display/cursor.rs`: cursor geometry and trail state helpers.
- `alacritty/src/display/mod.rs`: update, damage, and draw ordering.
- `alacritty/src/display/damage.rs`: old/new trail damage.
- `alacritty/src/event.rs`: animation redraw deadline.
- `alacritty/src/renderer/mod.rs`: trail draw entry point.
- `alacritty/src/renderer/shader.rs`: shader program lifecycle.
- `alacritty/src/config/ui_config.rs`: configuration.
- `alacritty/res`: GLES2/GLSL3 shader resources.

## Configuration target

Initial configuration is disabled by default and follows Alacritty's existing schema. Candidate fields:

```yaml
cursor:
  trail:
    enabled: false
    [cursor.trail.decay]
    fast = 80
    slow = 160

    [cursor.trail.start_threshold]
    horizontal = 1
    vertical = 1
    color: None
```

The exact nesting and types must match the current Alacritty config conventions before implementation.

## Milestones

1. Baseline and source audit — complete.
2. Worktree and session log — complete.
3. Pure cursor-trail state and config validation — complete.
4. Geometry and easing tests — complete.
5. GPU shader program and GLES2/GLSL3 resources — complete.
6. Display draw ordering and local damage — complete.
7. Animation scheduling and idle behavior — complete.
8. Debug logging, visual checks, and benchmarks — complete.
9. Compatibility cleanup and final review — complete.

## Architecture

### 1. Data Flow

```text
RenderableCursor (from terminal grid)
       │
       ▼
CursorTrailState::update (display/cursor.rs)
  ├─ Directional dot-product per corner
  ├─ Exponential ease-out: 1.0 - exp2(-10.0 * dt / decay)
  ├─ Threshold check & corner snap on finish
  └─ Sets clearing_frames = 3 upon animation end
       │
       ▼
CursorTrailState::damage_rect (display/cursor.rs)
  ├─ Unions previous bounding box with current bounding box (+1px anti-aliasing margin)
  └─ Injects damage into damage_tracker.frame() & next_frame()
       │
       ▼
Renderer::draw_cursor_trail (renderer/mod.rs)
  ├─ Uploads 4 NDC corner vertices (32 bytes) into dynamic VBO
  ├─ Binds shader program (trail.v.glsl, trail.f.glsl)
  ├─ Sets blend mode: gl::BlendFunc(gl::ONE, gl::ONE_MINUS_SRC_ALPHA)
  ├─ Fragment shader masks out active cursor area
  └─ Draws via gl::DrawArrays(gl::TRIANGLE_FAN, 0, 4)
       │
       ▼
swap_buffers_with_damage (display/mod.rs)
  └─ Wayland compositor receives union damage rect, composites clean buffers
```

### 2. Components Breakdown

- **Config Layer (`alacritty/src/config/cursor.rs`)**:
  - `CursorTrail` struct with `enabled`, `decay` (`fast`, `slow` in ms), `start_threshold` (`horizontal`, `vertical` in cells).
- **State Machine & Physics (`alacritty/src/display/cursor.rs`)**:
  - `CursorTrailState` holds 4 corner coordinates, previous cursor edges, timestamps, animation status, and damage union tracking.
  - Ports Kitty's corner decay math 1:1, varying decay rate per corner based on angle to target center.
- **Display Loop & Damage Tracking (`alacritty/src/display/mod.rs`)**:
  - Integrates trail update right after grid text cells are rendered and before standard cursor rectangles.
  - Unions damage across frames and maintains a 3-frame clearing countdown so multi-buffered swapchains never retain stale quads.
- **Event Scheduling (`alacritty/src/event.rs`, `scheduler.rs`, `window_context.rs`)**:
  - Introduces `Topic::CursorTrail` timer event driving 60 FPS compositor frames even when the terminal PTY is idle.
- **GPU Renderer Pipeline (`alacritty/src/renderer/mod.rs`, `alacritty/res/trail.*.glsl`)**:
  - `TrailRenderer` with dedicated vertex array, dynamic buffer, and compiled shaders.
  - Zero CPU compositing, zero texture readbacks, 32-byte dynamic attribute upload.

## Non-goals for this port

- Pixman fallback.
- Kitty image protocol.
- Particle systems or fullscreen blur.
- Search-bar and vi-mode cursor trails in the first version.
