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
different judge names.

## Usage

```
# local boxes, all three apps, 4 pages per document
python3 scripts/judge.py run \
  --runs-root vd --runs-root vdn --runs-root vdk \
  --judge pixel=pixel,pixel \
  --judge deepseek=http://192.168.87.91:4444/v1,deepseek-v4-flash \
  --judge glm=http://192.168.87.93:8888/v1,GLM-5.3-Flash-EXL3 \
  --controls --max-pages 4 --out judge-out
python3 scripts/judge.py report --out judge-out

# later, Claude as the reference judge
ANTHROPIC_API_KEY=... python3 scripts/judge.py run ... --judge claude=anthropic,claude-fable-5-1
```

Needs Pillow (`uv run --with pillow python3 scripts/judge.py …`). On macOS
the first LAN request triggers the Local Network privacy prompt for the
terminal app; grant it permanently under System Settings → Privacy &
Security → Local Network.
