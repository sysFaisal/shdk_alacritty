# Hybrid Kitty Graphics Protocol & Ghostty Architecture Specification

This document details the architectural integration of the Kitty Graphics Protocol with a decoupled, Ghostty-inspired cell synchronization model inside Alacritty, along with visual pipeline fixes and performance safeguards.

---

## 1. Overview & Architectural Motivation

Upstream Alacritty is engineered for minimal latency and strict terminal standards, but lacks native image graphics protocols. Naively bolting graphics onto the terminal grid often introduces two major problems:

1. **Grid Bloat & Memory Overhead:** Expanding `struct Cell` to carry image references increases the memory footprint across large scrollback buffers.
2. **Visual Inconsistencies & Texture Leaks:** OpenGL state conflicts between image blending (`SRC_ALPHA, ONE_MINUS_SRC_ALPHA`) and Alacritty's font Dual-Source Blending (`SRC1_COLOR, ONE_MINUS_SRC1_COLOR`) can cause font corruption and texture cache poisoning.

To solve this, a hybrid architecture was designed:
- **Kitty Graphics Protocol:** High-performance direct image streaming and Unicode placeholder (`U=1`) support.
- **Ghostty Decoupled Storage:** Out-of-band texture storage that keeps grid cells slim (24 bytes) while reliably synchronizing image tiles with text reflow.

---

## 2. Decoupled Pipeline Design

```text
Incoming PTY Escape Sequences (\x1b_G... \x1b\)
               │
               ▼
┌─────────────────────────────────────────────────────────────┐
│ 1. Decoupled Image Storage (alacritty_terminal/graphics.rs) │
│    - images: HashMap<u32, StoredImage> (Texture/Pixel Cache) │
│    - virtual_placements: HashMap<u32, VirtualPlacement>     │
│    - placements: Vec<ImagePlacement> (Direct placement crop) │
└──────────────────────────────┬──────────────────────────────┘
                               │
       ┌───────────────────────┴───────────────────────┐
       ▼                                               ▼
[Mode Direct (U=0)]                         [Mode Placeholder (U=1)]
Direct placement with UV crop               Grid cells hold U+10EEEE
                                            + Diacritics (row/col)
                                                       │
                                                       ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. Text Grid Pass (alacritty/src/display/mod.rs)            │
│    - Suppress U+10EEEE -> converted to ' ' (suppress font)  │
│    - Decode image_id (from cell.fg RGB)                      │
│    - Decode row & col tile (via diacritic_to_num)           │
│    - Zero bloat: struct Cell stays 24 bytes                 │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Isolated GPU Pass (alacritty/src/renderer/mod.rs)        │
│    - gl::Viewport(0, 0, width, height)                      │
│    - Blend: SRC_ALPHA, ONE_MINUS_SRC_ALPHA                  │
│    - Draw Image Quads & Tiles via image.v.glsl / image.f    │
│    - RESTORE Font Blend: SRC1_COLOR, ONE_MINUS_SRC1_COLOR   │
│    - RESET active_tex = 0 (prevents font transparent bug)   │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. Key Pipeline Fixes

### A. Subpixel Font Anti-Aliasing Protection
- **Root Cause:** Alacritty relies on OpenGL Dual-Source Blending (`SRC1_COLOR, ONE_MINUS_SRC1_COLOR`) for subpixel font anti-aliasing in GLSL3. If the image pass leaves standard alpha blending active, subsequent text rendering exhibits solid background boxes and corrupted glyph edges.
- **Solution:** Enforce strict OpenGL state restoration immediately after `ImageRenderer::draw`, resetting blend modes back to Dual-Source Blending and re-applying the terminal viewport.

### B. Texture Cache State Isolation
- **Root Cause:** Both `Glsl3Renderer` and `Gles2Renderer` cache the currently bound OpenGL texture ID (`self.active_tex`). When `ImageRenderer` unbinds textures (`gl::BindTexture(gl::TEXTURE_2D, 0)`), the text renderer's cache became desynchronized. It skipped binding the font atlas on subsequent passes, reading empty texture data and rendering text 100% transparent.
- **Solution:** Explicitly invoke `reset_active_tex()` at the start of each text rendering pass and immediately following any image rendering invocation.

### C. Unicode Placeholder (`U=1`) Handling & Missing Glyph Prevention
- **Root Cause:** Terminal file managers like [Yazi](https://github.com/sxyazi/yazi) print `U+10EEEE` with combining diacritics to mark image grid cells. Unhandled, FreeType attempts to rasterize this private-use codepoint, displaying missing-glyph boxes (`■`).
- **Solution:** During the grid presentation pass in `display/mod.rs`, cells with `U+10EEEE` are detected, image ID and row/col coordinates are extracted, and the character is replaced with a blank space (`' '`) with zero-width diacritics stripped before reaching FreeType.

### D. Sub-Rectangle Cropping & UV Normalization
- **Root Cause:** Hardcoded UV coordinates (`0.0..1.0`) cause cropped image slices to stretch and repeat as distorted horizontal strips.
- **Solution:** Compute precise normalized UV coordinates in `Renderer::draw_images`:
  ```rust
  let u0 = placement.src_x as f32 / img.width.max(1) as f32;
  let v0 = placement.src_y as f32 / img.height.max(1) as f32;
  let u1 = (placement.src_x + src_w) as f32 / img.width.max(1) as f32;
  let v1 = (placement.src_y + src_h) as f32 / img.height.max(1) as f32;
  ```

---

## 4. Unicode Diacritic Mapping

Row and column indexing in Kitty's Unicode placeholder protocol uses combining diacritics mapped according to Kitty's 310-entry codepoint table (`diacritic_to_num`). This ensures 100% protocol compliance and accurate grid alignment for multi-tile image displays.

---

## 5. Performance Optimizations & Production Safeguards

### A. Batch Quad Rendering
- **Problem:** Individual `glDrawArrays` and `glBufferSubData` calls per placeholder tile generated thousands of draw calls per frame on large image previews.
- **Solution:** Quads are batched per unique OpenGL texture. All tiles and placements sharing the same image are uploaded into a single dynamic vertex buffer and rendered in **one single `glDrawArrays(gl::TRIANGLES, ...)` call per texture**.

### B. Lazy GPU VRAM Texture Upload
- **Problem:** Eagerly uploading every stored image to GPU VRAM caused unbounded texture memory usage when scrolling or loading multiple files in Yazi.
- **Solution:** Only textures for images currently visible in the active viewport (or referenced by current placeholder tiles) are uploaded to OpenGL VRAM. Offscreen images remain in host RAM until needed.

### C. Z-Index Layering (UnderText vs OverText Passes)
- Kitty protocol specifies that images with `z_index < 0` must render underneath terminal text, while `z_index >= 0` render above text.
- `Renderer::draw_images` runs in two passes: `ImageZPass::UnderText` (before `draw_cells`) and `ImageZPass::OverText` (after `draw_cells`), ensuring accurate visual stacking.

### D. LRU Memory Cap & Parser Safeguards
- A 256MB memory cap with LRU eviction prevents unbounded RAM growth in `GraphicsState`.
- `GraphicsStreamParser` enforces a 128MB maximum payload buffer to guard against malformed or unterminated escape sequences.
- Cursor trail delta time (`dt`) is clamped between 1ms and 50ms to eliminate visual spikes during background tab switching.
