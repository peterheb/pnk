# G8 — Numbers grouped-table summary rules (checklist for Peter)

Purpose: name the `ColumnAggregateArchive.agg_type` codes beyond 2 (= Sum),
which pnk2json emits raw in `TableModel.grouping.aggregates[].rule`. One
fixture in the 158-file corpus is grouped (6914f46e51ab) and it uses Sum only.

Build in Numbers 15.x, save as `fixtures/golden/G8-golden-numbers-groups.numbers`:

1. One table, header row, columns: Project (text), Hours (duration), Cost
   (currency), Done (checkbox), Date (date). Ten body rows over three
   project names, two rows with an empty Project.
2. Organize by Project (Table > Organize By > Project).
3. In the group rows, set a different summary per column, one each:
   Hours = Average, Cost = Count, Date = Maximum, Done = Minimum (or
   Count Distinct if offered), and leave one column with no summary.
4. Add a second grouping level (Organize by Date > by month) so nested
   groups and the per-level `level` field get a value.
5. Collapse one group (so the collapsed state, if stored, is present).

What the round-4 decoder will read from it: the code per column, the
cached accumulator kinds (`number_count`, `min_value`, `max_value`,
`number_total_value`) per group, and the label-row / group-row heights.
