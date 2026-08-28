# Numbers Document Tree

The structure of a Numbers (.numbers) document: root archive, sheets, and the drawables/tables they contain, with registry type ids and proto message names.

All objects live in `Index.zip` as snappy-framed `.iwa` files; every object has a numeric identifier, and identifier **1** is the app-level document root — the Go index and numbers-parser both treat object 1 as `TN.DocumentArchive` [parser: dunhamsteve/iwork@02c26ebf] iwork2html/iwork2html.go:534; [parser: masaccio/numbers-parser@32387958] src/numbers_parser/constants.py:61 (`DOCUMENT_ID = 1`). Type ids below are from `.scratch/iwork/codegen/Numbers.json` and `Common.json` (captured from Numbers 3.0-era registries, so modern files may carry additional ids [parser: dunhamsteve/iwork@02c26ebf] codegen/README.md). Notably, TN.DocumentArchive = 1 and TN.SheetArchive = 2 (not 10000-series like Pages).

## Tree

```
Index.zip
└─ Index/*.iwa  (snappy-compressed proto archives)
   └─ Object 1: TN.DocumentArchive                       [1]
      ├─ sheets (1, repeated) → TN.SheetArchive          [2]
      │  (or TN.FormBasedSheetArchive [3] for form-linked sheets)
      ├─ stylesheet (4) → TSS.StylesheetArchive          [401]
      ├─ theme (6) → TN.ThemeArchive                     [12009]
      │  └─ super → TSS.ThemeArchive                     [402]
      ├─ sidebar_order (5) — required reference
      ├─ custom_format_list (9) — custom cell formats
      ├─ uistate (7) → TN.UIStateArchive                 [12026]
      ├─ super (8) → TSA.DocumentArchive (.scratch/otorp/Numbers/TSAArchives.proto → TSA.DocumentArchive)
      │  └─ super → TSK.DocumentArchive                  [200]
      └─ page_size (12), paper_id (11) — print setup

TN.SheetArchive [2]   (.scratch/otorp/Numbers/TNArchives.proto → TN.SheetArchive)
├─ name (1, required string)
├─ drawable_infos (2, repeated) — everything placed on the sheet canvas:
│  ├─ TST.TableInfoArchive  { super = 1 → TSD.DrawableArchive, tableModel = 2 }
│  │    └─ tableModel → TST.TableModelArchive — the actual table data
│  ├─ TSD.ImageArchive   [3005]  { super = 1, data = 11 (TSP.DataReference) }
│  ├─ TSD.ShapeArchive   [3004]  { super = 1 → TSD.DrawableArchive }
│  ├─ TSD.MovieArchive   [3007]
│  ├─ TSD.GroupArchive   [3008]  { super = 1, children = 2 }
│  ├─ TSCH.ChartDrawableArchive [5021] (via chart mediator, TN.ChartMediatorArchive [12006])
│  └─ TN.PlaceholderArchive [7]  { super = 1 → TSWP.ShapeInfoArchive }
├─ headers (18) / footers (19, repeated) → TSWP.StorageArchive [2001]
├─ print setup: in_portrait_page_orientation (3), show_page_numbers (5), content_scale (7),
│  page_order (8 → TN.PageOrder), print_margins (10 → TSD.EdgeInsetsArchive),
│  using_start_page_number (11), start_page_number (12), page_header_inset (13), page_footer_inset (14)
├─ style (22) → TN.SheetStyleArchive (fill/tab color)
├─ userDefinedGuideStorage (17)
└─ is_hidden (25), uses_single_header_footer (20), layout_direction (21)

TN.FormBasedSheetArchive [3]
├─ super (1) → TN.SheetArchive
└─ table_id (2) → TSP.CFUUIDArchive — the form is bound to one table

TST.TableInfoArchive  (the sheet-canvas wrapper for a table)
├─ super → TSD.DrawableArchive [3002] { geometry = 1, parent = 2 } — position/size on sheet
└─ tableModel (2, required) → TST.TableModelArchive { table_id = 1, tiles, columns, ... }

TSD.DrawableArchive [3002]  (base of all sheet objects)
├─ geometry (1) → TSD.GeometryArchive (position/size/rotation)
├─ parent (2) — group membership
└─ exterior_text_wrap (3), hyperlink_url, locked, comment, aspect_ratio_locked
```

