# Render-fidelity judging with vision models

## Why this exists

pnk renders Pages, Numbers, and Keynote documents in the browser. The
measure of the viewer is how closely its output matches what the Apple
apps themselves produce. Until now that comparison was done by eye: export
a document to PDF from the real app, screenshot the same pages in the
viewer, put them side by side, and look. That works for a dozen documents
in an afternoon and does not work for a corpus of 1,248.

`scripts/judge.py` automates the looking. It sends each golden/candidate
image pair to one or more vision language models with a fixed scoring
rubric and records a 0–10 score per page. Two things make the scores
usable rather than just numbers:

- **Controls.** Every document also gets a pair that should score 10 (the
  golden image against itself) and a pair that should score 0 (the golden
  image against a different page). A model that gets these wrong is not
  reading the page, and its other scores are discarded.
- **Cross-judge agreement.** Several models score the same pairs, and the
  report shows how well each agrees with the others. A model that is cheap
  to run can be checked against one that is expensive; if they rank the
  pages the same way, the cheap one can be used at scale.

The intended loop is: run the corpus through a judge, sort by score, read
the worst pages, fix the viewer, re-run. The judge does not find bugs; it
finds the pages worth a person's time.

## Scale

The rubric is in `scripts/judge_prompt.md`. In short:

| score | meaning |
|---:|---|
| 0 | a different page; treat as a pipeline or alignment failure |
| 1 | same page, unusable |
| 2–4 | major content missing or wrong, or layout broken enough to mislead |
| 5 | all content present and readable, serious layout problems |
| 6 | same page with moderate layout or style differences |
| 7–8 | everything present and placed; font, spacing, or color differences |
| 9 | near-identical |
| 10 | indistinguishable |

The model answers with a JSON object: score, whether it thinks the pair is
the same page, whether the content is complete, up to six named
differences, and a one-sentence summary. The prompt is versioned; change
`PROMPT_VERSION` in `judge.py` when it changes, because results are cached
per version.

## Running it

### 1. Produce image pairs

```
uv run --with pillow --with pyobjc-framework-Quartz --with pymupdf \
  python3 scripts/visual_diff.py --app pages --fixture some.pages --out runs/some
```

This opens the document in the real app (macOS only), exports a PDF,
rasterizes it to `runs/some/apple/page-N.png`, screenshots the same pages
in the viewer with Playwright to `runs/some/ours/page-N.png` (`sheet-N` for
Numbers, `slide-N` for Keynote), and writes side-by-side composites. Page N
is paired with page N. `--base-url https://pnk.vu` uses the live site
instead of a local build.

### 2. Score the pairs

```
export LUNA_API_KEY=...
uv run --with pillow python3 scripts/judge.py run \
  --runs-root runs \
  --judge luna=https://openrouter.ai/api/v1,openai/gpt-5.6-luna \
  --judge pixel=pixel,pixel \
  --controls --max-pages 4 --concurrency luna=4 --effort default \
  --out judge-out
```

`--runs-root` takes a directory of run directories (repeatable), `--run`
a single one. `--max-pages N` caps pages per document. Results append to
`judge-out/judgments.jsonl`; a second invocation sends only requests that
have no successful result yet, so judges can be added one at a time.

A judge is `--judge name=<spec>`:

| spec | meaning |
|---|---|
| `name=<base-url>,<model>[,api-key]` | any OpenAI-compatible chat completions endpoint: OpenRouter, a vendor API, or a local server (vLLM, llama.cpp, Ollama) |
| `name=anthropic,<model>` | the Anthropic Messages API |
| `name=pixel,pixel` | block SSIM between the two images mapped to 0–10; no network, used as a floor |

The API key is read from `<NAME>_API_KEY` (upper-cased judge name, so
`LUNA_API_KEY` in the example), then `OPENAI_API_KEY`, then the optional
third field of the spec. Anthropic judges use `ANTHROPIC_API_KEY`. Local
servers usually need no key. To use a different OpenRouter model, change
the model id; the ids are listed at https://openrouter.ai/models.

The judge name is part of the cache key. Use a new name for a new model, a
prompt experiment, or a different thinking setting.

Options that matter:

