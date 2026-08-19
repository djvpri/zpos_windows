# Rilis ZPos Kasir vX.Y.Z — Instruksi Agent Hermes (portabel, lintas komputer)

Prosedur ini AKURAT untuk agent Hermes yang berjalan di **komputer lain** (bukan
mesin/komp utama), agar bisa build exe + rilis auto-update yang KONSISTEN dengan
rilis sebelumnya. Baca penuh sebelum bertindak. **JANGAN generate private key
baru** — pakai key yang sama dengan rilis sebelumnya (syarat auto-update jalan).

---

## Prasyarat mutlak

1. **Repo**: `djvpri/zpos_windows` → clone lokal, branch `main` (BUILD DARI HEAD,
   bukan branch/cabang lama).
   ```bash
   git clone https://github.com/djvpri/zpos_windows.git
   cd zpos_windows && git fetch origin && git checkout main && git pull
   ```
2. **Private key Tauri + password** — WAJIB SAMA dengan rilis sebelumnya. Kalau
   kamu ada di mesin yang pegang key (`/opt/data/.zpos_updater.key` +
   `/opt/data/.zpos_updater_pass`), pakai langsung. **Kalau TIDAK punya key lokal**:
   gunakan jalur **CI via Secrets** (di bawah) — key tersimpan di GitHub Secrets,
   kamu tak perlu file key fisik.

## RILIS CEPAT VIA CI (kamu tak perlu key lokal / komp Windows)

Workflow `.github/workflows/release.yml` (`on: workflow_dispatch`) membangun exe,
sign dengan secret, upload release — satu trigger:
```
curl -sS -X POST -H "Authorization: Bearer <PAT>" -H "Content-Type: application/json" \
  -d '{"ref":"main","inputs":{"version":"0.1.47"}}' \
  https://api.github.com/repos/djvpri/zpos_windows/actions/workflows/release.yml/dispatches
```
ATAU klik **Actions → release → Run workflow**, isi `version`. Hasil: release `v<version>` + 3 asset (exe, setup.exe, latest.json ber-signature).

**Setup Sekali (owner/Hermes utama, sebelum CI bisa dipakai):** buat GitHub Secrets di repo (Settings → Secrets → Actions):
- `TAURI_SIGNING_PRIVATE_KEY` = isi file `/opt/data/.zpos_updater.key` (KONTEN, bukan path)
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` = isi `/opt/data/.zpos_updater_pass`
- `GH_PAT` = PAT GitHub writer (scope `repo`/`release`)

Tanpa 3 secret di atas, workflow release akan gagal di step sign/upload.

---

## Build exe (Windows, TIDAK butuh key)

Bangun di runner Windows / komputermu:

```bat
cd zpos_windows\src-tauri
cargo tauri build --bundles nsis
```

Hasil:
- `src-tauri\target\release\zpos-kasir.exe` (portable, ~14 MB)
- `src-tauri\target\release\bundle\nsis\ZPos Kasir_<ver>_x64-setup.exe` (~4 MB)

⚠️ Kalau `cargo tauri build` gagal `gobject-2.0` / `pkg-config` → kamu di WSL/Linux.
Build HARUS di Windows (runner `windows-latest` atau komp Windows). Dari WSL jangan
coba `cargo check`/`build` — selain gagal, ia REWRITE `Cargo.lock` dengan paket
Linux/Wayland (pollution; `git checkout src-tauri/Cargo.lock` utk buang).

**Alternatif CI (tanpa komp):** trigger GitHub workflow `build-windows` (file
`.github/workflows/build.yml`, `workflow_dispatch`):
```bash
curl -sS -X POST -H "Authorization: Bearer $PAT" -H "Accept: application/vnd.github+json" \
  -H "Content-Type: application/json" -d '{"ref":"main"}' \
  "https://api.github.com/repos/djvpri/zpos_windows/actions/workflows/build.yml/dispatches"
