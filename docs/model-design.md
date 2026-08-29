# pnk JSON Document Model — Design Notes

The JSON the pnk pipeline emits (Rust `pnk2json` → TS viewer). The model is for a
**reader/viewer, not an editor**: everything is resolved, flattened, and
self-contained. Source of truth for the format is `docs/format/` (start at
`INDEX.md`); this doc says how that format maps onto the model in
`model/src/*.ts`.

Files:

| file | contents |
|---|---|
| `model/src/primitives.ts` | colors, geometry, fills/shadows/strokes, curve primitives, resolved text styles, units + conventions |
| `model/src/shared.ts` | root envelope, TSWP text model, TSD drawable union, TST table model, TSCH chart model, TSCE placeholder |
| `model/src/pages.ts` | `PagesDocument` — both flavors (word-processing / page-layout) |
| `model/src/numbers.ts` | `NumbersDocument` — sheets as canvases |
| `model/src/keynote.ts` | `KeynoteDocument` — show → slides, masters resolved |

---

## 1. Conventions

### 1.1 Units (everywhere, no exceptions)

| quantity | unit | JSON shape | source |
|---|---|---|---|
| lengths / positions / sizes | points | `number`, `{x,y}`, `{width,height}` | proto floats are already points |
| angles | degrees | `angleDeg: number` | `TSD.GeometryArchive.angle` is **radians** [proto]; converter converts; Keynote build/transition `direction` stays a stored enum string |
| colors | hex | `"#rrggbb"` or `"#rrggbbaa"` | `TSP.Color` (see §2.3) |
| dates | ISO 8601 UTC | `"...Z"` string | `TSP.Date.seconds` / cell dates are doubles = seconds since 2001-01-01T00:00:00Z [proto + parser: numbers-parser `EPOCH`] |
| durations | seconds | `{seconds}` or `durationSec` | proto double seconds |

### 1.2 Optional vs null

- `field?: T` — **not specified**: the source never set a value (proto field
  absent, and no `*_null` flag). A viewer applies its own default.
- `field: T | null` — **explicitly unset**: the source deliberately cleared the
  value (TSS `*_null = true` flags per docs/format/styles.md).
- In the shipped model, styles are **resolved**, so `| null` fields are rare:
  the TSS null-flag only matters mid-resolution; after walking the parent chain
  the flag has been consumed (either something supplied a value, or the value is
  genuinely absent → `?`). `CellValue` uses a tagged union instead of nullable
  variants so `null` never becomes ambiguous.

### 1.3 No indirection

- No object ids, no `TSP.Reference`, no `TSP.DataReference` u64s in content
  positions (media ids survive only inside `MediaRef.dataId`, which points at
  the envelope's `media[]` inventory — that's a lookup key, not a dangling
  reference).
- Style inheritance (`TSS.StyleArchive.parent` chains) is resolved at convert
  time; emitted styles are flat.
- Text attribute-table offsets (`table_char_style` character indexes) are
  consumed by the splitter; runs carry text + style, nothing else
  (docs/format/text.md).
- Group children are embedded; groups nest, there is no `parent` back-pointer.

### 1.4 Rust-serde compatibility

camelCase fields; enums are string unions; discriminated unions carry a
`type`/`kind` string; no tuples, no branded/intersection tricks in serialized
positions, no `null` inside unions where a variant string works. Verified by
`tsc --strict` (see §6).

---

## 2. Mapping table (proto → model)

### 2.1 Document trees

