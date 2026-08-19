# Rilis ZPos Kasir vX.Y.Z — Instruksi Agent Hermes (portabel, lintas komputer)

Cara RILIS (bukan cuma build) utk agent Hermes di komputer mana pun. Jalur UTAMA
= **GitHub Actions `release.yml`** (build + sign + upload release, sekali trigger,
tanpa perlu key lokal maupun komp Windows). Build manual = opsi cadangan (bawah).

**Aturan emas: JANGAN generate private key baru.** Key Tauri tersimpan di **GitHub
Secrets** dan HARUS SAMA dengan rilis sebelumnya (syarat auto-update jalan dari
versi lama tanpa reinstall). Kalau kamu generate key baru, `latest.json` punya
signature yang tak cocok dengan pubkey bawaan app lama → auto-update PUTUS.

---

## ⚡ RILIS VIA CI (jalur utama — direkomendasikan)

Workflow `.github/workflows/release.yml` (`on: workflow_dispatch`) SAHABAT melakukan
SEMUA: build di runner Windows → sign setup.exe dgn key dari Secrets → isi
`latest.json` → buat/isi GitHub release v`<ver>` → upload 3 asset → verify.

**Trigger via curl** (PAT utk dispatch; tak perlu key di mesin ini):
```bash
curl -sS -X POST -H "Authorization: Bearer <PAT>" -H "Content-Type: application/json" \
  -d '{"ref":"main","inputs":{"version":"0.1.47"}}' \
  https://api.github.com/repos/djvpri/zpos_windows/actions/workflows/release.yml/dispatches
```
**ATAU lewat UI:** repo → **Actions** → **release** → **Run workflow** → isi `version` (tanpa awalan `v`) → Run.

**Sebelum rilis: verifikasi version bump uda di-commit di main** (3 file konsisten —
lihat checklist). Workflow build dari HEAD main.

**Hasil:** release `v<version>` + 3 asset (`zpos-kasir.exe`, `ZPos.Kasir_<_ver>...setup.exe`,
`latest.json` ber-signature valid). Verifikasi otomatis di step terakhir workflow.

> **Setup sekai (owner — sudah dilakukan utk key & pass):** 3 GitHub Secrets di
> repo Settings → Secrets → Actions:
> - `TAURI_SIGNING_PRIVATE_KEY` (isi file key = satu baris 348B, `untrusted comment:...`)
> - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (passphrase)
> - `GH_PAT` (PAT scope `repo`/`content:write` utk buat/upload release)
> Tanpa 3 secret ini, workflow release gagal di step sign/upload.
> ⚠️ JANGAN commit `latest.json` ber-signature ke repo (runtime asset; di-update
> via release).

---

## 🔧 Build exe manual (opsi cadangan — bila CI tak bisa / mau verifikasi lokal)

Hanya butuh Windows (runner atau komp). **Tak butuh key.**

```bat
cd zpos_windows\src-tauri
cargo tauri build --bundles nsis
```
Hasil:
- `src-tauri\target\release\zpos-kasir.exe` (portable, ~14 MB)
- `src-tauri\target\release\bundle\nsis\ZPos Kasir_<ver>_x64-setup.exe` (~4 MB)

⚠️ Di WSL/Linux `cargo build` gagal (`gobject-2.0` / `pkg-config`) DAN **merewrit
`Cargo.lock`** dgn paket Linux/Wayland (ajukan `git checkout src-tauri/Cargo.lock`
utk buang). Build harus di Windows.

---

## ✍️ Sign manual (bila build manual — tak perlu di jalur CI)

Kunci root-cause: `-f/--private-key-path` (path FILE), **BUKAN** `-k` (string).

```bash
cargo tauri signer sign \
  --private-key-path /path/to/.zpos_updater.key \
  --password "$(cat /path/to/.zpos_updater_pass)" \
  "/abs/path/ZPos Kasir_<ver>_x64-setup.exe"
```
- `-k/--private-key` = isi key sebagai STRING → kalau diberi path, FAIL
  `failed to decode base64 key: Invalid symbol ...` walau key valid.
- Output `<setup>.exe.sig` = SATU baris base64 (396–424 B) = nilai `signature`.
- Masih `Invalid symbol` setelah pakai `--private-key-path` → file key rusak/bukan
  pasangan pubkey (bukan soal flag).

---

## 📦 Isi latest.json + upload (manual — otomatis di jalur CI)

- `"signature"` = **RAW isi file `.sig`** (baca `.strip()`), simpan verbatim.
  JANGAN `base64.b64encode(...)` lagi → double-encode → updater gagal
  (`Invalid encoding in minisign data`).
- `"url"` = **dotted** `.../releases/latest/download/ZPos.Kasir_<ver>_x64-setup.exe`
  (titik, bukan spasi — GitHub ganti spasi→titik saat upload).
- Upload 3 asset (`zpos-kasir.exe`, `ZPos Kasir_<ver>_x64-setup.exe` nama asli,
  `latest.json`) ke release id via upload API.

---

## ✅ Verifikasi final

1. `GET /releases/<id>` → 3 asset lengkap.
2. Baca `latest.json` via **API octet-stream** (asset id; jangan `releases/download`
   karena CDN cache delay) → version=v<ver>, url dotted, `signature` TERISI
   (bukan `GANTI_`), satu baris, `base64.b64decode(sig).decode().startswith('untrusted comment:')`.

---

## 📋 Checklist cepat
- [ ] HEAD = main; version bump di **3 file** konsisten: `src-tauri/Cargo.toml`,
      `src-tauri/tauri.conf.json`, `src-tauri/Cargo.lock`.
- [ ] Jalur utama: trigger `release.yml` (workflow_dispatch) dgn `version` → build+sign+upload+verify otomatis.
- [ ] Key pakai GitHub Secrets — JANGAN generate baru (jaga auto-update).
- [ ] Kalau build manual: sign `--private-key-path` (bukan `-k`); latest.json signature = RAW `.sig`, url dotted.
- [ ] Identity git: `djvpri <sentarummedia@gmail.com>` (kalau clone baru).