- `--effort`: what to send about thinking. `default` sends nothing and is
  right for hosted APIs. `none` (the harness default) sends the fields that
  turn thinking off on vLLM for DeepSeek, GLM, and Qwen templates
  (`reasoning_effort` and `chat_template_kwargs`); `low`, `medium`, `high`
  pass those levels through. Details in `effort_params()` in `judge.py`.
  Use one setting per run: on some servers changing it invalidates the
  prompt prefix cache and every request is re-prefilled.
- `--concurrency N` and `--concurrency name=N`: parallel requests per
  judge. Default 1. See the lab notes before raising it against a local
  server.
- `--timeout` seconds per request (default 240). Note that a client killed
  mid-request leaves the request running on the server.

### 3. Report

```
uv run --with pillow python3 scripts/judge.py report --out judge-out
```

writes `judge-out/report.md`: per judge, mean and median score, control
accuracy, mean score per app and per document; for each pair of judges,
Spearman rank correlation, mean absolute difference, and the share of
pairs scored within one point of each other; and the pairs the judges
disagree on most, which are the ones to look at by hand.

On macOS the first request to a LAN address triggers the Local Network
permission prompt for the terminal application.

## Lab notes, 2026-09-02/03

We ran the harness with three recently released open-weight vision
models that fit on hardware we already had, served locally with vLLM, and
with Claude Fable 5.1 through the Anthropic API as the reference.

Inputs: 28 documents from the corpus (12 Pages, 8 Numbers, 8 Keynote), up
to 4 pages each: 87 real pairs, 28 identity controls, 22 misaligned
controls (six documents have one page and get no misaligned control); 137
requests per judge. Each image is resized to 1100 px tall and JPEG-encoded
before sending. Thinking was off unless the judge name says otherwise.

| judge | model | mean score | identity correct | misaligned correct | seconds per pair |
|---|---|---:|---:|---:|---:|
| claude | Claude Fable 5.1 (reference) | 5.91 | 28/28 | 20/22 | 7.9 |
| qwen | Qwen3.8-Flash-Next, thinking off | 6.66 | 28/28 | 21/22 | 10.2 |
| qwen-low | Qwen3.8-Flash-Next, low thinking | 6.18 | 28/28 | 20/22 | 28.4 |
| glm | GLM-5.3-Flash | 6.64 | 28/28 | 20/22 | 37.6 |
| deepseek | DeepSeek V4 Flash | 7.40 | 22/28 | 7/22 | 7.8 |
| pixel | block SSIM | 6.56 | 28/28 | 2/22 | 0.6 |

Mean score by app:

| judge | Keynote | Numbers | Pages |
|---|---:|---:|---:|
| claude | 7.31 | 3.95 | 5.69 |
| qwen | 7.81 | 4.79 | 6.61 |
| qwen-low | 7.41 | 4.74 | 5.86 |
| glm | 7.97 | 4.32 | 6.69 |
| deepseek | 8.69 | 4.63 | 7.72 |
| pixel | 8.12 | 3.89 | 6.58 |

Agreement of each judge with Claude on the 87 real pairs. Bias is the
judge's mean score minus Claude's.

| judge | Spearman ρ | mean abs. difference | within 1 point | bias |
|---|---:|---:|---:|---:|
| qwen-low | 0.94 | 0.60 | 92% | +0.28 |
| qwen | 0.93 | 0.95 | 76% | +0.75 |
| glm | 0.91 | 0.92 | 82% | +0.74 |
| deepseek | 0.71 | 1.89 | 55% | +1.49 |
| pixel | 0.54 | 1.76 | 57% | |

Between the local judges: GLM and Qwen (thinking off) ρ 0.94, 82% within
one point; Qwen thinking off vs low ρ 0.91.

Seconds per pair are wall-clock at the concurrency used (DeepSeek 2, GLM 1,
Qwen 4, Claude 4); GLM's figure includes a period when its server was
failing (below). The Claude run used about 470k input tokens.

### Conclusions

- **Qwen3.8-Flash-Next is a usable stand-in for Claude on this task.** With
  low thinking its scores are within one point of Claude's on 92% of pages
  and 0.28 higher on average. With thinking off it ranks pages equally well
  (ρ 0.93) but scores 0.75 higher, at a third of the time per pair. Use low
  thinking when the absolute score matters, thinking off when ranking is
  enough.
- **GLM-5.3-Flash is also usable** (ρ 0.91 with Claude) but was four times
  slower on our hardware.
