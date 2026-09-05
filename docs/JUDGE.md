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

### Next

Numbers: grouped shapes placed beside the wrong table (181f2b199bd3), zero-height line shapes, chart legend markers; then the locale question for number formatting.
Keynote: text position drift of a few points (measure RIPE 82's footer and greenberg's title first), chart markers and hidden legends on slides (Numbers-owned), then wrap differences from fallback faces.
Pages: the page-layout header master choice (26a356dc), box strokes Pages does not draw, the paragraph painting over an inline table (cf4b76a), then fonts; score the long documents with `--align-content` so pagination drift stops hiding the render.
Score more of the corpus, one or two pages per document, with Qwen; use
the ranked list to choose fidelity work; add a reference re-run with
Claude when the prompt changes again.
