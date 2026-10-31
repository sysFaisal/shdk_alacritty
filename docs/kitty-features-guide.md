# Panduan Fitur Porting Kitty & Konfigurasi (Custom Alacritty)

Dokumen ini menjelaskan seluruh fitur yang telah di-porting dari **Kitty** dan diadopsi dari arsitektur **Ghostty** ke dalam fork Alacritty ini, beserta panduan konfigurasi lengkap dan konversi satuannya.

---

## 1. GPU Cursor Trail (Efek Jejak Ekor Kursor)

Fitur ini memporting animasi jejak pergerakan kursor Kitty yang dirender langsung di GPU menggunakan shader OpenGL tanpa membebani CPU.

### A. Tabel Konfigurasi & Konversi Satuan (Kitty vs Alacritty)

| Konfigurasi Alacritty | Opsi Ekuivalen di Kitty | Tipe & Satuan Alacritty | Satuan di Kitty | Penjelasan & Contoh Konversi |
| :--- | :--- | :--- | :--- | :--- |
| `cursor.trail.enabled` | `cursor_trail` | `bool` (`true`/`false`) | `int` (ms timeout) | Mengaktifkan/menonaktifkan animasi jejak kursor. |
| `cursor.trail.decay.fast` | Nilai ke-1 di `cursor_trail_decay` | `u32` (**milidetik / ms**) | **Detik (float)** | Kecepatan peluruhan tercepat.<br>• **`100` ms** = `0.1` detik di Kitty<br>• **`150` ms** = `0.15` detik di Kitty |
| `cursor.trail.decay.slow` | Nilai ke-2 di `cursor_trail_decay` | `u32` (**milidetik / ms**) | **Detik (float)** | Kecepatan peluruhan terlambat.<br>• **`400` ms** = `0.4` detik di Kitty<br>• **`550` ms** = `0.55` detik di Kitty |
| `cursor.trail.start_threshold.horizontal` | Nilai X di `cursor_trail_start_threshold` | `u32` (**karakter / kolom**) | **Sel (cells)** | Jarak lompatan horizontal minimum agar efek aktif. |
| `cursor.trail.start_threshold.vertical` | Nilai Y di `cursor_trail_start_threshold` | `u32` (**baris**) | **Sel (cells)** | Jarak lompatan vertikal minimum agar efek aktif. |

> [!NOTE]
> **Mengapa Alacritty memakai ms (milidetik) bukan detik?**  
> Semua konfigurasi waktu di Alacritty (seperti `blink_interval = 750`) menggunakan integer milidetik. Di dalam mesin Rust, nilai `fast` dan `slow` dibagi 1000 (`/ 1000.`), sehingga secara fisika dan matematika perhitungannya **100% sama persis dengan Kitty**.

---

### B. Cara Mengatur Kecepatan Gerakan Kursor
Rumus animasi menggunakan *exponential ease-out*:
$$\text{step} = 1 - 2^{\frac{-10 \cdot \Delta t}{\text{decay}}}$$

- **Jika ingin efek LEBIH LAMBAT & HALUS (santai):**  
  **Naikkan** angka `decay` (misal: `fast = 150, slow = 550` atau `fast = 200, slow = 650`). Ekor kursor akan bertahan lebih lama di layar.
- **Jika ingin efek LEBIH CEPAT & RESPONSIF (gesit):**  
  **Turunkan** angka `decay` (misal: `fast = 80, slow = 250`). Ekor kursor akan menghilang sangat cepat.
- **Mengatur Ambang Batas (`start_threshold`):**  
  Jika diatur `horizontal = 2`, mengetik biasa huruf demi huruf (lompat 1 kolom) tidak akan memicu ekor kursor sehingga tidak mengganggu saat mengetik kata. Jejak baru muncul saat kursor melompat $\ge 2$ karakter atau berganti baris.

---

### C. Contoh Konfigurasi di `~/.config/alacritty/alacritty.toml`

```toml
[cursor]
style = { shape = "Beam", blinking = "On" }

[cursor.trail]
enabled = true

# Kecepatan animasi (dalam milidetik / ms):
# 100 ms = 0.1s di kitty | 550 ms = 0.55s di kitty
# Makin besar angkanya, makin lambat & lembut gerakannya.
decay = { fast = 150, slow = 550 }

# Batas minimal pergerakan kursor (dalam jumlah karakter/sel):
# Kursor harus melompat minimal 2 kolom atau 2 baris agar efek muncul.
start_threshold = { horizontal = 2, vertical = 2 }
```

