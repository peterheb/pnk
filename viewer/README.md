# pnk viewer

Zero-backend viewer for iWork documents (`.pages` / `.numbers` / `.key`).
Drop a file on the page; it is parsed **in your browser** by `pnk2json.wasm`
and rendered — no accounts, no upload, no backend, no runtime network calls
after the static assets load.

## The 3 commands

```sh
# 1. one-time: install esbuild + Playwright (viewer-local node_modules)
cd viewer && npm install

# 2. build: wasm-bindgen the converter, bundle TS -> viewer/dist/
npm run build          # runs ../scripts/build_viewer.sh

# 3. serve the static bundle at http://127.0.0.1:8123
npm run serve
```

## Gate (Playwright)

With the built bundle in `viewer/dist/`:

```sh
npm run build && npm test     # or: npx playwright test
```

Renders one real fixture per app (Keynote / Numbers / Pages) plus the
encrypted + legacy error paths, with screenshots under `/tmp/pnk-gate/`.
Fixtures live in `fixtures/crawl/` (gitignored) and are referenced by SHA-256
in `tests/gate.spec.ts`.

## Layout

- `src/wasm/` — generated wasm-bindgen glue (committed; regenerated on every
  build by the build script)
- `dist/` — build output (gitignored): `index.html`, `main.js`, `styles.css`,
  `wasm/pnk2json_wasm_bg.wasm`
- Types come from `../model/src` (type-only imports; erased at bundle time)

## Errors you'll see

Encrypted (`.iwph`/`.iwpv2`) and legacy (pre-iWork '13 `index.apxl`-era)
files are rejected by the converter before any parsing; the viewer maps those
rejections to human explanations. Everything stays client-side.
