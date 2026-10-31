# shdk_alacritty

just my personal fork of [alacritty](https://github.com/alacritty/alacritty) where i slapped together a few things i wanted for my own setup.

basically:
- ported the gpu cursor trail from kitty because it looks neat
- added kitty graphics protocol support so stuff like yazi image previews actually work
- borrowed ideas from ghostty's decoupled cell architecture so it doesn't mess up text rendering or bloat memory

---

## a quick heads up (read this first)

this thing is pretty much pure vibe coding and a weekend side project.
- only tested on linux (specifically arch linux on my own daily setup). haven't tested this on any other distro, let alone macos or windows
- there will be bugs in certain edge cases, random glitches, or unhandled sequences
- i made this primarily for myself, so expect rare to zero updates because honestly i'm too lazy to actively maintain or baby this repo
- if it breaks or panics on your machine, you're pretty much on your own (or feel free to fork it and patch it yourself)
- don't expect upstream alacritty PRs here; it is what it is

---

## what's inside

### 1. kitty cursor trail
ported the 4-corner directional easing math from kitty directly into an opengl shader. you can adjust the decay speed and jump thresholds in the config.

### 2. kitty graphics protocol
handles image placement sequences and unicode placeholders (`U=1`). tools like yazi can render image previews inside the terminal without crashing or spitting out tofu boxes.

### 3. ghostty-style cell layout
instead of packing heavy image structures into every cell, images live in a separate texture store. this prevents memory bloat in huge scrollbacks and stops text from randomly vanishing or breaking subpixel font blending.

---

## config snippet

if you want to turn on the cursor trail in your `alacritty.toml`:

```toml
[cursor]
style = { shape = "Block", blinking = "Off" }

[cursor.trail]
enabled = true

[cursor.trail.decay]
fast = 100
slow = 400

[cursor.trail.start_threshold]
horizontal = 2
vertical = 2
```

for more details and technical notes, check out:
- [kitty features & config guide](docs/kitty-features-guide.md)
- [graphics architecture notes](docs/graphics-architecture.md)

---

## building it

standard rust/cargo stuff:

```bash
cargo build --release
./target/release/alacritty
```

---

## license

This project is licensed under the **GNU General Public License v3.0 (GPL-3.0)**.

### Upstream Attributions & Licensing Breakdown
- **[Alacritty Core](https://github.com/alacritty/alacritty):** Original codebase copyright (c) 2017-2024 Alacritty contributors, originally distributed under the Apache License 2.0 and MIT License (see `LICENSE-APACHE` and `LICENSE-MIT`).
- **[kitty](https://github.com/kovidgoyal/kitty) Components:** GPU cursor trail easing formulas, decay math, and Kitty graphics protocol parser copyright (C) Kovid Goyal and kitty contributors, licensed under the GNU General Public License v3.0 (GPL-3.0).
- **[Ghostty](https://github.com/ghostty-org/ghostty) Architecture:** Decoupled cell layout and out-of-band texture cache model inspired by Ghostty, copyright (c) Mitchell Hashimoto and Ghostty contributors (MIT).

Under Section 5 of the GNU General Public License v3.0, combining GPL-3.0 ported code with Apache-2.0/MIT code requires the unified combined work to be distributed under **GPL-3.0**. Full legal text, copyright notices, and component breakdown are detailed in [**`LICENSE.md`**](LICENSE.md).
