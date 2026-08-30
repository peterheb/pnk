# Model design review — 2026-08-29 (steward pass)

Scope: `model/src/*.ts` + `crates/pnk2json/src/model.rs`, evaluated against
three criteria set by Peter mid-hackathon: (1) compact for performance,
(2) pnk2json as a useful library — easy/efficient for a viewer to render a
document or extract its content, (3) inheritance handled in the model so the
viewer doesn't have to think about it.

Verdict up front: the core design is sound and measured — pooling, dense
grids, bare-scalar fast paths and "resolved, flattened, self-contained" are
the right calls, with real numbers behind them (−87% envelope on the Zen
book; 474 MB → ~70-90 MB on the dense Numbers sheet). The gaps are almost
all in criterion 3: three places still make the viewer composite inheritance
chains itself, and one converter bug (now assigned) violated the model's own
"styles are resolved" contract.

## 1. Compactness — strong, two leaks

Working well (keep, do not revisit):
- Pooled `styles.para`/`styles.char` + per-table `formats`/`cellStyles`,
  omit-default emission, first-use order.
- Dense row-major `grid` with bare scalars for the common cell and `null`
  for absent — maps 1:1 onto `<tr>` walks, serde fast-path friendly.
- Bare-string paragraph items; `points` as flat pairs (ratified 2ed592d,
  Rust side landed); media bytes out of band (`media_bytes(dataId)`), never
  base64 in the envelope.

Leak A — **warnings are per-instance rows.** The corpus census found single
documents carrying 376 near-identical `table-degraded` rows (cdx-00055-1)
and one with 996-covered-cell merge warnings. On degraded docs the warnings
array can rival content size and drowns the viewer's warnings panel.
RECOMMENDATION (approved direction): aggregate at emission — dedupe on
`(code, message-template)` and add optional `count?: number` +
`paths?: string[]` (capped, e.g. 5 examples) to `Warning`. Additive, safe.

Leak B — **`TableModel.rows`/`columns` are positional-dense** ("length =
rowCount") while the type comment also says "absent entries = default"; on a
28k-row sheet that is 28k mostly-empty objects. RECOMMENDATION: ratify the
sparse reading — the arrays MAY be shorter than rowCount/columnCount
(truncated after the last non-default entry), and an all-default array is
omitted entirely. Positional semantics otherwise unchanged. Converter
truncates; viewer already treats absent as default.

## 2. Library usability — good bones, minor type hygiene

- The envelope answers the two consumer questions cheaply: "render it"
  (paint-order drawables, resolved geometry, no indirection) and "extract
  content" (walk `paragraphs[].items`, `grid`, `notes` — all text is
  reachable without style lookups; the markdown dumper is the proof).
- `GridCell`/`ParagraphItem` untagged unions have a documented ~10-line
  adapter cost for Go/Swift consumers — acceptable, stays.
- Type hygiene nits (fix opportunistically, no urgency):
  - `Sheet.style` uses bare `string` for `tabColor`/`fill` — should be
    `HexColor` / `Fill` like everywhere else.
  - `ChartModel.legendFrame` inlines `{x,y,width,height}` — should be `Rect`.
  - `ImageDrawable.adjustments` carries an open index signature — serde
    needs a flatten/map special case; consider closing the key set.

## 3. Inheritance — the real work. Ruling: the VIEWER never walks a chain.

The model's stated philosophy ("resolved, flattened, self-contained";
docs/model-design.md §1.3, §3.1-3.2) is right. Three places still leak
composition work to the viewer, and each produced a real rendering bug this
weekend:

### 3a. Pooled styles must be resolved through the FULL chain (bug, assigned)
Runs whose pooled char style carries only `{fontColor}` render at browser
defaults — the emitter omitted values it never resolved (theme/master hop
missing for placeholder-owned storages). Evidence: RIPE deck bc5a842a
footers ~12px inside 33pt text; body bullets ~16px vs Apple 40pt. The
omit-default contract is only valid on TOP of full resolution. Fix is
converter-side (styles.rs), in flight (agent P). No model change.

### 3b. Keynote slide ⇐ master composition (model change, landed here)
`Slide.background` absent = "inherit the master's" forced every viewer to
do `masterName → masters[] → background` lookups, and to invent its own
rules for which master drawables show under a slide (ghost "Title" prompts
vs real furniture — the covers() bug was the viewer thinking about
inheritance and getting it wrong). CONTRACT CHANGE (ratified):
- `Slide.background` is emitted RESOLVED by the converter (master chain
  walked); a slide with no effective fill omits it meaning "none", not
  "go look it up".
- NEW `Slide.masterDrawables?: Drawable[]` — the filtered, resolved
  underlay the viewer paints before `drawables`, verbatim: master furniture
  minus placeholder prompts that are superseded (or empty) on this slide.
  `masters[]` remains for reference/tooling, but a viewer that never reads
  it must render correctly.
Cost: structural duplication only (images stay MediaRefs), measured
acceptable; a 47-slide deck sharing one master repeats small JSON nodes,
not bytes.

### 3c. Pages template composition (same rule, flavor-split)
- Page-layout flavor: canvases are static — same treatment as Keynote:
  NEW `FloatingPage.templateDrawables?: Drawable[]` (resolved underlay per
  canvas, paint before `drawables`).
- Word-processing flavor: pages don't exist until the viewer paginates, so
  per-page baking is impossible BY NATURE. The contract stays "viewer
  composites per page from `pageTemplates` + section template names" — but
  everything INSIDE `pageTemplates` (drawables, headers/footers, styles) is
  already resolved, so composition is a name lookup + paint, never a chain
  walk. This is the one place the viewer legitimately thinks, and it is
  bounded.

### 3d. Table role styles (no change)
`TableStyle` role defaults + per-cell `cellStyleIndex` override is a
bounded two-level rule (role default → cell override), documented, and
pooling pre-composed styles per role×cell would bloat the pools. Keep.
Banding parity likewise stays a viewer rule.

## Dispatch — status of record (updated 2026-08-30)
- 3a resolved-style chain: DONE (fc89bbd runs merge the paragraph chain;
  36a128a gates phantom borders on the enable fields).
- 3b masterDrawables + resolved background: DONE (3a54c81 extraction;
  c248353/23a5a81 tightened paint-list membership to z-order/owned refs and
  sage-tag prompt slots).
- 3c templateDrawables: DONE (f73f7a6 fills page-layout underlays).
- 1A warning aggregation: DONE (counted rows + example paths; 376→2 on the
  worst doc). 1B sparse rows/columns: DONE (ratified truncation, 1b0bd5f).
- Post-review model additions, all through the steward loop: CellFormat
  .grouping, numberSurround, SectionColumns, footnotePlacement, DropCap,
  ListFormat marker-look fields, Shape/Textbox textFit ("grow"/"shrink",
  extracted from the real shrink_to_fit flags in 5fc9bcc).
- §2 hygiene nits (Sheet.style typing, legendFrame→Rect, adjustments index
  signature): still open, owners fix opportunistically with a steward ping.
- 2026-08-30: submission freeze. All rulings above are landed; §2 nits carry
  over as post-hackathon work.
