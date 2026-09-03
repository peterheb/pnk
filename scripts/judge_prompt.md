# Render-fidelity judge prompt — version 2

Two images follow. The FIRST is the GOLDEN reference: a page, worksheet, or
slide as exported to PDF by the application that created the document
(Pages, Numbers, or Keynote). The SECOND is the CANDIDATE: the same page as
rendered by a third-party viewer. Rate how faithfully the candidate
reproduces the golden.

Judge in this order: (1) is it the same page at all; (2) is all the content
there and readable — text, images, tables, charts, shapes, backgrounds;
(3) is the layout right — positions, sizes, column and row structure, page
breaks, alignment; (4) typography and styling — fonts, weights, sizes, line
breaks, colors, borders, spacing.

Ignore differences that are not the viewer's fault: overall image scale or
resolution, JPEG or anti-aliasing artifacts, a page shadow or frame around the
page, a slightly different paper margin around the content, and hairline
gridline weight.

If the page is a spreadsheet (Numbers), the golden is a printed page and the
candidate is the sheet as shown on screen. Also ignore: page breaks and rows
or columns that continue past the golden's page edge, page orientation and
margins, the scale of the tables relative to the image, faint background
gridlines outside the tables, and the language or separator style of dates
and numbers (a value such as "1.722,22" against "1,722.22", or "févr." against
"Feb", is the same value). Judge the tables' content, structure, cell
formatting and colors, the text boxes, and the charts.

Score on this scale:

- 0 — Different page. The two images differ so completely that the candidate
  could plausibly be a different page of the document, a blank, or the wrong
  file; treat this as a pipeline or alignment failure, not a rendering
  quality issue.
- 1 — Same page but unusable. Most of the content is missing, garbled, or
  unreadable.
- 2 to 4 — Bad. Major content is missing or wrong, or the layout is broken
  enough to mislead a reader. Use 4 when most content is present and
  readable but severe layout or formatting problems remain.
- 5 — Neutral bad. All content is present and readable, but there are
  serious layout or formatting problems: elements in the wrong place,
  overlapping, wrong sizes, missing images, charts, or backgrounds.
- 6 — Recognizably the same page with moderate layout or style differences.
- 7 to 8 — Office-suite quality. Everything is present and in the right
  place; visible differences remain in fonts, spacing, line breaks, colors,
  or chart styling. Use 8 when those differences are small.
- 9 — Near-identical. Only minor pixel-level differences: anti-aliasing,
  hairlines, sub-point shifts, font hinting.
- 10 — Pixel-perfect or indistinguishable.

Answer with ONE JSON object and nothing else, using exactly these keys:

{"score": <integer 0-10>,
 "alignment": "same" | "unsure" | "different",
 "content": "complete" | "partial" | "missing",
 "issues": [<up to six short strings naming concrete differences, most important first>],
 "summary": "<one sentence>"}
