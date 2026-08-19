# INSTRUKSI HERMES — Tanda tangani & selesaikan rilis v0.1.46 ZPos

Konteks: rilis ZPos windows v0.1.46. Commit bump + tag `v0.1.46` sudah di
`main` branch repo `djvpri/zpos_windows`. Biner (`zpos-kasir.exe` dan
`ZPos.Kasir_0.1.46_x64-setup.exe`) sudah TERUPLOAD di release GitHub
`djvpri/zpos_windows` tag `v0.1.46`. Yang tersisa: `latest.json` updater
manifest — butuh signature dari PRIVATE KEY Tauri yang ada di laptop ini.

File yang kamu lihat sekarang (dari repo):
- `release/v0.1.46/latest.json.template` — isi placeholder signature
- `release/v0.1.46/INSTRUKSI-HERMES.md` — file ini

Setup exe yang perlu di-sign BELUM di repo (binary besar, sudah ada di release
GitHub). Unduh dulu dari sini:
`https://github.com/djvpri/zpos_windows/releases/download/v0.1.46/ZPos.Kasir_0.1.46_x64-setup.exe`

## Langkah

### 1. Unduh setup exe 0.1.46
Dari link di atas, simpan sebagai `ZPos Kasir_0.1.46_x64-setup.exe` di folder
sementara, misal `C:\Z1POS_RLS\`.

### 2. Temukan private key Tauri
Cari di laptop ini:
```powershell
dir -Recurse -Filter *.pem -Path $HOME 2>$null | Select-Object -First 20 FullName
dir -Recurse -Filter *.key -Path $HOME 2>$null | Select-Object -First 20 FullName
# jalur umum: C:\Users\<user>\.tauri\tauri.key  atau  *.pem
```
Kalau ketemu, catat FullName. Kalau TIDAK ada: beri tahu user key hilang →
perlu `tauri signer generate` + ganti pubkey di `src-tauri/tauri.conf.json` +
REBUILD app (berat, minta rencana dulu, jangan langsung).

### 3. Generate signature
Dari folder `src-tauri` repo zpos_windows (ganti `<path-key>`):
```powershell
cd <lokasi-repo>\src-tauri
cargo tauri signer sign -k "<path-key>" "C:\Z1POS_RLS\ZPos Kasir_0.1.46_x64-setup.exe"
```
Output = SATU baris base64 signature (mulai `dU5...`). Simpan ke variabel/salin.

### 4. Isi latest.json dari template
Baca `latest.json.template` di repo ini (`release/v0.1.46/`). Ganti placeholder
`GANTI_INI_DENGAN_SIGNATURE_DARI_tauri_signer` dengan signature base64 langkah 3
(persis, tanpa kutip tambahan). Validasi: JSON harus parse. Simpan sebagai
`C:\Z1POS_RLS\latest.json`.

### 5. Upload latest.json sebagai release asset
Repository, release id v0.1.46 = `372846959`, repo = `djvpri/zpos_windows`.
Butuh GitHub PAT (minta ke user bila belum ada).
```powershell
curl.exe -X POST -H "Authorization: Bearer <PAT>" -H "Content-Type: application/json" `
  --data-binary "@C:\Z1POS_RLS\latest.json" `
  "https://uploads.github.com/repos/djvpri/zpos_windows/releases/372846959/assets?name=latest.json"
```
Sukses → HTTP 201. Verify: `GET .../releases/372846959` → asset `latest.json` ada.

### 6. Verifikasi akhir
Buka `https://github.com/djvpri/zpos_windows/releases/tag/v0.1.46` → harus ada 3
asset: `zpos-kasir.exe`, `ZPos.Kasir_0.1.46_x64-setup.exe`, `latest.json`.
`latest.json` → `signature` TERISI (bukan placeholder), `url` menunjuk setup 0.1.46.

## Catatan
- URL updater yang dicek app:
  https://github.com/djvpri/zpos_windows/releases/latest/download/latest.json
- JANGAN commit `latest.json` (yang signature sudah terisi) ke repo — ini runtime
  asset release, bukan source. Sekali di-upload ke release, app yang terpasang ambil dr sana.
- JANGAN commit private key / PAT ke repo.
- `release/v0.1.46/*` di repo BISA di-commit (template & instruksi, tanpa secret).
