#!/usr/bin/env python3
"""Render-fidelity judging: vision LLMs (or a pixel baseline) score our
renders against the apps' own PDF exports, page by page.

Pairs come from visual_diff.py output directories: `<run>/apple/page-N.png`
is the GOLDEN (the app's export rasterized) and `<run>/ours/{page,sheet,
slide}-N.png` the CANDIDATE (our viewer's frame screenshot). Every judge is
a named config — an OpenAI-compatible endpoint + model alias, the Anthropic
Messages API, or the built-in pixel baseline — and every (judge, prompt
version, golden, candidate) verdict is cached in judgments.jsonl, so adding
a judge later (or swapping the live model on an inference box and giving
it a new name) only runs the new one.

Controls calibrate the judges: an IDENTITY pair (golden vs golden, expect
10) and a MISALIGNED pair (golden page i vs candidate page j != i, expect 0)
per run tell you whether a judge respects the ends of the scale before you
trust its middle.

    # score every run under the visual_diff output roots with two judges
    python3 scripts/judge.py run --runs-root /path/to/vd /path/to/vdn \\
        --judge deepseek=http://192.168.87.91:4444/v1,deepseek-v4-flash-vision-exp \\
        --judge glm=http://192.168.87.93:8888/v1,glm-5.3-flash \\
        --judge pixel=pixel,pixel --controls --out judge-out

    # later: the Anthropic judge (ANTHROPIC_API_KEY in the environment)
    python3 scripts/judge.py run ... --judge claude=anthropic,claude-fable-5-1

    # the bake-off: agreement, control accuracy, per-app means
    python3 scripts/judge.py report --out judge-out

Harvest more pairs with visual_diff.py (`--batch success.tsv` or repeated
`--fixture`), pointing `--base-url` at a local viewer or https://pnk.vu —
the file is parsed in the browser either way.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import io
import json
import os
import random
import re
import sys
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PROMPT_PATH = REPO / "scripts" / "judge_prompt.md"
PROMPT_VERSION = "v1"

# Images are normalized to this height; widths follow the page's aspect.
# ~1100px keeps 9pt text legible for a vision model without blowing the
# request past what local servers accept.
IMAGE_HEIGHT = 1100
REASONING_EFFORT = "low"  # overridden by --effort
JPEG_QUALITY = 85


# ----------------------------------------------------------------- pairing

def find_pairs(run: Path):
    """(golden, candidate, page) triples for one visual_diff run dir."""
    apple = sorted((run / "apple").glob("page-*.png"), key=lambda p: int(p.stem.split("-")[-1]))
    ours_dir = run / "ours"
    kind = None
    for k in ("page", "sheet", "slide"):
        if list(ours_dir.glob(f"{k}-*.png")):
            kind = k
            break
    if kind is None or not apple:
        return kind, []
    ours = {int(p.stem.split("-")[-1]): p for p in ours_dir.glob(f"{kind}-*.png")}
    pairs = []
    for g in apple:
        n = int(g.stem.split("-")[-1])
        if n in ours:
            pairs.append((g, ours[n], n))
    return kind, pairs


def app_of(kind: str | None) -> str:
    return {"page": "pages", "sheet": "numbers", "slide": "keynote"}.get(kind or "", "unknown")


# ------------------------------------------------------------ normalization

def load_normalized(path: Path):
    """PIL image resized to IMAGE_HEIGHT (aspect kept), RGB."""
    from PIL import Image
    Image.MAX_IMAGE_PIXELS = None
    im = Image.open(path).convert("RGB")
    if im.height != IMAGE_HEIGHT:
        w = max(1, round(im.width * IMAGE_HEIGHT / im.height))
        im = im.resize((w, IMAGE_HEIGHT), Image.LANCZOS)
    return im


def jpeg_b64(im) -> tuple[str, str]:
    buf = io.BytesIO()
    im.save(buf, "JPEG", quality=JPEG_QUALITY, optimize=True)
    data = buf.getvalue()
    return base64.b64encode(data).decode(), hashlib.sha256(data).hexdigest()[:16]


# ------------------------------------------------------------------ judges

class Judge:
    """One scoring backend. `kind` selects the transport:
    openai  — OpenAI-compatible /chat/completions with image_url data URLs
    anthropic — Messages API with base64 image blocks
    pixel   — deterministic baseline: structural similarity mapped to 0-10
    """

    def __init__(self, name: str, kind: str, base_url: str, model: str, api_key: str | None, timeout: float):
        self.name, self.kind, self.base_url, self.model = name, kind, base_url.rstrip("/"), model
        self.api_key = api_key
        self.timeout = timeout

    @classmethod
    def parse(cls, spec: str, timeout: float) -> "Judge":
        # name=url,model[,api_key]   |   name=anthropic,model   |   name=pixel,pixel
        name, rest = spec.split("=", 1)
        parts = rest.split(",")
        target, model = parts[0], parts[1] if len(parts) > 1 else parts[0]
        key = parts[2] if len(parts) > 2 else None
        if target == "pixel":
            return cls(name, "pixel", "", "pixel", None, timeout)
        if target == "anthropic":
            key = key or os.environ.get("ANTHROPIC_API_KEY")
            if not key:
                sys.exit(f"judge {name}: ANTHROPIC_API_KEY not set")
            return cls(name, "anthropic", "https://api.anthropic.com", model, key, timeout)
        return cls(name, "openai", target, model, key or os.environ.get("OPENAI_API_KEY", "none"), timeout)

    # -- transports -------------------------------------------------------
    def score(self, prompt: str, golden_b64: str, cand_b64: str, golden_im, cand_im) -> dict:
        if self.kind == "pixel":
            return pixel_score(golden_im, cand_im)
        if self.kind == "anthropic":
            return self._anthropic(prompt, golden_b64, cand_b64)
        return self._openai(prompt, golden_b64, cand_b64)

    def _post(self, url: str, body: dict, headers: dict) -> dict:
        req = urllib.request.Request(url, data=json.dumps(body).encode(), method="POST",
                                     headers={"Content-Type": "application/json", **headers})
        with urllib.request.urlopen(req, timeout=self.timeout) as resp:
            return json.loads(resp.read())

    def _openai(self, prompt: str, golden_b64: str, cand_b64: str) -> dict:
        body = {
            "model": self.model,
            "temperature": 0,
            # Reasoning models (DeepSeek V4, GLM 5.3) think before answering and
            # the thinking counts against this budget: leave room, but also pin
            # the effort low so they cannot wander off for thousands of tokens.
            "max_tokens": 8000,
            **effort_params(REASONING_EFFORT),
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {"type": "text", "text": "GOLDEN (reference):"},
                    {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{golden_b64}"}},
                    {"type": "text", "text": "CANDIDATE (our render):"},
                    {"type": "image_url", "image_url": {"url": f"data:image/jpeg;base64,{cand_b64}"}},
                ],
            }],
        }
        out = self._post(f"{self.base_url}/chat/completions", body, {"Authorization": f"Bearer {self.api_key}"})
        choice = out["choices"][0]
        msg = choice["message"]
        text = msg.get("content")
        if isinstance(text, list):  # some servers return content parts
            text = "".join(p.get("text", "") for p in text if isinstance(p, dict))
        reasoning = msg.get("reasoning") or msg.get("reasoning_content") or ""
        if not text:
            # Budget exhausted mid-thought: parse whatever the model got to.
            text = reasoning
        usage = out.get("usage", {})
        return parse_verdict(text) | {
            "raw": (text or "")[:2000],
            "reasoning_chars": len(reasoning),
            "finish_reason": choice.get("finish_reason"),
            "usage": usage,
        }

    def _anthropic(self, prompt: str, golden_b64: str, cand_b64: str) -> dict:
        body = {
            "model": self.model,
            "max_tokens": 600,
            "temperature": 0,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {"type": "text", "text": "GOLDEN (reference):"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": golden_b64}},
                    {"type": "text", "text": "CANDIDATE (our render):"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": cand_b64}},
                ],
            }],
        }
        out = self._post(f"{self.base_url}/v1/messages", body,
                         {"x-api-key": self.api_key or "", "anthropic-version": "2023-06-01"})
        text = "".join(b.get("text", "") for b in out.get("content", []) if b.get("type") == "text")
        return parse_verdict(text) | {"raw": text[:2000], "usage": out.get("usage", {})}


def effort_params(effort: str) -> dict:
    """Request fields that steer thinking on OpenAI-compatible servers.
    Verified 2026-09-02 against vLLM serving deepseek-v4-flash: the model
    ignores reasoning_effort low/medium/high and thinks until max_tokens,
    but both reasoning_effort="none" and chat_template_kwargs.thinking=false
    switch thinking off. GLM-5.3 (EXL3 server) answered without thinking
    under effort low. "none" sends both switches."""
    if effort == "none":
        return {"reasoning_effort": "none", "chat_template_kwargs": {"thinking": False}}
    return {"reasoning_effort": effort}


def parse_verdict(text: str) -> dict:
    """First JSON object in the reply; a bare integer only when the reply is
    just that (a digit buried in a truncated chain of thought is not a verdict)."""
    m = re.search(r"\{.*\}", text, re.S)
    if m:
        try:
            v = json.loads(m.group(0))
            score = int(round(float(v.get("score"))))
            return {"score": max(0, min(10, score)), "alignment": v.get("alignment"), "content": v.get("content"),
                    "issues": v.get("issues", [])[:6] if isinstance(v.get("issues"), list) else [],
                    "summary": v.get("summary"), "parse": "json"}
        except (ValueError, TypeError, AttributeError):
            pass
    m = re.fullmatch(r"\s*(?:score\s*[:=]?\s*)?(10|[0-9])\s*/?\s*(?:10)?\s*\.?\s*", text or "", re.I)
    return {"score": int(m.group(1)) if m else None, "alignment": None, "content": None, "issues": [],
            "summary": None, "parse": "fallback" if m else "failed"}


def pixel_score(golden_im, cand_im) -> dict:
    """Baseline judge: mean structural similarity over 8x8 blocks of the
    grayscale images at a common size, mapped onto the rubric. Not a vision
    model — it anchors the bake-off (does an LLM beat an SSIM-ish number?)
    and gives the controls a deterministic answer."""
    from PIL import Image
    w, h = golden_im.size
    a = golden_im.convert("L").resize((w, h))
    b = cand_im.convert("L").resize((w, h), Image.LANCZOS)
    # coarse SSIM: compare 16x16 block means/variances (pure python, no numpy)
    def blocks(im):
        px = im.load()
        out = []
        for y in range(0, h - 15, 16):
            for x in range(0, w - 15, 16):
                vals = [px[x + i, y + j] for i in range(16) for j in range(16)]
                mu = sum(vals) / 256
                var = sum((v - mu) ** 2 for v in vals) / 256
                out.append((mu, var, vals))
        return out
    ba, bb = blocks(a), blocks(b)
    c1, c2 = (0.01 * 255) ** 2, (0.03 * 255) ** 2
    ssim = []
    for (ma, va, xa), (mb, vb, xb) in zip(ba, bb):
        cov = sum((p - ma) * (q - mb) for p, q in zip(xa, xb)) / 256
        ssim.append(((2 * ma * mb + c1) * (2 * cov + c2)) / ((ma * ma + mb * mb + c1) * (va + vb + c2)))
    s = sum(ssim) / max(1, len(ssim))
    # map: <0.2 -> 0 (different page), 0.2-0.5 -> 1-4, 0.5-0.9 -> 5-8, >0.97 -> 10
    if s < 0.2:
        score = 0
    elif s < 0.5:
        score = 1 + int((s - 0.2) / 0.3 * 3.99)
    elif s < 0.9:
        score = 5 + int((s - 0.5) / 0.4 * 3.99)
    elif s < 0.97:
        score = 9
    else:
        score = 10
    return {"score": score, "alignment": "different" if s < 0.2 else "same", "content": None,
            "issues": [f"ssim={s:.3f}"], "summary": f"block SSIM {s:.3f}", "parse": "pixel", "ssim": s}


# -------------------------------------------------------------------- run

def load_done(path: Path) -> set[tuple]:
    done = set()
    if path.exists():
        for line in path.read_text().splitlines():
            try:
                r = json.loads(line)
                done.add((r["judge"], r["model"], r["prompt_version"], r["golden_sha"], r["candidate_sha"]))
            except (ValueError, KeyError):
                continue
    return done


def cmd_run(args) -> int:
    global REASONING_EFFORT
    REASONING_EFFORT = args.effort
    prompt = PROMPT_PATH.read_text()
    judges = [Judge.parse(s, args.timeout) for s in args.judge]
    runs: list[Path] = []
    for root in args.runs_root:
        runs += sorted(p for p in Path(root).iterdir() if (p / "apple").is_dir())
    runs += [Path(r) for r in args.run]
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    log_path = out / "judgments.jsonl"
    done = load_done(log_path)
    rng = random.Random(args.seed)

    # build the work list: real pairs + controls
    tasks = []
    for run in runs:
        kind, pairs = find_pairs(run)
        if not pairs:
            continue
        if args.max_pages:
            pairs = pairs[: args.max_pages]
        for g, c, n in pairs:
            tasks.append({"doc": run.name, "app": app_of(kind), "page": n, "golden": g, "candidate": c, "control": None})
        if args.controls and len(pairs) >= 1:
            g, c, n = pairs[0]
            tasks.append({"doc": run.name, "app": app_of(kind), "page": n, "golden": g, "candidate": g, "control": "identity"})
            if len(pairs) >= 2:
                other = rng.choice([p for p in pairs if p[2] != n])
                tasks.append({"doc": run.name, "app": app_of(kind), "page": n, "golden": g, "candidate": other[1],
                              "control": "misaligned", "candidate_page": other[2]})
    print(f"[judge] {len(runs)} runs, {len(tasks)} pairs (controls {'on' if args.controls else 'off'}), "
          f"{len(judges)} judges: {', '.join(j.name for j in judges)}", flush=True)

    lock = threading.Lock()
    cache: dict[Path, tuple] = {}

    def prepared(path: Path):
        with lock:
            if path in cache:
                return cache[path]
        im = load_normalized(path)
        b64, sha = jpeg_b64(im)
        with lock:
            cache[path] = (im, b64, sha)
        return cache[path]

    def work(judge: Judge, t: dict):
        gim, gb64, gsha = prepared(t["golden"])
        cim, cb64, csha = prepared(t["candidate"])
        key = (judge.name, judge.model, PROMPT_VERSION, gsha, csha)
        if key in done:
            return None
        t0 = time.time()
        verdict, err = None, None
        for attempt in range(3):
            try:
                verdict = judge.score(prompt, gb64, cb64, gim, cim)
                break
            except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError, KeyError, ValueError) as e:
                err = f"{type(e).__name__}: {e}"[:300]
                time.sleep(2 * (attempt + 1))
        rec = {
            "judge": judge.name, "model": judge.model, "prompt_version": PROMPT_VERSION,
            "effort": REASONING_EFFORT if judge.kind == "openai" else None,
            "doc": t["doc"], "app": t["app"], "page": t["page"], "control": t["control"],
            "candidate_page": t.get("candidate_page", t["page"]),
            "golden": str(t["golden"]), "candidate": str(t["candidate"]),
            "golden_sha": gsha, "candidate_sha": csha,
            "seconds": round(time.time() - t0, 2), "ts": time.strftime("%Y-%m-%dT%H:%M:%S"),
        }
        rec.update(verdict or {"score": None, "parse": "error", "error": err})
        with lock:
            with log_path.open("a") as f:
                f.write(json.dumps(rec) + "\n")
            done.add(key)
        tag = t["control"] or "pair"
        print(f"  {judge.name:10s} {t['doc'][:12]} {t['app']:7s} p{t['page']:<3d} {tag:10s} -> {rec.get('score')} "
              f"({rec['seconds']}s){' ERR ' + err if err and not verdict else ''}", flush=True)
        return rec

    for judge in judges:
        with ThreadPoolExecutor(max_workers=args.concurrency if judge.kind != "pixel" else 4) as ex:
            list(ex.map(lambda t: work(judge, t), tasks))
    print(f"[judge] judgments in {log_path}")
    return 0


# ----------------------------------------------------------------- report

def spearman(xs: list[float], ys: list[float]) -> float | None:
    n = len(xs)
    if n < 3:
        return None
    def ranks(v):
        order = sorted(range(n), key=lambda i: v[i])
        r = [0.0] * n
        i = 0
        while i < n:
            j = i
            while j + 1 < n and v[order[j + 1]] == v[order[i]]:
                j += 1
            for k in range(i, j + 1):
                r[order[k]] = (i + j) / 2 + 1
            i = j + 1
        return r
    rx, ry = ranks(xs), ranks(ys)
    mx, my = sum(rx) / n, sum(ry) / n
    num = sum((a - mx) * (b - my) for a, b in zip(rx, ry))
    den = (sum((a - mx) ** 2 for a in rx) * sum((b - my) ** 2 for b in ry)) ** 0.5
    return num / den if den else None


def cmd_report(args) -> int:
    log_path = Path(args.out) / "judgments.jsonl"
    recs = [json.loads(l) for l in log_path.read_text().splitlines() if l.strip()]
    recs = [r for r in recs if r.get("prompt_version") == PROMPT_VERSION]
    judges = sorted({r["judge"] for r in recs})
    lines = [f"# Render-fidelity bake-off — prompt {PROMPT_VERSION}", ""]
    # per-judge summary
    lines += ["| judge | model | pairs | mean | median | identity ctrl (expect 10) | misaligned ctrl (expect 0) | parse failures | s/pair |",
              "|---|---|---:|---:|---:|---:|---:|---:|---:|"]
    by_judge = {}
    for j in judges:
        rs = [r for r in recs if r["judge"] == j]
        pairs = [r for r in rs if r["control"] is None and r.get("score") is not None]
        ident = [r["score"] for r in rs if r["control"] == "identity" and r.get("score") is not None]
        mis = [r["score"] for r in rs if r["control"] == "misaligned" and r.get("score") is not None]
        fails = sum(1 for r in rs if r.get("score") is None or r.get("parse") in ("failed", "error"))
        scores = sorted(r["score"] for r in pairs)
        mean = sum(scores) / len(scores) if scores else float("nan")
        med = scores[len(scores) // 2] if scores else float("nan")
        secs = sum(r["seconds"] for r in rs) / len(rs) if rs else 0
        ok_id = f"{sum(1 for s in ident if s >= 9)}/{len(ident)}" if ident else "-"
        ok_mis = f"{sum(1 for s in mis if s <= 1)}/{len(mis)}" if mis else "-"
        model = rs[0]["model"] if rs else ""
        lines.append(f"| {j} | {model} | {len(pairs)} | {mean:.2f} | {med} | {ok_id} | {ok_mis} | {fails} | {secs:.1f} |")
        by_judge[j] = {(r["doc"], r["page"], r["control"]): r["score"] for r in pairs}
    # per-app means
    apps = sorted({r["app"] for r in recs})
    lines += ["", "## Mean score by app", "", "| judge | " + " | ".join(apps) + " |", "|---|" + "---:|" * len(apps)]
    for j in judges:
        row = []
        for a in apps:
            s = [r["score"] for r in recs if r["judge"] == j and r["app"] == a and r["control"] is None and r.get("score") is not None]
            row.append(f"{sum(s) / len(s):.2f} (n={len(s)})" if s else "-")
        lines.append(f"| {j} | " + " | ".join(row) + " |")
    # agreement
    lines += ["", "## Agreement between judges (real pairs both scored)", "",
              "| a | b | n | Spearman ρ | mean abs diff | within 1 |", "|---|---|---:|---:|---:|---:|"]
    for i, a in enumerate(judges):
        for b in judges[i + 1:]:
            keys = sorted(set(by_judge[a]) & set(by_judge[b]))
            if not keys:
                continue
            xs = [by_judge[a][k] for k in keys]
            ys = [by_judge[b][k] for k in keys]
            rho = spearman(xs, ys)
            mad = sum(abs(x - y) for x, y in zip(xs, ys)) / len(keys)
            w1 = sum(1 for x, y in zip(xs, ys) if abs(x - y) <= 1) / len(keys)
            lines.append(f"| {a} | {b} | {len(keys)} | {rho if rho is None else f'{rho:.2f}'} | {mad:.2f} | {w1:.0%} |")
    # biggest disagreements — the pages worth a human look
    if len(judges) >= 2:
        lines += ["", "## Largest disagreements", "", "| doc | page | " + " | ".join(judges) + " |", "|---|---:|" + "---:|" * len(judges)]
        keys = set()
        for j in judges:
            keys |= set(by_judge[j])
        rows = []
        for k in keys:
            vals = [by_judge[j].get(k) for j in judges]
            present = [v for v in vals if v is not None]
            if len(present) >= 2:
                rows.append((max(present) - min(present), k, vals))
        rows.sort(reverse=True)
        for spread, (doc, page, _), vals in rows[:15]:
            lines.append(f"| {doc[:14]} | {page} | " + " | ".join("-" if v is None else str(v) for v in vals) + " |")
    # per-doc table
    lines += ["", "## Per document (mean over pages)", "", "| doc | app | pages | " + " | ".join(judges) + " |", "|---|---|---:|" + "---:|" * len(judges)]
    docs = sorted({(r["doc"], r["app"]) for r in recs if r["control"] is None})
    for doc, app in docs:
        row = []
        n = 0
        for j in judges:
            s = [r["score"] for r in recs if r["judge"] == j and r["doc"] == doc and r["control"] is None and r.get("score") is not None]
            n = max(n, len(s))
            row.append(f"{sum(s) / len(s):.1f}" if s else "-")
        lines.append(f"| {doc[:14]} | {app} | {n} | " + " | ".join(row) + " |")
    text = "\n".join(lines) + "\n"
    (Path(args.out) / "report.md").write_text(text)
    print(text)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0], formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="score pairs with one or more judges")
    r.add_argument("--runs-root", action="append", default=[], help="directory of visual_diff run dirs (repeatable)")
    r.add_argument("--run", action="append", default=[], help="one visual_diff run dir (repeatable)")
    r.add_argument("--judge", action="append", required=True,
                   help="name=<openai-compatible /v1 url>,<model>[,api_key] | name=anthropic,<model> | name=pixel,pixel")
    r.add_argument("--out", required=True, help="output dir (judgments.jsonl is appended and used as a cache)")
    r.add_argument("--controls", action="store_true", help="add identity and misaligned control pairs per run")
    r.add_argument("--max-pages", type=int, default=0, help="cap pages per run (0 = all)")
    r.add_argument("--concurrency", type=int, default=2, help="parallel requests per LLM judge")
    r.add_argument("--timeout", type=float, default=240.0)
    r.add_argument("--seed", type=int, default=1)
    r.add_argument("--effort", default="none", choices=["none", "low", "medium", "high"],
                   help="thinking budget for OpenAI-compatible judges (default none = thinking off; "
                        "use a distinct judge name per effort, the cache is keyed by name)")
    r.set_defaults(fn=cmd_run)
    p = sub.add_parser("report", help="summarize judgments.jsonl into report.md")
    p.add_argument("--out", required=True)
    p.set_defaults(fn=cmd_report)
    args = ap.parse_args()
    return args.fn(args)


if __name__ == "__main__":
    sys.exit(main())
