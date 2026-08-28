# TSCH Charts — chart model and data references

A chart is a TSD drawable (`TSCH.ChartDrawableArchive`) whose payload carries a
`TSCH.ChartArchive` model: chart type, style/preset references, an inline data
grid (`TSCH.ChartGridArchive`), and a mediator that ties the chart to its data
source. Charts store model data only — there is no rendered chart image in the
file; rendering is deferred to the reading app [inferred: the protos carry
types, styles, grid values, and style presets but no raster/vector render
output; parsers reconstruct charts from the model]. Provenance: the 15.3.1
extraction at `.scratch/otorp/Keynote/TSCHArchives.proto`,
`TSCHArchives.Common.proto`, `TSCHArchives.GEN.proto`, `TSCH3DArchives.proto`,
and `TSCHPreUFFArchives.proto` (`package TSCH`), plus the Go reference protos
under `.scratch/iwork/proto/TSCH/`.

## Object graph

The registry type ids (see [registry.md](registry.md)) — [parser:
dunhamsteve/iwork@02c26eb] `.scratch/iwork/index/common.go:793-897`:

| id | message |
| --- | --- |
| 5000 | `TSCH.PreUFF.ChartInfoArchive` (legacy "pre-UFF" chart) |
| 5002 | `TSCH.PreUFF.ChartGridArchive` (legacy inline grid) |
| 5004 | `TSCH.ChartMediatorArchive` |
| 5010-5017 | `TSCH.PreUFF.*Style/NonStyle` archives |
| 5020 | `TSCH.ChartStylePreset` |
| 5021 | `TSCH.ChartDrawableArchive` (modern chart root) |
| 5022-5029 | `TSCH.ChartStyleArchive`, `ChartNonStyleArchive`, `LegendStyleArchive`, `LegendNonStyleArchive`, `ChartAxisStyleArchive`, `ChartAxisNonStyleArchive`, `ChartSeriesStyleArchive`, `ChartSeriesNonStyleArchive` |

`TSCH.ChartDrawableArchive` (`.scratch/otorp/Keynote/TSCHArchives.proto:14-17`)
is a thin drawable wrapper: `super = 1` is a `TSD.DrawableArchive` (geometry,
wrap, title/caption — see [drawables.md](drawables.md)), everything else rides
on extension field 10000 (`unity`): the full `TSCH.ChartArchive` is nested
inside the same object via `extend .TSCH.ChartDrawableArchive { optional
.TSCH.ChartArchive unity = 10000; }` (TSCHArchives.proto:45-47). [proto]

Note on parsers: litchi decodes object types 5000, 5004, and 5021 directly as
`tsch::ChartArchive` ([parser: DevExzh/litchi@9229364]
`.scratch/litchi/crates/litchi-iwa/src/charts/metadata_extractor.rs:106-136`),
and per dunhamsteve/iwork those ids are actually the PreUFF chart info, the
mediator, and the drawable wrapper respectively — decoding 5000/5004 as
`ChartArchive` only yields plausible-looking garbage for legacy files
[inferred: discrepancy between the two parsers; verify against fixtures which
payload form each id actually carries in modern files].

## ChartArchive

`TSCH.ChartArchive` (TSCHArchives.proto:19-48) [proto]:

- Identity/layout: `chart_type = 1` (enum `TSCH.ChartType`),
  `scatter_format = 2` (`TSCH.ScatterFormat`: separate_x = 1 / shared_x = 2,
  `TSCHArchives.Common.proto:51-55`), `legend_frame = 3`
  (`TSCH.RectArchive { origin = 1, size = 2 }`, Common.proto:94-97),
  `series_direction = 5` (`SeriesDirection`: by_row = 1 / by_column = 2,
  Common.proto:57-61), `multidataset_index = 21`. [proto]
- Data: `grid = 7` (inline `TSCH.ChartGridArchive`), `mediator = 8`
  (`TSP.Reference` to a `TSCH.ChartMediatorArchive`),
  `contains_default_data = 6`, `is_dirty = 24`,
  `needs_calc_engine_deferred_import_action = 22`. [proto]
