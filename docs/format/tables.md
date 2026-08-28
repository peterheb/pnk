# TST Table Model

How Numbers (and Keynote/Pages tables) store a table: header storages, tiles, cell
value buffers, string/formula tables, and merges. Provenance: the 15.3.1 proto
extraction at `.scratch/otorp/Keynote/TSTArchives.proto` (`package TST`) plus
behavior verified in masaccio/numbers-parser.

## Object graph

A table is anchored by `TST.TableInfoArchive` (`TSTArchives.proto`, after line 310):
field 1 `super` is a `TSD.DrawableArchive` (the drawable base — see
[objects.md](objects.md)), field 2 `tableModel` is a `TSP.Reference` to
`TST.TableModelArchive`. Every table model carries the dimension facts:

- `TST.TableModelArchive` fields 6/7: `number_of_rows`, `number_of_columns`.
- Fields 9/10/11: `number_of_header_rows`, `number_of_header_columns`,
  `number_of_footer_rows`; fields 12/13 `header_rows_frozen`,
  `header_columns_frozen`; fields 14/15 (and 41/42) hidden row/column counts.
  [proto]
- Fields 16/17: `default_row_height`, `default_column_width` (doubles, points). [proto]
- Field 8 `table_name`, field 22 `table_name_enabled`. [proto]

All payload objects hang off the model through `TSP.Reference`s; resolve them via
the archive index (see [iwa.md](iwa.md)).

## DataStore

`TST.DataStore` (the "base data store"; referenced as `base_data_store` =
`TST.TableModelArchive` field 4) is the row/column/cell payload container:

- Field 1 `rowHeaders`: inline `TST.HeaderStorage` — `bucketHashFunction` +
  repeated `TSP.Reference buckets` (each a `TST.HeaderStorageBucket` whose
  `headers` are `TST.HeaderStorageBucket.Header { index = 1, size = 2 (float,
  row height), hidingState = 3, cell_style = 5, text_style = 6 }`). [proto]
- Field 2 `columnHeaders`: `TSP.Reference` to the column-header storage
  (same bucket shape; column widths). [proto]
- Field 3 `tiles`: inline `TST.TileStorage` — the actual cell data (below). [proto]
- Field 4 `stringTable`, field 5 `styleTable`, field 6 `formula_table`:
  `TSP.Reference`s to `TST.TableDataList` objects. Field 11 `format_table_pre_bnc`,
  field 22 `format_table`, field 13 `merge_region_map`, field 18
  `conditionalstyletable`, field 19 `commentStorageTable`,
  field 21 `control_cell_spec_table`. [proto]
- Fields 9/10 `rowTileTree`, `columnTileTree`: inline `TST.TableRBTree`s —
  flattened red-black trees mapping strip index -> tile/storage key
  (`TableRBTree.Node { key = 1, value = 2 }` as a flat `repeated Node`).
  numbers-parser ignores these trees and reconstructs the row map from the
  header buckets instead. [proto, parser: masaccio/numbers-parser@3238795
  src/numbers_parser/model.py:300-313 (`row_storage_map`)]
- Fields 7/8 `nextRowStripID`, `nextColumnStripID`. [proto]

### Header buckets -> row identity

Rows are stored sparsely: an empty row has no storage buffer. The authoritative
row list is the ordered `rowHeaders.buckets`; bucket `k`'s `headers[i].index` is
the model row index. numbers-parser builds `row -> storage-buffer ordinal` from
this (`row_storage_map`, model.py:300-313): iterate buckets in order, each
`Header.index` maps to the next storage-buffer slot. A missing row means no
buffer (all-empty row). Columns work the same way through `columnHeaders`. [parser:
masaccio/numbers-parser@3238795 src/numbers_parser/model.py:300-313]

## Tiles and cell storage

`TST.TileStorage` (TSTArchives.proto:145-152): `repeated Tile { tileid = 1,
tile = TSP.Reference }` (field 1), optional `tile_size = 2` (rows per tile),
optional `should_use_wide_rows = 3`. Tiles are separate archive objects
referenced by id; the `tileid` is a 0-based tile ordinal (row block number).
[proto; parser: masaccio/numbers-parser@3238795 src/numbers_parser/model.py:1292-1306,
writer sets `tileid = tile_idx` and `tile_size = 256` (constants.py:31,53
`DEFAULT_TILE_SIZE`/`MAX_TILE_SIZE` = 256)]

