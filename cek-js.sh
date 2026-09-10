#!/usr/bin/env bash
# Cek JS inline di src/index.html: sintaks + VARIABEL TAK TERDEKLARASI.
# Latar: node --check hanya menangkap error SINTAKS. Bug runtime seperti
# `ada.add(id)` (variabel yg dihapus saat refactor) LOLOS node --check tapi
# meledak saat dijalankan — pernah terjadi di mergeBonSync (v0.1.118).
# ESLint `no-undef` menangkapnya. Jalankan sebelum push.
set -euo pipefail
cd "$(dirname "$0")"

JS="$(mktemp "$(pwd)/.cek-js-XXXXXX.js")"
CFG="$(mktemp "$(pwd)/.cek-js-cfg-XXXXXX.mjs")"
trap 'rm -f "$JS" "$CFG"' EXIT

python3 - "$JS" <<'PY'
import re, sys
src = open("src/index.html").read()
blocks = re.findall(r"<script(?![^>]*\bsrc=)[^>]*>(.*?)</script>", src, re.S)
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