- Styles: `preset = 4` and `owned_preset = 23` (`TSP.Reference` to
  `TSCH.ChartStylePreset`), `chart_style = 9`, `chart_non_style = 10`,
  `legend_style = 11`, `legend_non_style = 12`,
  `value_axis_styles = 13` / `value_axis_nonstyles = 14` (repeated
  `TSP.Reference`), `category_axis_styles = 15` / `category_axis_nonstyles = 16`,
  `series_theme_styles = 17`, `series_private_styles = 18` and
  `series_non_styles = 19` (`TSP.SparseReferenceArray` — indexed slots for
  per-series styles), `paragraph_styles = 20`. The style message bodies are
  empty shells extending `TSS.StyleArchive`
  (`TSCHArchives.Common.proto:115-153`); all actual properties live in
  `TSCHArchives.GEN.proto` generic property maps (`ChartGenericPropertyMapArchive`
  line 437, `ChartAxisGenericPropertyMapArchive` line 495,
  `ChartSeriesGenericPropertyMapArchive` line 541). [proto]

Chart type enum (`TSCHArchives.Common.proto:10-39`) covers 2D and 3D variants
in one enum: `columnChartType2D = 1` ... `pieChartType3D = 16`, ...,
`bubbleChartType2D = 22`, `donutChartType2D/3D = 25/26`, `radarChartType2D = 27`. [proto]

## Chart data: the inline grid

`TSCH.ChartGridArchive` (TSCHArchives.proto:118-131) [proto]:

- `row_name = 1`, `column_name = 2` (repeated string) — category and series
  labels [inferred: role assignment depends on `series_direction`].
- `grid_row = 3` — repeated `TSCH.GridRow`, each `GridRow.value = 1` being a
  repeated `TSCH.GridValue` (`TSCHArchives.Common.proto:155-164`):
  `numeric_value = 1`, `date_value_1_0 = 2`, `duration_value = 3`,
  `date_value = 4` (doubles). [proto]
- `idMap = 4` — `ChartGridRowColumnIdMap` mapping stable string `uniqueId`s to
  row/column indices (survives reorder edits). [proto]

There is no separate "value references" message in the 15.3.1 protos: series
/category/value selection is expressed either through this inline grid (charts
with private data, e.g. in Keynote) or through the mediator's formulae (charts
bound to table data). [inferred: grep for `ValueReference|ValueRefs` over all
local protos finds nothing; the two mechanisms above are what exists]

## The mediator — how a chart points at table data

`TSCH.ChartMediatorArchive` (TSCHArchives.proto:133-137): `info = 1`
(`TSP.Reference`), `local_series_indexes = 2` / `remote_series_indexes = 3`
(repeated uint32 — which series are chart-local vs sourced remotely). [proto]

In Numbers the reference resolves to `TN.ChartMediatorArchive` (registry id
12006 — [parser: dunhamsteve/iwork@02c26eb]
`.scratch/iwork/index/numbers.go:41-45`), which extends the TSCH mediator and
carries the binding: `super = 1`, `entity_id = 2`, `formulas = 3` (inline
`TN.ChartMediatorFormulaStorage`), `columns_are_series = 4`,
`is_registered_with_calc_engine = 5`
(`.scratch/iwork/proto/TN/TNArchives.pb.go:813-820`). [parser:
dunhamsteve/iwork@02c26eb]

`TN.ChartMediatorFormulaStorage` (`.scratch/iwork/proto/TN/TNArchives.pb.go:741-750`)
holds `data_formulae`, `row_label_formulae`, `col_label_formulae`, `direction`,
and error-bar formula lists — all `TSCE.FormulaArchive`. [parser:
dunhamsteve/iwork@02c26eb]

So the chart→table link is: `ChartArchive.mediator` (`TSP.Reference`) →
mediator object → `TSCE.FormulaArchive`s whose expression ASTs reference tables
by UUID, not by direct `TSP.Reference` to a `TST.TableInfoArchive`:
`TSCE.FormulaArchive` carries `host_table_uid = 7` (`TSP.UUID`,
`.scratch/otorp/Keynote/TSCEArchives.proto:807-817`), and formula dependency
records name tables via `TSCE.UuidReferencesArchive.TableRef { owner_uuid = 1,
coord_set = 2 }` (TSCEArchives.proto:393-408). Table identity itself is the
table's UUID in the TST model — see [tables.md](tables.md). [proto]
Keynote charts (no spreadsheet engine tables) keep their data purely in the
inline `ChartGridArchive` [inferred: Keynote has no TST tables to point at;
fixtures should confirm the mediator is absent or formula-less there].

## Selection and UI state

- `TSCH.ChartSelectionArchive` (TSCHArchives.proto:207-211): `super = 3`
  (`TSD.DrawableSelectionArchive`), `chart = 1` (`TSP.Reference`), `paths = 2`
  — a hierarchy of `ChartSelectionPathArchive` (201-205: path type, sub-path,
  arguments naming `ChartAxisIDArchive { axis_type, ordinal }`, 191-194). [proto]
