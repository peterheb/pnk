# G6 — Numbers golden "acid" sheet (build checklist)

Build by hand in Numbers, save to
`fixtures/golden/G6-golden-numbers-acid.numbers`. One sheet, several small
tables, one feature per item. Authored by agent N (round 1, 2026-08-29) from
cross-validation gaps. Keep values simple/typeable and note each expected
rendering in this file as you build (the G5 pattern) so we can assert
cell-for-cell.

1. **Formats table** (one column per family, 3 rows each): Number w/ 1,000s
   separator ON and decimals=3; Currency USD grouped + EUR ungrouped + a
   negative in red-parens style; Percent 2dp; Fraction (halves AND "up to 2
   digits"); Scientific 2dp AND auto; Base 16 and Base 2; Duration in all
   three styles (compact/short/long) + one automatic-units; Date as `d`,
   `EEE, MMM d, yyyy`, and a custom format with literal text (e.g. `"wk" w`);
   Text format on a number; checkbox, star rating, slider, stepper, pop-up
   menu (control cells).
2. **Widths/heights table**: columns at exactly 30/60/120/240pt; rows at
   15/30/60pt; one hidden row and one hidden column; one cell with wrap ON
   containing a long sentence, one with wrap OFF that clips.
3. **Merge table** 5×5: a 1×3 horizontal merge, a 3×1 vertical merge, a 2×2
   block, and one merged cell centered+middle-aligned with a fill.
4. **Style table**: header row 2 deep + header column + footer row; banded
   rows ON with a visible color; one whole ROW styled via select-row
   (fill+bold), one whole COLUMN styled; individual cell borders: top-only
   heavy 3pt, dashed bottom, dotted right, "no border" override inside an
   otherwise bordered range; two different gridline visibility settings
   (vertical lines OFF).
5. **Table name** shown for exactly one table (checkbox ON), hidden for the rest.
