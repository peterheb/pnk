# G7 — Pages mini-fixture (build checklist)

Build by hand in Pages (word-processing template), save to
`fixtures/golden/G7-golden-pages-mini.pages`. Small and targeted: the four
features G5 does NOT exercise (verified in its archives, 2026-08-30).

1. **Two-column section**: after an intro paragraph, insert a section break,
   set the new section to 2 columns (Format > Layout > Columns) with a
   visible gutter, and fill it with ~2 paragraphs of text so both columns
   populate. Return to 1 column in a third section.
2. **Footnote near a page boundary**: enough body text that a footnote's
   ANCHOR lands in the last 3 lines of page 1 — the footnote body must force
   the page break to move up (tests page-space reservation during packing).
   Add a second, ordinary footnote mid-page for contrast.
3. **Table of contents**: Insert > Table of Contents > Document at the top,
   with at least two heading levels present so it populates (tests the TOC
   attachment model; today it renders as an unknown band).
4. **Drop cap variant**: one paragraph with a 2-line drop cap covering 2
   characters (G5's is 3-line/1-char — this catches parameter handling).

Export the same doc to PDF from Pages alongside and keep both.