- **DeepSeek V4 Flash is not usable under this prompt.** With thinking off
  it fails 15 of 22 misaligned controls and 6 of 28 identity controls, and
  scores 1.5 points above Claude. It gave 10 to a Pages cover whose rotated
  title bar the viewer places on the wrong edge, and 8 to a Numbers sheet
  whose lower half was missing from the screenshot. With thinking on it
  used the entire 8,000-token output budget on an identical pair and
  produced no verdict.
- **Every judge, including the pixel baseline, ranks Numbers lowest and
  Keynote highest.** Claude's means: Keynote 7.3, Pages 5.7, Numbers 4.0.
  Numbers is where the viewer's fidelity work is.
- **The pixel baseline is not a substitute.** It passes the identity
  control but scores 2 of 22 misaligned pairs correctly, because slides in
  one deck share a template, and its agreement with Claude is ρ 0.54.

### Things learned about running local vision models as judges

- **Reasoning models need thinking off or tightly bounded.** DeepSeek V4
  Flash ignores `reasoning_effort` low/medium/high on vLLM and thinks
  until `max_tokens`. `reasoning_effort: "none"` or
  `chat_template_kwargs.thinking: false` turns it off; Qwen's template uses
  `chat_template_kwargs.enable_thinking: false` and
  `chat_template_kwargs.reasoning_effort`. A verdict with thinking off is
  about 80 tokens; the harness caps `max_tokens` at 1,500 in that mode.
- **Keep one thinking setting per run.** On the GLM template the effort
  line is inside the part of the prompt gated on the thinking flag, so
  alternating settings invalidated the prefix cache and re-prefilled every
  request (26 s instead of 0.6 s).
- **Host memory, not GPU compute, was the limit.** The servers ran on
  machines with unified memory. Two configuration choices left about 4–6
  GiB free regardless of model size: the KV cache pool was sized as a
  fraction of total memory, and vLLM's host-side multimodal processor
  cache (`mm_processor_cache_gb`, default 4 GiB) filled with preprocessed
  image tensors, because every image in a run is distinct. Under two to
  four parallel image requests, free memory fell to 0.3 GiB within twenty
  minutes, the kernel evicted the memory-mapped weight pages, and each
  forward pass re-read weights from disk (1.9 GB/s of reads; decode from
  25 to 0.2 tokens per second). The engine later exited on an internal RPC
  timeout, which vLLM logs as a normal shutdown. The same happened with
  the next model after about 130 requests. With the KV pool reduced to
  580k tokens and the processor cache disabled, the server had 27 GiB
  free and stayed there through a full run at four parallel requests.
- **Watch request latency from the client.** The healthy runs held a flat
  median of 8–12 seconds per pair. The failing runs went from 25 seconds
  to several minutes within one five-minute window and did not recover.
- **A killed client leaves its requests running.** Stop a run with the
  harness's own timeout rather than killing the process while requests
  are in flight; each orphaned request keeps generating on the server.

### Problems in the image pairs

Found by reading the pairs the judges disagreed on:

- Numbers' PDF export paginates and scales a sheet onto printed pages,
  while the viewer shows one continuous canvas. Prompt v2 (2026-09-03)
  tells the judge to ignore pagination, margins, scale, and locale for
  spreadsheets. Under v1 the judges had scored page geometry rather than
  the viewer.
- The Numbers screenshot lost rows because the sheet canvas was sized
  from stale table frames; fixed in the viewer the same day (the canvas
  now grows to its rendered content).
- `--max-pages 4` samples short documents as heavily as long ones. Not
  yet changed.

### Numbers, round 1 (2026-09-03, Qwen thinking off)

After prompt v2 and three viewer fixes found by reading the judged pairs
(canvas sized to content; unwrapped cells kept on one line and spilling
over empty neighbors; content-sized text boxes wrapping), Qwen's mean
over the same 19 Numbers pages went from 4.79 (v1, old build) to 6.16
(v2, new build); the MTD tax workbook from 2.8 to 5.2 and the Italian
exam list from 2 to 7. The two numbers are not a controlled comparison,
since the prompt changed as well; the per-page composites are the
evidence for each fix. Remaining Numbers problems the judges name most:
number formats (integers shown as currency, a percentage as $0.95),
category grouping rows, chart legends and axis titles, and cell border
weight and alternating row shading.