| proto (docs/format doc) | model |
|---|---|
| `TP.DocumentArchive` [10000] + `TSA./TSK.DocumentArchive` (pages.md) | `PagesDocument` |
| `TP.DocumentArchive.body_storage` → `TSWP.StorageArchive` | `PagesDocument.body: StyledText` (word-processing flavor) |
| `TP.DocumentArchive.section` [10011] | `PagesDocument.sections` +, in page-layout flavor, the canvas in `floating[]` |
| body flow in page-layout docs | `PagesDocument.hiddenBody?: StyledText` — present only when the storage has ≥1 non-empty body paragraph; Pages 26.3 layout docs keep a live, findable body flow Apple never renders `[fixture-verified: Peter's G2 + UI screenshot; Convert-to-Layout's "body discarded" warning is rendering-level only]` — preserved rather than dropped, viewer ignores the field |
| `TP.FloatingDrawablesArchive` page_groups | `PagesDocument.floating: FloatingPage[]` |
| `TP.DrawablesZOrderArchive` [10015] | paint order of `floating[].drawables` |
| `TP.PageTemplateArchive` (masters) | `PagesDocument.pageTemplates` |
| `TN.DocumentArchive` [1] (numbers.md) | `NumbersDocument` |
| `TN.SheetArchive` [2] | `NumbersDocument.sheets[]` — the sheet **is** the canvas |
| `TN.FormBasedSheetArchive` [3] | `NumbersDocument.forms[]` (recorded, not rendered) |
| `KN.DocumentArchive` [1] → `KN.ShowArchive` [2] (keynote.md) | `KeynoteDocument` |
| `KN.SlideTreeArchive.slides` (authoritative order) | `KeynoteDocument.slides[]` |
| `KN.SlideNodeArchive` (navigator tree) | only `skipped`/`slideNumberVisible` flags harvested |
| `KN.ThemeArchive.templates` | `KeynoteDocument.masters[]` |
| `TSP.PackageMetadata` (object 2), `Metadata/*.plist` | `DocumentMeta` + `media[]` |

### 2.2 Text (TSWP) — docs/format/text.md

| proto | model |
|---|---|
| `TSWP.StorageArchive.text[0]` (single string; newlines split paragraphs) | `StyledText.paragraphs[]` |
| `table_para_style` entries (`character_index` → ParagraphStyleArchive) | `Paragraph.paraStyleIndex` into the document's `styles.para` pool (resolved through the TSS parent chain; absent = default) |
| `table_char_style` entries | `TextRun`s with `charStyleIndex` into the `styles.char` pool |
| null `object` entry = "keep previous" [parser: iwork2html.go:290] | splitter carries the previous style forward |
| `table_attachment` + U+FFFC | `InlineObjectRun { drawable }` — the drawable is embedded |
| `DrawableAttachmentArchive` h/v offsets | `InlineObjectRun.offset` |
| textual attachments (page number/count/footmark), smart fields | `FieldRun` |
| footnotes (`table_footnote` → contained storage) | `PagesDocument.footnotes[]` |

**Offset semantics:** attribute-table indexes are **UTF-16 code units**, not
code points (docs/format/text.md §Unicode handling). The splitter must count
UTF-16 units; this is the #1 way astral-plane emoji text gets mis-sliced.

Text styles are **pooled, not inlined** (same precedent as the formats pool):
document-wide `styles: { para, char }` deduped pools, ordered first-use.
Measured rationale: Zen book (1,539 paragraphs) = 9 distinct para shapes + 5
char shapes — 168 KB inline (31% of the envelope) collapses to ~21 KB pooled
(−87%); pages-717KB-1 = 2 shapes across 59 paragraphs, −32% envelope.
Drawable styles stay INLINE by design (measured: 59 nodes / 3 KB on the
biggest Keynote sample — pooling them is not worth the churn).

### 2.3 Colors — `TSP.Color`

[proto: .scratch/otorp/Keynote/TSPMessages.proto → TSP.Color] — `model`
rgb=1/cmyk=2/white=3, float components 0..1, `rgbspace` srgb=1/p3=2, `a`
default 1, `headroom` default 1 (HDR), c/m/y/k or w alternatives.

Conversion (documented in `primitives.ts`): rgb+srgb → scale 0..255 per
channel; p3 → nearest-sRGB approximation + `color-degraded` warning when
visibly out of gamut [inferred]; cmyk/white → naive formulas [inferred];
headroom ≠ 1 → clamp + warning. Alpha byte appended when `a ≠ 1`.

### 2.4 Drawables (TSD) — docs/format/drawables.md

