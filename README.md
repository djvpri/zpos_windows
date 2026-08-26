# ZPos Kasir — aplikasi desktop Windows (Tauri v2)

Kasir offline-first untuk ZPos. Transaksi diproses lokal (SQLite), lalu disinkronkan
ke server ZPos saat online. Berjalan tanpa koneksi; antrian transaksi offline
otomatis terkirim setelah connect.

## Unduh .exe

Pada repo ini: tab **Actions** → run terbaru (hijau) → **Artifacts** →
`zpos-kasir-windows-exe` → unzip → jalankan `zpos-kasir.exe`.
Portable — tak perlu install, simpan di mana saja.

## Setup (sekali)

1. Buka app. Klik **Setelan** (ikon gear) atau tombol **Sinkron**.
2. Isi:
   - **Base URL**: `https://z1pos.zomet.my.id`
   - **Token**: token akses ZPos (mengenali toko & hak kasir).
3. **Simpan & Sinkron** → unduh katalog produk + kategori + member ke perangkat.

Setelah itu katalog tersimpan lokal; kasir jalan offline.

## Cara pakai

- Cari/tap produk → masuk keranjang.
- Pilih metode bayar → **BAYAR** → transaksi diantrikan offline (badge jumlah antrian).
- **Sinkron** (manual) atau tunggu auto-sync (tiap 60 detik) → antrian dikirim ke server
  ZPos, katalog & member diperbarui.
- **Pilih Member** → harga khusus (tetap / diskon % / markup) diterapkan otomatis.
- **Gantung** (membutuhkan member) → simpan keranjang belum dibayar.

## Teknis

- Rust + Tauri v2, SQLite lokal (rusqlite), frontend HTML/CSS/JS polos.
- Model sinkron: pull-online (unduh katalog, push antrian transaksi).
- Endpoint server: `/api/produk`, `/api/kategori`, `/api/member`, `/api/transaksi`
  (auth Bearer token).

## Build Windows

`.exe` dibangun lewat GitHub Actions (`windows-latest`, `cargo tauri build --no-bundle`).
Cara build lokal: `cargo install tauri-cli` lalu `cargo tauri build` (perlu toolchain Windows).

### Build lokal (Windows)

```powershell
# Prasyarat sekali: Rust (rustup.rs) + WebView2 runtime + Tauri CLI
cargo --version          # pastikan Rust terpasang
cargo install tauri-cli --locked

# Dari folder src-tauri:
cargo tauri build --no-bundle     # release portable exe -> target\release\zpos-kasir.exe
cargo run                          # jalankan langsung utk tes (DevTools: klik badge versi kanan-bawah)
```

- Frontend `index.html` polos, `frontendDist: ../src` — **tanpa langkah build frontend**; hasilnya exe murni Rust.
- Build release **pertama lama** (kompilasi puluhan crate + lto), sesudahnya cepat.
- `cargo run` = debug build → startup lambat, tapi DevTools nempel utk lihat JS error.
- Log error: `%APPDATA%\my.id.zpos.kasir.app\zpos-errors.log` (sama utk build lokal maupun CI — identifier sama).
- Lokal di WSL/Linux **gagal** (butuh gtk/webkit system libs, non-root); build hanya jalan di Windows atau CI.
