# Hybrid Kitty Graphics Protocol & Cursor Trail Implementation Log

Dokumen ini mencatat seluruh implementasi arsitektur hybrid (referensi Ghostty & Kitty), perbaikan bug visual, status progres saat ini, serta panduan lengkap untuk sesi lanjutan (baik developer maupun AI agent di sesi baru).

> [!IMPORTANT]
> **ATURAN REPOSITORY:**
> - **JANGAN DI-COMMIT ATAU DI-PUSH** ke remote git publik (upstream Alacritty) tanpa izin pengguna, agar tidak mengotori repositori github asli.
> - Seluruh perubahan saat ini berada di working directory branch `cursor-trail-port`.

---

## 1. Ringkasan Masalah Sebelumnya & Penyebabnya

Pengguna melaporkan tampilan menjadi rusak/aneh saat memasang porting cursor trail & Kitty image protocol:
1. **Teks / Font Rusak (Garis-garis hitam, kotak padat, font pecah):**
   * *Akar masalah:* `ImageRenderer::draw` mengaktifkan `gl::BlendFunc(SRC_ALPHA, ONE_MINUS_SRC_ALPHA)` dan **tidak pernah mengembalikannya**. Alacritty menggunakan *Dual-Source Blending* (`SRC1_COLOR, ONE_MINUS_SRC1_COLOR`) untuk subpixel antialiasing font FreeType di GLSL3. State yang bocor ini membuat teks di frame berikutnya rusak.
2. **Preview Gambar Bergaris-garis / Strips Terulang:**
   * *Akar masalah:* Koordinat UV tekstur di-hardcode `0.0..1.0`. Saat aplikasi (seperti Yazi) mengirim placement irisan/sub-rectangle baris, seluruh gambar dipaksa masuk ke 1 baris grid.
3. **Karakter Kotak Hilang (`■`) di UI / Nama File:**
   * *Akar masalah:* Yazi menggunakan mode **Unicode Placeholder (`U=1`)**. Yazi mencetak karakter `U+10EEEE` ke teks buffer. Alacritty tidak meng-handle karakter ini dan membiarkan FreeType merendernya sebagai font teks biasa (menghasilkan kotak tofu missing glyph).
