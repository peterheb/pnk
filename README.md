# pnk

**Client-side Apple iWork viewer** — open `.pages`, `.numbers`, and `.key`
files entirely in your browser. The file is parsed from raw bytes by Rust
compiled to WebAssembly, emitted as a typed JSON model, and rendered by a
vanilla-TS viewer. No account, no upload, no backend.

Live at **[pnk.vu](https://pnk.vu)**. Built solo for
[Hackyard Yard #1](https://hackyard.tech/yards/yard-1) ("no accounts",
48 hours, 2026-08-28 → 30).

## Features

- Drag & drop anywhere; iWork '13+ flat files and package directories.
- **Keynote**: continuous slide scroll, master/theme underlays, shapes with
  arrowheads and reflections, image bullets, shrink-to-fit text, presenter
  notes.
- **Numbers**: per-sheet tabs, styled tables — merges, exact column widths,
  cell borders and outer frames, number/currency/fraction/base-n/duration
  formats.
- **Pages**: paginated word-processing with margins, headers/footers,
  footnotes, drop caps, lists; page-layout canvases with template underlays.
- Syntax-colored JSON model view (`json` in the nav) with download.
- Encrypted and pre-2013 legacy files are refused with a clear error card;
  no password prompt, nothing inspected server-side.
- Zero network requests after load — asserted in the test gate.
- Dark mode, mobile layout.

## Quick start

```bash
# viewer: wasm + bundle + static shell -> viewer/dist/
cd viewer && npm install && npm run build && npm run serve
# gate: strict tsc + Playwright (includes a zero-network assertion)
npm test

# CLI
cargo build --release
target/release/pnk2json document.pages > out.json      # compact JSON
target/release/pnk2json document.numbers --markdown    # readable fallback
target/release/iwadump document.key                    # raw IWA inspector
```

Prerequisites: rust + `wasm32-unknown-unknown` target, `wasm-bindgen` 0.2.127,
node 22+.

## Repo layout

| path | what |
| --- | --- |
| `crates/iwadump` | Rust CLI structure inspector for iWork '13+ documents |
| `crates/pnk2json` | converter lib + wasm binding + text/markdown dumpers |
| `viewer/` | vanilla-TS web app consuming `pnk2json.wasm` |
| `model/src/` | the TypeScript contract the JSON output obeys (strict tsc) |
| `docs/format/` | provenance-tagged iWork format reference — start at `INDEX.md` |
| `docs/model-design.md` | JSON model rationale and conventions |
| `docs/CONFORMANCE.md` | corpus + cross-validation results |
| `scripts/` | conformance / visual-diff / research harnesses |
| `fixtures/` | corpus (gitignored; `provenance.json` and golden checklists committed) |

## Format, short version

An iWork '13+ file is a ZIP whose `Index/` members are `.iwa` streams:
Snappy-compressed blocks wrapping a protobuf object database (TSWP text, TST
tables, TSD drawables, TSCH charts, TSCE formulas, KN Keynote, TSP storage).
Decoded from scratch in Rust. Every format claim in `docs/format/` carries a
provenance tag (`proto` / `parser` / `fixture-verified` / `inferred`); start
at [`docs/format/INDEX.md`](docs/format/INDEX.md).

## Verification

- `scripts/conformance.py` — 1,248-file Common Crawl corpus × JSON + markdown:
  2,488 ok, 8 controlled encrypted rejects, 0 defects, linear timing.
- `scripts/visual_diff.py` — side-by-side composites of our render vs PDF
  exported from the real apps (driven by AppleScript), judged per page.
- Golden fixtures (`fixtures/golden/`) hand-built in the real apps from
  one-feature-per-item checklists; converter output is byte-pinned in
  `cargo test`.
- Viewer gate: one real fixture per app, error cards, and zero non-blob
  network requests after load.

## License

MIT ([LICENSE-MIT](LICENSE-MIT)) or Apache-2.0
([LICENSE-APACHE](LICENSE-APACHE)), at your option.

Pages, Numbers, Keynote, and iWork are trademarks of Apple Inc. This project
is not affiliated with or endorsed by Apple.
