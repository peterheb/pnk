# G6 — Keynote golden "acid" deck (build checklist)

Build by hand in Keynote 15.3.1, save to
`fixtures/golden/G6-golden-keynote-acid.key`. One feature per slide, Basic
White theme unless noted, so failures isolate. Authored by agent K
(round 1, 2026-08-29) from cross-validation gaps; promoted from the campaign
notes into the repo per the G5 pattern.

1. Title & subtitle slide, default placeholders filled ("G6 Acid", "keynote golden").
2. Title & bullets: 3 bullet levels (dot/dash/number markers), one bullet with a soft line break.
3. A slide on a MASTER with dark gradient background + light text (tests resolved background).
4. Photo slide: one raster photo cropped with a mask (drag the crop handles), plus the same photo
   un-cropped at 50% size, one rotated 45°.
5. Shapes sampler: rectangle, rounded rect, oval, star, arrow, a 6pt straight horizontal line, a
   vertical line, a dashed line, a curved (bezier) line — each with distinct fill/stroke colors.
6. Text box (not placeholder) with: bottom-aligned text, middle-aligned text (two boxes), 20pt
   inset padding, and one box rotated 90°.
7. A slide with slide number visible ON; the next with it OFF.
8. One slide with a table (3x3, header row) and one with a bar chart (2 series).
9. A slide using a vector (PDF) placed image AND a gradient background chosen from the theme.
10. Skipped slide (right-click > Skip) between 9 and 11 (tests skipped flag).
11. Presenter notes on the last slide + a build-in animation on its title.

Additions from later rounds (2026-08-30):
12. A text box with MORE text than fits (tests auto-grow) and a title placeholder
    with a very long title (tests shrink-to-fit / textFit).
13. One connection line between two shapes, with an arrowhead on one end.

Export the same deck to PDF from Keynote alongside (File > Export To > PDF,
Best) and keep both.
