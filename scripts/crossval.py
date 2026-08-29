#!/usr/bin/env python3
"""Cross-validate pnk2json output against Apple's own renders.

Ground truth: each iWork file embeds a QuickLook preview (preview.pdf or
preview.jpg/png) rendered by Apple's own importer. We rasterize/collect it,
extract PDF text where available, and compare against pnk2json's JSON + markdown:

  JSON side   — table census (dims, non-null cells, format pool, merges) to answer
                "is that table empty?" — an all-null grid while the preview shows
                content is a converter bug (see 2026-08-28 DataList fix).
  Viewer side — per-app Playwright gate screenshots (npm test in viewer/) vs the
                preview bitmaps; human-eyeball equivalence, goal is good not perfect.

Usage:
  python3 scripts/crossval.py [--fixtures id1,id2,...] [--out /tmp/crossval] [--scan]

Needs: target/release/pnk2json; for preview.pdf rasterization/text:
  uv run --with pyobjc-framework-Quartz --with pyobjc-framework-PDFKit python3 ...
"""
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PNK2JSON = REPO / "target/release/pnk2json"


def load_success() -> dict[str, dict]:
    rows = {}
    tsv = (REPO / "fixtures/success.tsv").read_text().splitlines()
    hdr = tsv[0].split("\t")
    for line in tsv[1:]:
        r = dict(zip(hdr, line.split("\t")))
        if r.get("format") in ("keynote", "pages", "numbers") and int(r.get("bytes", 0)) > 0:
            rows[r["local_id"]] = r
    return rows


def default_picks(success: dict) -> list[str]:
    """2 smallest + 1 median-size per format — small, fast, representative."""
    import statistics

    picks = []
    for fmt in ("keynote", "pages", "numbers"):
        grp = sorted((r for r in success.values() if r["format"] == fmt),
                     key=lambda r: int(r["bytes"]))
        med = statistics.median(int(r["bytes"]) for r in grp)
        picks += [r["local_id"] for r in grp[:2]]
        picks.append(min(grp, key=lambda r: abs(int(r["bytes"]) - med))["local_id"])
    return picks


def extract_preview(src: Path, out: Path) -> str:
    """Extract embedded QuickLook preview. Returns kind: pdf|jpg|png|none."""
    with zipfile.ZipFile(src) as z:
        names = z.namelist()
        for name, kind in (("preview.pdf", "pdf"), ("preview.jpg", "jpg"), ("preview.png", "png")):
            if name in names:
                (out / f"preview.{kind}").write_bytes(z.read(name))
                return kind
    return "none"


def rasterize_pdf(pdf: Path, out: Path, max_pages: int = 3, dpi: int = 110) -> list[Path]:
    """Render PDF pages to PNGs via CoreGraphics (pyobjc). Returns page paths."""
    import Quartz  # type: ignore

    url = Quartz.CFURLCreateFromFileSystemRepresentation(None, str(pdf).encode(), len(str(pdf).encode()), False)
    doc = Quartz.CGPDFDocumentCreateWithURL(url)
    n = min(Quartz.CGPDFDocumentGetNumberOfPages(doc), max_pages)
    scale = dpi / 72.0
    written = []
    for i in range(1, n + 1):
        page = Quartz.CGPDFDocumentGetPage(doc, i)
        rect = Quartz.CGPDFPageGetBoxRect(page, Quartz.kCGPDFMediaBox)
        w, h = int(rect.size.width * scale), int(rect.size.height * scale)
        cs = Quartz.CGColorSpaceCreateDeviceRGB()
        ctx = Quartz.CGBitmapContextCreate(None, w, h, 8, 0, cs, Quartz.kCGImageAlphaPremultipliedLast)
        Quartz.CGContextSetRGBFillColor(ctx, 1, 1, 1, 1)
        Quartz.CGContextFillRect(ctx, Quartz.CGRectMake(0, 0, w, h))
        Quartz.CGContextScaleCTM(ctx, scale, scale)
        Quartz.CGContextDrawPDFPage(ctx, page)
        img = Quartz.CGBitmapContextCreateImage(ctx)
        dest_path = out / f"page-{i}.png"
        dest_url = Quartz.CFURLCreateFromFileSystemRepresentation(None, str(dest_path).encode(), len(str(dest_path).encode()), False)
        dest = Quartz.CGImageDestinationCreateWithURL(dest_url, "public.png", 1, None)
        Quartz.CGImageDestinationAddImage(dest, img, None)
        Quartz.CGImageDestinationFinalize(dest)
        written.append(dest_path)
    return written


