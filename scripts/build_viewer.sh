#!/usr/bin/env bash
# Build the pnk viewer (viewer/ -> viewer/dist/), fully static:
#   1. cargo builds pnk2json-wasm for wasm32-unknown-unknown
#   2. wasm-bindgen --target web emits JS glue + .wasm into viewer/dist/wasm
#   3. the generated glue is vendored into viewer/src/wasm/ (committed; the
#      tiny JS file is deterministic for a given wasm-bindgen version) so
#      esbuild can bundle `import init from "./wasm/pnk2json_wasm.js"`
#   4. esbuild bundles viewer/src/main.ts -> viewer/dist/main.js
#   5. the static shell (index.html, styles.css) is copied to viewer/dist/
#
# Prerequisites: cargo, wasm-bindgen 0.2.127 on PATH; `npm install` run once
# inside viewer/ (esbuild + playwright devDependencies). Optional: binaryen
# (wasm-opt) for the smallest module.
#
# Output layout (viewer/dist/): index.html  styles.css  main.js  wasm/*.wasm
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

ESBUILD="$ROOT/viewer/node_modules/.bin/esbuild"
if [ ! -x "$ESBUILD" ]; then
  echo "error: esbuild not found — run 'npm install' in viewer/ first" >&2
  exit 1
fi

echo "==> cargo build -p pnk2json-wasm (wasm32-unknown-unknown, profile wasm)"
# [profile.wasm] in Cargo.toml: size-tuned (opt-level s, fat LTO, abort, strip)
cargo build -p pnk2json-wasm --target wasm32-unknown-unknown --profile wasm

echo "==> wasm-bindgen --target web -> viewer/dist/wasm/"
mkdir -p viewer/dist/wasm viewer/src/wasm
wasm-bindgen target/wasm32-unknown-unknown/wasm/pnk2json_wasm.wasm \
  --target web --out-dir viewer/dist/wasm

# binaryen's wasm-opt shaves a further ~16% (1.60 MB -> 1.34 MB); optional
# so a machine without it still builds a working viewer.
if command -v wasm-opt >/dev/null 2>&1; then
  echo "==> wasm-opt -Oz"
  wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
    viewer/dist/wasm/pnk2json_wasm_bg.wasm -o viewer/dist/wasm/pnk2json_wasm_bg.wasm
else
  echo "    (wasm-opt not found — skipping; brew install binaryen for a ~16% smaller module)"
fi

echo "==> vendoring generated glue into viewer/src/wasm/"
cp viewer/dist/wasm/pnk2json_wasm.js viewer/src/wasm/pnk2json_wasm.js
cp viewer/dist/wasm/pnk2json_wasm.d.ts viewer/src/wasm/pnk2json_wasm.d.ts

# pdf.js (viewer/node_modules/pdfjs-dist, Apache-2.0) renders PDF media
# in-page. Its worker script is embedded in the bundle as a string and
# started from a blob: URL, so the served page makes no request after load.
echo "==> pdf.js worker -> viewer/src/gen/pdf.worker.txt"
mkdir -p viewer/src/gen
cp viewer/node_modules/pdfjs-dist/build/pdf.worker.min.mjs viewer/src/gen/pdf.worker.txt

echo "==> esbuild bundle -> viewer/dist/main.js"
"$ESBUILD" viewer/src/main.ts \
  --bundle --format=esm --target=es2022 \
  --loader:.txt=text \
  --outfile=viewer/dist/main.js

echo "==> static shell -> viewer/dist/"
cp viewer/index.html viewer/styles.css viewer/dist/

echo "viewer built: viewer/dist/  (serve: cd viewer && npm run serve)"