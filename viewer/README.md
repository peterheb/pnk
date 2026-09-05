# pnk viewer

Zero-backend viewer for iWork documents (`.pages` / `.numbers` / `.key`).
Drop a file on the page; it is parsed **in your browser** by `pnk2json.wasm`
and rendered — no accounts, no upload, no backend. The only request the
viewer can make after its static assets load is the substitute-font
stylesheet described below, and it is a setting.

## The 3 commands

```sh
# 1. one-time: install esbuild + Playwright (viewer-local node_modules)
cd viewer && npm install

# 2. build: wasm-bindgen the converter, bundle TS -> viewer/dist/
npm run build          # runs ../scripts/build_viewer.sh

# 3. serve the static bundle at http://127.0.0.1:8123
npm run serve
```

## Substitute fonts

Documents name fonts the reader often does not have (Calibri, Gill Sans, DIN
Condensed…). `src/fontmap.ts` maps every family in the corpus to a Google
Fonts substitute — a metric-compatible clone where one exists, otherwise a
face of the same class, otherwise nothing — and `src/webfonts.ts` requests
exactly the families, weights and slopes the open document uses. The stack in
the rendered CSS is: the document's own face, then its family, then the
substitute, then the browser generic, so a reader who has the real font never
sees the substitute. `docs/fonts.md` has the mapping and the reasoning.

The nav's **settings** menu carries one checkbox, "Load substitute fonts from
Google Fonts", stored in `localStorage` under `pnk.googleFonts`, **on by
default**. Turning it off removes the stylesheet, drops the substitutes from
every font stack, re-renders the open document, and leaves the viewer making
no network request at all.

Privacy: with the setting on, `fonts.googleapis.com` and `fonts.gstatic.com`
see the reader's IP address and the font family names requested. They do not
see the document — it is parsed in-page, and the CSP in `index.html` allows
no remote `connect-src`, `img-src` or `form-action` by which it could leave.

## Gate (Playwright)

With the built bundle in `viewer/dist/`:

```sh
npm run build && npm test     # or: npx playwright test
```

Renders one real fixture per app (Keynote / Numbers / Pages) plus the
encrypted + legacy error paths, with screenshots under `/tmp/pnk-gate/`.
Those tests run with the substitute-font setting OFF and assert zero runtime
network requests; one further test turns it on and asserts that the only
external requests go to `fonts.googleapis.com` / `fonts.gstatic.com`.
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
