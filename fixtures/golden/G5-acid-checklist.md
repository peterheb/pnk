# G5 — Pages word-processing "Acid" fixture (viewer-feature sweep)

Build by hand in Pages 26.3.1. Goal: one doc that exercises every feature the
viewer/model can express (plus deliberate unknown-type probes). This complements
G1 (text/unicode torture) — G5 is about **features**: lists, tabs, tables,
fields, footnotes, sections, floats, media.

Save to `fixtures/golden/G5-golden-pages-acid.pages` (save early, save often —
expected JSON is re-synced from whatever the file says at fix time).
Paste exotic strings from the fenced blocks below — hand-typing composes
codepoints and defeats byte-exact checks. Insertion-point lesson from G2: don't
press Ctrl-D in Pages; some key events route into the hidden body flow.

## Setup

1. New blank document (word processing default template).
2. Document panel (right sidebar, Document tab): set **Landscape** orientation —
   exercises meta orientation + wider page-frame rendering.
3. Set the doc title: **"pnk acid g5"** as first heading-ish line (plain bold
   text, 24pt — not a layout-style title box).

## Paragraph styles & layout

4. Alignment zoo — four one-line paragraphs: left, **center**, **right**,
   **justified** (Format > Text). One sentence each so justification actually
   stretches.
5. Indents — one paragraph with **first-line indent** (Format > Layout >
   First), one with **left indent + hanging** (left 36pt, first −36pt).
6. Line spacing — one paragraph **150%**, one **exactly 28pt** (Format >
   Layout > Line spacing dropdown: Multiple / Exactly).
7. Space before/after — one paragraph with **12pt before + 12pt after**.
8. Hyphenation **off** on one long-word paragraph, **on** (Format > Layout >
   Hyphenation checkbox) on another long-paragraph.
9. Keep-together: one paragraph with **"Keep lines together" + "Keep with
   next"** checked (Format > More panel).
10. Tab stops — one paragraph with **center tab at 3"** and **right tab at 6"**;
    content: `left<TAB>centered<TAB>right-end` (real Tab key in body text is
    fine — Option-Tab is only needed inside table cells).

## Lists

11. Numbered list, **restart**: three lines "One"/"Two"/"Three" as a numbered
    list (Format > List > Numbered), then three more lines "Restart One"/"Two"
    as a SEPARATE numbered list that restarts at 1 — bullet semantics from G1
    must show restart-on-One here too.
12. Nested bullets: 3 levels deep —
    `L0 bullet` / `L1 bullet` (Tab once) / `L2 bullet` (Tab twice) — marker
    indent must grow with level.
13. Lettered list (a. b. c.) two items; roman list (I. II. III.) two items.
14. **Checklist** (Format > Checklist, one item) — PROBE: may emit an
    unknown-type warning; that's the point.
15. Apply built-in paragraph styles from the styles drawer: **Quote** (one
    line) and **Caption** (one line) — named-para-style references.
16. Apply built-in character style **Emphasis** (italic-ish) to one word —
    named-char-style reference in the cStyle pool.

## Character zoo (one paragraph, "zoo:")

17. Runs, in order: **bold**, *italic*, <u>underline</u>, s̶t̶r̶i̶k̶e̶through̶,
    a **green + 20pt** run, a **Menlo** run (different font family),
    superscript `x²` (Format > Font > Baseline > Superscript), subscript
    `H₂O`, one run set to **German (de-DE)** language (Format > Advanced >
    Language & Region per-run override if offered — else skip), and a
    character **highlight/background-color** run if the UI offers it.

## Unicode (paste from blocks below)

