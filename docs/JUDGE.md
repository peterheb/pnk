# Render-fidelity judging (`scripts/judge.py`)

Vision LLMs score how faithfully the pnk viewer reproduces a page, sheet, or
slide against the real app's PDF export. It is the second half of the
ground-truth loop: `visual_diff.py` harvests the image pairs, `judge.py`
grades them. The rubric lives in `scripts/judge_prompt.md` (versioned; bump
`PROMPT_VERSION` when it changes, verdicts are cached per version).

## Pipeline

1. **Harvest pairs.** `scripts/visual_diff.py --app <app> --fixture <file>
   --out <run>` writes `<run>/apple/page-N.png` (app export, via PDF) and
   `<run>/ours/{page,sheet,slide}-N.png` (Playwright screenshot of the
   viewer, local dist by default, `--base-url https://pnk.vu` for the live
   site). Pages are paired 1:1 by index; skipped Keynote slides are excluded.
2. **Judge.** Each pair is resized to a common height, JPEG-encoded, and sent
   to every judge with the rubric. Verdicts append to
   `<out>/judgments.jsonl`, keyed by (judge name, model, prompt version,
   golden sha, candidate sha), so re-runs only score new work.
3. **Report.** `judge.py report --out <out>` writes `<out>/report.md`:
   per-judge mean/median, control accuracy, per-app and per-document means,
   and pairwise agreement between judges (Spearman ρ, mean |Δ|, within-1).

## Judges

A judge is `name=<spec>`; the name is the cache key, so use a fresh name for
a new model, prompt experiment, or thinking setting.

| spec | what it is |
|---|---|
| `name=http://host:port/v1,<model>[,api-key]` | any OpenAI-compatible chat server (vLLM, TabbyAPI, llama.cpp, …) |
| `name=anthropic,<model>` | Anthropic Messages API, key from `ANTHROPIC_API_KEY` (never on the command line) |
| `name=pixel,pixel` | deterministic block-SSIM baseline, no network |

Rotating the model on an inference box is just a new judge name pointing at
the same URL, e.g. `--judge qwen=http://192.168.87.91:4444/v1,<qwen-id>`.
`GET /v1/models` on the box lists the ids it actually serves; they rarely
match the nickname.

## Controls (`--controls`)

Every run also scores two synthetic pairs per document: **identity** (the
golden against itself, expect 10) and **misaligned** (the golden against a
different page's render, expect 0). The report counts how often each judge
gets them right. The pixel baseline passes identity every time and fails
misaligned most of the time, because two slides from one deck share a
template, which is exactly the judgement a vision model is supposed to add.

## Thinking budget (`--effort`)

Both local models are reasoning models. With thinking on, DeepSeek V4 Flash
spent the entire 8,000-token budget deliberating over an identical pair
(26k characters of "wait, let me look again") and never emitted a verdict.
`--effort none` (the default) sends `reasoning_effort: "none"` plus
`chat_template_kwargs: {"thinking": false}`; vLLM honours both, and a
verdict then takes 3 to 5 seconds. `low`/`medium`/`high` are passed through
for servers that honour them. To compare thinking on vs off, run twice with
different judge names, and do the two runs separately: the rubric prompt is
identical for every request, so a run is nearly all prefix-cache hits, but
on the GLM server toggling thinking invalidates the cached prefix (the
effort line sits inside the gated template region), and interleaving the
two settings re-prefills every request. With thinking off `max_tokens` is
capped at 1,500 so abandoned or runaway generations stay short.

## Concurrency, and how the GLM box died

`--concurrency` defaults to 1 per LLM judge; `--concurrency 2
--concurrency glm=1` sets a default and a per-judge override. The default
is 1 because of the first bake-off: the GLM server (vLLM v1, EXL3 weights,
tensor-parallel across two DGX Spark nodes) had about 4 GiB of host memory
headroom. Two-wide multimodal requests ate that in twenty minutes; a second
client accidentally started alongside made it four-wide, MemAvailable hit
0.3 GiB, the kernel evicted the mmapped weight pages, and every forward
pass re-read weights from NVMe (1.9 GB/s of reads, decode 25 → 0.2 tok/s).
An hour later the TP shared-memory link wedged and the engine died on a
`sample_tokens` RPC timeout, which vLLM reports as a graceful shutdown.
Watch `MemAvailable` on the head node when raising concurrency, and keep
clients from abandoning in-flight requests (a killed client leaves the
server generating).

## Usage

```
# local boxes, all three apps, 4 pages per document
python3 scripts/judge.py run \
  --runs-root vd --runs-root vdn --runs-root vdk \
  --judge pixel=pixel,pixel \
  --judge deepseek=http://192.168.87.91:4444/v1,deepseek-v4-flash \
  --judge glm=http://192.168.87.93:8888/v1,GLM-5.3-Flash-EXL3 \
  --controls --max-pages 4 --concurrency 2 --concurrency glm=1 --out judge-out
python3 scripts/judge.py report --out judge-out

# later, Claude as the reference judge
ANTHROPIC_API_KEY=... python3 scripts/judge.py run ... --judge claude=anthropic,claude-fable-5-1
```

Needs Pillow (`uv run --with pillow python3 scripts/judge.py …`). On macOS
the first LAN request triggers the Local Network privacy prompt for the
terminal app; grant it permanently under System Settings → Privacy &
Security → Local Network.

## First bake-off (2026-09-02, prompt v1, thinking off)

28 documents (12 Pages, 8 Numbers, 8 Keynote, up to 4 pages each), 87 real
pairs plus 28 identity and 22 misaligned controls, all judged blind by
each model. The GLM server fell over mid-run (see the concurrency section)
and the last 13 GLM verdicts were collected after it was rebuilt; its
s/pair figure includes the slow tail before the crash.

| judge | model | pairs | mean | identity (expect ≥9) | misaligned (expect ≤1) | s/pair |
|---|---|---:|---:|---:|---:|---:|
| deepseek | deepseek-v4-flash | 87 | 7.40 | 22/28 | 7/22 | 7.8 |
| glm | GLM-5.3-Flash-EXL3 | 87 | 6.64 | 28/28 | 20/22 | 37.6 |
| pixel | block SSIM | 87 | 6.56 | 28/28 | 2/22 | 0.6 |

Agreement on the real pairs: DeepSeek vs GLM Spearman ρ 0.68, mean gap
1.7 points, within one point 61% of the time; each LLM vs the pixel
baseline ρ 0.50–0.64.

Reading: **GLM is the judge to trust of the two.** It passes the controls
(it notices when the candidate is a different page and never docks an
identical pair) and its scores track
the known state of the viewer (Numbers weakest, Keynote strongest). DeepSeek
without thinking is generous and often does not look: it gave 10 to a Pages
cover whose rotated title bar we place on the wrong edge, and 8 to a Numbers
sheet whose lower half is missing from our capture. Both LLMs rank the apps
the same way as the pixel baseline, and all three agree Numbers is where the
fidelity work is.

Two harvest issues surfaced by reading the disagreements: Numbers exports
scale a whole sheet onto one PDF page while our screenshot is the viewport,
so long sheets lose rows on our side (harvester bug, not viewer), and a
per-document `--max-pages` of 4 over-samples short documents. Next: rotate
the Qwen checkpoint in as a third judge, then Claude via `anthropic,` as the
reference judge and re-derive the per-judge numbers against it.
