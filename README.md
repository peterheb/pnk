# pnk

**Client-side Apple iWork document viewer** — open `.pages`, `.numbers`, and
`.key` files entirely in your browser. Drop a file, it's parsed in-page from the
raw file bytes by Rust compiled to WebAssembly, rendered as JSON, and displayed.
No account, no login, no email, no upload — nothing ever leaves your machine.

Built for [Hackyard Yard #1](https://hackyard.tech/yards/yard-1) ("no accounts",
48-hour solo build).

## What it does

- **Drag & drop** a Keynote / Numbers / Pages document (iWork '13 and newer,
  flat file or package directory) onto the page.
- It is parsed **in your browser** by a Rust pipeline compiled to WebAssembly:
  ZIP container → Snappy-compressed IWA streams → protobuf (TSP) object
  database → a typed JSON document model.
- The viewer renders that JSON: Keynote slides with positioned shapes, images
  and presenter notes; Numbers sheets with styled tables; Pages word-processing
  flow with headings, paragraphs, and floating covers.
- Friendly error cards for the things it deliberately refuses: password-protected
  files (`.iwph` / `.iwpv2` — we never ask for a password) and legacy pre-2013
  iWork formats.
- **Zero network calls after load** — asserted in the Playwright gate. No
  accounts, no analytics, no server.

## Quick start

```bash
# viewer (TS + esbuild, no framework)
cd viewer && npm install && npm run build && npm run serve

# CLI converters
cargo build --release
target/release/pnk2json document.pages > out.json      # compact JSON
target/release/pnk2json document.numbers --markdown    # readable fallback
```

Playwright gate: `cd viewer && npm install && npm run build && npm test`.

## Repo layout

| path | what |
| --- | --- |
| `crates/iwadump` | Rust CLI structure inspector for iWork '13+ documents |
| `crates/pnk2json` | converter lib + wasm binding + text/markdown dumpers |
| `viewer/` | vanilla-TS web app consuming `pnk2json.wasm` |
| `model/src/` | the TypeScript contract the JSON output obeys (strict tsc) |
| `docs/format/` | provenance-tagged iWork format reference — start at `INDEX.md` |
| `docs/CONFORMANCE.md` | how correctness is proven; corpus + cross-validation results |
| `scripts/` | research tooling, conformance + cross-validation harnesses |
| `fixtures/` | local corpus (gitignored; `provenance.json` committed) |

## How the format works (short version)

An iWork '13+ file is a ZIP whose `Index.zip` holds `.iwa` members: sequences
of Snappy-compressed blocks wrapping protobuf messages — a small object database
(TSWP text, TST tables, TSD drawables, TSCH charts, TSCE formulas, KN Keynote,
TSP shared storage). We decode it from scratch in Rust; every format claim in
`docs/format/` is provenance-tagged (`proto` / `parser` / `fixture-verified` /
`inferred`). Start at [`docs/format/INDEX.md`](docs/format/INDEX.md).

## Verification

- `scripts/conformance.py` — the whole corpus × JSON + markdown: 2,488 ok,
  8 controlled encrypted rejects, 0 defects; timing stays linear.
- `scripts/crossval.py` — every file's embedded QuickLook preview (rendered by
  Apple's own importer) is compared against our output: table censuses, text
  tokens, empty-grid detection. 960/968 clean; every flag investigated.
- Viewer gate: one real fixture per app, encrypted + legacy error cards, and an
  assertion that **zero non-blob network requests** occur after load — the
  no-upload theme, enforced by test.

## Status

Built for Hackyard Yard #1 (2026-08-28/30). See `docs/CONFORMANCE.md` for the
reliability work and `docs/model-design.md` for the JSON model rationale.

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your
option. Format research is documented with per-claim provenance and source
commit hashes in `docs/format/`.
