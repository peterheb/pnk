# TSCE Calc Engine

The iWork calculation engine ("TSCE") stores formulas as archived ASTs, not as
source text. This doc records what the format carries and why pnk's viewer scope
defers re-executing it. Provenance: `.scratch/otorp/Keynote/TSCEArchives.proto`
(`package TSCE`, 15.3.1 extraction) plus masaccio/numbers-parser behavior.

## What a formula archive is

`TSCE.FormulaArchive` (TSCEArchives.proto:807-817):

- Field 1 `AST_node_array`: inline `TSCE.ASTNodeArrayArchive` — the formula body
  as a flat `repeated ASTNodeArchive` list in postfix (RPN) order. [proto]
- Fields 2-5: `host_column`, `host_row`, `host_column_is_negative`,
  `host_row_is_negative` — the host cell the formula was authored in, so
  relative references can be re-based. Fields 7-9: `host_table_uid`,
  `host_column_uid`, `host_row_uid` (UUID forms for uid-addressed tables). [proto]
- Field 6 `translation_flags` (`TSCE.FormulaTranslationFlagsArchive`,
  TSCEArchives.proto:802-809: excel_import_translation,
  contains_uid_form_references, contains_frozen_references, ...). [proto]

`TSCE.ASTNodeArrayArchive.ASTNodeArchive` (TSCEArchives.proto:682-741) is a
union-by-field node: `AST_node_type` plus optional payloads
(`AST_function_node_index`/`numArgs`, `AST_number_node_number`,
`AST_string_node_string`, reference sub-messages, whitespace strings, sticky
bits, ...). The `ASTNodeType` enum (same message) covers ~50 node kinds:
arithmetic/comparison operators (ADDITION_NODE=1 ... NOT_EQUAL_TO_NODE=12),
FUNCTION_NODE=16, literals (NUMBER/BOOLEAN/STRING/DATE/DURATION_NODE 17-21),
LOCAL_CELL_REFERENCE_NODE=27, CROSS_TABLE_CELL_REFERENCE_NODE=28, COLON_NODE=29,
UID_REFERENCE_NODE=48, LET_BIND_NODE=52 / VAR_NODE=53 / LAMBDA_NODE=55,
CATEGORY_REF_NODE=66, COLON_TRACT_NODE=67, SPILL_RANGE_NODE=70. pnk does NOT
need the full node taxonomy — see the enumerated list directly in the proto if
ever needed. [proto]

Formula text is never stored; it is re-synthesizable from the AST (numbers-parser
does exactly this). [parser: masaccio/numbers-parser@3238795
src/numbers_parser/formula.py:12-226 (`Formula` renders AST nodes to text)]

## How formulas attach to tables

Two independent attach points, both by `TSP.Reference`/key rather than inline:

1. **Cell buffers.** A cell's storage buffer carries flag 0x200 + an int32
   formula id (see [tables.md](tables.md), per-cell buffer flags). That id
   indexes a `FORMULA`-type `TST.TableDataList` (`DataStore.formula_table`,
   `.scratch/otorp/Keynote/TSTArchives.proto` → `TST.DataStore` field 6;
   `TST.TableDataList.ListEntry.formula = 5` is an inline
   `TSCE.FormulaArchive`). [proto + parser: masaccio/numbers-parser@3238795
   src/numbers_parser/model.py:1061-1069 (`formula_ast` walks
   `base_data_store.formula_table` entries and keys the AST node lists)]
2. **Non-cell formula owners.** Conditional styles, sort rules, hidden-state
   formulas, category/pivot aggregates, and merge owners reference formulas from
   their own archives (e.g. `TST.CellSpecArchive.formula = 2`,
   TSTArchives.proto:724-731; `TST.FormulaPredArgDataArchive`,
   TSTArchives.proto:864+; `TableInfoArchive.pasteboard_coord_mapper` is a
   `TSCE.CoordMapperArchive`, TSTArchives.proto after line 310). [proto]

Dependency tracking is a separate archive: `TSCE.FormulaOwnerDependenciesArchive`
(TSCEArchives.proto:410-428; `owner_kind = 3` selects the owner kind and
`range_dependencies = 5` holds `back_dependency` records with
`InternalRangeReferenceArchive { owner_id = 1, range = RangeCoordinateArchive = 2 }`).
numbers-parser uses these to find merge owners (model.py:877-908). [proto +
parser: masaccio/numbers-parser@3238795]

References inside the AST: `AST_local_cell_reference_node_reference` /
`AST_cross_table_cell_reference_node_reference` sub-messages hold absolute and
relative coordinate lists; cross-table refs carry the target
`TSP.CFUUIDArchive table_id` plus whitespace strings (TSCEArchives.proto
`ASTCrossTableReferenceExtraInfoArchive`). Cell coordinates in TSCE are
`TSCE.CellCoordinateArchive { packedData = 1 (fixed32), column = 2, row = 3 }`
(TSCEArchives.proto:976-980); ranges are
`TSCE.RangeCoordinateArchive { top_left_column, top_left_row, bottom_right_column,
bottom_right_row }` (TSCEArchives.proto:844-850). [proto]

## Why pnk defers the calc engine

pnk is a **viewer**, not a spreadsheet runtime. For rendering:

- **Evaluated results are already stored.** The last calculated value of a
  formula cell lives in the cell value itself (tile buffer: number/string/date/
  bool/error payload — see [tables.md](tables.md)). No re-execution needed to
  display what the author last saw. [inferred: cell storage layout carries
  values, not formulas, for every typed cell; confirmed against numbers-parser's
  reader, which never evaluates formulas to produce cell values]
- **Rendering formulas as text** only needs AST → string synthesis over
  `ASTNodeArchive` nodes; numbers-parser's `TableFormulas.formula()`
  (formula.py:229-265) does this with a ~30-entry node-type dispatch table
  (`NODE_FUNCTION_MAP`). A Go port of that dispatcher is tractable and
  fixture-verifiable without implementing function semantics. [parser:
  masaccio/numbers-parser@3238795 src/numbers_parser/formula.py:229-266]
- **Re-execution (TSCE evaluation) is out of scope**: it requires function
  semantics (~400 functions), dependency graph recalculation, volatile
  functions, and locale/date coercion — a full engine, worthless without an
  editing surface. Cross-table references (multi-sheet workbooks) would also
  pull in the whole object graph. [inferred: scope decision for the 48h
  hackathon viewer; revisit only if pnk adds editing]
- Editing would additionally require rewriting `TSP.Reference`/UUID ownership
  (`OwnerUIDMapperArchive`, TSCEArchives.proto) and dependency archives —
  more evidence this is the right cut line. [inferred]

## Known unknowns (fixture-verification queue)

- Whether `packedData` in `TSCE.CellCoordinateArchive` uses the same
  column-high/row-low 16-bit split as `TST.CellID` (numbers-parser only unpacks
  the TST form, model.py:919-924). [inferred: same convention likely; verify]
- Pre-BNC (`last_saved_in_BNC = false`) tables are explicitly unsupported by
  numbers-parser (model.py:1075-1077); pnk should treat them as
  render-with-best-effort and flag. [parser: masaccio/numbers-parser@3238795]
- numbers-parser's formula renderer warns (does not fail) on node types absent
  from its dispatch table — the Go port should do the same instead of
  erroring. [parser: masaccio/numbers-parser@3238795
  src/numbers_parser/formula.py:244-253]

## Related

- [tables.md](tables.md) — tile/cell storage, buffer flags, formula id linkage.
- [objects.md](objects.md) — TSP object graph and reference resolution.
- [iwa.md](iwa.md) — snappy container framing for the CalculationEngine files.