### Numbers, round 2 (2026-09-03, Qwen thinking off)

Number formats first, because the judges named them most often. The
worst case was a county budget workbook saved by an older Numbers
(pre-BNC cell storage), where a plain-integer column printed as
"$1,573.00" and a 95% cell as "$0.95". The cause was in the converter,
not the viewer: an old-format cell keeps one format key per kind it has
ever used (number, currency), and the converter took the last one. The
document's own PDF export settled which key Numbers displays (the
leading key; see docs/format/gotchas.md #13). Two more bugs surfaced in
the same workbook once its pages were readable: tiles listed out of
order put the title row of a 43-row table 28 rows down, and a stale
3628pt table frame made the sheet screenshot five times taller than
the table. Accounting-style currency ("$" at the left edge, amount at
the right) is now carried in the model and rendered.

Qwen's scores for that workbook's four pages, before and after:

| page | before | after | what changed |
| --- | --- | --- | --- |
| 1 | 6 | 8 | accounting style and 4-decimal rates; the screenshot no longer clips the right edge |
| 2 | 5 | 9 | integers, percent, and currency all match the export |
| 3 | 1 | 9 | row order and canvas height |
| 4 | 8 | 9 | canvas height (the judge had called the table "scaled down") |

The clip was in the harness: the viewer's app column is 1160px wide and
the sheet area scrolls inside it, so an element screenshot of a wider
sheet stopped at the column's edge. visual_diff now lifts that limit and
widens the browser viewport to the sheet for each shot.

### Numbers corpus scoring (2026-09-03, Qwen thinking off, 2 pages per document)

21 more Numbers documents, one per origin host, were exported from
Numbers and scored alongside the 9 from round 1: 30 documents, 56 pages,
mean 7.27. Score counts: 1 ×1, 2 ×1, 4 ×3, 5 ×1, 6 ×10, 7 ×7, 8 ×19,
9 ×14. Two harness problems surfaced on the way and are fixed in the same
branch: a document query that failed after Numbers had quit silently
turned the whole run into QuickLook previews, and an element screenshot
stopped at the viewer's 1160px column. One converter bug came out of the
scores directly: a Japanese screenshot name stored without the zip UTF-8
flag was decoded as cp437, so its image was reported missing (page
score 1, now 8).

What the judge names most often across the 56 pages, in order:

1. Text clipped in cells where Numbers grows the row to fit wrapped text
   (eight documents; the most common complaint by far). The viewer keeps
   the stored row height.
2. "formula error" shown where the export prints a value (three
   documents): cells whose cached result is absent.
3. Charts: legend missing, series colors swapped, gridlines, axis range
   (two documents with charts).
4. Category grouping rows and their totals missing (one document).
5. Sheet gridlines drawn behind tables (three documents; the export does
   not print them). Prompt v2 tells the judge to ignore them, but they
   still appear in the issue lists.

Two documents cannot be scored fairly by page: a 1,380-row sheet that the
export scales onto one page, and a 1,129 × 192 table (217,000 cells)
that the viewer takes too long to lay out; the second is a performance
item, not a fidelity one.

### Keynote, round 1 (2026-09-04, Qwen thinking off, every page)

Six decks from six origin hosts that no earlier run had judged: RIPE 75
(8 slides), Saint Mary's Press (15), pre-trib.org (19), a Bayesian
statistics lecture from atnf.csiro.au (25), an OCaml effect-handlers
talk from kcsrk.info (37), and a GitHub Actions deck from howtocode.io
(7). All 111 slides were exported from Keynote and scored before and
after the fixes, with the same exports on both sides.

The judge's first-five-pages sample scored 8.5 and named almost nothing
but font hinting; the defects were on later slides and were found by
reading the composites. Each fix was confirmed against Keynote's raster
by measuring rows of pixels, not by eye alone.

