#!/usr/bin/env python3
"""Conformance + reliability harness for pnk2json.

Runs the binary over every fixture in fixtures/success.tsv, records exit code,
wall time, stderr, and output size, then classifies failures and hunts for
super-linear timing (files whose time-per-byte is an outlier vs the median).

Usage:
  python3 scripts/conformance.py [--mode both] [--limit N] [--out PATH]

Exit codes of pnk2json: 0 = converted, 1 = rejected (encrypted/legacy/etc),
2 = usage error. Anything else (or timeout/panic text on stderr) is a defect.
"""
from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
BINARY = REPO / "target/release/pnk2json"
TSV = REPO / "fixtures/success.tsv"
CRAWL = REPO / "fixtures/crawl"
TIMEOUT_S = 60


def load_rows() -> list[dict]:
    rows = []
    lines = TSV.read_text().splitlines()
    header = lines[0].split("\t")
    for line in lines[1:]:
        parts = line.split("\t")
        row = dict(zip(header, parts))
        rows.append(row)
    return rows


def run_one(binary: str, path: Path, mode: str) -> dict:
    cmd = [binary, str(path)]
    if mode == "markdown":
        cmd.append("--markdown")
    t0 = time.monotonic()
    try:
        proc = subprocess.run(
            cmd, capture_output=True, text=True, timeout=TIMEOUT_S
        )
        wall = time.monotonic() - t0
        err = proc.stderr.strip()
        panic = "panic" in err or "RUST_BACKTRACE" in err
        return {
            "exit": proc.returncode,
            "wall_s": round(wall, 4),
            "stderr": err[:400],
            "panic": panic,
            "output_bytes": len(proc.stdout.encode("utf-8", "replace")),
            "timed_out": False,
        }
    except subprocess.TimeoutExpired:
        return {
            "exit": -1,
            "wall_s": TIMEOUT_S,
            "stderr": "TIMEOUT",
            "panic": False,
            "output_bytes": 0,
            "timed_out": True,
        }


def classify(row: dict, res: dict, mode: str) -> str:
    """Conformance verdict for one (fixture, mode)."""
    fmt = row["format"]
    if res["timed_out"]:
        return "TIMEOUT"
    if res["panic"]:
        return "PANIC"
    if fmt == "legacy-unknown":
        return "ok" if res["exit"] == 1 else "DEFECT:legacy-should-reject"
    # modern: keynote/pages/numbers
    if res["exit"] == 0:
        return "ok" if res["output_bytes"] > 0 else "DEFECT:empty-output"
    if res["exit"] == 1:
        err = res["stderr"].lower()
        if "password" in err or "encrypt" in err or "iwph" in err or "iwpv2" in err:
            return "ok:encrypted-reject"
        return "DEFECT:unexpected-reject"
    return f"DEFECT:exit{res['exit']}"


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--mode", choices=["json", "markdown", "both"], default="both")
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--out", default=str(REPO / "fixtures/conformance-report.json"))
    args = ap.parse_args()

    if not BINARY.exists():
        print(f"binary missing: {BINARY} — cargo build --release first", file=sys.stderr)
        return 2
    modes = ["json", "markdown"] if args.mode == "both" else [args.mode]
    rows = load_rows()
    if args.limit:
        rows = rows[: args.limit]

    records = []
    for i, row in enumerate(rows):
        sha, ext, fmt = row["sha256"], row["ext"], row["format"]
        crawl_dir = CRAWL if fmt != "legacy-unknown" else REPO / "fixtures/crawl_old"
        path = (REPO / "fixtures/crawl_old" / f"{sha}.unknown") if fmt == "legacy-unknown" else (CRAWL / f"{sha}.{ext}")
        if not path.exists():
            alt = (REPO / "fixtures/crawl_old" / f"{sha}.{ext}") if crawl_dir == CRAWL else (CRAWL / f"{sha}.{ext}")
            path = alt if alt.exists() else path
        if not path.exists():
            records.append({"local_id": row["local_id"], "format": fmt,
                            "mode": "skip", "verdict": "MISSING_FILE", "wall_s": 0})
            continue
        for mode in modes:
            res = run_one(str(BINARY), path, mode)
            verdict = classify(row, res, mode)
            records.append({
                "local_id": row["local_id"],
                "format": fmt,
                "mode": mode,
                "file_bytes": int(row["bytes"]),
                "verdict": verdict,
                **res,
            })
        if (i + 1) % 200 == 0:
            print(f"  … {i+1}/{len(rows)} rows", file=sys.stderr)

    # ---- aggregate ----
    defects = [r for r in records if r["verdict"].startswith(("DEFECT", "TIMEOUT", "PANIC", "MISSING"))]
    by_verdict: dict[str, int] = {}
    for r in records:
        by_verdict[r["verdict"]] = by_verdict.get(r["verdict"], 0) + 1

    timing = [r for r in records if r["mode"] == "json" and r["verdict"].startswith("ok")]
    walls = [r["wall_s"] for r in timing]
    sizes = [r["file_bytes"] for r in timing]
    if walls:
        med = statistics.median(walls)
        med_rate = statistics.median([b / max(w, 1e-6) for b, w in zip(sizes, walls)])
        outliers = sorted(
            (r for r in timing if r["wall_s"] > 4 * med and r["file_bytes"] > 0),
            key=lambda r: -r["wall_s"],
        )[:15]
        # Pearson r between wall time and input size (super-linear smoke signal)
        n = len(walls)
        mb, mt = statistics.mean(sizes), statistics.mean(walls)
        cov = sum((b - mb) * (t - mt) for b, t in zip(sizes, walls)) / n
        r_corr = cov / (statistics.pstdev(sizes) * statistics.pstdev(walls) or 1)
    else:
        med = med_rate = r_corr = 0
        outliers = []

    report = {
        "binary": str(BINARY),
        "rows": len(rows),
        "records": len(records),
        "verdict_counts": by_verdict,
        "timing": {
            "n": len(walls),
            "total_s": round(sum(walls), 2),
            "median_s": round(med, 4),
            "p95_s": round(sorted(walls)[int(0.95 * len(walls))], 4) if walls else 0,
            "max_s": round(max(walls), 4) if walls else 0,
            "median_bytes_per_s": round(med_rate),
            "time_vs_size_pearson_r": round(r_corr, 4),
        },
        "timing_outliers": [
            {"local_id": r["local_id"], "format": r["format"], "wall_s": r["wall_s"],
             "file_bytes": r["file_bytes"], "output_bytes": r["output_bytes"]}
            for r in outliers
        ],
        "defects": defects,
    }
    Path(args.out).write_text(json.dumps(report, indent=1))
    print(json.dumps({k: report[k] for k in ("rows", "records", "verdict_counts", "timing")}, indent=1))
    if outliers:
        print("\nSLOWEST / OUTLIERS:")
        for o in report["timing_outliers"]:
            print(f"  {o['local_id']} {o['format']:14} {o['wall_s']:7.3f}s in={o['file_bytes']:>10} out={o['output_bytes']:>9}")
    if defects:
        print(f"\nDEFECTS ({len(defects)}):")
        for d in defects[:20]:
            print(f"  {d['local_id']} {d['format']:14} {d['mode']:8} {d['verdict']} {d.get('stderr', '')[:120]}")
    else:
        print("\nNO DEFECTS.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
