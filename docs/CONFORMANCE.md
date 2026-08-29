# Conformance & Reliability — pnk2json

How we prove the converter is correct and fast, and the specs for hand-built
golden fixtures (built by a human in the real apps, asserted in tests).

## 1. Corpus harness

```bash
cargo build --release
python3 scripts/conformance.py --mode both   # json + markdown over all of success.tsv
```

Verdicts per (fixture, mode): `ok`, `ok:encrypted-reject` (exit 1 with
password/encryption wording), or defects (`DEFECT:*`, `TIMEOUT`, `PANIC`,
`MISSING_FILE`). JSON report lands in `fixtures/conformance-report.json`
(regenerable, not committed).

### Corpus health (2026-08-28, CC-MAIN-2026-34, 1,248 files × 2 modes)

| metric | value |
| --- | --- |
| verdicts | 2,488 ok + 8 ok:encrypted-reject (4 files × 2 modes) |
| defects | **0** (no panics, no timeouts, no unexpected rejects) |
| wall (json mode) | 24.3 s total; median 8.3 ms; p95 39.5 ms; max 2.59 s |
| throughput | median 241 MB/s of input |
| time↔size Pearson r | 0.30 — **no super-linear timing** |

### Findings

1. **JSON amplification on table-heavy Numbers files** — the one reliability
   risk. `cdx-00238-1`: 22 MB in → **474 MB JSON out** (21×); siblings → 175/175/147 MB.
   Cause: per-cell fully-resolved style objects (flattening contract) on sheets
   with tens of thousands of cells. Time stays linear (~270 MB/s out), but a
   browser viewer cannot ingest a 474 MB envelope. **Proposed fix (viewer phase
   decision):** per-table style pool — emit each distinct resolved style once
   per table (`styles: [...]`) and reference by index from cells; visually
   identical, orders of magnitude smaller. Tracked as a model amendment, not a
   harness defect.
2. **Huge-media Keynote files are safe** — files up to 1.2 GB (embedded movies)
   convert in 0.15–0.6 s with ~1–2 MB JSON: media bytes stay out of the
   envelope (media[] is an inventory; bytes come via `media_bytes(dataId)`).
3. **Encrypted files** — 4 modern fixtures (3 `.iwph`, 1 `.iwpv2`) exit 1 with
   friendly messages in both modes; considered correct behavior.
4. Known cosmetic: 2 nested-directory 2012-era bundles reject with
   "no iWork '13+ data found" instead of legacy wording (clean, no panic).

## 2. Hand-built golden fixtures — build specs for Peter

Build these **in the real apps**, save to `fixtures/golden/<name>.<ext>`, then:

```bash
target/release/pnk2json fixtures/golden/<name>.<ext> > fixtures/golden/expected/<name>.json
```

Eyeball the expected output against the contract below (that eyeball step IS
the test — a mismatch is either a converter bug or a wrong assumption; we
record which). Commit both file + expected JSON (gitignore now un-ignores
`fixtures/golden/`). Suggested builds, in priority order:

### G1 — `golden-pages-wp.pages` (word-processing flavor)

Body text, in order (use Show Invisibles while building):

1. `Plain opening paragraph.` — baseline.
2. Whitespace torture, one paragraph: leading 3 spaces, the word `a`, TAB,
   `b`, 2 trailing spaces, then a non-breaking space (⌥Space) glued between
   `c` and `d`. *Contract:* run text preserves all of it byte-exact —
   leading/trailing spaces, `\t`, U+00A0. No trimming anywhere.
3. Unicode torture paragraph (all in ONE paragraph): `café` (é as e + combining
   U+0301), `ﬁle` (typed as f-i-l-e), `日本語テキスト`, `مرحبا` (RTL), `👨‍👩‍👧` (family
   emoji, ZWJ sequence), curly `“quotes”` and an ellipsis `…`.
   *Contract:* code points round-trip byte-exact (no NFC normalization, no
   ligature substitution); the emoji arrives as its full code-point sequence;
   RTL text keeps logical order.
4. Formatting runs, one paragraph: `normal **bold** normal *italic* normal`
   with `bold`/`italic` actually formatted, plus a colored word and a
   20-pt word. *Contract:* paragraph splits into ≥5 runs with boundaries
   EXACTLY at format changes; run styles carry bold/italic/color/size;
   unstyled runs have absent (not null) style fields.
