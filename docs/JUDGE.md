# Render-fidelity judging (`scripts/judge.py`)

`judge.py` sends pairs of images to vision language models and asks each
model to score, from 0 to 10, how closely the pnk viewer's rendering of a
page, sheet, or slide matches the PDF export from Pages, Numbers, or
Keynote. `scripts/visual_diff.py` produces the image pairs; `judge.py`
scores them and compares the judges against each other. The scoring
instructions are in `scripts/judge_prompt.md`. They are versioned: change
`PROMPT_VERSION` in `judge.py` when the prompt changes, because scores are
cached per prompt version.

## Pipeline

1. **Produce pairs.** `scripts/visual_diff.py --app <app> --fixture <file>
   --out <run>` writes `<run>/apple/page-N.png` (rendered from the app's PDF
   export) and `<run>/ours/{page,sheet,slide}-N.png` (a Playwright
   screenshot of the viewer; local build by default, `--base-url
   https://pnk.vu` for the live site). Page N is paired with page N. Skipped
   Keynote slides are left out on both sides.
2. **Score.** Each pair is resized to 1100 px tall, JPEG-encoded, and sent
   to every judge with the prompt. Results are appended to
   `<out>/judgments.jsonl`. The cache key is (judge name, model, prompt
   version, golden image sha, candidate image sha); a second run scores only
   pairs that have no successful result yet.
3. **Report.** `judge.py report --out <out>` writes `<out>/report.md` with,
   per judge: mean and median score, control accuracy, mean score per app
   and per document; and for each pair of judges: Spearman rank
   correlation, mean absolute difference, and the share of pairs scored
   within one point of each other.

## Judges

A judge is given as `--judge name=<spec>`. The name is part of the cache
key, so use a new name for a new model, a prompt experiment, or a different
thinking setting.

| spec | meaning |
|---|---|
| `name=http://host:port/v1,<model>[,api-key]` | an OpenAI-compatible chat server (vLLM, TabbyAPI, llama.cpp) |
| `name=anthropic,<model>` | Anthropic Messages API; the key is read from `ANTHROPIC_API_KEY` |
| `name=pixel,pixel` | block SSIM between the two images, mapped to 0–10; no network |

To add a model that has been loaded on an existing server, add a judge
with a new name and the same URL. Run `GET /v1/models` first: the id the
server reports is the one to pass, and it is often not the informal name.

## Controls (`--controls`)

With `--controls`, each document also gets two synthetic pairs: the golden
image against itself (identity, expected score 10) and the golden image
against a different page's rendering (misaligned, expected score 0). The
report counts identity scores of 9 or more and misaligned scores of 1 or
less as correct. A judge that fails the misaligned control is not looking
at the page content. The pixel baseline passes identity every time and
fails misaligned most of the time, because slides in one deck share a
template and so have similar block statistics.

## Thinking (`--effort`)

The local models are reasoning models. With thinking on, DeepSeek V4 Flash
used all 8,000 output tokens on an identical pair and produced no verdict.
`--effort none`, the default, turns thinking off; `low`, `medium`, and
`high` are passed through to servers that support them. The request fields
sent for each setting are in `effort_params()` in `judge.py`:

| server | off | low/medium/high |
|---|---|---|
| vLLM + DeepSeek V4 | `reasoning_effort: "none"` or `chat_template_kwargs.thinking: false` | ignored; the model thinks until `max_tokens` |
| vLLM + GLM 5.3 | either of the above | `reasoning_effort` |
| vLLM + Qwen 3.8 -Next | `chat_template_kwargs.enable_thinking: false` | `chat_template_kwargs.reasoning_effort` |

The harness sends the union of these fields; each template ignores the
ones it does not use.

With thinking off, a verdict is about 80 output tokens, so `max_tokens` is
1,500; with thinking on it is 8,000.

Use one effort setting per run. The prompt text is identical for every
request, so the server's prefix cache serves most of each request. On the
GLM server the effort setting is inside the part of the chat template that
depends on the thinking flag, so alternating settings invalidates the
cached prefix and each request is re-prefilled (measured: 26 s instead of
0.6 s).

## Concurrency (`--concurrency`)

`--concurrency N` sets the number of parallel requests per LLM judge;
`--concurrency name=N` overrides it for one judge. The default is 1. The
pixel judge always runs 4 wide on local CPU.

The default is 1 because of what happened on 2026-09-02. The GLM server
(vLLM v1, EXL3 weights, tensor-parallel across two DGX Spark nodes) had
about 4 GiB of free host memory. Two parallel requests reduced that to 0.6
GiB in twenty minutes. A second client started by mistake made it four
parallel requests; free memory reached 0.3 GiB, the kernel evicted the
memory-mapped weight pages, and each forward pass re-read weights from
NVMe (1.9 GB/s of disk reads; decode speed went from 25 to 0.2 tokens per
second). An hour later the tensor-parallel shared-memory link stopped
responding and the engine exited on a `sample_tokens` RPC timeout, which
vLLM logs as a normal shutdown. The same server configuration with Qwen
3.8 stopped responding after 130 requests at 4 parallel.

Two settings on the server side caused the memory shortage: the KV cache
pool was sized as a fraction of unified memory, leaving about 6 GiB free
regardless of model size, and vLLM's host-side multimodal processor cache
(`mm_processor_cache_gb`, default 4 GiB) filled with preprocessed image
tensors because every image in a run is distinct. With the KV pool reduced
to 580k tokens and the processor cache disabled, the server had 27 GiB
free and stayed at 27 GiB through a 137-request run at 4 parallel.

