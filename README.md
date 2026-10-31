<p align="center">
  <img width="180" alt="Alacritty Custom Logo" src="https://raw.githubusercontent.com/alacritty/alacritty/master/extra/logo/compat/alacritty-term%2Bscanlines.png">
</p>

<h1 align="center">Shdk Alacritty</h1>

<p align="center">
  <strong>High-performance, GPU-accelerated terminal emulator enhanced with Kitty Cursor Trails, Kitty Graphics Protocol, and Ghostty-inspired Cell Architecture.</strong>
</p>

<p align="center">
  <a href="#about"><img src="https://img.shields.io/badge/upstream-Alacritty-blue?style=flat-square" alt="Upstream Alacritty"></a>
  <a href="#features"><img src="https://img.shields.io/badge/features-Kitty%20%7C%20Ghostty-orange?style=flat-square" alt="Kitty & Ghostty Features"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-GPL--3.0-green?style=flat-square" alt="GPL-3.0 License"></a>
  <a href="#quick-start"><img src="https://img.shields.io/badge/build-Rust%20%2F%20Cargo-red?style=flat-square" alt="Rust Build"></a>
</p>

---

## 📖 About This Fork

**`shdk_alacritty`** is a specialized fork of [**Alacritty**](https://github.com/alacritty/alacritty).

While upstream Alacritty prioritizes minimal feature complexity, modern terminal workflows frequently benefit from **rich graphics rendering** (e.g. in file managers like [Yazi](https://github.com/sxyazi/yazi), Neovim image plugins, CLI dashboards) and **fluid cursor animations**.

This fork merges the blazing-fast Rust core and OpenGL text pipeline of **Alacritty** with:
1. **Kitty's GPU Cursor Trail:** Native 4-corner directional easing animations rendered via dedicated OpenGL shaders.
2. **Kitty Graphics Protocol:** High-performance pixel streaming with direct placement and Unicode placeholder (`U=1`) support.
3. **Ghostty-Inspired Cell Architecture:** Zero-bloat, decoupled cell grid synchronization keeping memory overhead minimal without breaking subpixel font rendering.

---

## ✨ Key Features & Enhancements

### 🎯 1. Kitty GPU Cursor Trail (Ported from [Kitty](https://github.com/kovidgoyal/kitty))
- **GPU-Warped Easing Quad:** Each corner of the cursor moves independently using Kitty's exponential decay formula with directional dot-product damping.
- **Configurable Dynamics:** Fully tune decay speeds (`fast`, `slow`), distance thresholds, and enable/disable trails on the fly.
- **Wayland & X11 Aware:** Frame-scheduler driven to ensure butter-smooth 60+ FPS animation even when terminal content is completely idle.

### 🖼️ 2. Kitty Graphics Protocol (Ported from [Kitty](https://github.com/kovidgoyal/kitty))
- **Direct Image Placement (`U=0`):** Full support for RGB, RGBA, and PNG streaming escape sequences (`\x1b_G... \x1b\`).
- **Unicode Placeholder Mode (`U=1`):** Complete compatibility with [Yazi](https://github.com/sxyazi/yazi), Neovim plugins, and modern CLI tools using Unicode `U+10EEEE` + diacritic row/column indexing.
- **Terminal Scrolling Integration:** Images stay synchronized with terminal rows and cleanly scroll up with the grid.

### 🏛️ 3. Ghostty-Inspired Decoupled Cell Architecture (Inspired by [Ghostty](https://github.com/ghostty-org/ghostty))
- **Zero Memory Bloat:** Rather than enlarging `struct Cell` with heavy image references, image metadata is stored in an out-of-band texture cache (`HashMap<u32, StoredImage>`).
- **Font Subpixel Blending Preservation:** Fixes texture cache poisoning and protects Alacritty’s Dual-Source Blending (`SRC1_COLOR, ONE_MINUS_SRC1_COLOR`) for crisp FreeType font anti-aliasing.
- **Tofu Glyph Suppression:** Prevents missing-glyph tofu characters (`■`) from appearing when placeholder characters are received.

---

## 🚀 Quick Start

### Prerequisites
- Rust & Cargo (edition 2021+)
- OpenGL / OpenGL ES 2.0+ capable GPU driver
- Fontconfig, FreeType, and standard X11 / Wayland development libraries

### Building & Running

```bash
# Clone the repository
git clone https://github.com/sysFaisal/shdk_alacritty.git
cd shdk_alacritty

# Check and build with release optimizations
cargo build --release

# Run your custom Alacritty
./target/release/alacritty
```

---

## ⚙️ Configuration

Add the following to your `alacritty.toml` (usually located at `~/.config/alacritty/alacritty.toml`):

```toml
[cursor]
style = { shape = "Block", blinking = "Off" }

# Kitty Cursor Trail Configuration
[cursor.trail]
enabled = true

# Fast and slow decay timing in milliseconds
[cursor.trail.decay]
fast = 100
slow = 400

# Minimum movement in cells (horizontal, vertical) required to ignite the trail
[cursor.trail.start_threshold]
horizontal = 2
vertical = 2
```

For comprehensive configuration guides, benchmarks, and conversion notes, see:
- 📖 [**Kitty Features & Configuration Guide**](docs/kitty-features-guide.md)
- 📝 [**Graphics Pipeline & Decoupled Architecture**](docs/graphics-architecture.md)

---

## 🗂️ Repository Structure

```text
shdk_alacritty/
├── alacritty/                  # Frontend, windowing, display loop & OpenGL renderer
│   ├── res/
│   │   ├── image.v.glsl        # Vertex shader for image quads & placeholder tiles
│   │   ├── image.f.glsl        # Fragment shader for texture sampling & alpha
│   │   ├── trail.v.glsl        # Kitty cursor trail vertex shader
│   │   └── trail.f.glsl        # Kitty cursor trail fragment shader
│   └── src/
│       ├── config/cursor.rs    # Cursor trail TOML configuration bindings
│       ├── display/            # Screen damaged tracking, grid pass & cursor animator
│       └── renderer/           # GPU pipelines (text, rects, trails, images)
├── alacritty_terminal/         # Headless terminal emulation core
│   └── src/
│       ├── graphics.rs         # Kitty protocol parser & decoupled image storage
│       ├── event_loop.rs       # Stream interceptor for PTY graphics sequences
│       └── term/               # Terminal grid, scroll sync & graphics handling
├── docs/                       # Technical port logs, architecture & user guides
├── test-config/                # Ready-to-use demo configurations
└── LICENSE.md                  # Comprehensive GPL-3.0 licensing & upstream attributions
```

---

## ⚖️ License & Attributions

Due to the integration of ported algorithms and components from [**kitty**](https://github.com/kovidgoyal/kitty) (licensed under **GPL-3.0**), this combined repository is distributed under the terms of the **GNU General Public License v3.0 (GPL-3.0)**.

- **Alacritty Core:** Copyright (c) 2017-2024 Alacritty contributors (Apache-2.0 / MIT).
- **Kitty Cursor Trail & Graphics Protocol:** Copyright (C) Kovid Goyal and kitty contributors (GPL-3.0).
- **Decoupled Architecture Inspiration:** Inspired by Mitchell Hashimoto and the [Ghostty](https://github.com/ghostty-org/ghostty) project (MIT).

See [**`LICENSE.md`**](LICENSE.md) for full attribution breakdown and licensing text.