| proto | model |
|---|---|
| `TSD.DrawableArchive` geometry (position/size/radians-angle) | `DrawableCommon.position/size/angleDeg` |
| `TSD.ShapeArchive` + `PathSourceArchive` variants | `ShapeDrawable.geometry: ShapeGeometry` (see §2.5) |
| `TSWP.ShapeInfoArchive` (is_text_box) + owned storage | `TextboxDrawable` (or `ShapeDrawable.text` for shapes with text) |
| `TSD.ImageArchive` (`data` DataReference, modern; `database_data` TSP.Reference, legacy) | `ImageDrawable.image: MediaRef` resolved through the DataInfo registry (docs/format/media.md) |
| `TSD.MovieArchive` | `MovieDrawable` (+ `remoteUrl` for linked movies) |
| `TSD.GroupArchive` children | `GroupDrawable.children` embedded |
| `TSD.ConnectionLineArchive` endpoints (TSP.References) | anchors resolved; path baked as curves |
| `TST.TableInfoArchive` → `TableModelArchive` | `TableDrawable { common, table }` |
| `TSCH.ChartDrawableArchive` (unity ext 10000) | `ChartDrawable { common, chart }` |
| `TP.PlaceholderArchive` [7] / `KN.PlaceholderArchive` [7,12] | any drawable with `placeholder: { role, inherited }` |
| `TSD.FreehandDrawingArchive` (ext 100 on Group) | `GroupDrawable.freehand` |
| unknown type ids | `UnknownDrawable { typeId, typeName?, reason }` + warning |

### 2.5 Shapes → curves

`TSP.Path` is the universal curve language
[proto: TSPMessages.proto → TSP.Path, ElementType moveTo/lineTo/quadCurveTo/
curveTo/closeSubpath] and maps 1:1 onto `CurveElement`
(move/line/quad/cubic/close). Sources, in priority order:

1. `bezier_path_source.path` / `editable_bezier_path_source` (node + control
   points) → explicit `CurvePath` (editable-bezier smooth nodes become cubic
   segments).
2. `callout_path_source` → preset `"callout"` + `callout` tail parameters.
3. `scalar_path_source` (rounded rect / regular polygon / chevron + scalar) →
   `preset` + `scalar` + `naturalSize`.
4. `point_path_source` (arrows/star/plus enums) → `preset` + `naturalSize`.

Presets are named, not expanded: the viewer renders them (they are closed
vocabularies: docs/format/drawables.md lists the enums). `naturalSize` is the
design coordinate space for the path/preset; the drawable scales it into
`common.size`.

### 2.6 Tables (TST) — docs/format/tables.md

The tile/offset-buffer machinery is fully flattened:

| proto | model |
|---|---|
| `TableModelArchive` dimensions, header counts, frozen flags | `TableModel` scalar fields |
| `DataStore.rowHeaders/columnHeaders` buckets | `rows[]/columns[]` (index, sizePt, hidden) |
| tile `cell_storage_buffer` + packed offsets (BNC v5) | `grid[row][column]` — dense row-major, `null` for absent cells |
| cell type byte → payload flags | `CellValue` tagged union (dates → ISO, currency kept separate) |
| `TableDataList` STRING/RICH_TEXT_PAYLOAD entries | `CellValue` "text"/"richtext" |
| `TableDataList` FORMAT/CUSTOM_FORMAT | `formats[]` deduped pool, referenced by `TableCell.formatIndex` (custom formats degrade to `kind:"custom"` + raw string) |
| `merge_region_map` CellRanges (col<<16|row packedData) | `merges[]` anchor + span |
| `TableStyleNetworkArchive` role slots | `TableStyle` defaults; per-cell `cell_style`/`text_style` overrides resolved on top |
| `TST.CellStylePropertiesArchive` fills/strokes/vertical alignment/padding | `cellStyles: TableCellStyle[]` deduped per-table pool, referenced by `TableCell.cellStyleIndex` |

Cell layout is a **dense row-major `grid` with deduped `formats` and
`cellStyles` pools** instead of a flat sparse cell list: measured on the dense
28881×59 Numbers fixture, the flat `cells` design (pretty-printed JSON + a
`CellFormat` object duplicated per cell) pushed the envelope to 474 MB;
`grid` + format-pool + compact emission lands at ~70–90 MB, and row-major
maps 1:1 onto `<tr>` rendering.

Formulas: the cell's stored value **is** the last calculated result
(docs/format/calcengine.md) — the model never re-evaluates. The presence of a
formula becomes `cell.formula: TsceFormulaRef` (opaque).

### 2.7 Charts (TSCH) — docs/format/charts.md

