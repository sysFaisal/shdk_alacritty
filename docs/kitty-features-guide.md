# Kitty Features Guide & Configuration

This guide details all features ported from **Kitty** and adapted from **Ghostty** into this Alacritty fork, along with comprehensive configuration options and unit conversion guidelines.

---

## 1. GPU Cursor Trail (Smooth Easing Cursor Trail)

This feature ports Kitty's cursor movement trail animation rendered directly on the GPU via OpenGL shaders without CPU framebuffer overhead.

### A. Configuration Options & Unit Conversion

| Alacritty Option | Kitty Equivalent | Type & Unit in Alacritty | Unit in Kitty | Explanation & Conversion |
| :--- | :--- | :--- | :--- | :--- |
| `cursor.trail.enabled` | `cursor_trail` | `bool` (`true`/`false`) | `int` (ms timeout) | Toggle the GPU cursor trail animation on/off. |
| `cursor.trail.decay.fast` | 1st value in `cursor_trail_decay` | `u32` (**milliseconds / ms**) | **Seconds (float)** | Fastest corner decay rate.<br>• **`100` ms** = `0.1` sec in Kitty<br>• **`150` ms** = `0.15` sec in Kitty |
| `cursor.trail.decay.slow` | 2nd value in `cursor_trail_decay` | `u32` (**milliseconds / ms**) | **Seconds (float)** | Slowest corner decay rate.<br>• **`400` ms** = `0.4` sec in Kitty<br>• **`550` ms** = `0.55` sec in Kitty |
| `cursor.trail.start_threshold.horizontal` | X in `cursor_trail_start_threshold` | `u32` (**columns / cells**) | **Cells** | Minimum horizontal distance jump before trail ignites. |
| `cursor.trail.start_threshold.vertical` | Y in `cursor_trail_start_threshold` | `u32` (**lines / rows**) | **Cells** | Minimum vertical distance jump before trail ignites. |

> [!NOTE]
> **Why does Alacritty use milliseconds instead of seconds?**  
> All duration settings in Alacritty (e.g. `blink_interval = 750`) use integer milliseconds. Internally, `fast` and `slow` values are converted to seconds (`/ 1000.0`), ensuring mathematical parity with Kitty's physics.

---

### B. Tuning Cursor Motion

The animation step is governed by Kitty's exponential ease-out formula:

$$\text{step} = 1 - 2^{\frac{-10 \cdot \Delta t}{\text{decay}}}$$

- **Slower & Smoother Trail:** Increase `decay` (e.g., `fast = 150, slow = 550`). The trail lingers on screen longer.
- **Snappier & Faster Trail:** Decrease `decay` (e.g., `fast = 80, slow = 250`). The trail dissipates quickly.
- **Start Threshold (`start_threshold`):** Setting `horizontal = 2` prevents normal character typing (1-column advance) from igniting the trail, keeping text input distraction-free while animating larger jumps.

---

### C. Configuration Example in `~/.config/alacritty/alacritty.toml`

```toml
[cursor]
style = { shape = "Block", blinking = "Off" }

[cursor.trail]
enabled = true

# Animation speed (milliseconds):
# 100 ms = 0.1s in Kitty | 400 ms = 0.4s in Kitty
[cursor.trail.decay]
fast = 100
slow = 400

# Minimum jump distance in cells before animating:
[cursor.trail.start_threshold]
horizontal = 2
vertical = 2
```

---

## 2. Kitty Graphics Protocol

This fork implements the Kitty Graphics Protocol, enabling high-resolution image rendering inside terminal applications such as [Yazi](https://github.com/sxyazi/yazi), Neovim image plugins, and CLI dashboards.

### Supported Modes:
- **Direct Placement (`U=0`):** Direct image display with sub-rectangle cropping support.
- **Unicode Placeholders (`U=1`):** Decoupled grid layout with 310-entry Unicode diacritic row/column mapping.
- **Automatic Scrolling:** Images track grid scrolling and display offset changes.

---

## 3. Related Documentation & Architecture References

- **Architecture Specification:** [`docs/graphics-architecture.md`](graphics-architecture.md)
- **Cursor Physics & Geometry:** `alacritty/src/display/cursor.rs`
- **Cursor Trail Shaders:** `alacritty/res/trail.v.glsl` & `alacritty/res/trail.f.glsl`
- **Image Graphics Shaders:** `alacritty/res/image.v.glsl` & `alacritty/res/image.f.glsl`
- **Protocol Parser & State:** `alacritty_terminal/src/graphics.rs`
- **Licensing & Legal Attributions:** [`LICENSE.md`](../LICENSE.md)
