# ZPos Kasir — Verification Harness

Deterministik Playwright Chromium terhadap frontend `src/index.html` yang `__TAURI__`-nya
di-mock, membaca data dari SQLite fixture. Pola diterapkan dari ink-tide `__INKTIDE__`:
screenshot bukti visual + assertion numerik/deterministik + kontrol waktu (autoSync
dipotong 60s→50ms) agar run idempotent.

## Install (sekali)
```bash
cd harness
npm install
npx playwright install chromium   # unduh browser sekali
```

## Jalankan
```bash
node run.mjs            # semua scene → PASS/FAIL, exit 0/1
node run.mjs login      # satu scene: login | member | update | login_fail | schema
```
Screenshot bukti ditulis ke `harness/shots/*.png`. Selalu `PASS 50 assertion` saat
regression. Exit 0 = hijau, 1 = merah.

## Scene (kontrak yang dijaga)
- `login`       — login PIN sukses → badge kasir; render katalog dari fixture; hitung total cart.
- `member`      — fix 0.1.17: dropdown kategori member terisi dari fixture (diskon & markup %).
- `update`      — fix 0.1.18: frontend kirim `{payload:{targetPath, versiBaru}}` (bukan arg datar)
                 ke `terapkan_update` — guard regresi "missing required key payload".
- `login_fail`  — PIN salah → pesan error tampil.
- `schema`      — drift-check: SKEMA fixture selaras dgn skema backend `src-tauri/src/db.rs`.

## Arsip (tak di-commit)
- `harness/node_modules/` `harness/shots/` `harness/.fixtures/` — gitignored (buatan run).

## Catatan
Screenshot TIDAK di-assert byte-exact antar run (font/AA rendering Chromium headless
bervariasi) — sama spt cel-shader ink-tide yg tak image-exact antar GPU. Bukti =
assertion DOM deterministik (50/50 stabil); screenshot = artefak visual utk manusia.