| proto | model |
|---|---|
| `ChartType` enum (27 variants) | `ChartType` union + `threeD` flag |
| `ChartGridArchive` row/column names + GridValue values | `categories` + `series[]` (role assignment follows `series_direction` [inferred per charts.md]) |
| `series_direction` by_row/by_column | consumed during the assignment above |
| Keynote charts (private grid) | `dataStatus: "inline"` |
| Numbers mediator (`TN.ChartMediatorArchive` formulas) | `dataStatus: "table-bound"` + `dataBinding: TsceFormulaRef` (binding formulas are TSCE — opaque) |
| `legend_frame`, fill sets, axis/series generic property maps | `legendFrame`, `seriesColors` (best effort); everything else is **rendering** → deferred to the viewer, not modeled |

### 2.8 Calc engine (TSCE) — docs/format/calcengine.md

Deliberately **not** decompiled. `TsceFormulaRef { id, status: "unparsed",
warning }` records that a formula existed; the last-calculated value already
lives in the cell/chart data. If formula text is trivially re-synthesizable
(numbers-parser's ~30-node dispatch table approach), `sourceText` may be
filled — optional, never required.

---

## 3. Flattening rules

### 3.1 Style inheritance resolution order

For any style-bearing object (text run, cell, shape, media):

1. Start from the object's own style archive's property payload
   (`char_properties` / `para_properties` / `shape_properties` /
   `cell_properties` …).
2. Walk `TSS.StyleArchive.parent` to the root; **child overrides parent**,
   property by property. A `*_null = true` flag **clears** the inherited value
   and stops the walk for that property [proto: docs/format/styles.md].
3. Named-theme presets are NOT implicitly applied — only the object's chain
   matters (presets exist for the apps' UI).
4. Emit only properties that survived (all `?`).

Caveat carried from styles.md: multi-level chains beyond one parent are
proto-real but parser-unverified (dunhamsteve panics on recursion) — the
converter MUST implement full recursion and fixture-test it.

### 3.2 Placeholder chain (Keynote/Pages)

A slide (or page) placeholder — title/body/object/slide-number — is a
`KN./TP.PlaceholderArchive` extending the textbox chain. Resolution:

1. If the slide's placeholder carries text or explicit geometry/style
   overrides, bake them into the drawable.
2. Otherwise inherit geometry + style from the master slide's placeholder of
   the same `kind` (template chain: `KN.SlideArchive.template_slide`), and set
   `placeholder.inherited = true`.
3. Master placeholders with no slide-side counterpart stay in `masters[]`
   only.

### 3.3 Text splitting

`text[0]` + attribute tables → paragraphs/runs (§2.2). The splitter:

1. Slices at newline characters (UTF-16 aware).
2. Slices runs at `table_char_style` entry offsets; a null entry inherits the
   previous run's style.
3. Replaces U+FFFC positions with `InlineObjectRun` and textual/smart fields
   with `FieldRun`.
4. Emits **no offsets** — paragraph order + item order is the position.

### 3.4 Group coordinate re-basing

Proto children carry canvas-absolute geometry; the converter subtracts the
group's position so `GroupDrawable.children` live in group-local coordinates
(moving a group moves everything).

### 3.5 Media resolution

`DataReference.identifier` → `TSP.PackageMetadata.datas` (object 2) entry →
`Data/{file_name}` bytes (docs/format/media.md resolution chain). The envelope
`media[]` lists every DataInfo; `MediaRef.dataId` is the decimal string of the
u64 identifier (JS-safe). Missing bytes → `media-missing` warning, MediaRef
kept with names only.

---

## 4. Root envelope

Every document root carries:

- `meta: DocumentMeta` — app, application (Properties.plist `Application`),
  `fileFormatVersion`, build version history (BuildVersionHistory.plist),
  document id (DocumentIdentifier / PackageMetadata.revision), locale
  (TSK.DocumentArchive), created/modified when the source carries them.
- `warnings: Warning[]` — see §5.
- `fonts: string[]` — deduped font names harvested from resolved CharStyles;
  the viewer wants the font list before first paint.
- `media: MediaAsset[]` — the Data/ inventory (kind inferred from extension
  per docs/format/media.md).
- `styles: { para: ParaStyle[]; char: CharStyle[] }` — document-wide deduped
  text-style pools, ordered first-use; paragraphs/runs reference entries by
  index (absent index = unstyled/default). Tables carry their own deduped
  `cellStyles` pool per table. Drawable styles stay inline (measured: not
  worth pooling — §2.2).

---

## 5. Warnings taxonomy + registry-drift policy

`Warning { code, message, path?, detail? }` — anything dropped, degraded, or
unknown becomes a row; **no silent drops**. Codes (enum in shared.ts):

| code | when |
|---|---|
| `unknown-object-type` | MessageInfo.type has no trusted registry entry → payload skipped, id recorded **in hex, never a guessed name** (docs/format/registry.md recommendation) |
| `undecodable-object` | known type, payload failed to decode (length-delimited skip, docs/format/gotchas.md #6) |
| `unresolved-reference` | TSP.Reference/DataReference pointed nowhere |
| `unsupported-feature` | content exists, model can't represent it faithfully (e.g. pivot tables, cond-style rules) |
| `media-missing` | Data/ bytes absent |
| `color-degraded` | P3→sRGB approximation or HDR clamp |
| `legacy-variant` | pre-UFF charts, nested-Index.zip variant, etc. |
| `table-degraded` | pre-BNC tiles, broken offsets |
| `formula-unparsed` | TSCE AST kept opaque |

Registry policy per docs/format/registry.md: prefer keynote-parser 14.5 table
for KN ids, Common+Numbers/Pages JSONs for TN/TP ids; unknown ids stay opaque.

---

## 6. What is DROPPED (viewer-irrelevant or descoped)

| dropped | why |
|---|---|
| TSCE formula ASTs, dependency archives, OwnerUIDMapper | calc engine out of viewer scope (docs/format/calcengine.md); values already stored |
| Undo/command archives (`TSK./TSCK./KN./TP.Command*`), incremental patches (`should_merge`, `base_message_index`) | editing history, not content (docs/format/incremental.md) |
| `TN.UIStateArchive` scroll/zoom, selection archives (`TSD.CanvasSelectionArchive`, `TSCH.ChartSelection*`) | app UI state, not document content |
| `KN.RecordingEventTrack/MovieTrack` details | self-playing-recording machinery; existence noted |
| `TSD.ImageArchive` variant data beyond primary/original/thumbnail/svg (adjusted, enhanced, instant-alpha paths) | editing derivatives; viewer shows the primary image |
| RB-trees (`rowTileTree/columnTileTree`), tile segmentation, LargeArray segments | storage mechanics — flattened by the converter |
| 3D chart scene state (lighting, materials, textures) | rendering deferred; only the `threeD` flag survives |
| Themes' preset catalogs (`TSWP./TSD./TSA.ThemePresetsArchive`, color presets, fill sets) | UI affordances; resolved styles already bake in what's used |
| `VersionedStyles` snapshots in stylesheets (styles_for_10_0 …) | per-release style caches |
| Custom format list beyond what cells reference | unused formats are dead weight |
| `TSD.StrokePatternArchive` "smart stroke" parameter dictionaries | decorative stroke textures beyond dash pattern |
| iWork '08/'09 legacy content | out of scope entirely (docs/format/legacy.md) |

Anything else the converter meets and cannot model becomes `UnknownDrawable`
or a warning — never a silent drop.

---

## 7. How iwadump / pnk2json emit into these types

**iwadump** (structure inspector, phase 3): dumps raw archives; its output
"maps onto the model" in the sense that every object type id it prints should
be classifiable as (a) modeled here, (b) listed in §6 dropped, or (c) a
warning. If a fourth bucket appears, extend the model or §6 — don't guess.

**pnk2json** (Rust serde, phase 4) emission order:

1. Open container (docs/format/container.md): package vs flat zip, nested
   `Index.zip`, reject `.iwph`/legacy; read `Metadata/*.plist` + object 2
   (`TSP.PackageMetadata`) → `meta` + `media[]`.
2. Decode all `.iwa` streams into `Records[id] = (type, payload)`
   (docs/format/objects.md); unknown type ids → warnings, skip by declared
   length.
3. Walk the app tree (§2.1 mapping) resolving TSP.References as you go;
   missing targets → `unresolved-reference` warnings, content continues.
4. Resolve styles (§3.1), placeholders (§3.2), text (§3.3) during the walk —
   one pass, no id survives.
5. Collect fonts (dedupe/sort) and dedupe resolved text styles into the document-wide `styles.para`/`styles.char` pools (first-use order) and per-table `cellStyles`, then serialize with serde. Field names here ARE
   the serde names — keep them in sync (this file + `#[serde(rename_all =
   "camelCase")]` + variant renames for unions).

Suggested Rust module split mirrors the TS files 1:1, so a TS type and a Rust
struct with the same name stay reviewable side by side.

### TS typechecking

```
tsc --noEmit --strict --skipLibCheck model/src/*.ts
```

(is the verification gate; no package.json needed for the model sources.)

---

## 8. Fixture-verified notes (pnk2json, phases 4–5, 2026-08-28)

Findings from running the converter over the 968 modern fixtures in
`fixtures/success.tsv` (964 convert; 4 encrypted clean-reject). Each note
records what the corpus confirmed and what remains inferred.

### 8.1 TST vertical alignment — fixture-verified

`TST.CellStylePropertiesArchive.vertical_alignment` (field 8) appears in real
Numbers fixtures (e.g. `tableCell-0-bodyStyle` chains) carrying small int32
values (0/1/2 observed, `2` = bottom on a centered body style). The 0..3
order top/middle/bottom/justify shared with
`TSWP.ShapeStylePropertiesArchive.VerticalAlignmentType` holds on this
sample; pnk2json maps all four values. Padding (field 9, `TSWP.PaddingArchive`
{left,top,right,bottom}) confirmed present alongside it.
`[fixture-verified + proto]`

### 8.2 Pages master wiring — era drift confirmed in the corpus

Current-generation Pages documents carry page masters as
`TP.PageTemplateArchive` referenced from `TP.DocumentArchive.page_templates`
(field 48). Older (still iWork-'13-era) fixtures in the corpus instead carry
`TP.PageMasterArchive` **[10143]** — the message pages.md flagged as "absent
from the 15.3.1 extraction, present in older protos". Confirmed in the wild:
type id 10143 appears in real `.pages` files (with a sibling
`TP.CanvasSelectionArchive` [10132]), and those documents have no field-48
template list. pnk2json resolves field-48 templates fully and leaves
PageMasterArchive-era masters to the unknown/dropped path (content still
converts; the header/footer furniture of such masters is not modeled).
`[fixture-verified]`

### 8.3 P3 / HDR color policy — unchanged, [inferred]

No fixture produced a visibly-out-of-gamut P3 color or headroom > 1 during
conversion, so the §2.3 approximation policy (clamp + `color-degraded`
warning) remains implemented but **not yet fixture-exercised**. The policy
stays as documented; revisit if a wide-gamut fixture appears.

### 8.4 Storage splitting — UTF-16 offsets confirmed end-to-end

Converting keynote/pages fixtures with astral-plane content confirms that
attribute-table offsets are UTF-16 code units and that slicing on the
code-point-indexed buffer with a UTF-16 map reproduces paragraph and run
boundaries (docs/format/text.md §Unicode handling). U+FFFC attachment
entries (type 2003 → drawable; 2004/2007/2009 → textual fields;
2008 → footnote body storage) round-trip into `InlineObjectRun` / `FieldRun`
as designed. `[fixture-verified]`

### 8.5 Tables — BNC v5 buffers across the corpus

All decodable tiles in the 968-fixture modern set use storage version 5 /
wide rows; no pre-BNC tile forced the degraded path in this run. Cell type
byte 10 (currency) and decimal128 number payloads decode per the
numbers-parser layout; merges unpack as (col<<16|row). `[fixture-verified]`

### 8.6 Registry drift in practice

Modern fixtures carry type ids absent from the embedded (older-app-version)
tables — e.g. Keynote `0xde` (222, TST.TableStyleNetworkArchive era), Pages
`0xdb`/`219`, `0x2b03/0x2b06/0x2b07` (metadata-stream objects),
TSCH `5030`/`11014`. These surface as aggregated `unknown-object-type`
warnings with hex ids per §5 policy — never guessed names. Conversion of the
reachable document tree is unaffected. `[fixture-verified]`