5. A numbered list (3 items) and a bulleted list (2 items, one nested).
   *Contract:* list paragraphs carry list style + outline level; nested item
   has deeper indent/level.
6. A soft line break (Shift+Return) inside a paragraph between two words.
   *Contract:* same paragraph, two runs (or break marker) — NOT two paragraphs.

### G2 — `golden-pages-layout.pages` (page-layout flavor) + embeds

1. File → Convert to Page Layout (or start from a Layout template).
2. One text box: `Shapes can hold text too.` — *contract:* appears as a
   drawable with text, at its x/y/size in pts.
3. A default shape (rounded rectangle) with NO text. *contract:* path/geometry
   present, text absent (not empty-string vs not-specified — check which the
   model chose and assert it).
4. Insert → Image (any small jpg/png). *contract:* drawable kind=image with a
   media dataId; the referenced media bytes hash matches the inserted file.
5. Insert a straight-line/arrow shape. *contract:* line geometry + optional
   arrowhead style present.
6. Group the shape + line together (Shift-click, Group). *contract:* children
   re-based to group-local coordinates per model-design.
7. Add a page break so there are 2 pages. *contract:* two pages in order.

### G3 — `golden-numbers.numbers`

1. Sheet `S1`, table `Torture`: 4×4. Header row `Name`, `Qty`, `Price`, `Note`.
2. Row values: `Widget`, `3`, `4.5` (formatted as currency), `café ☕`;
   `日本`, `10`, `0.5` (percent format), `tab\there` (literal tab in cell);
   merged cell across 2 columns containing `merged!`; one formula cell
   `=SUM(B2:B3)` (shows 13.5).
   *Contract:* values typed per cell class (number vs string), number FORMAT
   captured (currency/percent), merge recorded (col<<16|row encoding per
   tables.md), formula cell carries `TsceFormulaRef` + last-calculated value.
3. Rename sheet 2 to `日本語シート`, add a 2×2 table.
   *Contract:* sheet name round-trips byte-exact.

### G4 — `golden-keynote.key`

1. Slide 1 (Title & Subtitle master): title `Deck Title`, subtitle
   `with “curly” quotes` — *contract:* placeholder roles resolved
   (title/subtitle), inherited style flags set.
2. Slide 2: a text box with two paragraphs (one bold), a shape with text
   `shape text`, an inserted image, and a presenter note `note line`.
   *contract:* drawables in z-order; note text in slide notes; image via
   media dataId.
3. Add a slide transition to slide 2. *contract:* transition present (shape
   per keynote.md; free-text delivery string).

## 3. Assertions in code (after G1–G4 land)

Each golden fixture gets a Rust integration test in `crates/pnk2json` that
converts the committed file and asserts the contract lines above against
`expected/<name>.json` (semantic JSON diff — ignoring key order, preserving
values). Failing an assertion means a converter regression, a model change
without contract update, or a genuinely wrong assumption — all worth a commit
explaining which.

## 4. Cross-validation against Apple renders (2026-08-28)

Two independent ground-truth channels, both scriptable, no GUI fiddling needed:

1. **Embedded QuickLook previews** — every fixture carries Apple's own render
   (`preview.pdf`, or `preview.jpg`/`preview-web.jpg`; flavor varies per file).
   `scripts/crossval.py` extracts them, rasterizes PDFs via CoreGraphics
   (pypdf for text), and compares against our JSON/markdown: table census
   (empty-grid detection) + normalized token overlap.
2. **Live app export** — `scripts/app_export_pdf.sh <doc> <out.pdf>` drives the
   real app over AppleScript (bundle id; dismisses the first-launch modal via
   Accessibility; waits for the document; exports; closes). Validated on
   Keynote: output matched the file's embedded preview exactly (both blank —
   `cdx-00259-1` is a genuinely degenerate portrait doc, confirmed by both
   channels agreeing).

### Finding: empty-grid tables (fixture-verified 2026-08-28)

`cdx-00006-5` (Numbers 11.1.2): our JSON emitted an all-null 8×4 grid, but the
file's own QuickLook preview shows a populated playlist. Cause: TST.Tile row
buffers reference cells **by key into `TST.TableDataList` archives** (columnar
indirection) — the tile walker only handled inline values. Blast radius on the
corpus: **45/358 tables all-null (11 fixtures), 148 more <25% populated**.
Fix dispatched to pnk2json (resolve DataList keys; warn on unresolvable, never
silent-null). `--scan` mode of the harness re-censuses the corpus after fixes.