- `TSCH.ChartCDESelectionArchive` (213-220) records a chart-data-editor
  row/column range selection. [proto]
- `TSCH.ChartUIState` (222-232) persists CDE cursor state and the
  multi-dataset index. [proto]

## Presets and fill sets

- `TSCH.ChartStylePreset` (TSCHArchives.proto:146-155) bundles chart/legend/
  axis/series/paragraph style references plus a `uuid`; chart presets are
  attached to the theme via `extend .TSS.ThemeArchive { optional
  .TSCH.ChartPresetsArchive extension = 120; }` (lines 157-162). [proto]
- `TSCH.ChartFillSetArchive` (139-144) — named series-fill sets (`identifier`,
  `lookup_string`, repeated `series_styles`); `ChartArchive` extension field
  10004 `last_applied_fill_set_lookup_string` (line 321) records the last
  applied set. [proto]
- `TSCH.PropertyValueStorageContainerArchive` (164-178) is the pasteboard-shaped
  style bundle (all style slots in one message) used by
  `TSCH.StylePasteboardDataArchive` (180-184). [proto]
- Reference lines: `ChartReferenceLinesArchive` (262-266) maps per-axis
  reference-line styles/non-styles, attached to `ChartArchive` via extension
  field 10005 (lines 348-350). [proto]

## 3D charts

All 3D scene state lives in `TSCH3DArchives.proto` (`package TSCH`), separate
from the 2D model [proto]:

- `TSCH.Chart3DFillArchive` (lines 55-60: `lightingmodel`, `textureset_id`,
  `fill_type`, `series_index`) attaches to any `TSD.FillArchive` via
  `extend .TSD.FillArchive { optional .TSCH.Chart3DFillArchive fill3d = 100; }`
  (lines 186-188) — 3D fills are a fill variant, not a chart-type flag. [proto]
- Lighting: `Chart3DLightingModelArchive` (91-95) picks a
  `Chart3DPhongLightingModelArchive` or `Chart3DFixedFunctionLightingModelArchive`
  plus an environment package; `Chart3DLightArchive` (77-89) defines named
  lights with ambient/diffuse/specular colors, intensity, attenuation, and a
  point/directional/spot subtype. [proto]
- Materials: `Chart3DPhongMaterialPackageArchive` (147-153) groups
  emissive/diffuse/modulate/specular/shininess materials, each extending
  `Chart3DTexturesMaterialArchive` (102-105) with image textures
  (`Chart3DTSPImageDataTextureArchive`, 155-160: modern `TSP.DataReference`
  fields 3/4, legacy `database_*` references 1/2) and tiling parameters
  (`Chart3DImageTextureTilingArchive`, 167-177). `Chart3DVectorArchive`
  (179-184) is a 4-float (x, y, z, w) vector. [proto]
- The chart-level 3D scene parameters (rotation, scale, viewport, lighting
  package, bar shape, bevel, inter-set depth gap) are generic properties on the
  chart style: `tschchartinfo3d*` fields in
  `TSCH.ChartGenericPropertyMapArchive`
  (`.scratch/otorp/Keynote/TSCHArchives.GEN.proto:438-445`). [proto]
- A deprecated 3D fill variant `TSCH.DEPRECATEDChart3DFillArchive` survives in
  `TSCHArchives.Common.proto:107-113`. [proto]

3D chart types are the `*ChartType3D` members of `TSCH.ChartType`; there is no
separate 3D chart message — the same `TSCH.ChartArchive` model, with 3D-ness
expressed through the type enum and the `tschchartinfo3d*` style properties
[inferred: proto structure; fixture-check how much of TSCH3DArchives appears
for a 3D chart].

## Parser support

- dunhamsteve/iwork registers all TSCH ids but performs no chart rendering
  ([parser: dunhamsteve/iwork@02c26eb] `.scratch/iwork/index/common.go:793-897`). [parser]
- litchi extracts chart metadata only (type name, series count, row/column
  names, default-data flag, best-effort title) by walking `ChartArchive.grid`
  ([parser: DevExzh/litchi@9229364]
  `.scratch/litchi/crates/litchi-iwa/src/charts/metadata_extractor.rs:106-165,201-238`);
  it finds titles via style/text-storage dependency traversal. [parser]
- keynote-parser has no chart handling at all ([parser:
  psobot/keynote-parser@56a4d3b] — no chart references in
  `.scratch/keynote-parser/keynote_parser/`). [parser]