## Field notes

- **TN.DocumentArchive** [1] — `.scratch/otorp/Numbers/TNArchives.proto → TN.DocumentArchive`. The document is just a list of sheets (`sheets`, field 1) plus shared resources: stylesheet (4), theme (6), sidebar_order (5, required), custom_format_list (9), and the (deprecated) calculation_engine (3). `super` (8) chains to TSA.DocumentArchive → TSK.DocumentArchive for the shared locale/annotation fields.
- **TN.SheetArchive** [2] — `.scratch/otorp/Numbers/TNArchives.proto → TN.SheetArchive`. A sheet is a free-form canvas: `drawable_infos` (field 2) holds every table, image, chart, shape, and text box placed on it, each carrying its own `TSD.GeometryArchive`. Header/footer text storages (fields 15/16) are deprecated in favor of the `headers`/`footers` lists (18/19). Print behavior (orientation, margins, page ordering, scale) is per-sheet, not per-document.
- **Tables** — tables are two objects: a `TST.TableInfoArchive` (the drawable wrapper with geometry) referencing a `TST.TableModelArchive` via `tableModel` (field 2) (.scratch/otorp/Numbers/TSTArchives.proto → TST.TableInfoArchive { super = 1, tableModel = 2 }; TST.TableModelArchive { table_id = 1 }). Full table internals in [tables.md](tables.md).
- **How numbers-parser walks this tree** — [parser: masaccio/numbers-parser@32387958]:
  - `IWork.open()` reads the package/zip, finds `.iwa` members, and stores each decoded object by identifier (src/numbers_parser/iwork.py:95-133, 188-230).
  - Sheet names come from `objects[DOCUMENT_ID].sheets`, filtering for `TNArchives.SheetArchive` instances (src/numbers_parser/model.py:260-263).
  - Tables are found per sheet by scanning all objects for `TableInfoArchive` and matching `tableModel.identifier` back to the sheet's drawable_infos (src/numbers_parser/model.py:281-296, `table_ids`/`table_info_id`); the `Sheet` and `Table` wrappers then expose them (src/numbers_parser/document.py:345-349, 470-474).
  - Custom formats are read via `objects[DOCUMENT_ID].super.custom_format_list` (src/numbers_parser/model.py:519-523) — i.e., through the TSA super chain.
- **Decoding** — the Go index maps id 1 → TN.DocumentArchive, 2 → TN.SheetArchive, 3 → TN.FormBasedSheetArchive, 7 → TN.PlaceholderArchive, 12009 → TN.ThemeArchive, 12026 → TN.UIStateArchive [parser: dunhamsteve/iwork@02c26ebf] index/numbers.go:9-170; everything else falls through to shared ids in index/common.go. The Go HTML converter renders each sheet as a section and walks `drawable_infos` in order [parser: dunhamsteve/iwork@02c26ebf] iwork2html/iwork2html.go:528-561.
- **UIState** — TN.UIStateArchive [12026] holds per-sheet scroll/zoom (via `SheetUIStateDictionaryEntryArchive`, .scratch/otorp/Numbers/TNArchives.proto → TN.UIStateArchive { sheet_uistate_dictionary_entry = 3 }) — useful for restoring view state, not document content.

Cross-references: object-envelope mechanics and TSP.Reference resolution in [objects.md](objects.md); drawable geometry/styles in [drawables.md](drawables.md); TST.TableModelArchive tile/storage internals in [tables.md](tables.md); stylesheets and themes in [styles.md](styles.md); chart drawables in [charts.md](charts.md). For comparison with the other apps, see [pages.md](pages.md) and [keynote.md](keynote.md).