---

## 2. Kitty Graphics Protocol (Preview Gambar di Terminal & Yazi)

Protokol ini memungkinkan aplikasi terminal modern seperti **Yazi**, **lf**, atau script preview untuk menampilkan gambar beresolusi tinggi langsung di dalam grid Alacritty.

### A. Fitur yang Didukung
1. **Dukungan Penuh Yazi (`U=1` Unicode Placeholders):**
   - Mendukung karakter private use `U+10EEEE`.
   - Menggunakan tabel 310 diacritic Kitty (`rowcolumn-diacritics.c`) untuk memetakan baris dan kolom tile gambar.
   - Mendukung diacritic ke-3 (MSB bit 24–31) untuk ID gambar berukuran besar.
   - Karakter placeholder otomatis disembunyikan (*suppressed*) dari FreeType font engine sehingga tidak muncul kotak tofu (`■`).
2. **Arsitektur Hybrid Decoupled (Terinspirasi Ghostty):**
   - Data piksel dan tekstur gambar disimpan terpisah dari grid teks 24-byte Alacritty.
   - Penempatan gambar tersinkronisasi presisi dengan koordinat sel dan cursor viewport.
3. **Dynamic Texture Invalidation (Peralihan Gambar di Yazi):**
   - Memiliki pelacak versi gambar monontonik (`version: u64`).
   - Saat berpindah antar file gambar di Yazi (misal dari gambar 1000x1000 ke 200x200), tekstur GPU langsung di-upload ulang secara instan tanpa bug tekstur lama tersangkut.
4. **Isolasi Blending & Antialiasing Teks:**
   - Rendering gambar memiliki *state isolation* penuh. Blending font atlas (`gl_SecondaryFragColorEXT` dual-source blending) dan unit tekstur font atlas terlindungi sehingga teks terminal tidak akan pudar atau hilang saat ada gambar.

### B. Format Gambar yang Didukung
- **Format Transmisi:** `f=32` (RGBA 32-bit mentah), `f=24` (RGB 24-bit), `f=100` (PNG).
- **Metode Transmisi:** Direct Base64 payload, Chunked transfer (`m=1` / `m=0`).
- **Aksi:** Transmit & Display (`a=T`), Transmit saja (`a=t`), Put (`a=p`), Query (`a=q`), Delete (`a=d`).

---

## 3. Kompatibilitas dengan Generator Tema (`~/.local/bin/gen_style.sh`)

Sistem tema desktop lokal Anda menggunakan `gen_style.sh` untuk memperbarui warna aksen dan font.

- `gen_style.sh` meng-generate file terpisah di `~/.local/state/hakuspace/theme/alacritty-style.toml`.
- File utama Anda `~/.config/alacritty/alacritty.toml` mengimpor file tema tersebut menggunakan `import = [...]`.
- **Kesimpulan:** Menjalankan `gen_style.sh` **tidak akan pernah menimpa atau menghapus konfigurasi `[cursor.trail]`** Anda. Pengaturan kursor Anda 100% aman dan permanen.

---

## 4. Lokasi File & Kode Utama

- Log Perjalanan Teknis Cursor Trail: [`docs/cursor-trail-log.md`](file:///home/sahaduka/forking/alacritty_cus/docs/cursor-trail-log.md)
- Log Perjalanan Teknis Kitty Graphics: [`docs/hybrid-kitty-graphics-log.md`](file:///home/sahaduka/forking/alacritty_cus/docs/hybrid-kitty-graphics-log.md)
- Kode Fisika & State Jejak Kursor: [`alacritty/src/display/cursor.rs`](file:///home/sahaduka/forking/alacritty_cus/alacritty/src/display/cursor.rs)
- Shader Jejak Kursor: [`alacritty/res/trail.v.glsl`](file:///home/sahaduka/forking/alacritty_cus/alacritty/res/trail.v.glsl) & [`alacritty/res/trail.f.glsl`](file:///home/sahaduka/forking/alacritty_cus/alacritty/res/trail.f.glsl)
- Parser & State Kitty Graphics: [`alacritty_terminal/src/graphics.rs`](file:///home/sahaduka/forking/alacritty_cus/alacritty_terminal/src/graphics.rs)
- Renderer GPU Gambar: [`alacritty/src/renderer/mod.rs`](file:///home/sahaduka/forking/alacritty_cus/alacritty/src/renderer/mod.rs)
- Informasi Lisensi Legal: [`LICENSE.md`](file:///home/sahaduka/forking/alacritty_cus/LICENSE.md)