If you raise concurrency, watch `MemAvailable` on the server during the
first hundred requests. A client that is killed leaves its in-flight
requests running on the server, so stop a client with the harness's own
timeout rather than by killing it while requests are outstanding.

## Usage

```
# local servers, all three apps, up to 4 pages per document
uv run --with pillow python3 scripts/judge.py run \
  --runs-root vd --runs-root vdn --runs-root vdk \
  --judge pixel=pixel,pixel \
  --judge deepseek=http://192.168.87.91:4444/v1,deepseek-v4-flash \
  --judge glm=http://192.168.87.93:8888/v1,GLM-5.3-Flash-EXL3 \
  --controls --max-pages 4 --concurrency 2 --concurrency glm=1 --out judge-out
uv run --with pillow python3 scripts/judge.py report --out judge-out

# Claude as a judge
ANTHROPIC_API_KEY=... uv run --with pillow python3 scripts/judge.py run ... \
  --judge claude=anthropic,claude-fable-5-1
```

Requires Pillow. On macOS the first request to a LAN address triggers the
Local Network permission prompt for the terminal application; the
permission can be granted permanently under System Settings → Privacy &
Security → Local Network.

## Results, 2026-09-02/03, prompt v1

Inputs: 28 documents (12 Pages, 8 Numbers, 8 Keynote), up to 4 pages
each: 87 real pairs, 28 identity controls, 22 misaligned controls (six
documents have one page and get no misaligned control). Each judge scored
every pair; 137 requests per judge. Thinking was off unless the judge
name says otherwise. Claude Fable 5.1 through the Anthropic API is the
reference judge.

| judge | model | mean score | identity correct | misaligned correct | seconds per pair |
|---|---|---:|---:|---:|---:|
| claude | claude-fable-5-1 (reference) | 5.91 | 28/28 | 20/22 | 7.9 |
| deepseek | deepseek-v4-flash | 7.40 | 22/28 | 7/22 | 7.8 |
| glm | GLM-5.3-Flash-EXL3 | 6.64 | 28/28 | 20/22 | 37.6 |
| qwen | qwen3.8-flash-next | 6.66 | 28/28 | 21/22 | 10.2 |
| qwen-low | qwen3.8-flash-next, low thinking | 6.18 | 28/28 | 20/22 | 28.4 |
| pixel | block SSIM | 6.56 | 28/28 | 2/22 | 0.6 |

Mean score by app:

| judge | Keynote | Numbers | Pages |
|---|---:|---:|---:|
| claude | 7.31 | 3.95 | 5.69 |
| deepseek | 8.69 | 4.63 | 7.72 |
| glm | 7.97 | 4.32 | 6.69 |
| qwen | 7.81 | 4.79 | 6.61 |
| qwen-low | 7.41 | 4.74 | 5.86 |
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

Agreement between the local judges:

| a | b | Spearman ρ | mean abs. difference | within 1 point |
|---|---|---:|---:|---:|
| glm | qwen | 0.94 | 0.63 | 82% |
| qwen | qwen-low | 0.91 | 0.79 | 77% |
| glm | qwen-low | 0.88 | 0.97 | 72% |
| deepseek | qwen | 0.69 | 1.44 | 72% |
| deepseek | glm | 0.68 | 1.68 | 61% |
| deepseek | pixel | 0.64 | 2.03 | 43% |
| glm | pixel | 0.50 | 1.97 | 48% |

Seconds per pair are wall-clock at the concurrency used (DeepSeek 2, GLM
1, Qwen 4, Claude 4); GLM's figure includes the slow period before its
server failed. The Claude run used about 470k input tokens.

What the numbers show:

- GLM and Qwen pass the controls and rank the pairs the same way Claude
  does (ρ 0.91 and 0.93). Both score about 0.75 points higher than
  Claude on average.
- Qwen with low thinking is the closest local judge to Claude: ρ 0.94,
  92% of pairs within one point, bias +0.28. With thinking off it ranks
  pairs equally well (ρ 0.93) but scores higher, and takes 10 seconds per
  pair instead of 28. Either is usable; use low thinking when the
  absolute score matters, thinking off when ranking is enough.
- DeepSeek V4 Flash with thinking off fails 15 of 22 misaligned controls
  and 6 of 28 identity controls, and scores 1.5 points above Claude. It
  scored 10 for a Pages cover whose rotated title bar the viewer places
  on the wrong edge of the page, and 8 for a Numbers sheet whose lower
  half is missing from the viewer screenshot. Its scores should not be
  used.
- All judges, including the pixel baseline, rank Numbers lowest and
  Keynote highest. Claude's means are 7.3 for Keynote, 5.7 for Pages, and
  4.0 for Numbers. Numbers is where the viewer's fidelity work is.

Two problems in the image pairs, found by reading the pairs the judges
disagreed on: Numbers' PDF export scales a whole sheet onto one page while
the viewer screenshot is the browser viewport, so long sheets are missing
rows on the viewer side (a `visual_diff.py` problem, not a viewer problem);
and `--max-pages 4` samples short documents more heavily than long ones.

Next: fix the two harvest problems above and re-run; then score more
documents, one or two pages each, with Qwen.