```
Artifact: `zpos-kasir-release` (berisi exe + setup.exe). Download via
`actions/artifacts` API.

---

## Sign setup.exe — pakai FLAG `-f/--private-key-path` (path FILE), BUKAN `-k`

Satu-satunya penyebab `failed to decode base64 key: Invalid symbol ...` = flag
yang salah: `-k/--private-key` mengharapkan **STRING isi key**, sedangkan kita
punya **path FILE**. Berikan path ke `-f/--private-key-path` (didukung `cargo
tauri` maupun `cargo-tauri`, versi 2.11.4 apa pun):

```bash
cargo tauri signer sign \
  --private-key-path /opt/data/.zpos_updater.key \
  --password "$(cat /opt/data/.zpos_updater_pass)" \
  "/abs/path/ZPos Kasir_<ver>_x64-setup.exe"
```
Output: `<setup>.exe.sig` = **SATU baris base64** (~396–424 B) = nilai `signature`.
Kalau masih `Invalid symbol` setelah pakai `--private-key-path`, itu artinya file
key rusak/bukan pasangan pubkey — bukan soal flag.

---

## Isi latest.json (jangan double-encode)

`"signature"` = **RAW isi file `.sig`** (baca `.strip()`), simpan verbatim.
JANGAN `base64.b64encode(...)` lagi — itu double-encode → updater gagal
(`Invalid encoding in minisign data`).

Gunakan template repo `release/<ver>/latest.json.template`. Isi:
```json
{
  "version": "<ver>",
  "notes": "...",
  "pub_date": "<ISO8601 UTC>",
  "platforms": {
    "windows-x86_64": {
      "signature": "<isi .sig, satu baris>",
      "url": "https://github.com/djvpri/zpos_windows/releases/latest/download/ZPos.Kasir_<ver>_x64-setup.exe"
    }
  }
}
```
- `url` WAJIB nama yang benar-benar tersaji = **dotted** (`ZPos.Kasir_...`, titik,
  bukan spasi). GitHub ganti spasi → titik saat upload.
- Verifikasi cepat: `len(sig)==408..424`, satu baris,
  `base64.b64decode(sig).decode().startswith('untrusted comment:')`.

---

## Upload + terbitkan release

Buat release utk tag `v<ver>` (bila belum ada), upload 3 asset:
`zpos-kasir.exe`, `ZPos Kasir_<ver>_x64-setup.exe` (nama asli BERSPASI di upload
file; GitHub simpan sbg dotted), `latest.json`.

Upload asset (pakai release `id`):
```bash
curl -sS -X POST -H "Authorization: Bearer $PAT" -H "Content-Type: application/json" \
  --data-binary @latest.json \
  "https://uploads.github.com/repos/djvpri/zpos_windows/releases/<id>/assets?name=latest.json"
```

**JANGAN commit `latest.json` ber-signature ke repo** — ia runtime asset; update
via release asset. (Template + instruksi boleh di-commit, tanpa secret.)

---

## Verifikasi final

1. `GET /releases/<id>` → asset lengkap: `zpos-kasir.exe`, setup.exe, `latest.json`.
2. Baca `latest.json` via **API octet-stream** asset id (bukan `releases/download`,
   karena CDN cache delay):
   ```bash
   curl -sSL -H "Bearer $PAT" -H "Accept: application/octet-stream" \
     "https://api.github.com/repos/.../releases/assets/<asset-id>" -o out.json
   ```
   → version=v<ver>, url dotted, `signature` TERISI (bukan `GANTI_`), 1 baris,
   decode-valid.
3. (Opsional) fetch `.../releases/latest/download/latest.json` setelah beberapa
   menit utk konfirmasi CDN.

---

## Checklist cepat
- [ ] HEAD = main, version bump di Cargo.toml + tauri.conf.json + Cargo.lock (3 file, konsisten).
- [ ] Build di Windows (runner/komp) `--bundles nsis` — tanpa key, tanpa CI secret.
- [ ] Key Tauri (`/opt/data/.zpos_updater.key` + pass) ADA di mesin ini — JANGAN generate baru.
- [ ] Sign pakai `~/.cargo/bin/cargo-tauri` (bukan `cargo tauri`).
- [ ] latest.json: signature = RAW `.sig`, url dotted.
- [ ] Upload 3 asset, verifikasi via API (bukan CDN).
- [ ] Identity git: `djvpri <sentarummedia@gmail.com>` (kalau clone baru).
