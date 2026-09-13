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

`--align-content` (Pages runs only; add `--with pymupdf` to the `uv run`)
pairs each export page with the viewer page whose text overlaps it most
instead of page N with page N: the export's text comes from
`apple/export.pdf`, the viewer's from the `ours/page-N.txt` dumps that
visual_diff writes next to each screenshot (runs made before 2026-09-05
have none and keep the default pairing). Overlap is shared words over the
smaller page's words; a page with fewer than 8 words on either side keeps
its own number. The record's `candidate_page` says which viewer page was
scored. Use it to score the render itself when the viewer's pagination
drifts from the app's; the default pairing scores pagination too.

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

### Keynote, round 2 (2026-09-05, Qwen thinking off, every page)

The two decks round 1 left worst: the atnf Bayesian lecture (25 slides,
55 equations) and the kcsrk OCaml talk (37 slides, connection lines).
Both were exported from Keynote once; the same exports are on both sides
of every score below. Schema and converter fixes came first, per the
round's brief; the viewer changes follow from them.

| defect | deck | cause | fix |
| --- | --- | --- | --- |
| equations drawn as grey boxes | atnf (11 of 25 slides) | Keynote stores each equation as a PDF with no raster twin; the source expression sat unread in `TSWP.EquationInfoArchive` extension fields on the image | converter: `ImageDrawable.equation` (source, format, depth, font); dumpers emit the source; viewer: pdf.js rasterizes PDF media in-page (bundled, blob: worker, no network) |
| curved connection lines drawn as two straight segments | kcsrk slide 8 | the stored path is move+line+line whose middle point is ON Keynote's quadratic; the converter rebaked it as a polyline scaled between trimmed chord endpoints | converter emits move+quad through the middle point, centre to centre, cut back where the curve leaves each shape's outline |
| dotted lines drawn nearly solid | kcsrk slides 7, 8 | `StrokePatternArchive.pattern` is in multiples of the stroke width, the model said points; [1,1] on a 2pt stroke rendered 1pt on/off | converter multiplies by the width and truncates to `count`; the export measures 2pt on / 2pt off |
| connection line to a shape moved after baking ends in empty space | kcsrk slide 8 ("k" box) | a free start plus a stale stored end | connected ends follow the shape's current centre; free ends stay where stored |
| callout tail missing | kcsrk slide 7 | the viewer drew a 10pt triangle on the top edge instead of a wedge to `tailPosition` | viewer draws the wedge from the facing edge to the apex (not scored: landed after the judge run) |

Qwen's mean over the 62 pages, before and after, same exports:

| doc | pages | before | after |
| --- | ---: | ---: | ---: |
| atnf Bayesian | 25 | 6.28 | 8.72 |
| kcsrk OCaml | 37 | 8.70 | 8.81 |
| all | 62 | 7.73 | 8.77 |

Pages scored 9 or more went from 43 to 53 of 62. Pages that moved by two
points or more, all up: atnf 2, 7, 8, 9, 10, 11, 12, 13, 15, 16, 17
(equations; 2 -> 9 on six of them) and kcsrk 8 (6 -> 9, the curves) and
21 (5 -> 8). Four pages dropped one point: three are hinting verdicts on
unchanged renders; kcsrk 6 names arrows that are plain line shapes ending
inside content-sized label boxes, which this round did not touch.

Confirmed against the export by measurement, not by eye: the curve peak
(y 336 measured, 334.8 predicted from the stored middle point), the curve
ends at the box edges (y 350.5 measured, 351 predicted), the dot pitch
(2.4/1.44pt on/off at 150dpi for a 2pt stroke), and the canvas equation on
atnf slide 2 (ink 287x67pt in the export, 285x66pt in ours).

#### Schema and converter findings

- **Equation source text was dropped.** `TSWP.EquationInfoArchive`
  extends `TSD.ImageArchive` with `equation_source_text` (103, LaTeX or
  MathML as typed; `equation_source_old` 100 is the same text in older
  files), `equation_depth` (102, baseline depth in points) and
  `equation_text_properties` (101: font size, name, colour). 52 of 484
  Keynote decks in the corpus carry equations; every sampled source is
  LaTeX. Added `ImageDrawable.equation` (TS + serde, documented in
  model-design.md and format/drawables.md); the text and markdown dumpers
  print the source in place of the image.
- **Dash patterns were in the wrong unit.** Fixed in the converter; the
  model's "points" contract now holds. No golden output changed.
- **Connection line routing type** (`ConnectionLinePathSourceArchive.type`,
  quadratic/orthogonal) was ignored; the path now encodes it (quad vs
  line elements). Not added as a field: the path carries it.
- **Hand-drawn stroke identity was dropped.** `Stroke.smartStroke` now
  carries the preset name ("Pencil", "Dry Brush", "Feathered Brush",
  "Chalk2", "Crayon", "Pen" in the corpus); the brush parameters stay
  dropped. G2 gains two names; expected JSON re-synced.
- **Inline equation size differs from the stored geometry in Keynote's
  export.** The image's geometry equals the PDF page (e.g. 203x40pt for
  `y=mx+b` at 45pt), and ours renders that. Keynote's export draws the
  same inline equation at a font-dependent scale: 1.208x when
  `equation_text_properties.font_name` is HelveticaNeue (45 -> 54.36pt,
  28 -> 33.82, 30 -> 36.24, 66 -> 79.72, read from the export's text
  spans), 1.005x for TimesNewRomanPS-ItalicMT, and 0.89 to 1.10x across
  the fonts of a second deck (1199f5d2). Canvas-level equations are 1:1.
  The factor is not derivable from the archive with what is known now
  (1.208 = HelveticaNeue's ascender over STIX's, which fits one font
  and not the others). Left as stored; noted for a later round.