`TST.Tile` (TSTArchives.proto:135-143): `maxColumn`, `maxRow`, `numCells`,
`numrows`, `repeated TileRowInfo rowInfos = 5`, `storage_version = 6`,
`last_saved_in_BNC = 7`, `should_use_wide_rows = 8`. Modern files are BNC
("big nested container") storage: `storage_version = 5`,
`last_saved_in_BNC = true`, `should_use_wide_rows = true`. [proto; parser:
masaccio/numbers-parser@3238795 src/numbers_parser/model.py:1072-1085 —
raises `UnsupportedError` on `last_saved_in_BNC == false`; writer emits
version 5 / wide rows, model.py:1271-1306]

`TST.TileRowInfo` (TSTArchives.proto:123-133) is one stored row:

- `tile_row_index = 1` — row index **within the tile** (writer computes
  `row - tile_row_offset` where `tile_row_offset = tileid * 256`; model.py:1170). [proto +
  parser: masaccio/numbers-parser@3238795]
- `cell_count = 2`. [proto]
- `cell_storage_buffer_pre_bnc = 3` / `cell_offsets_pre_bnc = 4`: legacy
  (pre-BNC) copies, present but stale in modern files. [proto]
- `cell_storage_buffer = 6` (bytes): concatenation of per-cell buffers. [proto]
- `cell_offsets = 7` (bytes): packed little-endian signed 16-bit column offsets
  into the buffer. With `has_wide_offsets = 8` the stored values are
  quarter-offsets (multiply by 4). Negative offsets (typically -1) mean the cell
  is absent/empty; the cell's span runs to the next positive offset (or the end
  of the buffer). There is one offset slot per column of the table. [proto +
  parser: masaccio/numbers-parser@3238795 src/numbers_parser/model.py:2781-2836
  (`get_storage_buffers_for_row`), encoding mirrored in model.py:1161-1195]

Per-cell buffer layout (v5, decoded in
`Cell._from_storage`, cell.py:830-960; encoded in `_to_buffer`, cell.py:990-1130):

- Byte 0: storage version (must be 5).
- Byte 1: cell type — the `TST.CellType` enum (TSTArchives.proto:12-23):
  genericCellType=0, spanCellType=1, numberCellType=2, textCellType=3,
  formulaCellType=4, dateCellType=5, boolCellType=6, durationCellType=7,
  formulaErrorCellType=8, automaticCellType=9 (rich text). numbers-parser also
  recognizes a currency cell type (see its `CURRENCY_CELL_TYPE`,
  cell.py:855-955 region). [parser: masaccio/numbers-parser@3238795
  src/numbers_parser/cell.py:855-955]
- Bytes 6-8: extra/format-kind bits (byte 6 bits select per-type format slots).
- Bytes 8-12: little-endian int32 storage flags; payload fields follow in flag
  order: 0x1 decimal128 number (16 bytes), 0x2 double (bool/duration), 0x4
  double seconds-since-2001-epoch date, 0x8 string-table id, 0x10 rich-text id,
  0x20 cell style id, 0x40 text style id, 0x80 cond style id, 0x100 cond rule
  id, 0x200 formula id, 0x400 control-cell id, 0x800 formula-error id, 0x1000
  suggestion id, 0x2000/0x4000/0x8000/0x10000/0x20000/0x40000 number/currency/
  date/duration/text/bool format ids. All ids are int32 keys into the
  table's `TST.TableDataList` tables. [parser: masaccio/numbers-parser@3238795
  src/numbers_parser/cell.py:855-911, 1064-1129]

The `TST.CellValueType` enum (TSTArchives.proto:26-36: empty=0, number=1,
string=2, provided=3, date=4, bool=5, duration=6, error=7, richText=8,
currency=9) is the proto-side value taxonomy used by command archives; the
tile buffers use `CellType` byte-1 values instead. [proto]

Dates: doubles are seconds relative to 2001-01-01T00:00:00Z (`EPOCH` in
cell.py; `DateCell` decode `EPOCH + timedelta(seconds=seconds)`, cell.py:928).
[parser: masaccio/numbers-parser@3238795]

## Data lists (string/style/formula tables)