4. **Teks Menjadi Tak Terlihat / Hilang Setelah Gambar Muncul (*Texture Cache Poisoning*):**
   * *Akar masalah:* `Glsl3Renderer` dan `Gles2Renderer` memiliki cache internal `self.active_tex: GLuint`. Jika `*self.active_tex == self.batch.tex()`, renderer melewati `gl::BindTexture`. Saat `ImageRenderer` meng-unbind tekstur (`gl::BindTexture(gl::TEXTURE_2D, 0)`), `self.active_tex` di renderer teks tidak di-reset. Pada frame berikutnya, renderer teks mengira atlas font masih ter-bind, sehingga pass 1 shader teks membaca tekstur kosong (alpha 0) dan membuat seluruh teks menjadi 100% transparan/hilang.
   * *Solusi:* Reset `self.active_tex = 0` di awal setiap pass `with_api` pada [`glsl3.rs`](file:///home/sahaduka/forking/alacritty_cus/alacritty/src/renderer/text/glsl3.rs) & [`gles2.rs`](file:///home/sahaduka/forking/alacritty_cus/alacritty/src/renderer/text/gles2.rs), dan panggil [`reset_active_tex()`](file:///home/sahaduka/forking/alacritty_cus/alacritty/src/renderer/mod.rs#L210) setelah `draw_images`.

---

## 2. Arsitektur Hybrid yang Telah Diterapkan (Ghostty-style)

Pendekatan hybrid memisahkan tanggung jawab secara bersih (*separation of concerns*):

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
│    - Suppress U+10EEEE -> diubah jadi ' ' (tanpa font draw) │
│    - Decode image_id (dari cell.fg RGB)                      │
│    - Decode row & col tile (via diacritic_to_num)           │
│    - Zero bloat: struct Cell tetap 24 byte                  │
└──────────────────────────────┬──────────────────────────────┘
                               │
                               ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Isolated GPU Pass (alacritty/src/renderer/mod.rs)        │
│    - gl::Viewport(0, 0, width, height)                      │
│    - gl::BlendFunc(SRC_ALPHA, ONE_MINUS_SRC_ALPHA)          │
│    - Dynamic UV mapping untuk direct placement & tiles      │
│    - Selalu pulihkan gl::BlendFunc(SRC1_COLOR, ...)         │
│    - Selalu pulihkan self.set_viewport(size_info)           │
└─────────────────────────────────────────────────────────────┘
```

---

## 3. Detail File yang Dimodifikasi

### A. [`alacritty_terminal/src/graphics.rs`](file:///home/sahaduka/forking/alacritty_cus/alacritty_terminal/src/graphics.rs)
1. **Crop Coordinates:**
   - Menambahkan field `src_width` dan `src_height` pada [`GraphicsCommand`](file:///home/sahaduka/forking/alacritty_cus/alacritty_terminal/src/graphics.rs#L39).
   - Parsing parameter `"w"` dan `"h"` pada parser APC control string.
   - Menambahkan field `src_x`, `src_y`, `src_w`, `src_h` pada [`ImagePlacement`](file:///home/sahaduka/forking/alacritty_cus/alacritty_terminal/src/graphics.rs#L179).
2. **Virtual Placement Storage:**
   - Menambahkan struct [`VirtualPlacement { columns, lines }`](file:///home/sahaduka/forking/alacritty_cus/alacritty_terminal/src/graphics.rs#L193) dan [`PlaceholderTile { point, image_id, row, col }`](file:///home/sahaduka/forking/alacritty_cus/alacritty_terminal/src/graphics.rs#L199).
   - Menambahkan `virtual_placements: HashMap<u32, VirtualPlacement>` pada [`GraphicsState`](file:///home/sahaduka/forking/alacritty_cus/alacritty_terminal/src/graphics.rs#L207).
   - Pada `a=T,U=1` atau `a=p,U=1`, koordinat virtual placement disimpan tanpa membuat placement kursor absolut duplikat.
3. **Canonical Diacritic Decoding:**
   - Mengimplementasikan fungsi [`diacritic_to_num(code: u32) -> Option<usize>`](file:///home/sahaduka/forking/alacritty_cus/alacritty_terminal/src/graphics.rs#L827) yang memetakan karakter Unicode combining mark (seperti `\u{0305}`, `\u{030d}`, dll.) menjadi indeks 0-based baris/kolom secara presisi sesuai tabel resmi Kitty (`kitty/gen/rowcolumn-diacritics.txt`).
4. **Unit Tests:**
   - Menambahkan unit test untuk decoding diacritics dan virtual placement parsing.

### B. [`alacritty/src/renderer/mod.rs`](file:///home/sahaduka/forking/alacritty_cus/alacritty/src/renderer/mod.rs)
1. **GL State Hygiene & Blending Isolation:**
   - Di method [`Renderer::draw_images`](file:///home/sahaduka/forking/alacritty_cus/alacritty/src/renderer/mod.rs#L186):
     ```rust
     gl::Viewport(0, 0, size_info.width() as i32, size_info.height() as i32);
     gl::Enable(gl::BLEND);
     gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
     self.image_renderer.draw(size_info, graphics, display_offset, tiles);
     gl::BlendFunc(gl::SRC1_COLOR, gl::ONE_MINUS_SRC1_COLOR); // WAJIB RESTORE!
     self.set_viewport(size_info);                          // WAJIB RESTORE!
     ```
   - Ini menjamin renderer teks dan kursor tidak pernah terkena polusi blend mode.
2. **Dynamic UV Quad Rendering:**
   - Direct placement menghitung UV `[u0, v0, u1, v1]` berdasarkan `(src_x, src_y, src_w, src_h)` terhadap ukuran asli gambar (`img.width`, `img.height`).
3. **Placeholder Tile Rendering (`U=1`):**
   - Menghitung UV tile per cell berdasarkan `(tile.col, tile.row)` terhadap total kolom/baris virtual placement.
   - Menggambar quad pas di atas cell terkait menggunakan batching bind texture yang efisien.

### C. [`alacritty/src/display/mod.rs`](file:///home/sahaduka/forking/alacritty_cus/alacritty/src/display/mod.rs)
1. **Placeholder Suppression & Interception:**
   - Sebelum cell dikirim ke `self.renderer.draw_cells`:
     ```rust
     if cell.character == '\u{10EEEE}' {
         // Ekstrak image_id dari cell.fg RGB
         // Ekstrak row & col dari diacritic zerowidth
         // Catat ke placeholder_tiles
         cell.character = ' '; // Jangan render glyph!
         cell.extra = None;    // Jangan render floating diacritics!
     }
     ```
   - Mencegah FreeType menggambar kotak `■` dan floating accents pada nama file.
2. **Sinkronisasi Render Images:**
   - Memanggil `self.renderer.draw_images(&size_info, &graphics, display_offset, &placeholder_tiles)`.

---

## 4. Status Progres & Verifikasi

- [x] **Kompilasi:** `cargo check` berhasil tanpa error.
- [x] **Unit Tests:** `cargo test -p alacritty_terminal graphics` lulus 4/4 tes.
- [x] **Build Binary:** `cargo build` berhasil menghasilkan binary di `target/debug/alacritty`.
- [x] **Isolasi GL:** Blending subpixel font FreeType terlindungi dari kebocoran state.
- [x] **Git Safety:** Belum ada commit atau push ke remote publik.

---

## 5. Cara Menjalankan & Menguji untuk Developer / Sesi Baru

Untuk mencoba binary hasil build:
```bash
cd /home/sahaduka/forking/alacritty_cus
./target/debug/alacritty
```

Untuk menguji dengan Yazi:
1. Jalankan `./target/debug/alacritty`.
2. Buka folder gambar dengan `yazi`:
   ```bash
   yazi
   ```
3. Perhatikan:
   - Teks nama file di panel kiri harus bersih (tidak ada kotak `■`).
   - Teks header `~Documents/Wallpapers` harus tajam tanpa garis hitam.
   - Gambar wallpaper di panel kanan harus muncul utuh dan proporsional (bukan irisan berulang).
   - Cursor trailing tetap aktif dan bersih.


---

## 4. Phase 4: Image Re-rendering & Versioning Fix (Dynamic Yazi Switching)

### 4.1 Root Cause of Static Image Bug
Ketika berpindah antar file di file manager (seperti Yazi), Yazi mengirimkan gambar baru dengan ID yang sama atau membersihkan placement lama:
1. **GPU Texture Cache Invalidation:** `ImageRenderer::draw` sebelumnya hanya mengecek `!self.textures.contains_key(id)`. Ketika gambar baru di-transmit dengan ID yang sama (misal `i=1`), CPU `GraphicsState.images` sudah diperbarui (ukuran & byte RGBA serigala 200x200), namun GPU `ImageRenderer` menganggap tekstur ID `1` sudah ada, sehingga melewati `gl::TexImage2D`. Akibatnya, GPU tetap merender isi tekstur lama (gunung 1000x1000) yang distorsi ke dalam dimensi layout preview serigala.
2. **Virtual Placements Cleanup:** Pada `Action::Delete` (`a=d`), `virtual_placements` tidak terhapus pada beberapa jalur fallback delete.
3. **Diacritic MSB Support:** Kombinasi diacritic ke-3 (zerowidth index 2) belum diekstrak untuk ID gambar > 16.7 juta (`image_id |= (msb << 24)`).

### 4.2 Perubahan Implementasi
1. **`StoredImage.version` & Monotonic Versioning (`alacritty_terminal/src/graphics.rs`):**
   - Menambahkan field `pub version: u64` pada `StoredImage`.
   - `GraphicsState` mengelola counter `next_image_version: u64`. Setiap pembaruan atau penambahan gambar menaikkan versi gambar.
2. **Version-Aware Texture Synchronization (`alacritty/src/renderer/mod.rs`):**
   - Struktur tekstur GPU diubah menjadi `textures: HashMap<u32, (gl::types::GLuint, u64)>`.
   - Pada awal render loop, jika tekstur GPU memiliki versi lebih rendah daripada `img.version`, Alacritty memanggil `gl::TexImage2D` untuk mengunggah dimensi dan pixel baru secara real-time.
   - Tekstur yang sudah dihapus dari CPU `GraphicsState.images` langsung dilepas dari GPU dengan `gl::DeleteTextures`.
3. **Pembersihan Virtual Placements & Transmit Response (`alacritty_terminal/src/graphics.rs`):**
   - Semua perintah delete `a=d` (baik `d=a`, `d=i`, maupun default) membersihkan `virtual_placements` yang sesuai.
   - Pengiriman gambar dengan `q=0` (quiet) membalas standar Kitty `\x1b_Gi={image_id};OK\x1b\` agar client sinkron.
4. **3rd Diacritic MSB Decoding (`alacritty/src/display/mod.rs`):**
   - Mengekstrak diacritic ke-3 dari combining characters untuk mendukung ID gambar penuh 32-bit.
5. **Unit Tests:**
   - Menambahkan `test_image_versioning_and_deletion` di `alacritty_terminal/src/graphics.rs` (5/5 unit tests passed).