- **Checked and present:** presenter notes (every slide carries a
  storage; both decks' are empty), skipped flags, transitions, builds,
  hyperlinks on runs, accessibility descriptions, group structure. Not
  present: a structured slide title (the dumpers derive it from the
  title placeholder; a `Slide.title` would save every consumer that
  walk), and empty notes are emitted as an empty `StyledText` rather
  than omitted. Both are proposals, not implemented.

What remains, in the order the judge names it:

1. Inline equation scale (above): the export draws them up to 21% larger
   than the stored geometry.
2. Plain line shapes that end inside content-sized (0x0) text boxes
   (kcsrk slide 6): Keynote stops them at the laid-out text edge.
3. Hand-drawn stroke rendering: the preset name is now in the model; the
   viewer still draws a plain stroke.
4. Wrap differences from fallback faces, unchanged from round 1.
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
| f82b2fa40fd4 | apostlesonline.org | 24 / 25 | 2 | 1.0 | 1.0 |
| 964b85d1b8b9 | i-campus.hokkyodai.ac.jp | 1 / 1 | 1 | 2.0 | 8.0 |
| ae1cc13b298f | rustedradishes.com | 7 / 7 | 2 | 3.0 | 2.5 |
| cf4b76a33f5a | johnwheeldonacademy.co.uk | 32 / 35 | 2 | 3.0 | 4.5 |
| 26a356dc8651 | strokeinformation.co.uk | 6 / 6 | 2 | 3.5 | 4.0 |
| 38a7da366cc3 | lakecitypresbyterian.org | 5 / 5 | 2 | 3.5 | 8.0 |
| eb2a7cde90d6 | paadopt.org | 61 / 67 | 2 | 3.5 | 7.0 |
| e2e0bff371c1 | financialplanningindubai.com | 3 / 3 | 2 | 5.0 | 6.0 |
| 48f5f124cdd9 | schule-schlotheim.net | 9 / 9 | 2 | 5.5 | 7.5 |
| 7b8e38edb184 | immobilienundleben.de | 9 / 9 | 2 | 5.5 | 5.5 |
| cace32e1ed60 | bdrp.ch | 5 / 6 | 2 | 5.5 | 6.0 |
| 77890685af37 | sa-uc.edu.iq | 1 / 1 | 1 | 6.0 | 6.0 |
| f43d849f63dd | likvi.de | 1 / 1 | 1 | 6.0 | 6.0 |
| d88d9139e2f5 | img.lucensoftware.com | 2 / 3 | 2 | 6.5 | 6.0 |
| 4047e81b0665 | bcss.org | 12 / 12 | 2 | 7.0 | 6.5 |
| 44d11ec89c32 | canineassistants.org | 14 / 14 | 2 | 7.0 | 7.0 |
| 904cec1c6651 | pearlpirie.com | 12 / 12 | 2 | 7.0 | 7.5 |
| 27254104743d | thelastamericanvagabond.com | 4 / 4 | 2 | 7.5 | 7.5 |
| 806df50f6150 | chemiedidaktik.uni-wuppertal.de | 25 / 25 | 2 | 7.5 | 7.5 |
| 87560fc1b5b0 | nfgymcheer.com | 2 / 2 | 2 | 7.5 | 7.5 |
| 1bd116a4fa8f | domaukcyjnyiglica.pl | 1 / 2 | 1 | 8.0 | 9.0 |
| 9a3616c756a7 | easy4me.info | 1 / 1 | 1 | 8.0 | 8.0 |
| bc5e6bd19210 | kobysh.com | 7 / 7 | 2 | 8.0 | 8.5 |
| all | 23 documents | | 41 | 5.46 | 6.27 |

The page counts are Pages' export against the final build; the cover
rule added pages to two documents (paadopt 63 to 67, bdrp 5 to 6) and
removed one from two others (schule-schlotheim 8 to 9 matches Pages
now; the Japanese form went from 2 pages to Pages' 1).

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
in the table above. Mean 5.46 to 6.27 over the 41 pairs; score
counts after: 0 ×2, 2 ×2, 4 ×3, 5 ×5, 6 ×9, 7 ×4, 8 ×10, 9 ×6. Pages
that moved by two points or more: lakecitypresbyterian 1 (0 to 9, the
black page), paadopt 1 (2 to 9, the cover), the Japanese form (2 to 8,
the table under the seals), schule-schlotheim 2 (4 to 8, tiered numbers
and the inherited footer), johnwheeldonacademy 2 (2 to 4) and
financialplanningindubai 2 (4 to 6), both pagination. No page dropped
by two; the four one-point drops are re-judged verdicts on renders that
changed only in line wrapping.

What remains, in the order the judge names it:

1. Pagination against Pages' own line breaks: six documents differ in
   page count and every pair after the divergence compares different
   content. Not a fidelity item the viewer can close without Pages'
   font metrics; a judge that aligns pages by content would score the
   render itself.
2. Table rows: heights taller than Pages' (likvi, immobilienundleben)
   and cell text not wrapping (financialplanningindubai). Numbers agent's
   files.
3. Linked text boxes (the proposal below).
4. Right-to-left paragraph alignment and bullet placement (two Arabic
   documents, unchanged at 6.0 and 2.5).
5. Fonts: weight and all-caps display faces.

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

### Numbers, round 3 (2026-09-05, Qwen thinking off, up to 3 pages per document)

Worked through the corpus-scoring list in order: formula-error cells,
row height from wrapped text, category grouping, charts. Schema and
converter first: every defect was checked against the JSON before the
viewer was touched. Eleven documents from the corpus scoring were
re-rendered against the same Numbers exports and re-scored.

| defect | documents | cause | fix |
| --- | --- | --- | --- |
| "formula error" printed where Numbers prints nothing | 16c9478d6d21, 5a89929253a1, fcb2c1c1c3cd | A formula-error cell (type 8) carries no cached value and, in the whole corpus (37 cells in 4 files), no error record; the converter invented the string "formula error" | Converter emits `v: null, type: "error"`; the viewer prints it blank (docs/format/tables.md §Formula-error cells) |
| Formula text absent from the model | 23 documents, 54,318 formula cells | `TsceFormulaRef` was a placeholder | New `formulas.rs` re-synthesizes the text from the TSCE AST; `status: "decoded"`, `sourceText`. Checked by re-evaluating the decoded text over the document's own grid: 47,496 formulas reproduce their cached values, 0 mismatches (the rest use functions the checker does not implement) |
| Table names missing when the caption is hidden | every table with `table_name_enabled` off | Name dropped with the caption | `name` always carried; `nameHidden: true` marks a hidden caption. Needed because formula text names tables ("Pflichtfächer::Table 1::C13") |
| Wrapped text clipped to one line, rows not grown | 90fbb6c53674 (and the same pattern in 5a89929253a1) | Per-cell styles whose chain does not set `text_wrap` over a wrapping body style; the converter left `textWrap` unset and the viewer read absent as "no wrap" | Converter resolves wrap through the section default (body/header/footer) when the cell chain is silent; rows then grow (a `<tr>` height is a minimum) |
| Category grouping rows and totals missing | 6914f46e51ab | Not in the model | New `TableModel.grouping` (grouped columns, summary rules, group tree with model row indexes, the app's cached totals) from `GroupByArchive`; the viewer inserts the label row and group rows ("▼ Muster 4h 27m") |
| Chart legend missing, no vertical gridlines, series colors "swapped", axis title inside the plot | baabe23e067f | JSON was right (legend visible, colors, `categoryGridlines: true`). The legend frame hangs below the chart frame and the sheet canvas clipped it; category gridlines were never drawn; the viewer painted the last series on top where Numbers paints the first; the category title assumed an in-frame legend | numbers.ts sizes the canvas to the legend frame; drawables.ts draws category gridlines, paints series in reverse order for line kinds, and places the title under the ticks when the legend is outside |

Qwen scores on the same exports, before and after:

| document | page | before | after | what the judge still names |
| --- | ---: | ---: | ---: | --- |
| 16c9478d6d21 | 1 | 4 | 6 | The bar chart in the 'Zusammenfassung' section is missing from the candidate render. |
| 16c9478d6d21 | 2 | 9 | 9 | Candidate image is cropped at the top, cutting off the very top border of the main title row. |
| 16c9478d6d21 | 3 | 8 | 8 | Text in the 'Module' column (e.g., '1.1 Grundlagen...') is significantly smaller in the candidate compared to the golden |
| 5a89929253a1 | 1 | 9 | 9 | Candidate text is rendered at a significantly larger scale than the golden reference |
| 5a89929253a1 | 2 | 7 | 9 | Boolean values in the rightmost columns are capitalized in the golden (e.g., 'TRUE') but lowercase in the candidate ('tr |
| 5a89929253a1 | 3 | 4 | 2 | Data labels (values) are missing from the bars in both charts |
| 5c152beb2a3b | 1 | 8 | 8 | Text in the 'Employee / Non-Employee' section is cut off on the right edge in the candidate |
| 5c152beb2a3b | 2 | 8 | 8 | Minor numerical discrepancies in totals (e.g., $1,060.71 vs $1,060.70) |
| 5c152beb2a3b | 3 | 8 | 8 | Numerical values differ slightly due to rounding (e.g., $8.48 vs $8.47, $228.71 vs $228.70, $849.71 vs $849.70) |
| 6914f46e51ab | 1 | 7 | 8 | Introductory paragraph text wraps differently (4 lines in candidate vs 3 in golden) |
| 6914f46e51ab | 2 | - | 6 | Data labels are placed outside the pie chart in the candidate, whereas they are inside in the golden. |
| 90fbb6c53674 | 1 | 7 | 9 | Date format in header differs ('Feb 19' vs 'févr. 19') |
| 90fbb6c53674 | 2 | 7 | 8 | Number formatting differs (e.g., '1,664.63' vs '1.664,63') |
| b2abceb03dbb | 1 | 9 | 8 | Text wrapping differs slightly in numbered items 1, 3, 4, 5, 6, 7, and 8 due to minor width or font metric differences |
| b2abceb03dbb | 2 | 5 | 5 | Extra table with raw variable names (e.g., 'turnover', 'taxTakenOffTradingIncome') appears on the right side |
| b2abceb03dbb | 3 | 2 | 4 | Layout is broken: the 'INCOME' table is split, with the input cells separated from their labels. |
| baabe23e067f | 1 | 8 | 8 | Y-axis scale on 'User Stories' chart differs (0-24 vs 0-21) |
| baabe23e067f | 2 | 6 | 8 | Chart legend markers changed from hollow diamonds to solid lines |
| baabe23e067f | 3 | 7 | 8 | Chart legend markers changed from hollow circles to solid lines |
| c4b881955676 | 1 | 8 | 7 | Text truncation in the 'WHAT IS CAUSE VALIDATION MATRIX?' section header (missing 'X') |
| c4b881955676 | 2 | 8 | 8 | Text truncation in the 'Project Title' field ('Reduction' is cut off) |
| cab63a6dd0de | 1 | 8 | 7 | Text truncation in the 7th Grade table (e.g., 'PRAYER/STRETCH/WALK' is cut off) |
| eb299192a219 | 1 | 7 | 7 | Text truncation in 'Total Charge' label (missing '(minimum charge is 4kg)') |
| eb299192a219 | 2 | 8 | 8 | Number formatting differs (decimal comma in candidate vs decimal point in golden) |
| eb299192a219 | 3 | 8 | 8 | Visible gridlines present in the candidate render (background artifact) |
| fcb2c1c1c3cd | 1 | 8 | 9 | Slight vertical compression of row heights in the candidate compared to the golden |

Mean over the 25 pages scored both times: 7.12 before, 7.48 after.

The "after" renders come from this branch before the Pages round was
merged; the two viewer-sensitive documents (6914f46e51ab, baabe23e067f)
were re-rendered on the merged tree and the PNGs are byte-identical.
Pages that moved by one point in either direction (c4b881955676 p1,
cab63a6dd0de p1, b2abceb03dbb p1, 5a89929253a1 p3) name the same issues
before and after; the renders of those cells did not change. c4b881955676
and 5c152beb2a3b were flagged for "text wrapping" but their clipped cells
are one-line cells that Numbers fits in slightly wider columns; nothing
in the model was wrong. One regression was caught and fixed on the way:
the first wrap fill ran after `strip_cell_defaults` had erased an explicit
wrap=false, so cab63a6dd0de's Excel-imported no-wrap cells wrapped; the
strip now happens at emission.

#### Schema and converter findings

- `TsceFormulaRef.status` gains `"decoded"`; `sourceText` holds the
  formula text; `warning` is present only when status is `"unparsed"`
  (it was required before; it was one constant row). Chart
  `dataBinding` refs stay unparsed.
- `TableModel.name` is emitted whenever stored; `nameHidden: true` when
  the caption is off. Before, a hidden name was dropped, which left
  cross-table formula references unresolvable.
- `TableModel.grouping` (new, additive): `columns`, `aggregates`
  (`rule` is the stored code; 2 = sum is inferred from one fixture,
  other codes are unnamed), `groups` (value, model row indexes,
  children, cached `totals` with sum/count/min/max), table-level
  `totals`. One grouped table exists in the 158-file corpus, so the
  decoder is verified on one file.
- Formula-error cells: `v: null` with `type: "error"`; the string
  "formula error" no longer appears in output.
- Cell wrap: `textWrap` on a pooled cell style is now resolved through
  the table's section default when the cell's chain is silent.
- Row heights: the format has no per-row fit-to-content flag
  (`HeaderStorageBucket.Header` is index/size/hidingState/numberOfCells).
  The fitted heights exist only as a layout cache
  (`TableInfoArchive.layout_engine.width_height_cache`) that 2 of 3
  flagged files do not carry, so the model keeps the stored size and the
  viewer grows rows from content. Documented in docs/format/tables.md.
- Proposals not implemented: (1) the grouped view's category column
  width (`SummaryModelArchive.category_column_width`, 50pt in the
  fixture) could be carried instead of the viewer's 30pt constant;
  (2) `sourceText` for chart data bindings (`TN.ChartMediatorArchive`
  formulas) through the same decoder; (3) formula text for pre-BNC
  (v4) cells is decoded too, but those cells cache no value, so a
  consumer gets the formula and an empty cell.

#### What remains (ranked)

1. Value-axis top when no bound is pinned: Numbers ends baabe's "User
   Stories" axis at the data maximum (0, 5.25, 10.5, 15.75, 21) but
   rounds "Story Points" to 60 for a maximum of 56; the stored axis
   archives are identical, so the rule is not in the fields we read.
2. eb299192a219's rich-text cell "Total Charge (minimum charge is 4kg)"
   wrapped in the round-2 viewer and is clipped now with an identical
   model (cell style wrap on); viewer/src/text.ts changed in both
   Keynote rounds (ba0f9a2, df3fd4f) and is the place to look.
3. Column widths: several flagged "truncation" cells (c4b881955676's
   "WHAT IS CAUSE VALIDATION MATRIX?", 5c152beb2a3b's disclaimer) are
   one-line cells that fit in Numbers because its columns are a little
   wider; measure the export's column positions against the stored widths.
4. Table names containing operator characters in formula text: Numbers
   may quote them; unverified (66ba951f59ea has names like "＋問題").
5. Group summary rule codes other than 2 (sum) are unnamed; a fixture
   with average/count/min/max groupings would settle them.
6. Formula text for cells whose AST uses durations, LET/LAMBDA, linked or
   category references, or the legacy handle-based reference nodes stays
   `"unparsed"`; none occur in the corpus.

### Numbers, round 4 (2026-09-05, Qwen thinking off, up to 3 pages per document)

Schema and converter first, per Peter's priority. A census of the 158
Numbers fixtures (`crates/pnk2json/examples/ncensus.rs`, `ncharts.rs`,
`nmeta.rs`) listed the spreadsheet metadata the JSON dropped; the round
carried it, then fixed the round-3 regression and two rendering rules.
Five documents from round 3 were re-rendered against the same Numbers
exports and re-scored; twelve documents from hosts not judged before were
exported and scored, two pages each.

| defect | documents | cause | fix |
| --- | --- | --- | --- |
| Chart data bindings opaque | every table-bound chart (718 mediators in the corpus) | `dataBinding` was a placeholder, and only emitted when the chart had no cached grid, which Numbers charts always have | `TN.ChartMediatorArchive` formulas decoded through formulas.rs: `dataBinding.sourceText` = union of the series ranges, new `bindings { series, rowLabels, columnLabels }`; 4,170 of 4,170 binding formulas end in TSCE function id 175 (unknown to numbers-parser), printed as its argument list in chart scope (docs/format/calcengine.md §Chart bindings) |
| Cell comments dropped | 16c9478d6d21 | storage flag 0x80000 not read | `TableCell.comment { text, author, date }` |
| Controls dropped (pop-up menus, checkboxes, sliders) | 5 documents | flag 0x400 read and discarded | `TableModel.controls` pool + `TableCell.control`; pop-up option lists from `PopUpMenuModel` (eb299192a219: three pop-ups, nine unit models) |
| Sort panel rules dropped | 6 documents | `sort_order` (f44) not read | `TableModel.sortRules [{ column, descending }]` |
| Custom format identity | baabe23e067f, 4b5a7b9d32af | only the pattern was carried | `CellFormat.name` ("Plus/Minus Integer", "Custom Format 3") |
| Conditional formatting silent | 12 documents | the fired rule was folded into the style with no trace | per-table `unsupported-feature` warning with cell and rule-set counts |
| Grouped view's category column at a 30pt constant | 6914f46e51ab | `SummaryModelArchive.category_column_width` not carried | `TableGrouping.categoryColumnWidthPt` (50pt); group and label rows at the default row height |
| Rich-text cell "Total Charge (minimum charge is 4kg)" clipped to one line | eb299192a219 | Round 3's `applyCellStyle` runs twice per cell (section, then cell) and added a wrap class each time without removing the other; `.cell-nowrap .styled-text { white-space: pre }` then won. Not viewer/src/text.ts as round 3 guessed | the later pass replaces the class (tables.ts) |
| One-line cells clipped ("WHAT IS CAUSE VALIDATION MATRIX?", "Impact if Addressed") | c4b881955676, 5c152beb2a3b | Column widths are exact: 5c15's export grid lines sit at 72 + the stored cumulative widths, c4b8's header words at 72 + width + 2.5pt padding. The clip is a substitute font: Calibri Bold 9pt is 141.7pt wide in the export in a 159pt column; Calibri is not installed and Helvetica Neue runs about 10% wider | an unwrapped cell with no empty neighbor to spill into gets a bounded horizontal scale (down to 0.82) of its content, as `applyTextFit` does for shapes |
| Value-axis top (0-24 where Numbers prints 0-21) | baabe23e067f | The stored axis archives of the 21 and 60 charts are byte-identical apart from titles; Numbers rounds the maximum, not the step, and labels top×k/N (5.25, 10.5, 15.75, 21; 4,750 steps in 5a89929253a1) | Numbers documents: the maximum rounds up to a multiple of 10^k (k = floor(log10 max)) when its leading digits are 2.7 or more, else to a multiple of 10^(k−1). 34 of 40 exported charts match (39 from baabe23e067f and 5a89929253a1 plus the two Keynote cases already in this file); the previous ladder rule matched 15. The six misses land one unit higher (1650→1800, 2097→2200, 22399→24000). Keynote keeps the ladder |

Qwen scores on the same exports, before and after (the five touched documents):

| document | page | before | after | what the judge still names |
| --- | ---: | ---: | ---: | --- |
| 5c152beb2a3b | 1 | 8 | 9 | Minor vertical spacing differences in the 'For Internal Use Only' section |
| 5c152beb2a3b | 2 | 8 | 9 | Minor numerical rounding differences in totals ($8.48 vs $8.47) |
| 5c152beb2a3b | 3 | 8 | 8 | Numerical values differ slightly in the 'Travel' column ($8.48 vs $8.47) |
| 6914f46e51ab | 1 | 8 | 8 | Introductory paragraph text wraps differently (4 lines vs 3 lines) |
| 6914f46e51ab | 2 | 6 | 6 | Data labels are placed outside the pie slices in the candidate |
| baabe23e067f | 1 | 8 | 8 | Chart markers are hollow circles in the golden but solid dots in the candidate (the axis is no longer named) |
| baabe23e067f | 2 | 8 | 8 | Chart legend markers are solid lines, hollow circles in the golden |
| baabe23e067f | 3 | 8 | 8 | Chart legend markers changed from hollow circles to solid lines |
| c4b881955676 | 1 | 8 | 8 | Header column 'Impact if Addressed (1-5)' wraps to two lines in the candidate |
| c4b881955676 | 2 | 7 | 8 | Header column 'Impact if Addressed (1-5)' wraps to two lines in the candidate |
| eb299192a219 | 1 | 7 | 8 | Number formatting differs (decimal comma in candidate vs decimal point in golden) |
| eb299192a219 | 2 | 9 | 8 | Number formatting differs ('523.4' vs '523,4') |
| eb299192a219 | 3 | 8 | 8 | Number formatting differs (decimal comma in candidate vs decimal point in golden) |

Mean over the 13 pages: 7.77 before, 8.00 after. The eb299192a219 page-2
drop names the same decimal-separator difference before and after (the
document is locale it_IT and Numbers' export prints the machine's
en locale); the render of that page did not change.

Twelve more documents, one per origin host not judged before
(`fixtures/success.tsv`), exported from Numbers and scored, two pages each
(16 pages; 5401d297f316 failed to render in the harness — a sheet-tab
click timed out — and was replaced by 021084ac7183):

| document | host | pages | mean | what the judge names |
| --- | --- | ---: | ---: | --- |
| 17891b89da2f | itdtllc.com | 2 | 8.5 | footer section spacing compressed; empty grid rows at the bottom |
| 181f2b199bd3 | tokyomusicrise.jp | 1 | 6 | the equipment diagram (KEYBOARD / BASS AMP / DRUM groups) drawn beside the song table instead of inside the set-list section; the QR code over the URL; date "1/11(日)" where the export prints "1/11(Sun)" |
| 33499baadcc3 | cdnweb.fakturoid.cz | 1 | 8 | a header row wraps to two lines (one in the export), shifting everything below |
| 3383a82d3b32 | twiki.di.uniroma1.it | 1 | 9 | row spacing, font hinting |
| 4b5a7b9d32af | slaa-ontario.org | 1 | 7 | "$100.00" where the export prints "$100" (a custom "¤#,##0.00' ea.'" format); column widths; alignment |
| 51c6da51390e | dvvfw3pu42z1e.cloudfront.net | 1 | 9 | cropping of the canvas |
| 66ba951f59ea | www.hokudaicoach.com | 2 | 8 | callout wrapping; a box with only a top border; thinner gridlines |
| 737c7eccbed4 | www.rotostreetjournal.com | 1 | 9 | anti-aliasing |
| 9f9ef28d93d7 | www.mushroomcrew.com | 1 | 9 | faint gridlines |
| dfdc8f8391b2 | www.democracyinaction.us | 1 | 9 | font rendering |
| e8625984c6c3 | www.anam.mx | 2 | 4.5 | page 1 scores 9; page 2 of the export is a 30pt-wide strip (the sheet's overflow column), scored 0 as "corrupted" — a harness pairing artifact, not a rendering defect |
| 021084ac7183 | eps-pedagogie.web.ac-grenoble.fr | 2 | 7 | a 1×1 pop-up table painted yellow where the export prints white (a pre-BNC fired-rule index, see findings); zero-height line shapes drawn as 32pt bars |

Mean over the 16 pages 7.63; 8.13 without the strip page.

#### Schema and converter findings

- `ChartModel.dataBinding` is now decoded (`sourceText` = the series
  ranges joined with ","); new `ChartModel.bindings` carries every binding
  formula by role. Charts over grouped tables bind through category
  references (node 66) and stay "unparsed" (6914f46e51ab). The binding was
  never read before because the extraction was gated on "no inline grid".
- `TableCell.comment`, `TableModel.controls` + `TableCell.control`,
  `TableModel.sortRules`, `TableGrouping.categoryColumnWidthPt`,
  `CellFormat.name`: all additive, documented in docs/model-design.md
  §2.6/§2.7 and docs/format/tables.md.
- Already carried and verified against the census: hidden rows and
  columns (`rows[].hidden` agrees with the hidden-state extents on every
  file, 534d58ee7d21: 15 rows + 7 columns), merged ranges, sheet order and
  names and `hidden`, header row/column counts, cell hyperlinks (152 in
  16c9478d6d21), number-format identity (kind, decimals, currency code,
  grouping, accounting, pattern; now also the custom format's name).
- Filters: every `FilterSetArchive` in the corpus has zero rules, so
  nothing to carry yet; the archive path is documented for when one shows
  up.
- Conditional formatting: the rules are not modeled; the warning names
  the count. On pre-BNC files the stored fired-rule index is not
  reliable: 021084ac7183's 1×1 pop-up tables store rule 15 of 48- and
  55-rule sets and the export paints them white where rule 15 is yellow.
  Proposal: for v4 cells, drop the fired-rule overlay unless the rule set
  has fewer rules than the stored index range seen on verified files
  (cdrky: 0-2 of 2-3), or evaluate the predicate for the simple
  "cell equals" kinds.
- Group summary rule codes other than 2 (sum) remain unnamed; the request
  is `fixtures/golden/G8-numbers-groups-checklist.md`.
- Not a schema item but found on the way: the second run of a rich-text
  cell whose char style carries no size (eb299192a219 "(minimum charge is
  4kg)") renders at the cell's 26pt where Numbers draws it smaller; the
  run's resolved style omits the size as a default (12pt), so the viewer
  inherits the paragraph's. Pages-owned text.rs/text.ts; left as a
  proposal.

#### What remains (ranked)

1. 181f2b199bd3: grouped shapes positioned about 100pt above where the
   export draws them, beside the wrong table; the group positions in the
   JSON (y 549-575) match the export's set-list section, so the viewer's
   canvas placement is wrong for these groups, not the model. Keynote-owned
   drawables.ts.
2. Zero-height line shapes on Numbers sheets drawn at their 32pt natural
   height (021084ac7183 page 2); drawables.ts.
3. Chart legend markers: line charts draw a line segment where Numbers
   draws hollow circles or diamonds (every baabe23e067f page); the marker
   shape lives in the series style (`symbol` fields), not read.
4. Number formatting with the document locale (eb299192a219, it_IT prints
   "523,4" in the viewer; the export uses the machine locale): decide
   which one is right for a viewer and make it a setting.
5. The value-axis misses (six of 40 land one unit higher); a fixture with
   twenty charts over maxima 1000-3000 would settle the threshold.
6. Header text size in 33499baadcc3 and 16c9478d6d21 ("text larger /
   smaller than the export"): the header-row text style's size versus the
   per-cell style chain; check which archive the export follows.
7. Custom currency formats with a suffix ("¤#,##0.00' ea.'" prints "$100"
   in the export, "$100.00" here): the pattern's decimals apply only when
   the value has them.
8. 5401d297f316 does not render in the harness (a sheet-tab click times
   out); check whether the 100×25 table with seven user-hidden rows hangs
   the layout pass.

### Keynote, round 3 (2026-09-05, Qwen thinking off, four slides per deck)

Schema and converter work first, per the round's brief, then a corpus
scoring pass: 17 more Keynote decks from 17 origin hosts that no earlier
run had judged, chosen from a feature survey of one deck per host (charts,
tables, image fills, master image backgrounds, groups, movies, builds,
hand-drawn strokes, CJK text, 4:3 and 16:9 sizes, saves from Keynote 6
through 14). All 17 were exported from Keynote once; the same exports are
on both sides of every score. The judge scored the first four slides of
each deck (68 pairs; the first two alone give the same mean to within 0.05,
so the two-slide figure the Numbers corpus section uses is comparable).
The two round-2 decks were re-rendered against their round-2 exports to
confirm the connection-line and equation fixes by eye.

| defect | decks | cause | fix |
| --- | --- | --- | --- |
| no structured slide title; every consumer walked drawables for the title placeholder | all | not modelled | converter derives `Slide.title` at emission; the dumpers read it |
| every slide emitted an empty presenter-notes `StyledText` | all | the storage every slide carries was emitted regardless of content | notes omitted when blank; 2,219 of 12,925 corpus slides keep theirs |
| inline equations up to 22% smaller than Keynote draws them | atnf, ustc | Keynote re-sets an inline equation so the math's x-height matches the run font's: scale = x-height(font) / x-height(STIXGeneral-Italic, 0.428 em) | converter multiplies the inline image's size and baseline depth by the factor from a table of x-heights and records it in `equation.displayScale` |
| connection lines into content-sized (0x0) label boxes ran into the word | kcsrk slides 6, 14, 17, 32 | a 0x0 frame has no laid-out box, so the stored endpoint (the text's centre anchor) was kept | the anchor walk builds the laid-out box (path natural size hung from the anchor by the text's alignment); the line ends at its edge |
| the outline polygon for connection anchors was never built | kcsrk | the anchor walk read `msg(3)` on the DrawableArchive level (text wrap), not the ShapeArchive's path source | reads the level above |
| slide-number fields drawn as "‹page number›" | icecube slide 2, s.u-d-l, ecanja | page-number/page-count fields carry no value in the archive | converter fills the value with the slide's position and the show's slide count |
| slide numbers at the browser default size and colour | ripe82, LIGO | a field item took the attachment's empty style, not the run's resolved style | field items take the run's style unless the attachment carries one (text.rs, shared) |
| background-removed images drawn with their original rectangle; a crop window showed a legend Keynote leaves blank | icecube slide 1 | `TSD.ImageArchive.instantAlphaPath` (field 10) was dropped; 1,276 images in 54 corpus decks carry one | additive `ImageDrawable.instantAlphaPath`; the viewer clips the image to it |
| 0x0 shapes with text painted nothing | deeplearningbook footer, handtracker affiliation marks | the 0-height-shape rule adopted the path height and left a 0-wide box | a 0x0 shape takes the content-sized text path with its path natural size |
| superscripts at full size | handtracker cover | CSS `vertical-align: super` keeps the size | 2/3 size, raised to the cap height (text.ts, shared; measured 50px against 77px caps at 150dpi) |
| thin shapes (rules, footer bars) drawn a few points low; ecanja's 6pt footer bars fell off the slide | ecanja, and every deck with divider lines | an inline `<svg>` rests on its line box's baseline, so a shape shorter than the strut's ascent was pushed down by the gap | `.canvas-drawable > svg { display: block }` (styles.css); measured in the DOM: the bars' top moved from 521px to 509.6px against the frame, where the stored geometry puts them |
| a bottom-aligned 0-height slide-number box hung its number below the slide edge | icecube slide 1 | the vertical anchor shift is a fraction of the box's own height, and a 0-height box with an absolutely placed text layer stayed 0 tall | the box takes its content's height before the shift (not scored: landed after the judge run) |
| hand-drawn strokes drawn plain | ripe76, greenberg, ripe85 | the preset name was in the model, the viewer ignored it | displacement + grain filter for Chalk/Crayon/Pencil/Dry Brush; Pen and Feathered Brush stay plain (Keynote's export of them differs from a plain stroke only by a taper) |

Qwen's mean over the 68 pages, before and after, same exports. The corpus
ranking (per deck, first four slides, before the fixes) doubles as the
list of where to look next.

| doc | host | features | pages | before | after |
| --- | --- | --- | ---: | ---: | ---: |
| c3582f317d54 | events.icecube.wisc.edu | 17-fonts,groups,dynamicwave,reflection | 4 | 7.00 | 8.25 |
| 2bb490dc3bad | www.deeplearningbook.org | 1024x768,image-fills,M6.6 | 4 | 7.25 | 7.75 |
| c0f7137c5111 | indico.psi.ch | connection-lines,builds | 4 | 8.50 | 8.50 |
| 2bf304c480eb | ecanja.eu | 720x405,tables,groups,many-media | 4 | 8.50 | 8.50 |
| 5e6cf24f0405 | ripe82.ripe.net | charts,table,gradient-theme | 4 | 8.75 | 8.75 |
| 3775cc34726f | indico.pnp.ustc.edu.cn | cjk,equations,movies,table | 4 | 8.75 | 8.75 |
| b6b440463fe4 | handtracker.mpi-inf.mpg.de | M6.6,movies,groups,builds | 4 | 8.75 | 9.25 |
| c184e5a76807 | ipbriopreto.org.br | image-bg-every-slide,image-fills,notes | 4 | 8.75 | 8.75 |
| a72a174eabc2 | www.kab-bayern.de | 720x405,custom-theme,image-bg,notes,M7 | 4 | 8.75 | 8.75 |
| 1e4ab104ce4a | dcc-llo.ligo.org | 720x540,custom-theme,M7 | 4 | 8.75 | 8.75 |
| 40c5f2efeb36 | greenberg.science | tables,builds,smartStroke,notes,M9 | 4 | 9.00 | 9.00 |
| 79259c0f302c | assets.science.nasa.gov | 960x540,master-bg,movies,reflection,notes | 4 | 9.00 | 9.00 |
| 7e31810eb36b | s.u-d-l.com | cjk-heavy,no-media,black-theme | 4 | 9.00 | 9.00 |
| 5c82aee24d64 | matthew.brecknell.net | groups-129,basicblack | 4 | 9.00 | 9.00 |
| 9d5dcf6003a5 | ripe76.ripe.net | charts,4:3,smartStroke,M8 | 4 | 9.25 | 9.25 |
| 70699bd2790f | makeabilitylab.cs.washington.edu | movies,notes,16-fonts | 4 | 9.25 | 9.00 |
| a8c00fb99049 | kuwapyon.net | T2.2.1-save,cjk,movie,720x540 | 4 | 10.00 | 10.00 |
| all | | | 68 | 8.72 | 8.84 |

Pages that moved: up — deeplearningbook 2 (7 → 8), deeplearningbook 3 (7 → 8), ustc 1 (9 → 10), handtracker 1 (9 → 10), handtracker 3 (8 → 9), icecube 1 (6 → 7), icecube 2 (7 → 8), icecube 3 (7 → 9), icecube 4 (8 → 9); down — ustc 2 (10 → 9), makeabilitylab 4 (10 → 9). Slides scored 9 or more went from 49 to 52 of 68; the first two slides alone give 8.74 → 8.85. The pages that dropped a point are hinting and "slightly lower" verdicts on renders whose only change is the thin-shape and superscript rules; icecube 1 (6 → 7) is the Instant Alpha fix, icecube 2 (7 → 8) the slide number, icecube 3 (7 → 9) the thin-shape rule.

Confirmed against the export by measurement, not by eye: the inline
`y = mx + b` on atnf slide 10 (245.4pt wide in the export's text spans,
245.7pt in ours after the fix, 203.4pt before); the export's STIX sizes
54.36 / 48.88 / 54.67 / 30.15pt against the stored 45 / 40 / 50 / 30pt for
HelveticaNeue / HelveticaNeue-Light / AvenirNext-Regular / Times Italic,
each equal to the CoreText x-height ratio to four digits; the arrow into
"computation" on kcsrk slide 6 ending at x=203.1 (the label's left edge)
instead of 269.4 (its centre); the superscript "1" on handtracker's cover
50px tall against 77px caps with its bottom 26px above the baseline.

What the judge names most often across the 68 pages, in order:

1. Font substitution (25 complaints, 14 decks). Faces this Mac does not
   have (CMU Serif, Produkt, Graphik, Poppins), where Keynote and the
   browser pick different fallbacks; the judge also reads a bold fallback
   as a substitution (icecube's `Produkt-Light` run with `bold: true`).
   Nothing to extract; not addressed.
2. Missing elements (24, 10 decks): footer citations, slide numbers,
   affiliation marks above logos, footer bars, a crop window Keynote
   leaves blank. All fixed this round (0x0 shapes, field values, thin
   shapes, Instant Alpha) except one chart legend (below).
3. Position drift of a few points, "slightly lower" (19, 12 decks). The
   thin-shape baseline shift accounts for the divider lines; the text
   cases are not explained yet.
4. Colour and background (17, 9 decks): slide-number colour (fixed),
   background tone "slightly warmer" on image-backed slides (colour
   management of the export raster versus the browser; not addressed).
5. Slide numbers (16, 6 decks): placeholder text, size, colour. Fixed.
6. Line breaks from font metrics (16, 7 decks): one word moving between
   lines; the fallback-face problem from rounds 1 and 2.
7. Images (8, 6 decks): background-removed images drawn with their
   rectangle (fixed), logos a few points off.
8. Charts (3, 2 decks): line-chart markers Keynote hides, a legend the
   export omits (proposals below).

#### Schema and converter findings

- **Slide titles and empty notes** (proposed in round 2, implemented):
  `Slide.title` is the plain text of the first title placeholder with
  text. 5,879 of 12,925 corpus slides get one. A deck whose author typed
  titles into free text boxes and left the placeholder empty (RIPE 77,
  bc5a842a) gets none; the field means "title placeholder text", and a
  largest-text heuristic was not added. `Slide.notes` is now present only
  when the storage has visible text.
- **Inline equation display scale** (round 2's open item): found. Keynote
  sets an inline equation so that STIX's x-height matches the run font's
  x-height; the stored PDF is at the nominal size and the export re-sets
  it. `EquationInfo.displayScale` (additive) records the factor; the
  converter applies it to the image's size and depth. The x-height table
  covers the fonts that set inline equations in the corpus (588 of 1,776
  inline equations are HelveticaNeue); fonts not in the table keep the
  stored geometry.
- **Instant Alpha paths were dropped.** `ImageDrawable.instantAlphaPath`
  (additive) carries the kept region in naturalSize pixel space. The
  keep-inside reading rests on one deck's export; docs/format/drawables.md
  marks it inferred.
- **Slide-number fields had no value and no style.** Both fixed at
  emission (value: converter; style: text.rs field items now take the
  run's resolved style).
- **Connection anchors** for 0x0 text boxes: the laid-out box is derived
  at emission from the path natural size and the text's alignment, the
  same rule the viewer uses to place such boxes. Not a model change: the
  line's path already carries the result.
- **Chalk stroke colour.** ripe76 stores the circles' fill colour as the
  Chalk2 stroke colour; Keynote draws a pale speckled ring. The model has
  the name and colour as stored; the lightening is a viewer rule marked
  inferred. The brush parameters (`TSD.SmartStrokeArchive` field 5, a
  reference dictionary) stay dropped.
- **Charts on slides** (Numbers-owned files, not changed; proposals):
  ripe82 slide 4's line chart draws data-point markers that Keynote's
  export does not (no per-series symbol flag in `ChartModel`); ripe76
  slide 10's column chart shows a legend square that the export omits
  (`legendVisible` absent, `legendFrame` at (-397, -277), outside the
  chart: an off-chart frame should read as hidden); the same chart's
  reference lines ("RIPE NCC pool", "/8") are not modelled.
- **Fonts.** 14 of 17 decks drew a font-substitution complaint. Most are
  faces this Mac lacks (CMU Serif, Produkt, Graphik), where Keynote and
  the browser fall back differently; nothing to extract. One is a data
  shape: icecube's footer run is `Produkt-Light` with `bold: true`, which
  the browser synthesises as bold on the fallback face and Keynote draws
  regular. Left as is; a rule that a weight-named face overrides the bold
  flag would need a fixture with the font installed.
- **Locale.** ripe82's table prints "3,91%" in ours and "3.91%" in the
  export: the document locale is de and Keynote formats with the machine's
  locale. Not a defect in the model.
- **Checked and present:** builds, transitions, hyperlinks, skipped
  flags, movies (poster + bytes), groups, masks, master backgrounds and
  image fills on all 17 decks.

What remains, in the order the judge names it:

1. Wrap differences from fallback faces: one word per slide moving
   between lines, on ten of 17 decks. The natural-size widths in the
   archive bound this for content-sized boxes; fixed boxes have no stored
   layout to lean on.
2. Charts on slides: markers, hidden legends, reference lines (above).
3. Text position drift of a few points ("slightly lower"), named on 12
   decks without a common cause found; RIPE 82's footer and greenberg's
   title are the cases to measure first.
4. Hand-drawn strokes: the filter approximates the look; the brush
   parameters are still not read.

### Pages, round 2 (2026-09-05, Qwen thinking off, two pages per document)

The 23 round-1 documents, scored on the same exports as round 1 (round
1's final scores are the "before" column), plus 277d7233 (salemub.org, a
church bulletin), the second corpus document with linked text boxes,
exported fresh. Mean 6.27 to 6.95 over the 41 comparable pairs; score
counts after: 0 ×1, 2 ×2, 4 ×2, 5 ×2, 6 ×10, 7 ×3, 8 ×7, 9 ×14. Four
documents moved by two points or more (f82b2fa4 1.0 to 9.0, 7b8e38ed 5.5
to 9.0, 26a356dc 4.0 to 6.0, 77890685 6.0 to 8.0, eb2a7cde 7.0 to 9.0);
the largest drop is one point (cf4b76a33f5a, 4.5 to 3.5: pages 1 and 2
render as before, the page-2 verdict moved from 4 to 2).

Defects fixed, with cause and fix:

| defect | documents | cause | fix |
| --- | --- | --- | --- |
| every text box backed by a `text_flow` came out empty: the two linked chains, and 14 single boxes in 8 documents | 26a356dc ("Who Are We?"), 277d7233 (two-column announcements), 3c73e668, 494113af, 619804f0, 7629eb7b, 77890685, 806df50f, ab78e6eb, bdbcfdc2 | the converter preferred `owned_storage` (empty beside a flow) and read `text_flow` through the TSP.Reference wrapper, which never resolved | converter: the flow's `text_storage` wins; a flow with 2+ boxes emits `TextboxDrawable.flow { id, index, count }` with the text on index 0; viewer: after layout the lines that do not fit a box move to the next box in the chain |
| cover title on page 2, 25 pages against Pages' 24, every later pair off by one | f82b2fa4 | a "Move with Text" image filling the page (524×810 on a 576×774 printable area) excluded the body; Pages flows the text over such an object, since the anchor paragraph cannot leave the page without it | viewer: an anchored object that leaves no room (full column width, reaching the printable bottom) excludes nothing and paints behind the text |
| text-box text 1.25× too large | 7b8e38ed page 2, and every 12pt run in a text box | `strip_char_defaults` dropped a resolved `fontSizePt` of 12 (model-design §1.5 gives the field no default), so the viewer used its 15px chrome size | converter: the size is kept (pooled styles, no per-run cost); goldens G1/G2 re-synced after a visual check, four `fontSizePt: 12` lines |
| a 66pt title box grew to 200pt and covered the box below it | 7b8e38ed page 1 | "grow" text boxes had no ceiling; the box ends with five empty 24pt paragraphs Pages clips | viewer: growth is capped at 1.5× the stored height |
| Arabic paragraphs laid out left-to-right with right alignment: markers on the left, periods at the wrong end, justified last lines at the left | 77890685, ae1cc13b | `writing_direction` was not read; and the documents store the "natural" default, which Pages resolves from the first strong character | converter reads `ParagraphStylePropertiesArchive.writing_direction` (38); viewer derives a natural direction from the first strong character and sets it on the paragraph and its list row |
| a shorter render kept a stale 25th page shot and composite | harness | `visual_diff` never deleted an earlier run's `ours/page-N.png` | deleted before each render |

| document | host | judged | before | after |
| --- | --- | ---: | ---: | ---: |
| f82b2fa40fd4 | apostlesonline.org | 2 | 1.0 | 9.0 |
| ae1cc13b298f | rustedradishes.com | 2 | 2.5 | 2.0 |
| 26a356dc8651 | strokeinformation.co.uk | 2 | 4.0 | 6.0 |
| cf4b76a33f5a | johnwheeldonacademy.co.uk | 2 | 4.5 | 3.5 |
| 7b8e38edb184 | immobilienundleben.de | 2 | 5.5 | 9.0 |
| 77890685af37 | sa-uc.edu.iq | 1 | 6.0 | 8.0 |
| cace32e1ed60 | bdrp.ch | 2 | 6.0 | 6.0 |
| d88d9139e2f5 | img.lucensoftware.com | 2 | 6.0 | 5.5 |
| e2e0bff371c1 | financialplanningindubai.com | 2 | 6.0 | 6.0 |
| f43d849f63dd | likvi.de | 1 | 6.0 | 6.0 |
| 4047e81b0665 | bcss.org | 2 | 6.5 | 7.5 |
| 44d11ec89c32 | canineassistants.org | 2 | 7.0 | 6.5 |
| eb2a7cde90d6 | paadopt.org | 2 | 7.0 | 9.0 |
| 27254104743d | thelastamericanvagabond.com | 2 | 7.5 | 7.5 |
| 48f5f124cdd9 | schule-schlotheim.net | 2 | 7.5 | 7.5 |
| 806df50f6150 | chemiedidaktik.uni-wuppertal.de | 2 | 7.5 | 8.0 |
| 87560fc1b5b0 | nfgymcheer.com | 2 | 7.5 | 7.5 |
| 904cec1c6651 | pearlpirie.com | 2 | 7.5 | 7.0 |
| 38a7da366cc3 | lakecitypresbyterian.org | 2 | 8.0 | 8.0 |
| 964b85d1b8b9 | i-campus.hokkyodai.ac.jp | 1 | 8.0 | 7.0 |
| 9a3616c756a7 | easy4me.info | 1 | 8.0 | 7.0 |
| bc5e6bd19210 | kobysh.com | 2 | 8.5 | 8.5 |
| 1bd116a4fa8f | domaukcyjnyiglica.pl | 1 | 9.0 | 9.0 |
| all | 23 documents | 41 | 6.27 | 6.95 |

277d7233 (new this round, 2 pages): 6 and 8; its page 1 is the
two-column chain, where our column break falls one heading later than
Pages' because of font metrics.

Content-aligned pairing (`judge.py --align-content`, added this round):
at two pages per document it changes nothing, since the drift is
fractional there. At four pages it re-pairs 5 of 74 pairs and moves the
mean by 0.02 (6.43 to 6.45): where an export page straddles two viewer
pages, neither pairing compares the same content. It matters once the
offset is a whole page: over the first 12 pages of the two long
documents, cf4b76a33f5a scores 2.17 with the default pairing and 4.0
aligned (viewer pages 8–13 stand in for export pages 7–12), eb2a7cde90d6
3.25 and 5.5. Those are the numbers that describe the render rather than
the pagination.

What remains, in the order the judge names it:

1. Pagination against Pages' line breaks: cf4b76a (32/35 pages) and
   eb2a7cde (61/67) still diverge from page 7 and page 6; the aligned
   pairing measures around whole-page offsets only. cf4b76a also
   substitutes Helvetica for Calibri, which changes every line break.
2. Page-layout headers take the wrong master: 26a356dc page 2 prints the
   template's placeholder text ("6 JANUARY 2026", "CURABITUR LEO") where
   Pages prints the section's "JUN /JUL 26", "ISSUE 3".
3. Box strokes Pages does not draw: a rectangle around 26a356dc's
   "NEWSLETTER" box; a dashed box around e2e0bff3's "CHECKLIST" where
   Pages draws a dotted rule under it.
4. cf4b76a page 1: the paragraph after an inline table paints over the
   table, and the table's last row lands on page 2.
5. ae1cc13b page 2 (0 both rounds): tighter line spacing puts more of
   the article on page 1; the dotted rule under the byline sits 180pt
   lower than Pages draws it.
6. Anchored objects in right-to-left paragraphs: 77890685's photo sits
   about 30pt right of Pages' position; the horizontal offset may be
   measured from the other edge.

#### Schema and converter findings

Data that was in the archives and absent or wrong in the JSON, and what
was done (proof fixtures in parentheses):

- Text boxes with a `text_flow`: the flow's storage holds the text and
  the `owned_storage` beside it is empty — in all 19 such boxes across
  the corpus (9 documents, Pages only; no Keynote or Numbers fixture has
  a `text_flow`). The converter emitted the empty storage. Now the flow
  storage wins, and a flow with 2+ textboxes is a chain:
  `TextboxDrawable.flow { id, index, count }`, text on index 0,
  continuation boxes empty (26a356dc, 277d7233). docs/format/text.md
  records the survey.
- `ExteriorTextWrapArchive.fit_type` and `alpha_threshold` were dropped.
  Now `TextWrap.fit` ("bounding-box" when stored 0; absent = 1, the
  contour fit, which 99% of wraps in all three apps store) and
  `TextWrap.alphaThreshold` (absent = 0.5). The naming is inferred: no
  fixture proves it, because f82b2fa4's cover — the document behind the
  round-1 proposal — is an opaque PNG, and Pages puts its title inside the
  frame for a different reason (the no-room rule above).
- Tracked changes: `PagesDocument.changes[] { kind, paragraphIndex, text,
  author, date }` from `TSWP.ChangeArchive` and its session's
  `TSK.AnnotationAuthorArchive`; the body stays the accepted view; the
  round-1 warning is gone; the markdown dump lists them (55d37c2b: 3
  insertions — two attachments and a paragraph break — and 1 deletion by
  "Maria V", 2025-12-08).
- Comments outside the body: none in the corpus. The 18
  `TSD.CommentStorageArchive`s in the five commented Pages documents are
  all body highlights. Cell comments would come from
  `TST.TableModelArchive.commentStorageTable` (19) / cell
  `comment_storage` (10), shape comments from `TSWP.CommentInfoArchive`;
  neither occurs in a fixture, so they stay unmodeled.
- `ParagraphStylePropertiesArchive.writing_direction` (38) was not read;
  it is now, though no corpus document stores it — Arabic documents keep
  the "natural" default, so the viewer resolves the direction from the
  text (77890685, ae1cc13b).
- `fontSizePt` of 12 was stripped from resolved character styles against
  the model's stated contract; kept now (G1: 1 pooled style, G2: 3).
- Wrap type 5 is "largest" (Pages' Automatic), confirmed on
  4047e81b0665, where Pages flows the body beside a type-5 text box;
  f82b2fa4's type-5 cover is not wrapped because it fills the page.

Proposals not implemented:

- Alpha-fit wrap rendering. Design: decode the image once (before the
  document renders), take per 3pt row slab the widest transparent run,
  and turn the slabs into stacked floats — a full-width float for a
  closed slab, a left and a right float leaving the gap for an open one
  — in place of the rectangular band. It was written and then removed:
  no corpus document has a wide wrapping image with transparency, so
  nothing could verify it.
- Pages text boxes are marked `textFit: "grow"` like Keynote's; Pages
  keeps the stored frame and clips (7b8e38ed's title box). Emitting
  "grow" only for Keynote, or an explicit "clip" for Pages, would let the
  viewer drop the 1.5× cap.
- Comments in text boxes and table cells (above): the model hooks would
  be `TextboxDrawable.comments` and `TableCell.comments`, once a fixture
  exists.

### Pages, round 3b (page structure and corpus) (2026-09-06, Qwen thinking off, two pages per document)

A corpus pass first: 215 of the 238 Pages origin hosts had no judged
document. 25 were picked from them, one per host, by a pnk2json feature
survey of all 215 candidates so that they cover page-layout documents
(7, with 1 to 16 sections), multi-section word-processing documents (5),
a 65-page report with footnotes and a master-page watermark, a two-column
book, two documents with explicit column widths, six documents with 14 to
107 floating or anchored objects, two long numbered lists, and a
248-paragraph form. All 25 were exported from Pages and the first two
pages of each were scored (47 pairs). Mean 7.19 before the round's
fixes, 7.43 after, on the same exports. Score counts before: 1 ×1, 2 ×1,
5 ×8, 6 ×10, 7 ×3, 8 ×3, 9 ×20, 10 ×1; after: 1 ×1, 5 ×8, 6 ×8, 7 ×3,
8 ×5, 9 ×21, 10 ×1.

The ranked corpus, worst first (pages: Pages' export / this viewer,
after the fixes):

| document | host | pages | judged | before | after |
| --- | --- | ---: | ---: | ---: | ---: |
| 5c07d836849b | primus-minden.de | 11 / 13 | 2 | 3.5 | 4.5 |
| 4659b5b6a8db | creativeipadclassroom.com | 7 / 7 | 2 | 5.0 | 5.0 |
| b31db8225fc6 | cosmeticsupport.com | 65 / 72 | 2 | 5.5 | 5.5 |
| bdbcfdc26a60 | sedgefieldchurch.org | 16 / 16 | 2 | 6.0 | 5.5 |
| 7edb1b23ebd6 | engeco.mc | 4 / 4 | 2 | 5.5 | 6.0 |
| 93229becc769 | outburst.au | 4 / 4 | 2 | 6.0 | 6.0 |
| c5c6beffa264 | orte-der-unsichtbarkeit.de | 4 / 5 | 2 | 5.5 | 6.0 |
| 6a8fc1809d35 | cumberland.gov.uk | 12 / 13 | 2 | 7.0 | 7.0 |
| dc4de3c02235 | strongroots.ca | 4 / 5 | 2 | 7.0 | 7.0 |
| dd965179a23c | signoradeicalzini.it | 10 / 11 | 2 | 7.0 | 7.0 |
| 10a06959a8c7 | voordeklas.com | 9 / 12 | 2 | 7.5 | 7.5 |
| 1ea99385959d | crhf.org | 10 / 12 | 2 | 7.5 | 7.5 |
| 2cb7a126f3a4 | kevinhoneycutt.org | 4 / 4 | 2 | 7.0 | 7.5 |
| 25f4b519ec3f | maitressemegane.fr | 1 / 1 | 1 | 8.0 | 8.0 |
| 95577fa077a3 | maitrefafa.fr | 3 / 3 | 2 | 8.0 | 8.0 |
| bd5599cb5b49 | maniscalcovini.it | 1 / 1 | 1 | 2.0 | 8.0 |
| 529b69bade51 | meta.ipadschule.ch | 12 / 15 | 2 | 9.0 | 8.5 |
| 0b412bfe34b1 | burlovevent.se | 6 / 6 | 2 | 8.5 | 9.0 |
| 16b4195d1cc6 | transcendencetoolbox.com | 8 / 8 | 2 | 9.0 | 9.0 |
| 2dd0f3849d78 | primaryresources.co.uk | 13 / 13 | 2 | 9.0 | 9.0 |
| 7a86fa49bfef | ipadlernen.de | 4 / 4 | 2 | 8.5 | 9.0 |
| ad9cc81fff28 | u-helmich.de | 2 / 2 | 2 | 9.0 | 9.0 |
| c7a568f0b655 | rucool.marine.rutgers.edu | 7 / 7 | 2 | 9.0 | 9.0 |
| da3ef450931a | brfnyponet.se | 1 / 1 | 1 | 9.0 | 9.0 |
| 2725d84498fb | prayerletters.com | 2 / 2 | 2 | 9.5 | 9.5 |
| all | 25 documents | | 47 | 7.19 | 7.43 |

The seven page-layout documents score 7.5 to 9.5; the low scores are all
word-processing documents, and in nine of them the page count differs
from Pages'. Two pages per document hides most of what this round fixed:
the header fix on 5c07d836 shows on page 2 only when page 1 stops
overflowing (a line-pitch difference, below), and the watermark fix on
b31db822 changes its page 1 but the judge's verdict there is dominated by
the clipped title box.

Round-2 documents re-exported and scored after the fixes (round-2 scores
were on different exports): 26a356dc8651 7 and 7 (6.0 in round 2),
e2e0bff371c1 7 and 6 (6.0), 77890685af37 9 (8.0), cf4b76a33f5a 5 and 4
(3.5).

Defects fixed, with cause and fix:

| defect | documents | cause | fix |
| --- | --- | --- | --- |
| five of six bottles missing and a white box across a banner | bd5599 | six photos named `FullSizeRender-N.jpg` are HEIC (`ftypheic`), which Chrome does not decode; the JSON called them images by extension | converter: `MediaAsset.format: "heic"` sniffed from the ISO BMFF brand (24 HEIC files in 4 corpus documents, 1 Pages, 3 Keynote); viewer: an image that fails to decode swaps in the drawable's JPEG thumbnail |
| page-layout page 2 printed the template's "6 JANUARY 2026 · CURABITUR LEO" header where Pages prints "JUN /JUL 26 · ISSUE 3" | 26a356dc (round-2 item 2) | `inherit_previous_header_footer` ("Match previous section") makes a section show the previous section's headers whatever its own masters store; the viewer preferred a master's own non-empty text | converter: the previous section's resolved storages are copied into the inheriting section's masters (cloning a shared master); the viewer no longer walks sections. 453 of the corpus's 569 sections set the flag |
| header text printed over the section title bar on every page after the cover | 5c07d836 (also 7edb1b23, not fixed there) | Pages starts the body at max(top margin, header margin + header height); the viewer used the top margin (31pt) under a three-line header | viewer: the header row is measured per template and a top exclusion band pushes the body (68.6pt in Pages' export, matched) |
| header and "Seite \| 1" footer on a cover page Pages prints bare | 5c07d836 | the section names a first-page master with empty storages and the viewer fell through to the parity master | viewer: a named first-page master is used as is; empty means none |
| "DRAFT" master watermark painted over the cover | b31db822 | the cover is a no-room anchored image at z-index -1, below template furniture at z-index auto | viewer: furniture paints at z-index -2; Pages hides it under the cover and shows it from page 2 |
| a one-paragraph banner paginated to two pages | bd5599 | the wrapping photos fill the page, so the empty body paragraph was pushed to a new page | viewer: empty paragraphs after the last visible one never open a page |
| a dashed box around "FINANCIAL REVIEW" and around "NEWSLETTER" where Pages draws a dotted rule below, or rules above and below | e2e0bff3, 26a356dc (round-2 item 3) | the paragraph border's position (`border_positions` 45 / `deprecated_borders` 15) was not read; the viewer drew all four sides, and a 0.002-long dash as "dashed" | converter: `ParaStyle.borderSides` (bits 1 top, 2 bottom, 4 all, 8 left, 16 right, inferred from three fixtures); viewer draws the named sides, dots when the dash is no longer than the stroke |
| document settings absent from the JSON | every Pages fixture | `TP.SettingsArchive` was read for the flavor and footnote kind only | `meta.createdAt` (never filled before), `PagesDocument.template`, `.language`, `.hyphenation`, `.rightToLeft`; the viewer hyphenates by language when the setting is on |
| "unequal-width columns degraded to 1 equal columns" warning | 12 of 215 surveyed documents | a single explicit-width column warned as unequal columns | converter: warn for two or more columns only |

What remains, in the order the judge names it after the fixes:

1. Line pitch: 5c07d836's cover prints "Mappe von:" and "Lerngruppe:"
   20.6pt apart in Pages (11pt text) and 32pt apart here, so the last
   line spills to a page of its own and every later page of the
   document is one page behind its floating objects (11 pages in Pages,
   13 here). Text area (Pages A's files).
2. Rotated wrapping objects: 10a06959's tilted polaroid (page 2) takes
   its bounding box as the exclusion; Pages wraps to the rotated
   contour and keeps "Het script" above it (baseline 161pt, contour
   top-left corner about 180pt). The overflow cascades into three extra
   pages. 25 word-processing documents carry rotated wrapping objects
   (186 objects, 120 of them anchored).
3. Shape image fills are in the JSON (4659b5b6: two `fill.type:
   "image"`) and not painted: the "Spelling Workbook" box is white where
   Pages shows a photo of letters. drawables.ts (Keynote's file).
4. 7edb1b23: a header of seven empty paragraphs and four body
   paragraphs holding only anchored logos; Pages prints the title at
   102pt from the top, this viewer at 54pt. Neither "header height
   pushes the body" nor "empty lines take their line height" reproduces
   102pt; the header row is skipped by the measurement because it has
   no text. Needs a fixture built by hand.
5. Mirrored shapes: 26a356dc's sidebar arrow (`right-arrow` preset)
   points left in Pages. `TSD.GeometryArchive.flags` (3) is not read;
   `DrawableCommon.flipped` exists but is filled from PathSource flips
   only. tsd.rs (Keynote's file).
6. b31db822's cover shape prints a fifth paragraph ("V13 27 November
   2014") that Pages clips: the `textFit` item (Pages A).
7. Unexamined judge verdicts: bdbcfdc2 page 1 (title text duplicated
   behind the image), c5c6beff (vertical order of a question and an
   instruction box), 6a8fc180 page 2 (two input boxes missing), 93229bec
   (headline face), 4659b5b6 page 2 (media placeholders).
8. 77890685's photo sits about 25pt higher than in Pages (round 2
   guessed 30pt to the right; the horizontal position matches within
   8px at 150dpi).
9. The anchor-paragraph wrap exemption (round 1, 6d4f8527) is contradicted
   by 26a356dc's WhatsApp paragraph, whose own lines Pages wraps beside
   the icon; here they run under it and leave a gap.

#### Schema and converter findings

Data that was in the archives and absent or wrong in the JSON, and what
was done (proof fixtures in parentheses):

- `TP.SettingsArchive` (DocumentArchive field 7) fields never read:
  `creation_date` (26) and `orig_template` (25) in all 323 corpus files,
  `language` (21) differing from the locale's in 10, `hyphenation` (9) on
  in 10, `document_is_rtl` (18) in 1 (77890685). Now `meta.createdAt`,
  `PagesDocument.template`, `.language` (only when it differs),
  `.hyphenation`, `.rightToLeft` (7a86fa49: "10_For_Sale_Bicycle",
  2021-04-29T11:00:24+0200). `footnote_format`, `footnote_numbering`,
  `facing_pages` are default in every corpus file and stay unmodeled;
  `section_authoring` is set in 3 files and `paper_id`/`printer_id` in
  most, none of which affects extraction.
- Media container format: `MediaAsset.kind` came from the file name, so
  HEIC bytes under `.jpg` names were "image" and undecodable. Now
  `MediaAsset.format` ("heic" | "avif") from the `ftyp` brand (bd5599).
- Header/footer inheritance was left to the viewer as a chain walk
  across sections (docs/model-review.md §3 forbids it); the converter
  now resolves it into the section's own masters (26a356dc, 48f5f124).
- Paragraph border position (`border_positions` 45, `deprecated_borders`
  15) was used only as an on/off gate; `ParaStyle.borderSides` carries
  the sides (e2e0bff3, 0c563c6d: value 2 = below; 26a356dc: 3 = above and
  below). The bit meanings for 8 and 16 (left, right) are inferred from
  the enum's value set (0-4, 8-11, 16-19, 24), not from a fixture.
- `TP.SectionArchive.section_hyperlink_uuid` (31) and
  `TP.DocumentArchive.uses_single_header_footer` (21),
  `citation_records` (13), `merge_data` (50): absent from every corpus
  file; nothing to model.
- The `MediaAsset` inventory reads each image's first 12 bytes now; the
  survey found no `avif` in the corpus.

Proposals not implemented:

- `TSD.GeometryArchive.flags` (3): the horizontal/vertical flip bits.
  26a356dc's arrow proves a flip is stored somewhere the converter does
  not read; the bit assignment needs a fixture with one known flip
  (Keynote's file).
- Rotated wrap contour: emit nothing new; the viewer has `angleDeg` and
  could take, per exclusion band, the rotated rectangle's top edge at
  the text's start side instead of the bounding box.
- Header height as data: `PageTemplate.headerHeightPt`/`footerHeightPt`
  measured by Pages are not stored (the archives keep only the margins);
  the viewer measures its own rendering.

### Numbers, round 5 (2026-09-06, Qwen thinking off, up to 3 pages per document)

Round 4's remaining list in order: the grouped shapes beside the wrong
table (181f2b199bd3), the "zero-height line shapes" (021084ac7183),
chart legend markers (baabe23e067f), the number-formatting locale; then
fifteen documents from hosts not judged before. Every defect was traced
to the JSON first. Thirteen documents from rounds 3 and 4 were
re-rendered against their existing Numbers exports and re-scored before
and after on the same build of main (363933e) plus this branch.

| defect | documents | cause | fix |
| --- | --- | --- | --- |
| Grouped shapes beside the wrong table section | 181f2b199bd3 | Not the groups: the table drew 982px for a 907pt frame. The 55 stored row heights predict every gridline of the export within 2px and sum to the frame, but a CSS row height is a minimum and cells with 4px padding and a line box pushed 8-17pt rows to 22px | tables.ts boxes each cell's content at the row's stored height (vertical clip only, so unwrapped text still spills); spanned cells take the sum of their rows. Table height 982 → 908; the groups now sit in the set-list section |
| Cell text 4px too low in nearly every table | corpus-wide (6,070 of 9,700 cell styles in a 70-file census) | `TSWP.PaddingArchive` omits a side that is 0: the stock body style stores left/right/bottom = 2 and no top; Excel imports store only left/right = 5. The viewer read a missing side as its 4px/8px CSS default. Measured on the exports: a top-aligned cap sits 1.4pt under the cell top (c4b881955676 r7c0), middle-aligned text centres 0.5pt above the cell centre (181f r6c19, eb29 r3c4, c4b8 r3), a bottom-aligned baseline sits 3.9pt above the cell bottom = 2pt inset + descent (5c152beb2a3b) | an absent side is 0 when a padding object exists |
| Blank unsized rows collapsed to 3px | 17891b89da2f (55 rows with no stored height), 021084ac7183 | Numbers fits an unsized row to its content and an empty cell still counts one line of its text style: 17891's blank rows are 16pt in the export; 021084ac7183's 1x3 tables store size 0 with a 90pt Noteworthy-Light cell and draw 151.65pt (= 90 × 1.585 + 8 + 1), which round 4 misread as "zero-height line shapes drawn as 32pt bars" | an unsized row's cell box asks for one line (`min-height: 1lh`). 07_Calendar's mini-months (the case the old comment cited) still match the export |
| Rich-text continuation line clipped | eb299192a219 "Total Charge (minimum charge is 4kg)", 5c152beb2a3b's link cell | the paragraph carries the cell's 26px size, so every CSS line box includes a 26px strut and the 12px continuation line was 31px tall (62px in a 48px row). Numbers sizes each line by its runs. Round 4 blamed a missing run size; the run carries 12pt | the paragraph's unitless leading moves onto its inline elements (spans and anchors) and the strut collapses; a capped cell that still overshoots by up to 20% tightens its leading instead of clipping |
| Pre-BNC conditional fills painted where the export is white | 021084ac7183 (1x3 tables blue, 1x1 "0" cell yellow) | the v4 cell's fired-rule index is not reliable: rule 15 of a 55-rule set on empty cells compares against the number 2 (formula nodes 63, 17 = 2.0, 11); rule 15 of the 48-rule set carries the string "entre 5 et 10" on a numeric cell | tables.rs drops a fired rule whose predicate constant cannot match the cell's type or an empty cell; the v5 path (cdrky verified) is untouched. The 1x3 tables print white; the "0" cell stays yellow because both of its pooled styles carry the fill as a base style (see what remains) |
| Chart legend keys: a line segment where Numbers draws the data symbol | baabe23e067f (every line chart) | `ChartArchive.series_non_styles` (19) → `TSCH.Generated.ChartSeriesNonStyleArchive` showsymbol/symboltype and the style chain's symbolsize were never read | new `ChartSeries.symbol { kind, sizePt? }`; the viewer keys a line series that shows symbols with a hollow circle and draws markers from the field (kind 0 = hidden: the "Adjusted Story Points" chart stores no non-style, draws no markers and keys with a line, which the proto default of showsymbol = false predicts) |
| "$2.00" for a custom currency format that prints "CA$2.00 ea." | 4b5a7b9d32af | `TSK.CustomFormatArchive.default_format` (type 274) carries `currency_code = "CAD"` (f3) and the pattern `¤#,##0.00' ea.'` (f18); the converter emitted the pattern and the name and dropped the code | `CellFormat.currencyCode` now carries it for custom formats (looked up by uuid the way the name is); the viewer renders ¤ patterns and takes the symbol from Intl in the formatting locale ("CA$" in en-US, "$" in en-CA), which is what NSNumberFormatter prints |
| First column and captions cut at the canvas edge | baabe23e067f ("Sprint Summaries 2019" at x = -10; sheet 2's "Sprint x" and "Planning" at y = -9) | the canvas started at 0 where Numbers' export starts at the content's bounding box (the gridline measurements in this round assumed that origin and matched) | numbers.ts shifts the drawables' origin box by the negative extent. Landed after the scored run; the judge's "titles cut at the top edge" on page 2 is this |
| Number and date locale | eb299192a219, 181f2b199bd3 | Numbers formats in the machine locale: eb29 stores `locale_identifier = en_EE` (round 4 recorded it as it_IT; en_EE is a comma-decimal region) and the en_US export prints "523.4"; 181f stores ja_JP and the export prints "1/11(Sun)" | a setting (localStorage `pnk.numberLocale`): "document" (default, Peter's ruling: one rendering for every reader) or "browser" (what Numbers prints on the reader's machine). Four lines each in main.ts and index.html; the rule is in tables.ts |

Qwen scores on the same exports, before and after (28 pages; the
e8625984c6c3 page-2 strip scored 0 before as "corrupted" and 9 after,
which is judge noise on a 30pt-wide page and is excluded from both means
below):

| document | page | before | after | what the judge still names |
| --- | ---: | ---: | ---: | --- |
| 021084ac7183 | 1 | 7 | 7 | text wrapping in the purple circle (3 lines vs 4) |
| 021084ac7183 | 2 | 6 | 7 | the "0" box yellow where the export is white |
| 021084ac7183 | 3 | 7 | 7 | "0" values shown where the export shows empty cells |
| 17891b89da2f | 1-3 | 9, 9, 9 | 9, 9, 9 | vertical separators, row alignment |
| 181f2b199bd3 | 1 | 6 | 9 | "1/11(日)" where the export prints "1/11(Sun)" (the locale setting) |
| 33499baadcc3 | 1 | 9 | 8 | "Kč" where the export prints "CZK" (Intl's symbol for CZK in the document's cs locale; the browser-locale setting prints "CZK") |
| 3383a82d3b32 | 1 | 9 | 9 | resolution |
| 4b5a7b9d32af | 1 | 8 | 6 | header block shifted; the page is a 4227pt-tall sheet and the composite scales it to a strip |
| 5c152beb2a3b | 1-3 | 9, 9, 8 | 9, 8, 9 | totals rounding ($1,060.71 vs .70) |
| 66ba951f59ea | 1-3 | 8, 9, 8 | 8, 8, 8 | a 5th instruction line the export cuts off; box borders |
| 6914f46e51ab | 1-2 | 8, 5 | 9, 6 | intro wrapping; pie labels outside the slices |
| baabe23e067f | 1-3 | 8, 8, 8 | 8, 8, 8 | "Averages" header missing; titles cut at the top; page 3's legend |
| c4b881955676 | 1-2 | 9, 9 | 9, 9 | logo size; footer crop |
| e8625984c6c3 | 1 | 9 | 8 | one continuous page where the export paginates |
| eb299192a219 | 1-3 | 8, 9, 8 | 8, 9, 8 | 5.47 vs 5.48; decimal comma |

Mean over the 27 pages: 8.11 before, 8.15 after (7.82 and 8.18 over all
28; most of the 28-page gain is the strip page). The scores move on five
pages: 181f2b199bd3 (+3, the row-height fix), 021084ac7183 page 2 (+1,
the fills), 6914f46e51ab (+1, +1), 5c152beb2a3b page 3 (+1), and down on
33499baadcc3 (the CZK symbol), 4b5a7b9d32af (the judge reading a
4227pt-tall sheet scaled into a strip differently on two runs; its
render is unchanged apart from the row heights, which now sum to the
stored 3965pt where they ran 4086 before), 5c152beb2a3b page 2,
66ba951f59ea page 2 and e8625984c6c3 page 1 (the judge names the same
things before and after). The judge does not see the cell inset fix or
the chart legend keys at this page scale; those were confirmed against
the exports by measurement (the inset numbers above) and by eye
(baabe23e067f's legends).

Fourteen more documents, one per origin host not judged before
(`fixtures/success.tsv`; 30 of 55 Numbers hosts were unjudged), exported
from Numbers and scored, up to two pages each (17 pages). Two picks were
dropped: 901b43822fa1 (gotoportugal.eu) has no file under
fixtures/crawl, and 675f65591c27 (hayappy.com, 3.6 MB) never opens in
Numbers within the harness's 90 s. Numbers also stopped opening
documents after the first export of the run and every later export fell
back to the embedded QuickLook preview until the app was quit; those
runs were discarded and re-exported.

| document | host | pages | mean | what the judge names |
| --- | --- | ---: | ---: | --- |
| e14a63a92477 | www.tonychachere.com | 2 | 0 | an 8930pt-tall sheet scaled into a strip; "candidate wider than the golden" |
| af6119acf94b | californiaglobe.com | 2 | 5.5 | a 2292pt-tall sheet scaled into a strip; the render matches the export by eye |
| a720beed1ab2 | egmtemperingga.com | 2 | 8 | a "Column1, Column2, ..." header row the export does not print; grid at the top-left |
| dc3dc072c897 | celebrant.institute | 1 | 8 | "$" where the export prints "A$" (AUD in the document's en_AU locale; the browser-locale setting prints "A$") |
| 7253a6d256ca | online210.psych.wisc.edu | 1 | 8 | white margin on the right |
| 6359764330a7 | www.shaapb.fr | 1 | 9 | text wrapping in one cell |
| 81706ab71fe9 | aoiro-chiba.jp | 1 | 9 | font hinting |
| 9fedd6476d98 | www.krfy.org | 1 | 9 | a legend box's text truncated at the right |
| a12887dc38e5 | files.causeofamerica.org | 1 | 9 | slight vertical compression |
| d83ffe52557d | knea.org | 1 | 9 | the caption's last digits (the composite cuts our taller render) |
| dfd8d16858a9 | assets.ctfassets.net | 1 | 9 | anti-aliasing |
| eb25e763b62f | adl-security.be | 1 | 9 | aspect ratio |
| fac62c5609f0 | igbildendekunst.at | 1 | 9 | "Standort" wraps to two lines in the export, one here |
| 76f95117ebe8 | coco-lo.net | 1 | 9 | vertical spacing in the lower section |

Mean over the 17 pages 7.29; 8.62 without the two strip documents. The
only rendering item in the list is a720beed1ab2's header row: the table
stores "Column1".."Column7" as its header-row values and the export
prints them blank; not investigated this round.

#### Schema and converter findings

- `ChartSeries.symbol { kind, sizePt? }` (additive, docs/model-design.md
  §2.7): kind 0 = hidden, 1 = circle; the corpus stores only 0 and 1 on
  line charts (6 series with the f32::MAX "automatic" size, 6 with 2-5pt).
  Scatter series store no non-style in the corpus and stay unknown.
- `CellFormat.currencyCode` on custom formats (an existing field that was
  never filled on the custom path): the code lives in the custom format's
  `default_format.currency_code`, not on the cell's format struct.
- Padding: an absent side of `TSWP.PaddingArchive` is 0 (proto has no
  default; the stock body style and Excel imports prove it). The JSON was
  right; the viewer's fallback was wrong. No model change.
- Row heights: a stored 0 or an absent entry means "fit to content", and
  the content includes an empty cell's one line at the row's text style.
  The stored non-zero height is exact in the export (four documents
  measured). No model change; the rule is now in tables.ts.
- Chart type 11 (`twoAxisChartType2D`, TSCHArchives.Common.proto) is
  "other": 0ab5dd52841e (www.waclimate.net) stores 666 of them, 1,332
  series, and every one renders through the fallback. Proposal: map it
  to the column family for the plot and colour slots, and carry the
  per-series axis assignment (`ChartArchive` axis maps) so a viewer can
  draw the line series against the second axis. Not done this round.
- Pre-BNC conditional formatting: the fired-rule index cannot be trusted
  on v4 cells. The predicate archive is `FormulaPredicatePrePivotArchive
  { formula, predicate_type, qualifiers, param indices }`; the type enum
  is not in the extracted protos, so only the type-level check above is
  implemented. Proposal: name the predicate types from a fixture with one
  rule of each kind (docs/format request) and evaluate the numeric and
  text comparisons.
- Locale: `TSK.DocumentArchive.locale_identifier` is the only locale the
  archive stores, and Numbers does not format with it. The viewer's
  setting is the honest answer; nothing to add to the model.
- Auto-fit row heights measured for the Pages agent (text.ts owns the
  per-face line-height table): Numbers fits an unsized row at line height
  + top/bottom insets with the stroke inside the pitch. Helvetica 11pt
  with 0/2 insets: 16.0pt per row (17891b89da2f), so the line is 14pt,
  where the table says 1.0 (11pt); Times New Roman 12pt: 17.5pt rows
  (3383a82d3b32), a 15.5pt line against the table's 1.15 (13.8);
  Helvetica Neue 12pt with 4/4 insets: 22.08pt rows (33499baadcc3), a
  14pt line where 1.193 gives 14.3 and our 1px border makes the row 23.3.
  The residuals go both ways by 1-2pt per row; not changed here.

#### What remains (ranked)

1. Two-axis charts (type 11): 666 charts in one document render as
   "other"; the proposal above.
2. Auto-fit row leading: the per-face measurements above; 17891b89da2f
   draws 55 rows at 14pt where the export has 16pt (110pt over the sheet).
3. 021084ac7183's 1x1 "0" cell: yellow in both pooled styles (base fill,
   not a fired rule) where the export prints white; the cell is a pop-up
   control cell, so the export may be drawing the control's own look.
4. 6914f46e51ab page 2: pie data labels outside the slices where Numbers
   puts them inside (5-6 both rounds).
5. baabe23e067f: the "Averages (3 Sprint Rolling Avg)" header cell and
   titles cut at the top of the canvas (a 0-y drawable above the first
   table).
6. Value-axis maxima (round 4's item 5) and the group summary rule codes
   (G8 checklist) are unchanged.
7. Composite pairing: a sheet shorter than its export page (d83ffe52557d)
   or taller (4b5a7b9d32af, e8625984c6c3) is scaled or cut by the
   harness and the judge scores the crop; `--align-content` does not
   apply to sheets.

### Pages, round 3a (text and pagination) (2026-09-06/07, Qwen thinking off, four pages per document, content-aligned)

Eight documents: the three the work list named (cf4b76a33f5a,
eb2a7cde90d6, ae1cc13b298f), the two round 3b left for this area
(5c07d836849b, 77890685af37), and three that prove particular fixes
(4047e81b0665, 7b8e38edb184, b31db8225fc6). The same Pages exports as
rounds 2 and 3b; "before" is the merged main (a71e915 plus Numbers'
#19) rendered fresh, "after" this branch. Scored with `--align-content`,
up to four pages each, 29 pairs. Mean 6.03 to 7.00.

| document | host | pages (Pages / before / after) | judged | before | after |
| --- | --- | ---: | ---: | ---: | ---: |
| cf4b76a33f5a | johnwheeldonacademy.co.uk | 32 / 35 / 32 | 4 | 4.5 | 7.5 |
| b31db8225fc6 | (round 3b) | 65 / 65 / 65 | 4 | 5.5 | 8.0 |
| 5c07d836849b | primus-minden.de | 11 / 11 / 11 | 4 | 7.5 | 8.5 |
| 4047e81b0665 | bcss.org | 12 / 12 / 12 | 4 | 5.5 | 6.2 |
| 77890685af37 | sa-uc.edu.iq | 1 / 1 / 1 | 1 | 8.0 | 9.0 |
| 7b8e38edb184 | immobilienundleben.de | 9 / 9 / 9 | 4 | 8.0 | 8.0 |
| eb2a7cde90d6 | paadopt.org | 61 / 67 / 63 | 4 | 9.0 | 8.8 |
| ae1cc13b298f | rustedradishes.com | 7 / 7 / 7 | 4 | 1.8 | 1.5 |
| all | 8 documents | | 29 | 6.03 | 7.00 |

5c07d836's "before" already has Numbers' round-5 row heights, which
took its cover from 13 pages to 11 on their own; the cell-spacing fix
below is what moved its page 1 from 6 to 9. ae1cc13b's pages 2-4 score
0 in both columns: the document's Tajawal is not installed here, Pages
substitutes a naskh face with a different line height and glyph width,
and every page after the first holds different text (below).

Defects fixed, with cause and fix:

| defect | documents | cause | fix |
| --- | --- | --- | --- |
| an inline table floated at its anchor plus a 72/15pt offset, the next paragraph painted over it, its last row ran off the page | cf4b76a (page 1), 4047e81b (three tables), 12 corpus tables, G5's inline image | the converter marked any attachment with an offset of 4pt or more "anchored"; on a wrap-type-0 object the offsets are its cached laid-out position (G5's hand-built inline image stores 125/21.7) | converter: `anchored` follows the exterior wrap kind alone; type 0 is Pages' "Inline with Text" and the table splits across the page break like Pages' |
| every line 5-9% too tall: cf4b76a at 36 pages against Pages' 32, eb2a7cde 67 against 61 | every word-processing document | the viewer laid a line out at (ascent + descent + gap) × size × multiple, and the paragraph block's strut was the chrome's system face, so a Carlito run's 13.87px line box measured 16.8px | viewer: Pages' rule, measured on 24 exports (87 paragraphs, 43 within 0.15pt against 3 under the old rule): rounded ascent + rounded descent, times the multiple, plus the rounded gap; Helvetica/Times/Courier/Hoefler at round(1.2 × size); the block takes its dominant run's font stack |
| pagination measured with the fallback face, then the Google Fonts substitute restyled the pages: body text over the footer | 4047e81b page 6, every substituted document | `display=swap` and an immediate render | viewer: `loadSubstituteFonts` resolves once every face is usable (4s cap); `renderDocument` awaits it, with a generation guard |
| table rows taller than Pages': the cover table's rows 29pt against 20.64, cf4b76a's first row 36.8 against 25.9 | 5c07d836 (cover, and the page cascade behind it), cf4b76a, 4047e81b | cell paragraphs kept their style's line-spacing multiple and space before/after; Pages lays cell text out single-spaced with neither (three exports measured) | viewer (tables.ts, Numbers-owned): cell paragraphs take the natural single line height and no margins |
| after Numbers' #19, a Pages table clipped its cells at the stored row height: the six-row cover table fit page 1 where Pages breaks it after five rows | cf4b76a page 1 | the stored height is exact in Numbers' export and a minimum in Pages' (22 stored, 25.9 drawn) | viewer (tables.ts): Pages documents do not box cells |
| a heading stranded at a page bottom; a paragraph marked to stay whole broken across pages | cf4b76a's numbered headings (six styles carry both flags) | `keepWithNext` and `keepLinesTogether` have been in the model since the hackathon; the paginator never read them | viewer: up to three keep-with-next paragraphs leave with the paragraph that moves; keep-lines-together moves whole |
| a Pages text box marked "grow": a 66pt title box grew (round 2 capped it at 1.5×); a cover shape printed a fifth paragraph Pages clips | 7b8e38ed, b31db822 (round 3b item 6) | every non-shrink text box was emitted `textFit: "grow"`; Pages keeps the stored frame and clips | converter (drawables.rs, Keynote-owned): "grow" only for Keynote text boxes; G2 golden re-synced (four lines) after a visual check |
| a fixed frame's tolerance fit shrank the title to 0.6 because the five empty paragraphs after it counted as overflow | 7b8e38ed page 1 | the fit measured the whole content | viewer (drawables.ts): a fixed frame measures to the last inked block, through the inner's own rect ratio so the page's fit-to-viewport transform does not count |

What remains, in the order it matters:

1. Arabic documents in a face Pages substitutes: ae1cc13b names
   Tajawal-Bold (not installed); Pages' export draws a naskh face at a
   14.6pt pitch for 12pt at 1.15×, this viewer a heavier sans at 16.1pt,
   and page 1 holds more text here. 77890685's Helvetica Neue 14pt
   Arabic runs at 28.5pt in Pages (the glyphs come from a fallback face
   and Pages takes the line height from it) against 24.6 here; its photo
   sits 23pt higher for the same reason. Modelling this needs the
   fallback face Pages picks per script, which the archive does not
   store.
2. eb2a7cde is 63 pages against 61: the remaining drift is the Verdana
   16pt headings and the TOC box; cf4b76a lags Pages by half a page at
   page 20 (32/32 pages) — the per-page composites show the same
   breaks up to page 12 and a slow drift after, not yet located.
3. b31db822's cover shape: the fixed-box tolerance path lays its
   right-aligned lines wider than the frame and cuts them at the left;
   Pages wraps them (drawables.ts).
4. `h_offset_type` / `v_offset_type` on the attachment (0, 1, 2 in the
   corpus) are read by nothing; their meaning is unverified.
5. Numbers' round-5 measurement of auto-fit row leading (Helvetica 11 at
   about 14pt per line, Times 12 at about 15.5) against this round's
   text rule (13 and 14 plus cell padding) — the two were measured on
   different things (a Numbers row against a Pages line) and have not
   been reconciled.
6. 4047e81b page 4 (score 4 both rounds): the judge names truncation at
   the bottom and missing content at the top; not examined.

#### Schema and converter findings

- Inline versus "Move with Text": the converter decided by the
  attachment's h/v offset (4pt or more = anchored). The archive decides
  by the drawable's `ExteriorTextWrapArchive.type`: 0 is Pages' "Inline
  with Text", and an inline object's offsets are its cached laid-out
  position (G5's inline image: 125/21.7; cf4b76a's table: 72.25, the
  left margin, and 15.6). Corpus: 382 body attachments store type 0,
  370 with a 0,0 offset; all 264 body tables store type 0. Fixed;
  docs/format/text.md records it. The JSON field is unchanged
  (`InlineObjectRun.anchored`, doc comment corrected).
- Wrap type 4 (`"right"`) is the second most common kind in the corpus
  (139 images, 104 text boxes, 54 groups) and pushes text below a
  full-width object like the other wraps (87560fc1 page 1), so it is
  not "None"; whether 3/4 are left/right stays [inferred].
- `textFit`: emitted "grow" for every non-shrink text box in all three
  apps. Pages keeps the stored frame and clips (7b8e38ed, b31db822);
  now only Keynote boxes carry "grow". G2's four boxes lost the value;
  the render is unchanged.
- Line spacing: 27254104743d's four "exactly 12pt" styles resolve
  correctly (`lineSpacingExactPt`); the 12.0pt pitch on its other Arial
  paragraphs is Pages' rounding rule, not a dropped value. No converter
  change.
- `keepWithNext`, `keepLinesTogether`, `widowControl`: the first two
  are in the model and now read; `widow_control` (26) is neither read
  nor modelled (the paginator keeps two lines on each side regardless).
- Pages' line-height rule (rounded ascent + rounded descent, × multiple,
  + rounded gap) and its cell rule (single, no before/after) are viewer
  knowledge, not archive data; docs/format has no place for app layout
  behaviour, so they live in text.ts and tables.ts with the fixtures.

Proposals not implemented:

- A `ParaStyle.widowControl` field from paragraph property 26, so the
  paginator can turn the two-line rule off where the style does.
- `InlineObjectRun.offsetOrigin` from `h_offset_type` / `v_offset_type`,
  once a fixture shows what 1 and 2 mean (cf4b76a's docx-imported
  objects store 2 where Word positioned them relative to the page).

### Keynote, round 4 (2026-09-06/07, Qwen thinking off, four slides per deck)

Schema and converter work first, per the round's brief, starting from
round 3's open item (text a few points off, "slightly lower" on 12 decks):
RIPE 82's footer and greenberg's title were measured against the exports'
PDF text spans, and the cause turned out to be two converter gaps and one
wrong table in the viewer. Then 20 more decks from 20 origin hosts no
earlier run had judged, chosen to cover faces the metrics table had not
seen (Times New Roman, Georgia, Gill Sans, Avenir, Menlo, Baskerville,
Palatino, DIN, Canela, Chalkboard, Trebuchet) and features (tables,
charts, groups, connection lines, equations, movies, image fills, 4:3 and
16:9). All 20 were exported from Keynote once; the same exports are on
both sides of every score; the judge scored the first four slides of each
(80 pairs).

| defect | decks | cause | fix |
| --- | --- | --- | --- |
| text 5-9pt above its place in the export (RIPE 82 title, subtitle, footer) | every deck | `textInsets` was never filled: TSWP.ShapeStylePropertiesArchive.padding (field 6, null flag 5; ColumnStyle 11/10) sits on the theme's shape styles and reaches the drawable through the TSS parent chain; RIPE 82's boxes carry 5.625pt on every side, the export's spans start exactly there | converter resolves the padding through the chain like the vertical alignment; the viewer applies it as padding on the text layer (G2 re-synced: fifteen 4pt insets, confirmed against Pages' export) |
| first baseline and line pitch off by up to 0.2 em (Helvetica, Times, Courier, Palatino, Hoefler) | every deck with those faces | the viewer's line-height table held CoreText's ascent+descent+leading (Helvetica 1.0); Keynote lays out with AppKit's NSLayoutManager default line height (1.2) and baseline offset (0.97), and puts the extra above the baseline; measured on 37 exports; a face the Mac lacks gets Helvetica's numbers because Keynote substitutes Helvetica | `viewer/src/fontmetrics.ts`: AppKit's height, baseline offset and descent for the 184 corpus faces this Mac has; per paragraph the viewer sets the pitch, gives the block the run's face (the strut was the page's system font), and shifts the block by the alignment-dependent difference (first baseline for top, last descent for bottom, none for middle) |
| a 34pt heading with a forced break and a 20pt line under it pitched the small line at 40.8pt; the middle-aligned block grew 23% and ran into the title (enog slide 4) | enog | one pitch per paragraph | each run carries its own pitch; the paragraph's strut is the smallest run's |
| footer text below the slide edge on all 20 slides (ippp); a right-aligned URL box cut off at the edge (enog); an author box shifted left (ijclab) | ippp, enog, ijclab, ripe76, c184 | `TSD.GeometryArchive.flags` bits 1 and 2 say which point `position` names: with bit 1 clear x is the paragraph-alignment anchor (left, centre or right edge), with bit 2 clear y is the vertical-alignment anchor (top, centre or bottom edge). Round 1 read flags 0 as the centre on both axes, which is the middle+centred case; flags 1 (1,012 text shapes in 37 decks) kept its stored y | converter re-anchors to top-left per the text's alignment; text-less shapes keep their stored corner; checked on every flagged text box in 37 exports |
| block arrows drawn with a full-height shaft (perimeter slides 2, 4) | perimeter, and every horizontal block arrow since round 1 | `preset.endsWith("right")` is false for "right-arrow", so the shaft fraction applied to the width | `startsWith` |
| equations as white boxes on a dark slide (perimeter) | perimeter | pdf.js paints a white page ground | transparent ground; pdf.js 6 opens an alpha-less context when handed a bare canvas, so the context is opened with alpha (the interim judge caught the black bars this produced first) |
| a rotated photo panel drawn axis-aligned (lofar slide 4) | lofar | the mask carries the angle (335.6); the model had it, the viewer ignored it | the window and its image rotate about the window's centre |
| the theme's stock photo behind a slide's own picture (ulmen slide 3, greenberg slide 3) | ulmen, greenberg, c184, ripe76, enog, casaelite | a master's media placeholder has no KN.PlaceholderArchive, so it carried no role and stayed in `masterDrawables` whenever the slide's copy had moved; `TSD.ImageArchive.flags` bit 1 marks it (115 master images at 1, 50 at 3 in 37 decks; plain pictures are 0) | role `"media"` (converter + model doc); the master's copy leaves the underlay, the slide's copy paints whatever `objectPlaceholderVisibility` says (ulmen stores false and Keynote draws the photo) |
| a 90-degree timeline rule 19pt left of its circle (michaelbrooks slides 3, 4) | michaelbrooks | the rule carries a storage with one empty run, so the zero-height TEXT anchoring shifted it | anchoring needs visible text |
| an author's name wrapped to two lines (ijclab slide 1) | ijclab | Keynote's auto-height box is 255.4pt for a 243.2pt name that has 241.0pt after insets and both indents; Keynote lets the line run past the right indent | 3% wrap slack on auto-height boxes, as the zero-size boxes already had |
| shape image fills painted as their tint or a grey (Pages B's proposal, 4659b5b6a8db; deeplearningbook slide 8) | deeplearningbook and 16 more hosts | the model carried the fill; the viewer had no pattern path | SVG pattern (tile at pixel size, scale techniques as the slide background); an absent tinted tile composes the tint over the tone its name carries: (47,125,173) in the export, (128,206,254) over white before, within a few units now |

Qwen's mean over the 80 pages, before and after, same exports. The corpus
ranking (per deck, first four slides, before the fixes) doubles as the
list of where to look next.

| doc | host | pages | before | after |
| --- | --- | ---: | ---: | ---: |
| 0e4ad34c2823 | events.perimeterinstitute.ca | 4 | 6.00 | 9.00 |
| 9ad6cfab0ac1 | conference.ippp.dur.ac.uk | 4 | 6.75 | 8.75 |
| 85c3a6f17ca8 | www.enog.org | 4 | 7.25 | 9.00 |
| c35bd31d6622 | talks.cpsievert.me | 4 | 7.50 | 7.75 |
| c7429dce86a7 | indico.lofar.eu | 4 | 7.50 | 8.50 |
| d345acfcf1b4 | www.iangoodfellow.com | 4 | 7.50 | 7.25 |
| b12878635228 | ulmen-grundschule.de | 4 | 8.00 | 9.00 |
| bd1b298e8f6e | indico.ijclab.in2p3.fr | 4 | 8.00 | 8.25 |
| 122a2376a130 | indico.cfnssbu.physics.sunysb.edu | 4 | 8.25 | 8.75 |
| 251aeddf1bf3 | neas.dev | 4 | 8.25 | 8.25 |
| 7666b78dc0c3 | ripe72.ripe.net | 4 | 8.25 | 8.25 |
| 3b2f7e554743 | www.esup-portail.org | 4 | 8.75 | 8.75 |
| 75e6174464ca | indico.flatironinstitute.org | 4 | 8.75 | 8.75 |
| 8dfc1557a3cf | domimplantformation.fr | 4 | 8.75 | 9.50 |
| e525ca919ab7 | michaelbrooks.ca | 4 | 8.75 | 9.00 |
| f2eff6b857c2 | www.mathed.page | 4 | 8.75 | 8.75 |
| fedc639ba4a3 | uni.heiko-etzold.de | 4 | 8.75 | 8.50 |
| d7177cefaf6b | www.hamradioworks.org | 4 | 9.00 | 9.00 |
| e0ab4fc82ca4 | www.casaelitegroup.com | 4 | 9.25 | 9.50 |
| 33e6c1222216 | homepages.inf.ed.ac.uk | 4 | 9.75 | 9.75 |
| all | | 80 | 8.19 | 8.71 |

Pages that moved by two points or more, all up: perimeter 2 (4 -> 9) and
4 (2 -> 9, equations and arrows), sunysb 4 (7 -> 9, equations), enog 2
(7 -> 9) and 4 (5 -> 9, the anchor and the mixed-size pitch), domimplant
1 (8 -> 10), ippp 3 (6 -> 9) and 4 (5 -> 9, the footer), ulmen 3 (5 -> 9,
the stock photo), lofar 4 (6 -> 9, the rotated mask). Pages at 9 or more
went from 44 to 60 of 80. Three pages dropped one point: ulmen 1 and
goodfellow 1 are hinting and substitution verdicts on renders whose
change is the line metrics; heiko-etzold 4 names blurred text in diagram
boxes that a scaled group draws small in both runs.

An interim judge run, before the last fixes, caught three regressions
that are fixed in the same branch and counted in the after column:
equations drawn as black bars (pdf.js 6 opens an alpha-less context when
handed a canvas), an italic first run making its whole paragraph italic
(the block took the run's weight and style), and timeline rules moved by
the text-anchor rule (text-less shapes).

Confirmed against the exports by measurement, not by eye: RIPE 82 slide
1's four text blocks within 1.6pt of the export's ink rows (5 to 9pt high
before); greenberg's bottom-aligned title within 1.3pt (2.5pt low before,
4pt high with a top-only rule); enog slide 4's twelve lines within 2pt;
ippp's footer at 1032pt against 1031; michaelbrooks' rule at x=513.1
against 512; the block arrow's 32pt shaft on a 100pt box from the stored
0.34; the survey behind fontmetrics.ts (baselines.py, bottoms.py in the
round's scratch directory): 37 decks, first baseline/em per face equal to
NSLayoutManager's baseline offset to two or three digits (Helvetica 0.970,
Arial 0.901-0.905, AvenirNext 0.994-1.004, TimesNewRomanPSMT 0.891,
Canela 1.202), the last baseline of a bottom-aligned block one descent
above the text area, the line-spacing multiple leaving the first baseline
where it is.

#### Schema and converter findings

- **Text insets were dropped.** `textInsets` existed in the model since
  the hackathon and every emitter wrote `None`. The padding is on the
  theme's shape styles (TSWP.ShapeStylePropertiesArchive.padding, field
  6 with null flag 5; the older ColumnStyle at 11/10) and resolves through
  the TSS parent chain. Values in the corpus: 4pt (Keynote's default),
  5.625 (a 1920x1080 scale of 4), 3.6, 7.2, 1.0, 3.0. Pages documents
  carry 4 and 8 on their shapes; the viewer applies them there too, and
  G2's export confirms the offsets (text at x=79.98 for a box at 76).
- **Geometry flags name the anchor.** `TSD.GeometryArchive.flags` bits 1
  and 2 (documented in docs/format/drawables.md, inferred, with the
  census). The converter re-anchors, so the model's top-left contract
  holds and no consumer sees the flag. Round 1's centre rule was the
  middle+centred special case.
- **Media placeholders had no identity.** They are plain
  TSD.ImageArchive/MovieArchive, not KN.PlaceholderArchive;
  `TSD.ImageArchive.flags` bit 1 marks them (inferred from the census and
  two exports; bit 2 appears with it on placeholders that hold a picture).
  `KN.SlideArchive.objectPlaceholder` (field 30) would name them too but no
  deck in the 37 writes it. New role value `"media"` on
  `placeholder.role` (model/src/keynote.ts, docs/model-design.md §3.2).
- **Line metrics are AppKit's, not CoreText's.** Not a schema matter (the
  archive stores no metrics) but a converter-adjacent fact every renderer
  needs: Keynote's pitch is NSLayoutManager.defaultLineHeightForFont, the
  first baseline its defaultBaselineOffsetForFont, the extra above the
  baseline; for Helvetica, Times, Courier, Hoefler and Palatino AppKit
  inflates ascent+descent by 1.2. Faces the Mac lacks are laid out with
  Helvetica's numbers (the export's spans of OpenSans, Rubik, ScalaSansPro
  and AdobeClean decks are all drawn in Helvetica at 0.970 / 1.2).
- **Mask rotation was in the model and unused.** `mask.common.angleDeg`;
  viewer only.
- **Empty paragraphs carry no size.** A blank paragraph has no run, so a
  consumer cannot know its line height (Keynote uses the paragraph style's
  font). Proposal, not implemented: an empty run with the resolved
  character style, or a `size` on `Paragraph`. Blank spacer paragraphs are
  common in decks; the viewer currently gives them the inherited size.
- **Keynote overruns the right indent.** ijclab's auto-height box is 1pt
  wider than text + insets + left indent and 5.4pt narrower than that plus
  the right indent, and the export draws one line. Either the right indent
  does not bound an auto-sized box or the stored width omits it; left as a
  viewer slack, noted for a fixture with the font installed.
- **Checked and present:** image fills (technique, tint, data reference),
  mask geometry with angle, chart series symbols (Numbers' round 5 field;
  RIPE 82 slide 4's line-chart markers now follow it), builds, transitions,
  notes, hyperlinks, instant-alpha paths on the 20 new decks.

What remains, in the order the judge names it:

1. Faces this Mac lacks (CMU Serif, Fira Code, Open Sans, Scala Sans,
   Produkt): Keynote draws Helvetica, the viewer a class substitute or
   the Google face; the judge reads every one as a substitution. Policy,
   not a defect (docs/fonts.md).
2. Text position drift of 1-3pt on the decks whose faces the export
   substitutes, where the browser's substitute has a different ascent from
   Helvetica's and the paragraph shift is computed from the substitute.
3. Title weight: a "Bold" cut named on a run with `bold: false` (ippp,
   domimplant) draws regular in Keynote and bold here (round 3's note).
4. Hand-drawn strokes (brush parameters), unchanged.

### Numbers, round 6 (2026-09-12, GLM thinking off, up to 3 pages per document)

Fourteen Numbers origin hosts were still unjudged. www.simplicityhub.co.uk
is the template family covered in earlier rounds and was skipped. The
eleven files from static.uahirise.org and the one from fs-fussballtalente.de
are Keynote decks (`kind: keynote`, `.key` in the origin URL) that the crawl
index labelled `.numbers`; they belong to the Keynote rounds.
www.cambridgecitywide.org's file is a 28,881 × 59 property database whose
one table is 575,596pt tall, over the page limit. www.homeworkforyou.com's
three plain tables were left out. That leaves eight usable hosts, all
surveyed, plus the www.2bobradio.org.au playlist that served as the
harness smoke test on the same build (its export was reused). No candidate
contains a chart, so the chart items under "### Next" were not exercised.
Two judges scored every page (14 pages, up to 3 per document):
GLM-5.3-Flash-EXL3 on the LAN server ("glm", thinking off) and
qwen/qwen3.8-flash through OpenRouter ("qwen-or", four requests in
parallel, 1-9 s per pair). Neither is the Qwen3.8-Flash-Next checkpoint
rounds 1-5 used, so the means are not comparable with theirs. The
fidelity work was done on the GLM scores; the qwen-or pass was added
after the round ended.

| document | host | pages | why |
| --- | --- | ---: | --- |
| e886b6eec13a | media.voog.com | 3 | three sheets, 27 merges, three pop-up controls, conditional formatting, rich-text cells; a v4.0 sibling of the eb299192a219 calculator from rounds 3-5 |
| bd3a64fbd954 | www.delpupitrealasestrellas.com | 1 | banded rows, header row, image-fill cells (emoji), boolean cells, percent format, a pre-BNC file (Numbers 2.3.4) |
| 2c11610b44c5 | densoaustralia.com.au | 2 | 80 × 22 table, 20 merges, 42 wrap styles, formula cells, an image on each sheet, a Numbers 1.5 file (storage version 3) |
| ee805c92fc38 | mariusebertsblog.com | 2 | date cells, header row and column, a merged footer cell, wrapped header text, an empty default table on sheet 2 |
| b191fa6fd022 | www.liceoartisticoenzorossi.edu.it | 1 | 30 × 12 form with 7 merges, 2pt block borders, wrapped header cells |
| d7dfa8a4b67b | www.swanseavirtualschool.org | 1 | image-fill cells (musical symbols), wrapped body cells with no cell style, header row and column |
| 1132f0aa39be | clemens-august-schule-bonn.de | 1 | eight images and a 0 × 0 text box on a sheet with no table (13 MB) |
| 1066e1585031 | oddio.space | 2 | two sheets, header column with fills, image-fill cells in the last rows (10 MB) |
| efbed96fa653 | www.2bobradio.org.au | 1 | duration format, a merged title row, bold column; the smoke-test document |

Every defect was traced to the JSON before the viewer was touched.

| defect | documents | cause | fix |
| --- | --- | --- | --- |
| 166 cells missing: the yellow input cells, both FALSE columns, the "0" totals and the numeric formula results | 2c11610b44c5 | The storage-version-3 decoder refused flag bit 3 as unknown and dropped the cell. Bit 3 is the formula key, stored between the format and string keys as in v4: type-8 cells `[cs][ts][fmt][formula][trailing]` at flags 0x8e (no cached value), type-6 cells `[cs][ts][fmt][formula][f64]` at 0xae with 0.0 = FALSE, type-3 cells at 0x9e whose string is the cached "0", type-2 cells at 0xae caching their f64. A corpus census (`PNK_DEBUG_V3`) finds v3 cells in four files and no other types | tables.rs consumes the key, emits types 6 and 8, and carries the formula ref (decoded by formulas.rs: `SUM(D11÷12)×3.141×B11÷$D$37`) |
| Checkbox cells print "false" | bd3a64fbd954 (12 cells) | v4 storage has no control-spec list (the v5 flag 0x400); the bool cell's leading format key names a format of TSK type 263 (CHECKBOX in numbers-parser's `FormatType`) and nothing else, which the decoder read as an unknown number format | tables.rs pools a checkbox control per table for a type-6 cell whose lead format is 263; the viewer already draws ☐/☑ for a checkbox control |
| "2.5" and "3.5" where the export prints "2 1/2" and "3 1/2" | 2c11610b44c5 (pipe sizes) | the v3 and v4 format mappings folded a fraction format (type 262, accuracy 0xFFFFFFFF in f11) into a plain number; the v5 path marks it "fraction" | tables.rs emits the same `fraction`/`fraction-N` and `scientific` markers on the pre-BNC paths; the viewer's fraction renderer prints "2 1/2" |
| Boolean cells print "false" where the export prints "FALSE" | 2c11610b44c5 (every unchecked column; round 3 named the same on 5a89929253a1) | lowercase constant in `valueToText` | tables.ts prints TRUE/FALSE |
| Gray behind the emoji header cells where the export is white | bd3a64fbd954 | an image fill set `background-image` and left the section's `#bec0bf` `background-color` under the PNG | tables.ts clears the colour under an image fill |
| Checkbox rows white where the export bands them | bd3a64fbd954 | the body style and every cell style store `fill: null`, and renderTable cleared the banded fill for a cell style with an explicit null. Numbers alternates body rows whose cells have no fill of their own, and null is the stock body style's state. Measured on the export: three bands of (239,239,239), 42px at 150dpi = 20pt each; the after-render has the same three at #efefef, 20pt; the before-render none | tables.ts repaints the band when the cell's own fill is null (0839b6d2, the document behind the clear, stores solid fills today and is unaffected) |
| Third-column cells on one line where the export wraps them over two and three lines | d7dfa8a4b67b ("4 crotchet beats in a bar", "Bb (flat) 1st finger ...") | the cells store no cell style over a wrapping body style; `applyCellStyle` runs twice per cell and the second pass, with `style` undefined, took the else-branch and reset the cell to nowrap | tables.ts leaves the section's wrap in place when the cell has no style of its own |
| Title "Gummitwist fangen" wrapped into two lines over the first photo | 1132f0aa39be | the 0 × 0 text box stores a natural size of 156 × 19.7pt for 40pt text: a cache from before the text was resized. The Numbers branch of `anchorZeroSizeText` wraps a 0 × 0 box between its natural width and 355pt | drawables.ts (Numbers-only branch) keeps the nowrap path when the natural height cannot hold one line of the box's own text; 6914f46e51ab's five boxes, the case behind the 355pt cap, store 21-36pt for 13-22pt text and still wrap |

Scores on the same exports, before and after, both judges:

| document | page | glm before | glm after | qwen-or before | qwen-or after | what the judges still name |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| 1066e1585031 | 1 | 9 | 9 | 9 | 9 | scaled larger, tighter margins |
| 1066e1585031 | 2 | 3 | 4 | 5 | 6 | the three cell images the export lacks (see findings) |
| 1132f0aa39be | 1 | 6 | 8 | 6 | 8 | photo crops (Haba, Zuck); images scaled up |
| 2c11610b44c5 | 1 | 6 | 8 | 6 | 9 | header text line breaks; "Total Qty." cut to "Total" in the export |
| 2c11610b44c5 | 2 | 9 | 9 | 9 | 9 | logo slightly higher and further left |
| b191fa6fd022 | 1 | 8 | 8 | 9 | 9 | the second block's header row left-aligned, not centred; rows compressed |
| bd3a64fbd954 | 1 | 7 | 9 | 6 | 8 | checkbox glyphs small squares, not filled rounded rectangles; wrapping in the description cells |
| d7dfa8a4b67b | 1 | 8 | 9 | 8 | 9 | "Bb (flat) ..." wraps to four lines, three in the export; the title at the top edge |
| e886b6eec13a | 1 | 9 | 9 | 9 | 9 | decimal comma (the locale setting) |
| e886b6eec13a | 2 | 8 | 8 | 8 | 8 | decimal comma |
| e886b6eec13a | 3 | 7 | 7 | 7 | 8 | sheet gridlines and cell borders where the export prints a clean bordered box; decimal comma |
| ee805c92fc38 | 1 | 9 | 9 | 9 | 9 | decimal comma |
| ee805c92fc38 | 2 | 8 | 8 | 9 | 8 | scaled larger and shifted to the top-left of the page |
| efbed96fa653 | 1 | 9 | 9 | 9 | 9 | table fills the page width instead of sitting inside the margins |

Mean over the 14 pages: glm 7.57 before, 8.14 after; qwen-or 7.79
before, 8.43 after. Both judges move the same three pages up by two or
more: 1132f0aa39be (the title on one line), 2c11610b44c5 page 1 (the 166
cells, the fractions: glm +2, qwen-or +3), bd3a64fbd954 (checkboxes,
banding, white behind the emoji). d7dfa8a4b67b and 1066e1585031 page 2
move up by one under both; the second names the same export-side omission
before and after. No page goes down under glm; qwen-or drops
ee805c92fc38 page 2 by one (the empty table; its remark is the page
position both times). The two judges never differ by three or more on a
page; the widest gap is two, on 1066e1585031 page 2 (glm 3/4, qwen-or
5/6), where both name the images the export lacks, so neither is wrong
about the page. The 2c11610b44c5 page was re-judged by glm once more
after the fraction fix (8 both times; the first remark moved from the
fractions to header line breaks).

#### Schema and converter findings

- Storage version 3 (Numbers 1.5-era) cell layout, now in the decoder's
  doc comment: `[03][00][type][?]` + u64 flags + keys in the order cell
  style (bit 1), text style (bit 7), format (bit 2), formula (bit 3),
  string (bit 4), f64 (bit 5), rich text (bit 9). Types seen in the corpus:
  0, 2, 3, 6, 8, 9. Dates and durations (5, 7) do not occur in v3 storage
  in the corpus and stay unhandled with a warning.
- v4 checkbox: format type 263 on a bool cell. numbers-parser's
  `FormatType` also names RATING = 267; 264-266 are not in its enum and no
  v4 file in the corpus stores them, so only the checkbox is mapped.
- `TableCellStyle.fill: null` means "no fill of its own" and does not
  suppress the table style's banding. No model change; the rule is in
  tables.ts.
- Cell image fills: the image is the whole fill. No model change.
- b191fa6fd022's second block header ("COGNOME E NOME", row 12) prints
  bold and centred in the export but its cells store no text-style key
  (flags 0x16 = cell style, format, string) where the identical rows 6, 18
  and 25 store one (0x96, text style 4, bold and centred). The cell style
  archive (2568) carries only its parent, name and cell_properties. Where
  Numbers gets the look from is not in the fields read; left open.
- 1066e1585031 sheet 2: Numbers' PDF export prints the three tall rows
  without their cell images (CoreGraphics logged a PDF error during the
  export); the viewer draws them. The judge scores the page 3 for the
  images the export lacks. Not a viewer defect.
- No model or main.ts change was needed; no proposals.

#### What remains (ranked)

1. b191fa6fd022's row-12 header look (above): a v4 question about where a
   cell without a text-style key takes bold and centre from.
2. Export scale: e886b6eec13a's Revision Log (a 782pt-wide table on a
   595pt A4 portrait page) and 2c11610b44c5's 1624pt-wide sheet are
   printed fitted to the page width, although both store
   `print.contentScale` 1.0, so the export page shows the sheet smaller
   than the viewer's 1:1 render and the judge names "scaled larger" on
   those pages (the other seven documents store 0.61-0.73 or -1). Harness
   pairing, not rendering.
3. Hairline weight: ee805c92fc38 sheet 2's 0.35pt black gridlines print
   lighter in the export than the rgba(0,0,0,0.35) 1px line here.
4. d7dfa8a4b67b: "Bb (flat) 1st finger right back and 2nd finger instead
   of 3rd" wraps to four lines here and three in the export (the
   substitute face runs wider in a 90pt column).
5. Unchanged from round 5: two-axis charts (type 11), auto-fit row
   leading per face, a720beed1ab2's header row, pie labels inside the
   slices. No document in this round has a chart.

### Keynote, round 5 (2026-09-12, GLM and qwen-or thinking off, four slides per deck)

Twelve decks from twelve origin hosts no earlier run had judged, chosen
from a feature survey of the 104 unjudged hosts under 40 MB (one deck per
host; hosts with a single deck preferred, since that is where the
unfamiliar templates are). The survey counted tables, charts, groups,
connection lines, masks, reflections, list levels, slide-number fields,
non-Latin text, gradients, shadows and builds per deck. All twelve were
exported from Keynote once; the same exports are on both sides of every
score; two judges scored the first four slides of each (38 pairs):
GLM-5.3-Flash-EXL3 on the LAN, and qwen/qwen3.8-flash through OpenRouter
under the judge name qwen-or (not the Qwen3.8-Flash-Next checkpoint of
rounds 1-4, so the means are not comparable with theirs).

| doc | host | slides | why |
| --- | --- | ---: | --- |
| 6d0a262a9ad4 | tpc.ispras.ru | 50 | Cyrillic throughout, four tables, 57 groups, image fills, gradients, Wingdings markers, 4:3 |
| 6dbe87e0ee22 | matija.pretnar.info | 77 | 63 charts on slides (column, scatter), equations, hand-drawn strokes, gradients |
| 5de28ef46913 | www.bibelportal.de | 1 | three tables with merged cells, 154 rows, a 2970x2100 slide |
| c5b5d668d69a | wiki.classe.cornell.edu | 1 | 17 connection lines, 27 content-sized shapes, saved by Keynote 6.5 |
| eba343cf501f | highfivecreate.com | 1 | Japanese text, 11 orthogonal connection lines |
| 157b84e8e0c3 | anyoneteach.com | 23 | lists nested to level 3, 161 shadows, masks, portrait 540x720 |
| 441130d2a359 | archive.jonbell.net | 44 | 55 image fills, 70 groups, 59 shadows, slide numbers on 43 slides, a table |
| 6ee4ea590b7f | media.ncd.life | 27 | 331 builds, 60 groups, a reflection, 23 masks, 117 rotated objects, eight faces |
| 79b11d2dd8d2 | www.starlingx.io | 15 | pie and stacked-bar charts, masks, opacity |
| 5f81854f90cf | senseiichiba.com | 3 | Japanese text in a Latin face, three Instant Alpha images, masks, builds |
| 4f9c2bbd0349 | stween.co.uk | 39 | gradient backgrounds, 35 masks, 34 shadows, slide numbers on 32 slides, hand-drawn strokes |
| 2406adf5cf99 | indico.cmb-s4.org | 17 | connection lines, equations, slide numbers, a table, Futura, notes |

| defect | decks | cause | fix |
| --- | --- | --- | --- |
| bold runs drawn regular in a substitute face (tpc.ispras slide 1 subtitle, ncd.life slide 1 title) | tpc.ispras, ncd.life, and every PowerPoint-import deck with a Google-substituted face | the font list names faces, not runs: "Calibri" with `bold: true` on the run never names "Calibri-Bold", so Carlito was requested at weight 400 only and Chromium drew the 700 run in the 400 face without emboldening it | webfonts.ts requests the bold counterpart of every regular face the substitute ships |
| a title's last word wrapped (cmb-s4 slide 1) | cmb-s4, and every run with tracking | `trackingPt` was applied as points; the export's 116pt Futura line is 1752pt wide, which the browser reproduces at -0.02em (1750) and not at -0.02px (1833) | text.ts applies tracking as em (shared file; proposal below) |
| a two-paragraph 0x0 box wrapped its second line at a stale width (cmb-s4 slide 2) | cmb-s4 | the "taller than one line" wrap rule compared the natural height with one line; two single-line paragraphs are two lines tall | the threshold counts the paragraphs |
| 27 label boxes with no fill, stroke or background (classe slide 1) | classe | Keynote 6.5 stores content-sized shapes as 0x0 with a unit-square path and no natural size; the 0x0 SVG painted nothing | a 0x0 shape with text takes its fill as background and its stroke as border on the content-sized box |
| connection lines converging on one point (classe slide 1) | classe | the 0x0 anchors had no laid-out box, so the lines kept stored endpoints 160pt stale | the converter estimates the laid-out box from the text (0.55 em per character, 1.2 em per paragraph, plus insets; marked inferred) and routes to its centre, trimmed at its edge |
| orthogonal connectors drawn as a fan of bent lines (highfive slide 1) | highfive | the stored path is move + line + line through a middle point; the converter rebaked it as a polyline. Keynote draws an elbow: perpendicular out of the from-shape, a bus through the middle point on the crossed axis, perpendicular into the to-shape | converter routes the elbow; the crossed axis is the gap the middle point sits in (the one it centres in when both hold it); a stale end moves the middle point with the end that still matches |
| arcs drawn as chevrons (pretnar slides 3, 4) | pretnar | editable-bezier "sharp" nodes were emitted as straight segments; the arcs start on a sharp node whose out-handle is 17pt away | a segment is a cubic whenever either handle leaves its node (G2's zigzag gains a cubic whose handles lie on the chord; re-synced) |

Both judges' means over the 38 pages, before and after, same exports.
The GLM before column doubles as the ranking of where to look next.

| doc | host | pages | GLM before | GLM after | qwen-or before | qwen-or after |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| c5b5d668d69a | wiki.classe.cornell.edu | 1 | 3.00 | 9.00 | 4.00 | 9.00 |
| eba343cf501f | highfivecreate.com | 1 | 6.00 | 9.00 | 6.00 | 9.00 |
| 6dbe87e0ee22 | matija.pretnar.info | 4 | 7.00 | 7.50 | 6.75 | 8.00 |
| 2406adf5cf99 | indico.cmb-s4.org | 4 | 8.50 | 9.25 | 8.50 | 9.00 |
| 5f81854f90cf | senseiichiba.com | 3 | 8.67 | 8.67 | 8.67 | 8.67 |
| 6ee4ea590b7f | media.ncd.life | 4 | 8.75 | 8.75 | 8.50 | 8.50 |
| 441130d2a359 | archive.jonbell.net | 4 | 9.00 | 9.00 | 9.00 | 9.00 |
| 4f9c2bbd0349 | stween.co.uk | 4 | 9.00 | 9.00 | 8.75 | 8.75 |
| 5de28ef46913 | www.bibelportal.de | 1 | 9.00 | 9.00 | 9.00 | 9.00 |
| 79b11d2dd8d2 | www.starlingx.io | 4 | 9.00 | 9.00 | 9.00 | 9.00 |
| 157b84e8e0c3 | anyoneteach.com | 4 | 9.50 | 9.50 | 9.75 | 9.75 |
| 6d0a262a9ad4 | tpc.ispras.ru | 4 | 9.50 | 9.25 | 9.25 | 9.00 |
| all | | 38 | 8.55 | 8.89 | 8.50 | 8.87 |

Pages that moved by two points or more (GLM; qwen-or in the last column):

| slide | before | after | what changed | qwen-or |
| --- | ---: | ---: | --- | --- |
| wiki.classe.cornell.edu 1 | 3 | 9 | filled label boxes; connection lines to the boxes' current centres | 4 to 9 |
| highfivecreate.com 1 | 6 | 9 | orthogonal connectors as elbows | 6 to 9 |
| matija.pretnar.info 4 | 6 | 9 | arcs as curves | 7 to 9 |
| indico.cmb-s4.org 1 | 8 | 10 | tracking as em: the title wraps where the export does | 8 to 9 |
| matija.pretnar.info 3 | 4 | 3 | arcs as curves; the scatter curve is still missing | 2 to 5 |

Agreement between the two judges over the 76 scored pairs: 97% within
one point, mean absolute difference 0.25, qwen-or 0.04 below GLM. No
pair differs by three or more; the largest gap is pretnar 3 after (GLM
3, qwen-or 5), where both name the same thing, the scatter chart's
curve drawn as a flat line with a stray legend, and differ only on how
much the fixed arcs earn back. By eye the slide has every stroke of the
export except that curve, so 5 is the fairer score. Under GLM, pages
at 9 or more went from 30 to 35 of 38 (qwen-or: 28 to 33). Two pages
dropped one point under GLM:
tpc.ispras 4 (10 to 9, "sub-pixel shifts" on an unchanged render) and
pretnar 3 (4 to 3: the arcs are now right and the judge names what is
left, the scatter chart's curve drawn as a flat line with a legend the
export omits; Numbers lane, below). The bold subtitle on tpc.ispras 1 and
the unwrapped bold line on cmb-s4 2 are fixed on renders the judge
already scored 9 and 8; ncd.life 1's title is Mulish 700 from Google
Fonts here and Helvetica-Bold in the export (Keynote lacks Mulish), the
substitution policy of docs/fonts.md and not a defect.

Confirmed against the exports by measurement, not by eye: cmb-s4's
title line at 1750.1pt in the browser under -0.02em against the export's
1752.2pt (1832.9 under -0.02px); its bold line at 1606pt on both sides
once unwrapped; highfive's bus at y=216.8 from the stored middle point
against the export's 216; the elbow's stem at the top box's centre
(x=162) and its drop at each child's centre; classe's label boxes
centred on their stored anchors in the export (A1 spans 29-72pt for a
stored x of 50); pretnar's arc nodes read from the archive with
iwadump (node 1 at (13.0, 0) with its out-handle at (-4.3, 17.1)).

#### Schema and converter findings

- **Tracking is a fraction of the font size.** `CharStyle.trackingPt`
  carries `TSWP.CharacterStyle.tracking` unchanged, and the value is a
  fraction of the em (Apple's inspector shows it as a percentage; the
  measurement above). The viewer now applies it as em. Proposal, not
  implemented (model file): rename to `tracking` with the unit in its
  doc comment, or document `trackingPt` as em with a deprecation note;
  the converter's value does not change either way.
- **Orthogonal connection lines** (`ConnectionLinePathSourceArchive.type`
  1) now route as elbows at emission; the path carries the result, as the
  quadratic case did in round 2. Documented in docs/format/drawables.md
  as inferred from one export.
- **Content-sized shapes without a natural size.** Keynote 6.5 (classe,
  `M6.5.3`) stores a text shape as 0x0 with a unit-square path and no
  `naturalSize` where later versions store the laid-out size. The model
  has no field for the laid-out box, and the archive does not carry it;
  the converter's estimate lives only in the connection-line anchor
  walk. A `laidOutSize` on the shape would let the viewer and the dumpers
  share one estimate; not proposed, since the viewer measures the real
  text and only the converter needs a number.
- **Editable-bezier node types** do not decide straightness: a sharp node
  can carry handles. Corrected in tsd.rs; docs/format/drawables.md
  updated.
- **Font list versus run flags.** The envelope's `fonts` names faces as
  stored; PowerPoint-import decks store family names with bold and italic
  flags on the runs. A consumer that loads substitutes needs the flags,
  which the list does not carry. The viewer now over-requests (the bold
  counterpart of every regular face). Proposal: the converter adds the
  weight-named cut ("Calibri-Bold") to `fonts` when a run sets `bold` on
  a family name, so the list names what the runs need; lives in ctx.rs
  and styles.rs.
- **Numeric table cells left-aligned in a Keynote table** (tpc.ispras
  slide 8): the table's body cell style says left, the numeric cells'
  own style carries no alignment, and Keynote's export right-aligns the
  numbers; "0.125" is stored as a text cell and the export right-aligns
  it too. Numbers-owned (tables.rs, tables.ts); not touched. Noted for
  the Numbers lane.
- **Scatter chart on a slide drawn as a flat line** (pretnar slide 3):
  `scatterFormat: "shared-x"` with the x values in the second series;
  and a legend the export omits (`legendVisible: false` is stored and the
  viewer still prints "Region 1 Untitled 107"). charts.ts, Numbers-owned;
  not touched.
- **CJK text in a Latin face** (senseiichiba slides 1, 2: Druk-Medium
  runs of Japanese). The export sets the kanji in PingFang SC and the
  kana in Hiragino Sans (Keynote's per-script fallback), and the
  paragraph's 0.8 line-spacing multiple gives 214.8pt between two 150pt
  lines on slide 2 (1.432 em) and 188pt between two 236pt lines shrunk
  to 167.6pt on slide 1; the viewer pitches with Helvetica's numbers
  (the face this Mac lacks) and draws the lines 30% closer. The rule is
  not derived yet; noted.
- **Checked and present:** builds, transitions, notes, slide-number
  fields, masks with angles, Instant Alpha paths, image fills, group
  nesting, hand-drawn stroke names, table merges on all twelve decks.

What remains, in the order the judge names it:

1. Faces this Mac lacks and the wrap differences they cause (cmb-s4's
   Futura is present, so its wraps are fixed; Calibri, Druk, Mulish,
   FreightSans are not). Policy (docs/fonts.md).
2. CJK text in a Latin face: the line pitch of the fallback faces
   (above).
3. Charts on slides in the Numbers lane: pretnar's scatter charts and
   hidden legends; starlingx's pie and stacked bar were not in the
   judged four slides.
4. Hand-drawn strokes: ncd.life slide 4's arrows are thin wobbly Pen
   strokes in the export and plain 6pt strokes here (brush parameters,
   unchanged since round 3).
5. Numeric alignment in Keynote tables (Numbers lane).

### Pages, round 4 (2026-09-12, GLM thinking off and qwen/qwen3.8-flash via OpenRouter, up to 3 pages per document)

Nine documents from hosts no earlier round had judged, picked from a
pnk2json feature survey of all 183 candidates (none of the 183 carries a
footnote or a multi-column section; the survey found no such document to
fill those slots). Two judges scored the same pairs: GLM-5.3-Flash on the
LAN (the judge of the earlier rounds' comparison table, not the Qwen
checkpoint rounds 1-3 used) and qwen/qwen3.8-flash through OpenRouter
(judge name qwen-or, a hosted model, not the Qwen3.8-Flash-Next
checkpoint). Up to three pages per document, page N against page N,
21 pairs.

| document | host | pages (Pages / ours) | why |
| --- | --- | ---: | --- |
| 4d268c4161c1 | metodistkirken-odense.dk | 13 / 13 | multi-page Calibri text with a page-number footer |
| fe2facece68e | sarahpetrich.com | 3 / 3 | newsletter: dated header, five floating photos with wrap, lists |
| 8e92cf882b53 | astrid-lindgren-schule-ulm.de | 5 / 5 | form of 14 tables with 16 merges, tabbed header, Wingdings boxes |
| bbb758110464 | yskick.com | 2 / 2 | Japanese text set in Times-Roman (the glyphs come from a fallback face) |
| 54119baf383d | australianlaceguild.com.au | 7 / 7 | lists and five inline images |
| 4ccaddd3f0b5 | internetgeography.net | 2 / 2 | worksheet of 15 anchored text boxes, groups and an image, 18pt margins |
| 1d3f1667cd27 | feature3.net | 29 / 29 | 7,100-word document, pagination drift check |
| f2a516705f13 | theo.ac.cy | 1 / 1 | page-layout poster in polytonic Greek |
| 4ecab480ce51 | blog.kakaocdn.net | 1 / 1 | Korean one-page form table (the smoke-test lead) |

Defects fixed, with cause and fix:

| defect | documents | cause | fix |
| --- | --- | --- | --- |
| body text ran under floating photos narrower than 60% of the column (two anatomy figures on page 1, two photos on page 2) | fe2facece68e; 48f5f124 checked | `pageExclusion` made a band only for wide objects and let the text run under narrow ones | viewer (pages.ts): each narrow wrapping object excludes its own side — the side with more room, whatever the wrap kind (48f5f124's logos store type 4 at the right margin and Pages sets the title to their left) — as a staircase `shape-outside` polygon on a left or right float per vertical cluster; boxes as wide as their widest step so list rows land beside the object; a gutter under a quarter of the column still makes a band |
| a label shape Pages keeps off the page (x = -101) came on-page beside a photo | fe2facece68e page 2 | the exclusion float's box pushed the later zero-width anchor float to its right and the drawables moved with it | viewer: `fixAnchorDrift` repins anchor floats sideways as it does vertically |
| the body started at the top margin under an empty header: every box 26pt high on 4ccaddd, the title and logos 64pt high on 7edb1b23 (round 3b item 4) | 4ccaddd3f0b5, 7edb1b23ebd6 | (1) `hfPushForPage` measured only templates with header text; (2) the push was a band float, which moves line boxes but not the box of a paragraph holding only anchored objects, so the objects stayed at the margin | viewer: empty header paragraphs count (their `<br>` gives them a line), each column measured at the full text width; the push is the page container's top padding. 4ccaddd's body now sits at 32pt against Pages' 44, 7edb1b23's at 138 against 135.6 |
| header "…Unterstützungsbedarfs   Stand: 09.2021" collapsed its 40 spaces and centred | 8e92cf882b53 | headers are outside `.pages-print`, whose `pre-wrap` rule keeps spaces | styles.css: `.pages-hf p` keeps spaces and tabs |
| "Diese Meldung geht direkt…" and "☐ Mutter ☐ Vater" bold in a bold-styled cell | 8e92cf882b53 | `applyCharStyle` set the weight only for `bold: true`; a run resolved to `bold: false` inherited the cell's 700 | viewer (text.ts): explicit false sets 400 / normal, unless the face name carries its own weight or slant |
| table rows 3pt taller than Pages' (33.0 against 30.3 per row; page 2 started two rows early) | 8e92cf882b53; 5c07d836 checked | cells used the unrounded natural height with the gap on every line; the face never matched because Chrome serializes plain family names unquoted | viewer (tables.ts): Pages cells take the rounded single-spaced rule (text.ts FONT_METRICS) with the gap dropped after the paragraph's last line: 31.0 now; 5c07d836's TOC rows stay at 20.6 |
| Japanese text 14pt per line where Pages lays out 17 (12pt Times-Roman): page 1 held 8 more lines and page 2 different content | bbb758110464; 9cd3036a checked (16 for 11pt) | the line height came from Times-Roman; Pages takes the glyphs from Songti SC and the line from it | viewer (text.ts): the largest rounded ascent, descent and gap over the faces on the line, including the fallback: serif faces + CJK text = Songti SC (1.06/0.34/0, hhea), geometric-shape symbols = Hiragino Mincho (its 0.5 gap: the "●" lines at 23) |
| banding missing: header and footer rows white where Pages fills #efefef | 4ecab480ce51 | the round-3b rule cleared banding for any cell style with fill "none"; here the body style is fill-less too, so the value is inherited | viewer (tables.ts): the cell's "none" turns banding off only over a filled section default (0839b6d2 checked: still white) |
| leading spaces of cell text collapsed ("연락처(핸드폰) :" at the left instead of centred by 36 spaces) | 4ecab480ce51 | plain-text cells had `white-space: normal` | viewer (tables.ts): pre-wrap / pre when the text has leading or repeated spaces |
| a four-line paragraph moved whole to the next page where Pages keeps two lines; 30 pages against 29 | 1d3f1667cd27 | `splitOverflow` returned null when the largest fitting prefix left one line behind | viewer: the cut moves up to the last line that leaves two |

Scores on the same exports, before and after the fixes, both judges
(GLM: mean 6.90 to 8.15 over 20 pairs, one verdict unparsed on each
side; qwen-or: 6.48 to 8.24 over 21). Score counts, GLM before: 3 ×5,
6 ×4, 7 ×1, 8 ×1, 9 ×6, 10 ×3; after: 4 ×1, 5 ×2, 8 ×5, 9 ×11, 10 ×1.
qwen-or before: 2 ×1, 4 ×7, 5 ×2, 6 ×1, 8 ×1, 9 ×8, 10 ×1; after: 4 ×1,
6 ×3, 7 ×1, 8 ×1, 9 ×14, 10 ×1.

| document | host | judged | GLM before | GLM after | qwen-or before | qwen-or after |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| bbb758110464 | yskick.com | 2 | 4.5 | 8.5 | 3.0 | 7.0 |
| fe2facece68e | sarahpetrich.com | 3 | 3.0 | 7.3 | 4.0 | 7.3 |
| 8e92cf882b53 | astrid-lindgren-schule-ulm.de | 3 | 4.0 | 8.0 | 4.3 | 8.0 |
| 4ccaddd3f0b5 | internetgeography.net | 2 | 6.0 | 4.5 | 5.0 | 6.5 |
| 4d268c4161c1 | metodistkirken-odense.dk | 3 | 8.7 | 9.0 | 7.7 | 9.0 |
| 4ecab480ce51 | blog.kakaocdn.net | 1 | 7.0 | 9.0 | 8.0 | 9.0 |
| 54119baf383d | australianlaceguild.com.au | 3 | 9.7 | 9.0 | 9.0 | 9.0 |
| f2a516705f13 | theo.ac.cy | 1 | 9.0 | 9.0 | 9.0 | 9.0 |
| 1d3f1667cd27 | feature3.net | 3 | 9.3 | 9.3 | 9.3 | 9.3 |
| all | 9 documents | 21 | 6.90 | 8.15 | 6.48 | 8.24 |

Pages that moved by two points or more under both judges: fe2facece68e
1 and 3 (GLM 3 to 9 and 3 to 8; qwen-or 4 to 9 both — the side wraps),
8e92cf882b53 1, 2, 3 (GLM 6/3/3 to 8/8/8, qwen-or 5/4/4 to 9/6/9 —
header spaces, bold, row heights), bbb758110464 1 and 2 (GLM 6/3 to
8/9, qwen-or 4/2 to 8/6 — the CJK line height). Under one judge:
4ecab480 (GLM 7 to 9, the banding), 4d268c4161c1 page 3 (qwen-or 5 to
9, the widow rule) and 4ccaddd3f0b5 page 1 (qwen-or 4 to 6, the header
push). The one drop is 4ccaddd3f0b5 page 2 under GLM (6 to 4): the
anchored boxes moved 14pt down toward Pages' position while the
page-relative title stayed, so the overlap between them grew (item 1
below); qwen-or scored the same page 6 to 7.

Pages where the judges differ by three or more, read by eye: 4d268c4161c1
page 3 before (GLM 8, qwen-or 5) is missing its last six-line paragraph,
a third of the page's text, so 5 is the right score; 4ccaddd3f0b5 page 2
after (GLM 4, qwen-or 7) has every box and its text with the title over
the first line of two boxes, a 6; bbb758110464 page 2 after (GLM 9,
qwen-or 6) is the same content with one line moved to page 1 and slightly
different spacing, an 8. qwen-or is not the checkpoint earlier rounds
used; its before mean (6.48) sits 0.42 under GLM's on the same pairs.

What remains, in the order the judges name it after the fixes:

1. Anchored objects whose offset is page-relative (`v_offset_type` 1):
   4ccaddd3f0b5's title box prints over the top boxes on both pages
   (qwen-or 6 and 7, GLM see table); the proposal below.
2. Text in a text box does not wrap around a floating image over it
   (4ccaddd's map: the question runs under the map).
3. fe2facece68e page 2 (4 both judges): the column beside the two photos
   is narrower here than in Pages, which wraps to the photos' opaque
   contour (`textWrap.fit` absent = alpha) rather than their frame, so
   "Technique #2" lands lower and one line spills to page 3. The alpha
   contour is round 2's unimplemented proposal.
4. bbb758110464 page 2 (6): the "○" lines (U+25CB, HiraMin in Pages)
   and the form's underlined blanks differ in pitch; sans faces with CJK
   text get no fallback height at all (PingFang's metrics are not on
   disk).
5. 8e92cf882b53 page 2 (6): the second page starts one row earlier than
   Pages' — the remaining 0.7pt per row is the 1px border CSS draws for a
   0.5pt stroke, 16 rows a page.
6. The gap Pages leaves under a header: 4ccaddd's body is 12pt high.
7. Helvetica line breaks (1d3f1667cd27: Arimo stands in, a few lines
   break differently; the page count matches now).

#### Schema and converter findings

- Wrap type 4 (`textWrap.kind: "right"`, 139 images and 104 text boxes
  in the corpus) does not name the text's side: 48f5f124 stores it on
  logos at the right margin and Pages sets the cover title to their
  left. The viewer takes the side with more room for every kind; the
  proto's names for 3/4 stay unverified.
- `TSWP.DrawableAttachmentArchive` fields 2/4 (`h_offset_type`,
  `v_offset_type`) decide what the offset is measured from, and the
  converter drops them. 4ccaddd3f0b5 proves one value: its title box
  stores v type 1 and offset 17.75, and Pages draws it at page y 17.75
  (baseline 70.45 = 17.75 + 4 inset + 48 ascent) while its type-0
  siblings sit at the paragraph top (44pt) + offset. The logo group
  stores type 1 too. This viewer draws type-1 objects at paragraph top +
  offset, 26pt low here.
- Empty header storages: the archive stores one empty paragraph per
  column (StorageArchive with no text, 4ccaddd), and Pages still lays the
  header out at the paragraph's line height. What Pages adds under the
  header is not known: with the header height alone the body lands 12pt
  high on 4ccaddd (32 vs 44), 2pt low on 7edb1b23 (138 vs 135.6), and on
  5c07d836's page 2 at Pages' 68.6.
- CJK fallback faces: Pages' export names them (STSongti-SC for Latin
  serif runs in bbb758 and 9cd3036a, HiraMinProN for "●", PingFangSC for
  a Courier title's ideographic space); the archive stores only the run's
  font. Metrics from the fonts' hhea tables (Songti SC 1.06/0.34/0,
  Hiragino 0.88/0.12/0.5); PingFang's are not on disk, so sans faces get
  no fallback height.
- Pages cell rows (8e92cf882b53, gridlines from the export's drawing
  list): a row is padding + Σ lines, each line = rounded ascent +
  rounded descent (times the multiple: the two-line Calibri 8 cell at
  1.079× has a 10.65 pitch; the single-line Arial-Bold 12 row at the
  same multiple measures 14, so which rows take it is not settled), with
  the gap between the lines of a paragraph and none after its last line
  (Calibri 10's en-space line is 11, not 13). The viewer applies the
  single rule with the gap dropped.
- Cell fill "none" is emitted for both an own and an inherited none
  (`TableCellStyle.fill: null`); the viewer tells them apart by the
  section default's fill (4ecab480 vs 0839b6d2). A converter flag would
  be cleaner but the same rule works.

Proposals not implemented:

- `InlineObjectRun.offsetOrigin?: { h?: "paragraph" | "page" | "margin"; v?: … }`
  from `h_offset_type` / `v_offset_type` (round 3a's proposal, now with a
  proof: 4ccaddd3f0b5's title box, v type 1 = page top; 7edb1b23 and
  cf4b76a store 2). The viewer would place type-1 objects at the page
  origin + offset.
- Text inside a text box does not wrap around a floating image over it
  (4ccaddd's map inside the "With the help of the diagram" box; Pages
  runs the question to the right of the map). No model change; the
  canvas would need an exclusion per overlapping wrapping object
  (drawables.ts, Keynote's file).

### Next

Numbers: b191fa6fd022's row-12 header (bold and centred in the export, no text-style key in the v4 cell); hairline weight on 0.35pt gridlines (ee805c92fc38); two-axis charts (type 11, 666 in 0ab5dd52841e); auto-fit row leading per face (17891b89da2f 14 vs 16pt rows); the a720beed1ab2 header row the export prints blank; pie labels inside the slices (6914f46e51ab). The unjudged Numbers hosts are used up; the next round re-scores earlier documents or the other files of judged hosts.
Keynote: CJK text in a Latin face (senseiichiba 5f81854f: Keynote pitches the lines with PingFang/Hiragino's metrics, 1.432 em on slide 2); charts on slides in the Numbers lane (pretnar 6dbe87e0's scatter curve and hidden legend; tpc.ispras 6d0a262a's numeric cells left-aligned); the font list naming the bold cut a run's flag asks for (proposal in round 5); faces the Mac lacks (policy, docs/fonts.md); hand-drawn brush parameters; then 20 more decks from unjudged hosts.
Pages: page-relative anchored objects (`v_offset_type` 1: 4ccaddd3f0b5, proposal in round 4); text-box text wrapping around floating images over it (4ccaddd, drawables.ts); the alpha wrap contour (fe2facece68e page 2, round 2 proposal); sans faces with CJK text (PingFang metrics unknown) and Arabic documents in a face Pages substitutes (ae1cc13b, 77890685); the gap Pages leaves under a header (4ccaddd 12pt); 1px borders for 0.5pt cell strokes (8e92cf882b53, 0.7pt a row); rotated wrapping objects (10a06959, 25 documents); b31db822's cover shape lines cut at the left (drawables.ts); `h_offset_type`/`widow_control` unmodelled; reconciling Numbers' row-leading measurements with the round-3a line rule; 4047e81b page 4; then the unexamined verdicts in round 3b's list.
Score more of the corpus, one or two pages per document, with Qwen; use
the ranked list to choose fidelity work; add a reference re-run with
Claude when the prompt changes again.