| defect | decks | cause | fix |
| --- | --- | --- | --- |
| list rows taller than their text, then the whole body shrunk to fit | RIPE 75 | the 1.5× marker span set the flex row's height | marker contributes no line height |
| code blocks 30% too tight | kcsrk | "at least 20pt" line spacing rendered as exactly 20pt | min/max modes bound the natural height |
| "Questions?" 60pt low, over the email link; a code block 200pt low | RIPE 75, kcsrk | a 0-height box with "middle" alignment hung its text below the anchor | Keynote centres the block on the stored y |
| 124pt cover title on one line off both slide edges | pre-trib | 0×0 box rendered nowrap; its natural size was 1821×370, two lines | wrap at the natural width, bounded shrink for the wider fallback face |
| white mat and shadow around the hidden part of a cropped photo | Saint Mary's | border and drop shadow on the image box, not the mask window | frame and shadow on the window, stroke centred on its edge |
| block arrows drawn as chevrons with a fat shaft | atnf | fixed 0.35/0.45 guesses | converter carries TSD.PointPathSource.point; head 64pt, shaft edge 0.34 |

Qwen's mean over the 111 pages, before and after. Two regressions the
re-judge caught on the way (a stale natural height shrinking a list, and
hanging trailing spaces painting their background across a diagram) are
fixed in the same branch and included in the after column.

| doc | pages | before | after |
| --- | ---: | ---: | ---: |
| RIPE 75 | 8 | 8.2 | 9.0 |
| Saint Mary's Press | 15 | 8.7 | 9.0 |
| pre-trib.org | 19 | 8.7 | 9.0 |
| atnf Bayesian | 25 | 6.0 | 6.3 |
| kcsrk OCaml | 37 | 8.1 | 8.6 |
| howtocode Actions | 7 | 8.6 | 8.3 |
| all | 111 | 7.86 | 8.23 |

Pages that moved by two points or more:

| slide | before | after | what changed |
| --- | ---: | ---: | --- |
| RIPE 75 8 | 5 | 9 | zero-height box centred on its anchor |
| Saint Mary's Press 12 | 5 | 9 | frame and shadow on the crop window |
| atnf Bayesian 10 | 2 | 4 | arrow proportions; the inline equations are still grey boxes |
| atnf Bayesian 19 | 6 | 9 | arrows |
| atnf Bayesian 20 | 6 | 9 | arrows |
| kcsrk OCaml 6 | 4 | 8 | code box position |
| kcsrk OCaml 8 | 4 | 7 | code box position; its curved connectors are still straight |
| kcsrk OCaml 18 | 6 | 9 | code line pitch |
| kcsrk OCaml 20 | 6 | 9 | code line pitch |
| kcsrk OCaml 28 | 7 | 9 | code line pitch and position |
| kcsrk OCaml 33 | 4 | 7 | line pitch |
| pre-trib.org 1 | 4 | 9 | title wraps to two lines |

Slides scored 9 went from 75 to 89 of 111. No slide dropped by more than a point; the ones that dropped one point are
hinting and anti-aliasing verdicts on unchanged renders, plus one OCaml slide where the code box, now in Keynote's place, is crossed by connection lines that should be curves.

What remains, in the order the judge names it:

1. Equations. Keynote stores each equation as a PDF (`equation-N.pdf`)
   with no raster twin, and the viewer draws a grey box for it. 41 of
   the 484 Keynote decks in the corpus carry one; the Bayesian lecture
   has them on 11 of 25 slides, and its score is capped by them. In-
   browser PDF rasterization (pdf.js, Apache-2.0) is the fix; it would
   also cover pasted vector art without a thumbnail. Needs
   `worker-src blob:` in the viewer's CSP.
2. Curved and dotted connection lines drawn straight (kcsrk slide 8).
3. Hand-drawn ("sketch") stroke styles, which the judge names on every
   deck that uses them and which no fix here addresses.
4. Wrap differences from fallback faces (Franklin Gothic, Gill Sans):
   one word per slide moving between lines.

### Pages, round 1 (2026-09-05, Qwen thinking off, two pages per document)

Pages had no judged round of its own; the 2026-09-02 calibration run
scored 12 Pages documents. This round picked 23 more, one per origin
host, chosen from a pnk2json feature survey of all 325 Pages fixtures so
that they cover different things: docx/doc imports with lists and
tables, page-layout newsletters, a two-column landscape bulletin, a
Japanese form, two Arabic documents, a 61-page textbook with a table of
contents, a 24-page service booklet, a 2013-era save, and a chart. All
23 were exported from Pages; the first two pages of each were scored
(41 pairs). Mean 5.46. Score counts: 0 ×3, 2 ×5, 4 ×5, 5 ×3, 6 ×9,
7 ×6, 8 ×7, 9 ×3.