18. A paragraph mixing: composed `café`, fi-ligature word `file`, NBSP between
    two words (copy from block — must appear as `\u00a0` in our JSON after the
    escaping change), ideographic space, one straight-quote probe
    `'single' "double"` and `...` typed-as-three-dots (paste; normal typing
    elsewhere may smart-quote — that's fine and expected).
19. CJK paragraph: `日本語のテキスト、句読点と「括弧」。ひらがな・カタカナ・漢字`
20. RTL paragraph: `العربية مع رقم 123 مضمّن` — logical order, embedded digits.
21. Bidi mixed: `English ثم عربي then English again`.
22. Emoji paragraph: family ZWJ, flag, skin-tone thumb, ™ + ™️ (variation
    selectors) — from the block.
23. Soft line break: one paragraph, Shift-Return mid-way (`Soft<TAB>line
    break` style like G1: two lines in one paragraph, U+2028).

## Inline objects & fields

24. Inline image: Insert > Photo/Choose, small, click **Move with text**
    (inline) — an InlineObjectRun with attachment.
25. Hyperlink: select "acid link", Cmd-K → `https://example.com/acid`.
26. Insert > **Footnote** with rich content inside (a bold word). Later a
    second footnote containing CJK + emoji. (Tests footnote streams, not just
    the mark.)
27. Fields: Insert > **Page Number** in the body once, Insert > **Page Count**
    once, Insert > **Date & Time** — set the date field to **not**
    auto-update (fixed) so saves stay byte-stable.

## Table (Insert > Table — plain, 4 columns × 4 rows)

28. Header row ON (Table panel > Header Rows = 1) + **Alternating row
    color** ON — table style + stripes.
29. **Merge** two cells in row 2 (select both > Format > Merge Cells).
30. Number formats: column 2 **Currency** ($1,234.56 pattern), column 3
    **Percent**, column 4 **Date & Time** — the formats pool.
31. Formula: in the header row's last cell put `=SUM(B2:B3)` — formula cell.
32. Cell styling: one cell **fill color**, one cell **vertical alignment
    bottom**, one cell with a long wrapped string.

## Floating objects (anchored on body page 1)

33. Textbox, floating, two runs inside (bold + colored) — "Shapes can hold
    text too" story from G2.
34. Plain shape (star or pentagon): fill + stroke + **shadow** + **opacity
    50%** + **rotation ~15°**.
35. Second shape; select both > **Group** (Arrange > Group).
36. Floating image with **text wrap: around** (Format > Arrange > Text Wrap).
37. Z-order: select the group > Arrange > **Send to Back**.
38. Lock one shape (Arrange > **Lock**).

## Sections & columns

39. Insert > **Page Break** (body flow page break — the thing G1/G2 taught us
    Pages stores in the text stream).
40. Insert > **Section Break**. In section 2: Format > Layout > Columns = **2**.
    Add a couple of body paragraphs there so the column split shows.

## Headers/footers

41. Footer: Insert > Page Number. Header: type "acid header". Section 2: leave
    "Match previous section" ON for both — headersFootersMatchPrevious.

## Probes (stretch — include if quick, expected to warn)

42. Insert > **Chart** (bar, a few data points) — chart decode/render probe.
43. Insert > **Equation** (LaTeX `E=mc^2`) — attachment-image probe.

## Stability rules

- PASTE the exotic lines from the block below; never hand-type composed chars.
- No trailing spaces at paragraph ends (Pages strips those on save — by
  design); internal/mid-paragraph extra spaces are fine and wanted.
- Close the document when done (no lingering edit session).

```
café file café​ file           <- line 1 uses composed é + fi; then paste NBSP line below
acid with NBSP between: acid<NBSP>nbsp
日本語のテキスト、句読点と「括弧」。ひらがな・カタカナ・漢字
العربية مع رقم 123 مضمّن
English ثم عربي then English again
👨‍👩‍👧‍👦 🏳️‍🌈 👍🏽 ™ ™️
'single' "double" ... straight-quote probe
Soft↩line break target: type "Soft" press Shift-Return type "line break"
```

(That block contains, in order: U+FB01 ligature in "file", composed é;
U+00A0 NBSP in "acid nbsp"; CJK incl. corner brackets and middle dots;
RTL Arabic with embedded digits; bidi mix; ZWJ family / flag / skin-tone /
trademark pair; straight quotes + three-dot ellipsis.)

## After you save (our loop, not yours)

We convert with `pnk2json --pretty`, eyeball against the file's embedded
`preview.pdf` and a viewer screenshot, author `expected/G5-golden-pages-acid.json`
to storage truth, wire it into the golden assertion test, and commit the pair.
Anything that renders wrong in our viewer vs Apple's preview becomes a
converter/viewer ticket, not a fixture edit.