`TST.TableDataList` (TSTArchives.proto:227-258): `listType` (STRING=1, FORMAT=2,
FORMULA=3, STYLE=4, FORMULA_ERROR=5, CUSTOM_FORMAT=6, MULTIPLE_CHOICE_LIST_FORMAT=7,
RICH_TEXT_PAYLOAD=8, CONDITIONAL_STYLE=9, COMMENT_STORAGE=10, IMPORT_WARNING=11,
CONTROL_CELL_SPEC=12), `nextListID`, and `repeated ListEntry` where
`ListEntry { key = 1, refcount = 2, string = 3, reference = 4, formula =
TSCE.FormulaArchive = 5, format = 6, custom_format = 8, rich_text_payload = 9,
comment_storage = 10, cell_spec = 12 }`. The per-cell buffer ids index into
these lists by `key` (strings via `DataLists.lookup_value`, model.py:133-216;
formula ASTs via `formula_ast`, model.py:1061-1069). [proto + parser:
masaccio/numbers-parser@3238795]

## Cell value union (command/archive side)

For editing/commands, cells travel as `TST.Cell` (TSTArchives.proto:624-653):
`valueType = 2` (a `CellValueType`), with `numberValue = 5` (double),
`stringValue = 6`, `boolValue = 7`, decimal 128-bit split across
`decimal_value_low = 32` / `decimal_value_high = 33`, plus per-kind
`TSK.FormatStructArchive` slots (fields 9, 11-14, 16-17, 19, 30-31), style
references (3/4/21), rich text (20), comments (23), borders (27), and
`cell_spec` (28, a `TST.CellSpecArchive` for control cells; its field 2 is a
`TSCE.FormulaArchive`). [proto]

`TSCE.CellValueArchive` (TSCEArchives.proto:1084-1097) is the TSCE-side union:
`cell_value_type` (NIL/BOOLEAN/DATE/NUMBER/STRING) plus typed payloads and
`error_value` (`TSCE.ErrorCellValueArchive`, TSCEArchives.proto:1075-1082). It
appears in popup menus (`TST.PopUpMenuModel.tsce_item`, TSTArchives.proto:171),
conditional-style thresholds, pivot summaries, and group values. [proto]

## Merges

The DataStore's `merge_region_map` (field 13) references a
`TST.MergeRegionMapArchive` (TSTArchives.proto:655-657): repeated
`TST.CellRange`, where `CellRange { origin = CellID, size = TableSize }`
(TSTArchives.proto:106-109). `CellID.packedData` and `TableSize.packedData` are
fixed32 with **column in the high 16 bits and row in the low 16 bits**
(unpack `>> 16` / `& 0xFFFF`; packed as `(col << 16) | row`). [proto + parser:
masaccio/numbers-parser@3238795 src/numbers_parser/model.py:919-924 (unpack),
model.py:1153-1154 (pack)]

numbers-parser resolves merges in priority order (model.py:936-942
`merge_cells`): (1) the merge owner's formula store — merge ranges appear as
`COLON_TRACT_NODE` ASTs in `TST.MergeOwnerArchive` (field 47 of TableModelArchive;
model.py:849-876); (2) `TSCE.FormulaOwnerDependenciesArchive` range dependencies
filtered to `OwnerKind.MERGE_OWNER` (model.py:877-908); (3) the
`merge_region_map` CellRanges (model.py:909-935). Within a merged region only
the top-left cell carries content; other cells decode as merge references
(cell.py:941-947 + `MergeCells` bookkeeping, model.py:102-130). [parser:
masaccio/numbers-parser@3238795]

## Coordinate recap

- Model dimensions: `TST.TableModelArchive.number_of_rows/columns`. [proto]
- Tile number = `row / tile_size` (`TST.TileStorage.tile.tileid`); in-tile row
  = `row % tile_size` = `TileRowInfo.tile_row_index`. Column position comes
  from the offset slot index, not an explicit field. [inferred from
  numbers-parser writer model.py:1262-1306 + reader model.py:1071-1097; verify
  against fixtures for tables whose tiles are not row-block-aligned]

## Formula linkage

Cell buffers reference formulas by int32 id into the `FORMULA`-type
`TST.TableDataList` (`DataStore.formula_table`, DataStore field 6), whose
entries carry inline `TSCE.FormulaArchive` values. See [calcengine.md](calcengine.md)
for the TSCE side. The style/format tables are analogous `TableDataList`s of
type STYLE/FORMAT/CUSTOM_FORMAT. [proto]