def pdf_text(pdf: Path) -> str:
    try:
        from pypdf import PdfReader  # type: ignore
    except ImportError:
        return ""
    try:
        return "\n".join((page.extract_text() or "") for page in PdfReader(str(pdf)).pages)
    except Exception:
        return ""


TOKEN = re.compile(r"[A-Za-z0-9À-ž][A-Za-z0-9À-ž'’.-]{2,}")


def tokens(s: str) -> set[str]:
    return {t.casefold() for t in TOKEN.findall(s)}


def table_census(doc: dict) -> list[dict]:
    census = []
    for sh in doc.get("sheets", []):
        for d in sh.get("drawables", []):
            tb = d.get("table")
            if not isinstance(tb, dict):
                continue
            nonnull = sum(1 for row in tb["grid"] for c in row if c is not None)
            census.append({
                "sheet": sh.get("name"),
                "table": tb.get("name"),
                "dims": f'{tb["rowCount"]}x{tb["columnCount"]}',
                "nonnull": nonnull,
                "fill_pct": round(100 * nonnull / max(1, tb["rowCount"] * tb["columnCount"]), 1),
                "formats": len(tb.get("formats", [])),
                "merges": len(tb.get("merges", [])),
            })
    return census


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--fixtures", help="comma-separated local_ids (default: 2 smallest + median per format)")
    ap.add_argument("--out", default="/tmp/crossval")
    ap.add_argument("--scan", action="store_true", help="corpus-wide table census only (no previews)")
    args = ap.parse_args()

    success = load_success()
    ids = args.fixtures.split(",") if args.fixtures else default_picks(success)
    outdir = Path(args.out)
    outdir.mkdir(parents=True, exist_ok=True)

    if args.scan:
        census_all = []
        for lid, r in success.items():
            src = REPO / f"fixtures/crawl/{r['sha256']}.{r['ext']}"
            p = subprocess.run([str(PNK2JSON), str(src)], capture_output=True, text=True, timeout=120)
            if p.returncode != 0 or not p.stdout.strip():
                continue
            doc = json.loads(p.stdout)
            for c in table_census(doc):
                c["local_id"] = lid
                census_all.append(c)
        empties = [c for c in census_all if c["nonnull"] == 0]
        print(f"scan: {len(census_all)} tables | all-null: {len(empties)}")
        for c in empties[:40]:
            print("  EMPTY", c["local_id"], c["table"], c["dims"])
        (outdir / "scan-census.json").write_text(json.dumps(census_all, indent=1))
        return 0

    print(f"{'fixture':44} {'fmt':8} {'preview':7} {'tables':6} verdict")
    failures = 0
    for lid in ids:
        r = success.get(lid)
        if not r:
            print(f"{lid:44} MISSING from success.tsv")
            failures += 1
            continue
        work = outdir / lid
        work.mkdir(parents=True, exist_ok=True)
        src = REPO / f"fixtures/crawl/{r['sha256']}.{r['ext']}"

        kind = extract_preview(src, work)
        pages: list[Path] = []
        text = ""
        if kind == "pdf":
            pages = rasterize_pdf(work / "preview.pdf", work)
            text = pdf_text(work / "preview.pdf")
        md = subprocess.run([str(PNK2JSON), str(src), "--markdown"], capture_output=True, text=True, timeout=120)
        js = subprocess.run([str(PNK2JSON), str(src)], capture_output=True, text=True, timeout=120)
        if js.returncode != 0:
            print(f"{lid:44} {r['format']:8} {kind:7} CONVERT-FAIL")
            failures += 1
            continue
        (work / "out.json").write_text(js.stdout)
        (work / "out.md").write_text(md.stdout)
        doc = json.loads(js.stdout)
        census = table_census(doc)

        verdicts = []
        if text:
            pt, mt = tokens(text), tokens(md.stdout)
            missing = pt - mt
            if len(missing) > max(6, 0.10 * len(pt)):
                verdicts.append(f"TEXT-MISSING {len(missing)} tokens e.g. {sorted(missing)[:6]}")
        allnull = [c for c in census if c["nonnull"] == 0]
        if allnull:
            verdicts.append(f"EMPTY-GRID x{len(allnull)} {[c['table'] for c in allnull][:3]}")
        verdict = "; ".join(verdicts) if verdicts else "ok"
        if verdicts:
            failures += 1
        print(f"{lid:44} {r['format']:8} {kind:7} {len(census):<6} {verdict}")

    print(f"\nartifacts: {outdir}/  | failures: {failures}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
