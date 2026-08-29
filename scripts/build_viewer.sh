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
# inside viewer/ (esbuild + playwright devDependencies).
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

echo "==> cargo build -p pnk2json-wasm (wasm32-unknown-unknown, release)"
cargo build -p pnk2json-wasm --target wasm32-unknown-unknown --release

echo "==> wasm-bindgen --target web -> viewer/dist/wasm/"
mkdir -p viewer/dist/wasm viewer/src/wasm
wasm-bindgen target/wasm32-unknown-unknown/release/pnk2json_wasm.wasm \
  --target web --out-dir viewer/dist/wasm

echo "==> vendoring generated glue into viewer/src/wasm/"
cp viewer/dist/wasm/pnk2json_wasm.js viewer/src/wasm/pnk2json_wasm.js
cp viewer/dist/wasm/pnk2json_wasm.d.ts viewer/src/wasm/pnk2json_wasm.d.ts

echo "==> esbuild bundle -> viewer/dist/main.js"
"$ESBUILD" viewer/src/main.ts \
  --bundle --format=esm --target=es2022 \
  --outfile=viewer/dist/main.js

echo "==> static shell -> viewer/dist/"
cp viewer/index.html viewer/styles.css viewer/dist/

echo "viewer built: viewer/dist/  (serve: cd viewer && npm run serve)"