| document | host | pages (Pages / ours) | judged | before | after |
| --- | --- | ---: | ---: | ---: | ---: |
AFTER_TABLE

What the judge names most often across the 41 pages, in order:

1. Content on the wrong page: text from the next page on this one, the
   bottom of the page missing, or a cover page shared with the body
   (eight documents). Some of this is the pagination difference noted
   under problems below; the rest was the viewer ignoring the text wrap
   of floating objects (a cover photo or a full-page text box pushes the
   body to the next page in Pages) and pushing a table under an anchored
   object.
2. Headers and footers missing or drawn differently: absent on page-layout
   canvases, absent where a section inherits them from the previous
   section, and boxed or centred where Pages draws them plain (six
   documents).
3. List numbering: "1, 2, 3 … 16" where Pages prints tiered "1.1 … 4.4"
   (one document, named on both pages).
4. Tables: rows taller than Pages', text not wrapping in cells, borders
   drawn where Pages draws none (four documents).
5. Fonts: weight, a decorative face, an all-caps face (Bebas Neue) shown
   in mixed case (three documents).
6. Right-to-left layout: paragraph alignment and bullet placement in the
   Arabic documents (two documents).
7. One black page (a 0-height rule stored with an 8.9e-17pt natural
   height scaled the stroke to 1.9e8pt).
8. A Gantt chart drawn as stacked bars (Numbers agent's file, not
   touched here).

Defects fixed, with cause and fix:

| defect | documents | cause | fix |
| --- | --- | --- | --- |
| table of contents shown as an "unmodeled TSWP.TOCAttachmentArchive" box | eb2a7cde, 55d37c2b, ee7036ce | attachment type 2241 not handled; the TOC is a ShapeInfo (TOCInfoArchive 2240) with its own laid-out storage | converter: the TOC becomes an inline text box (tab leaders and page numbers included) and `tableOfContents.entries` is filled from its TOCEntryInstanceArchives |
| whole page black | 38a7da36 | a 0-height rule with an 8.9e-17pt natural height; the stroke scale divided by it | viewer: a natural axis under 1e-3pt is degenerate and does not scale the stroke |
| body text on the cover page and every page after it one page early | eb2a7cde (61 pages) | floating objects that wrap the body (the cover photo, a full-page text box) did not push the flow | viewer: wide (≥60% of the text width) wrapping floating objects become full-width exclusion bands; a page they fill holds no body text |
| 350pt gap before a form's table | 964b85d1 | two 38pt anchored seal boxes at the right margin; CSS moved the 493pt table below their float | viewer: a table pushed under an earlier anchor float collapses that float's exclusion and moves back up (Pages overlaps them) |
| list numbered 1 … 16 instead of 1.1 … 4.4 | 48f5f124 | `tiered_numbers` (ListStyleArchive 25) not read; sub-levels never restarted | converter: `ListFormat.tiered` and a computed `Paragraph.listNumber` (restart at a stored number, else continue; deeper levels restart after a shallower item); viewer and markdown dump print the path |
| footer missing on the second section's pages | 48f5f124 | `inheritPreviousHeaderFooter` ignored | viewer: a section whose masters carry no header/footer text takes the previous section's parity template |
| page-layout header "JUN / JUL 26 · ISSUE 3 · ©" and page number missing | 26a356dc | (1) the document's field-48 template container is a type the registry does not know, and the page masters the sections reference were skipped; (2) layout canvases never drew headers/footers | converter: every TP.PageMasterArchive in the graph joins `pageTemplates`; viewer: layout canvases draw their section's headers/footers and page-number fields |
| "en" language on every run of a document whose locale is en_US | (regression caught by the G2 golden) | table_language spans split runs even when the tag equals the locale | converter: run boundaries and `language` only where the emitted language changes |

Qwen's scores for the same 41 pairs, same exports, before and after are
in the table above. AFTER_SUMMARY

#### Schema and converter findings

Data that was in the archives and absent or wrong in the JSON, and what
was done (proof fixtures in parentheses):

- Table of contents: dropped as an unknown attachment. Now an inline
  text box plus `tableOfContents.entries[] { text, pageNumber, level,
  paragraphIndex }` (eb2a7cde: 82 entries; 55d37c2b: 65, deduplicated
  across its seven per-section TOC boxes).
