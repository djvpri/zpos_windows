#!/usr/bin/env bash
# Cek JS inline di src/index.html: sintaks + VARIABEL TAK TERDEKLARASI.
# Latar: node --check hanya menangkap error SINTAKS. Bug runtime seperti
# `ada.add(id)` (variabel yg dihapus saat refactor) LOLOS node --check tapi
# meledak saat dijalankan — pernah terjadi di mergeBonSync (v0.1.118).
# ESLint `no-undef` menangkapnya. Jalankan sebelum push.
set -euo pipefail
cd "$(dirname "$0")"

# TEMP RELATIF — path absolut MSYS (/c/...) tak dipahami python/eslint native
# Windows saat MSYS2_ARG_CONV_EXCL diset (konversi path dimatikan). Nama relatif
# aman utk semuanya krn kita sudah `cd` ke root repo.
JS=".cek-js-$$.js"
CFG=".cek-js-cfg-$$.mjs"
trap 'rm -f "$JS" "$CFG"' EXIT

python3 - "$JS" <<'PY'
import re, sys
src = open("src/index.html", encoding="utf-8").read()
# Ambil isi <script> TANPA src. Catat: '<script>' bisa muncul di KOMENTAR JS
# (mis. "...WebView2 cache menyuntik dua <script>...") — regex naif berhenti di
# situ & meninggalkan sisa JS di luar blok, sehingga tak pernah di-lint.
# Jadi: scan berurutan, pasangkan tag yg BENAR (<script...> ... </script>).
blocks, i = [], 0
while True:
    m = re.compile(r"<script(?![^>]*\bsrc=)[^>]*>", re.I).search(src, i)
    if not m:
        break
    end = src.find("</script>", m.end())
    if end < 0:
        break
    body = src[m.end():end]
    # blok nyata selalu punya JS sungguhan; tag hantu di komentar tak diikuti </script> jauh
    if body.strip():
        blocks.append(body)
    i = end + len("</script>")
open(sys.argv[1], "w").write("\n;\n".join(blocks))
print(f"script blocks: {len(blocks)}")
PY

cat > "$CFG" <<'EOF'
export default [{
  files: ["**/*.js"],
  languageOptions: {
    ecmaVersion: 2022, sourceType: "script",
    globals: {
      window:"readonly", document:"readonly", localStorage:"readonly", console:"readonly",
      setTimeout:"readonly", clearTimeout:"readonly", setInterval:"readonly", clearInterval:"readonly",
      fetch:"readonly", alert:"readonly", confirm:"readonly", prompt:"readonly",
      navigator:"readonly", location:"readonly", requestAnimationFrame:"readonly",
      performance:"readonly", Blob:"readonly", URL:"readonly", FileReader:"readonly",
      Event:"readonly", CustomEvent:"readonly", Image:"readonly", Audio:"readonly",
      Option:"readonly", FormData:"readonly", AbortController:"readonly",
      structuredClone:"readonly", queueMicrotask:"readonly", crypto:"readonly",
      __TAURI__:"readonly", print:"readonly",
    },
  },
  rules: { "no-undef": "error" },
}];
EOF

echo "== node --check =="
node --check "$JS" && echo "OK (sintaks)"
echo "== eslint no-undef =="
npx --yes eslint --config "$CFG" "$JS" && echo "OK (tak ada variabel tak terdeklarasi)"