- `TSWPTOCPageNumberAttachmentArchive` read field 3 (the bookmark name)
  before field 2 (the page number); split from NumberAttachmentArchive.
- Comments: `table_highlight` → HighlightArchive → TSD.CommentStorageArchive
  was dropped. Now `comments[] { anchorParagraphIndex, text, author, date,
  quotedText, replies }`; five corpus documents carry them (381bbbac: 9
  comments by one author). Body storage only; comments in text boxes and
  cells are not collected.
- Tracked changes: `table_insertion` / `table_deletion` were ignored, so
  deleted text was emitted as if present. The text is now the accepted
  view (deletions omitted, insertions kept) with one warning carrying the
  counts (55d37c2b: 3 insertions, 1 deletion). The markup itself
  (author, date per change) is not modeled; one corpus document has it.
- Bookmarks: `table_bookmark` was dropped although in-document hyperlinks
  reference bookmarks as `#<uuid>`. Now `bookmarks[] { id, name,
  paragraphIndex }` (eb2a7cde: 3; 55d37c2b: 117), and a run `hyperlink`
  of `#<id>` resolves against them.
- Run language: `table_language` (a string per span) was ignored. Now
  the run's `CharStyle.language` when its primary subtag differs from the
  document locale (964b85d1: en_US runs in a ja_JP form; 77890685: Arabic
  runs in an en_US résumé). Apple's `__multilingual` marker is filtered
  from both the table and the style property.
- List numbering: only the marker style was emitted; numbers were
  computed by the viewer with no sub-level restart and no tiered labels.
  Now `Paragraph.listNumber` (computed) and `ListFormat.tiered`.
- Page masters: only the objects behind DocumentArchive field 48 were
  templates. In 26a356dc that field points at an object of type 0x2721
  and the sections reference the masters directly; every
  TP.PageMasterArchive in the graph is now a template. The corpus has
  1,707 PageMasterArchives across all 323 documents and no
  TP.PageTemplateArchive at all.
- Section and page setup, headers and footers (three column storages
  each), footnotes with anchors, tab stops (position, alignment,
  leader), hyperlinks, inline versus anchored placement with wrap kind
  and margin, tables in the body flow, section columns and gutter: present
  and checked against real documents; no change.
- Document title and author: Properties.plist carries only UUIDs and
  the format version; the only author names in a Pages file are the
  annotation authors. Nothing to extract; `meta.author` stays empty.

Proposals not implemented:

- Linked text boxes (`TSWP.FlowInfoArchive` with several `textboxes`):
  the whole flow lands in the first box and the continuation boxes are
  empty (26a356dc's "Who Are We? Ben, Dan, Mandy…" list). Two corpus
  documents use them. Model: `TextboxDrawable.flow?: { id, index }`;
  the viewer would lay the shared storage out across the chain.
- Text wrap fit: `ExteriorTextWrapArchive.fit_type` and
  `alpha_threshold` are dropped. f82b2fa4's cover is a transparent PNG
  whose frame is opaque; Pages wraps the title into the transparent
  interior, which needs the alpha channel. Carry `fit` and
  `alphaThreshold` in `TextWrap` so a renderer can choose.
- Tracked-change markup (author, date, kind per range) as a
  `changes[]` list; one corpus document.
- Comments inside text boxes and table cells.

#### Problems in the image pairs

- Pages paginates by line with its own font metrics; six of the 23
  documents have a different page count in the export and in the viewer
  (32/35, 61/63, 24/25, 9/8, 1/2, 1/2). From the first page where the
  counts diverge, every pair compares different content and the judge
  scores 0-2 for "a different page". The four documents scored 3.0 or
  lower before the fixes are all of this kind, and the same exports are
  scored after, so the before/after table shares the handicap.
- The judge scores an all-caps display face (Bebas Neue) rendered in a
  fallback font as a capitalization error; the JSON carries no
  capitalization because the font, not the style, is uppercase.

### Next

Numbers: row height from wrapped text, then the cached formula results.
Keynote: PDF media (equations first), then curved connection lines. Score more of the corpus, one or two pages per document, with Qwen; use
the ranked list to choose fidelity work; add a reference re-run with
Claude when the prompt changes again.
