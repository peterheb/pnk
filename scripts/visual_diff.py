#!/usr/bin/env python3
"""Apple ground-truth vs our viewer render — visual comparison harness.

Takes one .pages/.numbers/.key fixture and produces, under an output dir:

  apple/           per-page PNGs rasterized from the iWork app's PDF export (~150dpi)
  ours/            our viewer render screenshot + element bounding boxes + model JSON
  composites/      per-Apple-page side-by-side (Apple | ours) composites
  crops/           zoomed side-by-side crops of regions of interest
  summary.md       pages compared, regions cropped, diff heuristics

Safety: the Apple side works on a COPY of the fixture opened under a distinct
document name (suffix "-visualdiff-copy"). The script only ever exports/closes
the document whose name matches that copy's name; if the copy's name never
appears among the app's open documents, the Apple side aborts and we fall back
to the fixture's embedded QuickLook preview (page 1 only).

Usage:
    /path/to/venv/bin/python scripts/visual_diff.py \
        --fixture fixtures/golden/G5-golden-pages-acid.pages --out /tmp/g5-visual
    # other apps:
    /path/to/venv/bin/python scripts/visual_diff.py --app numbers --fixture ... --out ...

Requires: pillow, pyobjc-framework-Quartz (PDF rasterization), pymupdf
(Apple-side text anchors), plus the repo's viewer (node + @playwright/test in
viewer/node_modules) and target/release/pnk2json.
"""
from __future__ import annotations
import csv
import argparse
import json
import os
import shutil
import subprocess
import sys
import time
import urllib.request
import zipfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
PNK2JSON = REPO / "target/release/pnk2json"
VIEWER = REPO / "viewer"

# ---------------------------------------------------------------- Apple side

APPLE_DOC_SUFFIX = "-visualdiff-copy"
APP_NAMES = {"pages": "Pages", "numbers": "Numbers", "keynote": "Keynote"}
OPEN_TIMEOUT_S = 90
EXPORT_TIMEOUT_S = 120


def _osascript(script: str, timeout: float = 60) -> str:
    return subprocess.run(
        ["osascript", "-e", script], capture_output=True, text=True,
        timeout=timeout, check=True,
    ).stdout.strip()


def _doc_names(app_name: str) -> list[str]:
    out = _osascript(
        f'tell application "{app_name}" to if it is running then get name of every document',
        timeout=OPEN_TIMEOUT_S,
    )
    return [s for s in out.split(", ") if s] if out else []


def export_via_app(app_name: str, fixture: Path, work: Path, log) -> tuple[Path | None, str]:
    """Open a renamed COPY in the iWork app, export PDF, close the copy (no save).

    Returns (pdf_path | None, mode) where mode is "<app>-export" or
    "fallback-preview" (with the PDF being None in fallback mode).
    """
    copy_stem = fixture.stem + APPLE_DOC_SUFFIX
    copy_path = work / "apple-work" / (copy_stem + fixture.suffix)
    copy_path.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(fixture, copy_path)
    pdf_path = work / "apple" / "export.pdf"
    pdf_path.parent.mkdir(parents=True, exist_ok=True)

    # Apps are keyed by document name; they report either the bare stem or the
    # full filename depending on open state, so accept both spellings.
    candidates = {copy_path.name, copy_stem}

    def find_copy() -> str | None:
        names = _doc_names(app_name)
        for c in candidates:
            if c in names:
                return c
        return None

    existing = find_copy()
    if existing:
        log(f"copy {existing!r} is already open in {app_name}; exporting it")
    else:
        names = _doc_names(app_name)
        if any(c in names for c in (fixture.name, fixture.stem)):
            # informational only: the copy is distinguishable by its own name
            log(f"note: user's original {fixture.name!r} is also open in {app_name}")
        log(f"opening copy {copy_path} in {app_name} (docs open: {names})")
        # -g: open without bringing the app to the foreground — exports are
        # driven by Apple events and need no focus, and stealing the user's
        # focus mid-keystroke was the harness's worst side effect.
        subprocess.run(["open", "-g", "-a", app_name, str(copy_path)], check=True, timeout=30)
        deadline = time.time() + OPEN_TIMEOUT_S
        while time.time() < deadline:
            existing = find_copy()
            if existing:
                break
            time.sleep(2)
        else:
            log(f"copy never appeared in {app_name} within {OPEN_TIMEOUT_S}s; aborting Apple side")
            return None, "fallback-preview"

    # Guard: re-verify by name immediately before export/close. We target the
    # copy document BY ITS NAME only — the user's original is never addressed.
    try:
        _osascript(
            f'tell application "{app_name}" to export document "{existing}" '
            f'to POSIX file "{pdf_path}" as PDF',
            timeout=EXPORT_TIMEOUT_S,
        )
    except Exception as e:
        log(f"export failed: {e}")
        _close_doc(app_name, existing, log)
        return None, "fallback-preview"

    if not pdf_path.exists() or pdf_path.stat().st_size == 0:
        log("export produced no PDF")
        _close_doc(app_name, existing, log)
        return None, "fallback-preview"

    _close_doc(app_name, existing, log)
    return pdf_path, f"{app_name.lower()}-export"


def _close_doc(app_name: str, doc_name: str, log) -> None:
    """Close ONLY the document whose name matches exactly. Never saves."""
    try:
        names = _doc_names(app_name)
        if doc_name not in names:
            log(f"close: {doc_name!r} not open anymore (open: {names})")
            return
        _osascript(
            f'tell application "{app_name}"\n'
            '  repeat with d in documents\n'
            f'    if name of d is "{doc_name}" then\n'
            "      close d saving no\n"
            "      exit repeat\n"
            "    end if\n"
            "  end repeat\n"
            "end tell",
            timeout=OPEN_TIMEOUT_S,
        )
        log(f"closed {app_name} copy {doc_name!r} without saving")
    except Exception as e:
        log(f"WARN: could not close copy {doc_name!r}: {e} "
            "(user's documents were never addressed)")


def extract_preview(fixture: Path, out_dir: Path) -> Path | None:
    """Fixture's embedded QuickLook preview (fallback: page 1 only)."""
    out_dir.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(fixture) as z:
        names = z.namelist()
        for name, ext in (("preview.pdf", "pdf"), ("preview.jpg", "jpg"), ("preview.png", "png")):
            if name in names:
                p = out_dir / f"preview.{ext}"
                p.write_bytes(z.read(name))
                return p
    return None


# ------------------------------------------------------------- rasterization

def rasterize_pdf(pdf: Path, out_dir: Path, dpi: int = 150, max_pages: int = 100) -> list[Path]:
    """PDF pages -> PNGs via CoreGraphics (pyobjc), ported from crossval.py."""
    import Quartz  # type: ignore

    out_dir.mkdir(parents=True, exist_ok=True)
    url = Quartz.CFURLCreateFromFileSystemRepresentation(
        None, str(pdf).encode(), len(str(pdf).encode()), False)
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
        dest_path = out_dir / f"page-{i}.png"
        dest_url = Quartz.CFURLCreateFromFileSystemRepresentation(
            None, str(dest_path).encode(), len(str(dest_path).encode()), False)
        dest = Quartz.CGImageDestinationCreateWithURL(dest_url, "public.png", 1, None)
        Quartz.CGImageDestinationAddImage(dest, img, None)
        Quartz.CGImageDestinationFinalize(dest)
        written.append(dest_path)
    return written


# ------------------------------------------------------------- our render

RENDER_JS = r"""
const fs = require("fs");
const { chromium } = require(process.env.PW_MODULE);

(async () => {
  const [fixture, shotPath, bboxPath, baseURL] = process.argv.slice(2);
  const browser = await chromium.launch();
  const page = await browser.newPage({
    viewport: { width: 1280, height: 900 },
    deviceScaleFactor: 2,
  });
  await page.goto(baseURL + "/");
  await page.setInputFiles("#file-input", fixture);
  await page.waitForSelector("#pages-view, #numbers-view, #keynote-view", { timeout: 30000 });
  // tables/images settle after first paint
  await page.waitForTimeout(1500);

  // Deck mode (.key): the viewer shows all slides in one continuous scroll,
  // lazily rendered. Click each thumbnail (forces render + scrolls there) and
  // screenshot that slide's CANVAS FRAME only, one PNG per slide, so
  // composites align 1:1 with Apple's per-page rasters (no caption/notes).
  let slideCount = 0;
  const items = page.locator(".slide-list-item");
  if ((await items.count()) > 0) {
    slideCount = await items.count();
    const shotDir = shotPath.replace(/\/[^/]+$/, "/");
    fs.mkdirSync(shotDir, { recursive: true });
    for (let i = 0; i < slideCount; i++) {
      await items.nth(i).click();
      const frame = page.locator(`.slide-stage[data-slide-index="${i}"] .canvas-frame`).first();
      await frame.waitFor({ state: "visible", timeout: 10000 });
      await page.waitForTimeout(250);
      await frame.screenshot({ path: `${shotDir}slide-${i + 1}.png` });
    }
  }

  // Pages mode: word-processing pagination and layout canvases both render
  // as .pages-page frames. Screenshot each frame so composites pair 1:1
  // with Apple's per-page rasters (the proportional slice of one tall
  // screenshot drifts by a fraction of a page per page).
  const pageFrames = page.locator("#pages-view .pages-page");
  const pageCount = await pageFrames.count();
  if (pageCount > 0) {
    const shotDir = shotPath.replace(/\/[^/]+$/, "/");
    fs.mkdirSync(shotDir, { recursive: true });
    for (let i = 0; i < pageCount; i++) {
      const frame = pageFrames.nth(i);
      await frame.scrollIntoViewIfNeeded();
      await page.waitForTimeout(100);
      await frame.screenshot({ path: `${shotDir}page-${i + 1}.png` });
    }
  }

  // the sticky top bar would print over anything scrolled beneath it
  // (CSSOM, not a style tag: the viewer's CSP allows no inline styles)
  await page.evaluate(() => {
    document.querySelectorAll("#topnav, header").forEach((h) => { h.style.visibility = "hidden"; });
  });

  // Numbers mode: sheets sit behind tabs, one visible at a time. Activate
  // each tab and screenshot its canvas as sheet-N.png; composites pair
  // them with Apple's pages in order (Numbers exports one sheet per page
  // when the sheet fits, so the pairing holds for typical documents).
  const sheetTabs = page.locator("#numbers-view .sheet-tab");
  const sheetCount = await sheetTabs.count();
  if (sheetCount > 0) {
    const shotDir = shotPath.replace(/\/[^/]+$/, "/");
    fs.mkdirSync(shotDir, { recursive: true });
    for (let i = 0; i < sheetCount; i++) {
      await sheetTabs.nth(i).click();
      const canvas = page.locator(`#numbers-view .sheet-area[data-sheet-index="${i}"] .sheet-canvas`).first();
      await canvas.waitFor({ state: "visible", timeout: 10000 });
      await page.waitForTimeout(400);
      await canvas.screenshot({ path: `${shotDir}sheet-${i + 1}.png` });
    }
    await sheetTabs.nth(0).click();
    await page.waitForTimeout(300);
  }

  await page.screenshot({ path: shotPath, fullPage: true });

  const bboxes = await page.evaluate(() => {
    const grab = (el) => {
      const r = el.getBoundingClientRect();
      return { x: r.x, y: r.y + window.scrollY, w: r.width, h: r.height };
    };
    const out = { tables: [], imageParagraph: null, view: null, innerWidth: window.innerWidth };
    const view = document.querySelector("#pages-view, #numbers-view, #keynote-view");
    if (view) out.view = grab(view);
    document.querySelectorAll("table.sheet-table").forEach((t) => {
      const cap = t.querySelector("caption.table-caption");
      const cells = [];
      t.querySelectorAll("tr").forEach((tr, r) => {
        Array.from(tr.cells).forEach((td, c) => {
          cells.push({ r, c, text: (td.textContent || "").trim(), ...grab(td) });
        });
      });
      out.tables.push({ caption: cap ? cap.textContent : "", ...grab(t), cells });
    });
    for (const p of document.querySelectorAll("#pages-view p")) {
      if (/Here is a small inline image/.test(p.textContent || "")) {
        out.imageParagraph = grab(p);
        break;
      }
    }
    return out;
  });
  bboxes.slideCount = slideCount;
  fs.writeFileSync(bboxPath, JSON.stringify(bboxes, null, 2));
  await browser.close();
})().catch((e) => { console.error(e); process.exit(1); });
"""


def ensure_viewer_server(base_url: str, log) -> subprocess.Popen | None:
    """Reuse a running esbuild viewer server or spawn one; returns handle or None.

    esbuild's serve mode stops when stdin hits EOF, so the child's stdin must
    stay open (a pipe we simply never close).
    """
    host, port = base_url.replace("http://", "").split(":")
    try:
        urllib.request.urlopen(f"{base_url}/", timeout=2)
        log(f"reusing viewer server already at {base_url}")
        return None
    except Exception:
        pass
    proc = subprocess.Popen(
        [str(VIEWER / "node_modules/.bin/esbuild"), "--servedir=dist",
         f"--serve={host}:{port}", "--log-level=warning"],
        cwd=VIEWER, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT,
    )
    deadline = time.time() + 30
    while time.time() < deadline:
        try:
            urllib.request.urlopen(f"{base_url}/", timeout=2)
            log(f"viewer server started at {base_url} (pid {proc.pid})")
            return proc
        except Exception:
            time.sleep(0.5)
    proc.terminate()
    return None


def render_ours(fixture: Path, work: Path, base_url: str, log) -> tuple[Path, dict] | None:
    """Convert + load the fixture in the served viewer; screenshot + bboxes."""
    json_out = work / "ours" / "model.json"
    json_out.parent.mkdir(parents=True, exist_ok=True)
    conv = subprocess.run([str(PNK2JSON), str(fixture)], capture_output=True, text=True)
    if conv.returncode != 0 or not conv.stdout.strip():
        log(f"pnk2json failed: {conv.stderr.strip()[:400]}")
        return None
    json_out.write_text(conv.stdout)

    shot = work / "ours" / "render.png"
    bbox_path = work / "ours" / "bboxes.json"
    js = work / "render.js"
    js.write_text(RENDER_JS)
    r = subprocess.run(
        ["node", str(js), str(fixture), str(shot), str(bbox_path), base_url],
        env={**os.environ, "PW_MODULE": str(VIEWER / "node_modules/@playwright/test")},
        cwd=VIEWER, capture_output=True, text=True, timeout=180,
    )
    if r.returncode != 0 or not shot.exists():
        log(f"viewer render failed: {r.stderr.strip()[:600]}")
        return None
    model = json.loads(conv.stdout)
    return shot, {"bboxes": json.loads(bbox_path.read_text()), "model": model}


# ---------------------------------------------------------------- comparisons

def composite_rows(shot: Path, work: Path, log) -> list[Path]:
    """Side-by-side composites, Apple page-N vs our render N.

    Decks (.key): ours/slide-N.png (stage element screenshots) align 1:1 with
    Apple's per-page rasters. Everything else: Apple page | proportional slice
    of our continuous-flow render — approximate, for locating, not verdicts.
    """
    from PIL import Image

    apple_pages = sorted((work / "apple").glob("page-*.png"), key=lambda p: int(p.stem.split("-")[-1]))
    if not apple_pages:
        return []
    out_dir = work / "composites"
    out_dir.mkdir(parents=True, exist_ok=True)

    # Deck mode: per-slide stage screenshots
    slide_shots = sorted((work / "ours").glob("slide-*.png"),
                         key=lambda p: int(p.stem.split("-")[-1]))
    if slide_shots:
        return composite_deck(apple_pages, slide_shots, out_dir, log)
    # Pages mode: per-page frame screenshots pair 1:1 too, at a taller
    # canvas so body text stays legible.
    page_shots = sorted((work / "ours").glob("page-*.png"),
                        key=lambda p: int(p.stem.split("-")[-1]))
    if page_shots:
        return composite_deck(apple_pages, page_shots, out_dir, log, canvas_h=1100)
    sheet_shots = sorted((work / "ours").glob("sheet-*.png"),
                         key=lambda p: int(p.stem.split("-")[-1]))
    if sheet_shots:
        return composite_deck(apple_pages, sheet_shots, out_dir, log, canvas_h=900)

    ours = Image.open(shot)
    ap = Image.open(apple_pages[0])
    scale = ap.width / ours.width  # ours -> apple page width
    ours_scaled_h = int(ours.height * scale)
    ours_scaled = ours.resize((ap.width, ours_scaled_h), Image.LANCZOS)

    stack_h = ap.height * len(apple_pages)
    ours_scaled_h2 = ours_scaled_h
    written = []
    for i in range(len(apple_pages)):
        y0 = int(i * ap.height / stack_h * ours_scaled_h2)
        y1 = int((i + 1) * ap.height / stack_h * ours_scaled_h2)
        ours_slice = ours_scaled.crop((0, y0, ap.width, min(y1, ours_scaled_h2)))
        canvas = Image.new("RGB", (ap.width * 2 + 12, ap.height), "#d0d0d8")
        canvas.paste(Image.open(apple_pages[i]).convert("RGB"), (0, 0))
        canvas.paste(ours_slice, (ap.width + 12, 0))
        out = out_dir / f"composite-page-{i + 1}.png"
        canvas.save(out)
        written.append(out)
    log(f"wrote {len(written)} per-page composites")
    return written


def composite_deck(apple_pages, slide_shots, out_dir: Path, log, canvas_h: int = 480) -> list[Path]:
    """Apple page-N | our slide-N, each scaled to a common height."""
    from PIL import Image

    written = []
    n = max(len(apple_pages), len(slide_shots))
    for i in range(n):
        a_img = o_img = None
        if i < len(apple_pages):
            a_img = Image.open(apple_pages[i]).convert("RGB")
            a_img = a_img.resize((int(a_img.width * canvas_h / a_img.height), canvas_h), Image.LANCZOS)
        if i < len(slide_shots):
            o_img = Image.open(slide_shots[i]).convert("RGB")
            o_img = o_img.resize((int(o_img.width * canvas_h / o_img.height), canvas_h), Image.LANCZOS)
        a_img = a_img or _blank()
        o_img = o_img or _blank()
        canvas = Image.new("RGB", (a_img.width + o_img.width + 12, canvas_h + 22), "#d0d0d8")
        canvas.paste(a_img, (0, 22))
        canvas.paste(o_img, (a_img.width + 12, 22))
        out = out_dir / f"composite-page-{i + 1}.png"
        canvas.save(out)
        written.append(out)
    log(f"wrote {len(written)} per-slide deck composites")
    return written


def _unguard_pil() -> None:
    """A 65-page word-processing render is a ~320 Mpx PNG: over Pillow's
    decompression-bomb guard, but it is our own screenshot."""
    from PIL import Image
    Image.MAX_IMAGE_PIXELS = None


def row_diff_bands(apple_page: Path, shot: Path, band: int = 40) -> list[tuple[int, int]]:
    """Locate rows where the Apple page and the proportional ours-slice
    disagree. Heuristic LOCATOR ONLY: layouts differ, so the abs diff is noisy
    by design and must not be read as a verdict."""
    from PIL import Image, ImageChops

    ap = Image.open(apple_page).convert("L")
    ours = Image.open(shot).convert("L").resize(ap.size, Image.LANCZOS)
    diff = ImageChops.difference(ap, ours)
    bands = []
    w, h = diff.size
    px = diff.load()
    for y in range(0, h, band):
        total = 0
        for yy in range(y, min(y + band, h), 2):
            for xx in range(0, w, 8):
                total += px[xx, yy]
        n = (min(y + band, h) - y) // 2 * (w // 8)
        if n and total / n > 30:
            bands.append((y, y + band))
    return bands


def _blank():
    from PIL import Image
    return Image.new("RGB", (200, 60), "#eeeeee")


def side_by_side(apple_img, ours_img, out: Path, label_a="Apple", label_o="ours") -> None:
    """Paste two PIL images (top-aligned) side by side with a labeled gutter."""
    from PIL import Image, ImageDraw

    h = max(apple_img.height, ours_img.height)
    gutter = 12
    canvas = Image.new("RGB", (apple_img.width + ours_img.width + gutter, h + 22), "#404048")
    d = ImageDraw.Draw(canvas)
    d.text((4, 4), label_a, fill="#ffffff")
    d.text((apple_img.width + gutter + 4, 4), label_o, fill="#ffffff")
    canvas.paste(apple_img.convert("RGB"), (0, 22))
    canvas.paste(ours_img.convert("RGB"), (apple_img.width + gutter, 22))
    out.parent.mkdir(parents=True, exist_ok=True)
    canvas.save(out)


def _crop(img, rect, margin=0):
    from PIL import Image

    x0, y0, x1, y1 = rect
    x0, y0 = max(0, int(x0 - margin)), max(0, int(y0 - margin))
    x1, y1 = min(img.width, int(x1 + margin)), min(img.height, int(y1 + margin))
    return img.crop((x0, y0, x1, y1)) if x1 > x0 and y1 > y0 else Image.new("RGB", (10, 10), "#dddddd")


def find_anchors(pdf: Path, needles: list[str], dpi: int = 150) -> dict[str, list[dict]]:
    """Apple-PDF text anchors via pymupdf: needle -> [{page, x0,y0,x1,y1}]."""
    import pymupdf

    s = dpi / 72.0
    hits: dict[str, list[dict]] = {}
    doc = pymupdf.open(pdf)
    for page_no, page in enumerate(doc, start=1):
        for needle in needles:
            for r in page.search_for(needle):
                hits.setdefault(needle, []).append({
                    "needle": needle,
                    "page": page_no,
                    "x0": r.x0 * s, "y0": r.y0 * s, "x1": r.x1 * s, "y1": r.y1 * s,
                })
    doc.close()
    return hits


def crop_regions(pdf: Path | None, preview: Path | None, shot: Path,
                 bboxes: dict, work: Path, dpi: int, log) -> list[Path]:
    """Zoomed side-by-side crops of the regions of interest."""
    from PIL import Image

    crops = work / "crops"
    crops.mkdir(parents=True, exist_ok=True)
    written = []

    apple_pages = {i + 1: Image.open(p).convert("RGB")
                   for i, p in enumerate(sorted((work / "apple").glob("page-*.png"), key=lambda p: int(p.stem.split("-")[-1])))}
    if preview is not None and preview.suffix == ".pdf" and not apple_pages:
        for i, p in enumerate(rasterize_pdf(preview, work / "apple", dpi=dpi, max_pages=1)):
            apple_pages[i + 1] = Image.open(p).convert("RGB")

    ours = Image.open(shot).convert("RGB")
    dsf = ours.width / bboxes["innerWidth"] if bboxes.get("innerWidth") else 2.0

    def ours_bbox_crop(bbox, margin=12):
        if not bbox:
            return None
        return _crop(ours, (bbox["x"] * dsf, bbox["y"] * dsf,
                            (bbox["x"] + bbox["w"]) * dsf, (bbox["y"] + bbox["h"]) * dsf), margin)

    def apple_around(needle_hits, extra_w=0, extra_h=0):
        """Crop the Apple page around the first anchor hit, extended right/down."""
        for h in needle_hits:
            img = apple_pages[h["page"]]
            return _crop(img, (h["x0"], h["y0"] - 14, h["x1"] + extra_w, h["y1"] + extra_h), 6)
    ours_tables = {t.get("caption") or f"table{i}": t
                   for i, t in enumerate(bboxes.get("tables", []), start=1)}
    hits = find_anchors(pdf, ["A1", "B3", "C3"], dpi) if pdf else {}
    a_img = None
    for needle in ("A1", "B3", "C3"):
        a_img = apple_around(hits.get(needle, []), extra_w=500, extra_h=110)
        if a_img:
            break
    o_img = ours_bbox_crop(ours_tables.get("Table 1"), margin=8)
    if a_img and o_img:
        side_by_side(a_img or _blank(), o_img or _blank(),
                     crops / "table1-3x3.png", "Apple (Table 1)", "ours (Table 1)")
        written.append(crops / "table1-3x3.png")

    # --- Table 2 (5x4): whole table, merged region, degraded cells ---------
    # Apple renders this table narrower than the model's stored size, so
    # anchor extents come from text hits, not model geometry.
    ROW_LABELS = ["Header", "Alternating", "Row", "Color", "Wrapped String"]
    hits = find_anchors(pdf, ROW_LABELS + ["Merged"], dpi) if pdf else {}
    a_img = None
    table_origin = None
    row_anchors: dict[int, dict] = {}
    alt = hits.get("Alternating", [])
    if alt:
        page_no = alt[0]["page"]
        # "Header"/"Row" also occur in prose; keep only hits on the table page
        for r, label in enumerate(ROW_LABELS):
            hs = [h for h in hits.get(label, []) if h["page"] == page_no]
            hs.sort(key=lambda h: h["y0"])
            if hs:
                row_anchors[r] = hs[0]
        if 1 in row_anchors:
            table_origin = row_anchors.get(0) or row_anchors[1]
            # whole-table crop: from the first row anchor down past the last
            top = min(h["y0"] for h in row_anchors.values())
            bottom = max(h["y1"] for h in row_anchors.values()) + 40
            left = min(h["x0"] for h in row_anchors.values())
            ap_img = apple_pages[page_no]
            right = ap_img.width - 150  # inside the page's right margin
            a_img = _crop(ap_img, (left - 10, top - 12, right, bottom), 0)

    o_img = ours_bbox_crop(ours_tables.get("Table 2"), margin=8)
    if a_img and o_img:
        side_by_side(a_img or _blank(), o_img or _blank(),
                     crops / "table2-5x4.png", "Apple (Table 2)", "ours (Table 2)")
        written.append(crops / "table2-5x4.png")

    tb = ours_tables.get("Table 2")
    if tb and row_anchors:
        cell_of = {(c2["r"], c2["c"]): c2 for c2 in tb.get("cells", [])}
        degraded = {"r2c1": (2, 1), "r2c2": (2, 2), "r2c3": (2, 3),
                    "r3c2": (3, 2), "r4c3": (4, 3)}
        for name, (r, c) in degraded.items():
            ours_cell = cell_of.get((r, c))
            if ours_cell:
                o_cell = _crop(ours, (ours_cell["x"] * dsf, ours_cell["y"] * dsf,
                                      (ours_cell["x"] + ours_cell["w"]) * dsf,
                                      (ours_cell["y"] + ours_cell["h"]) * dsf), 12)
            else:  # merged-away or missing cell: fall back to fractional rects
                fr = (tb["x"] + tb["w"] * c / 4, tb["y"] + tb["h"] * r / 5,
                      tb["x"] + tb["w"] * (c + 1) / 4, tb["y"] + tb["h"] * (r + 1) / 5)
                o_cell = _crop(ours, tuple(v * dsf for v in fr), margin=18 * dsf)
            a_cell = None
            if row_anchors:
                ap_img = apple_pages[alt[0]["page"]]
                top = row_anchors[r]
                y0 = top["y0"] - 6
                y1 = row_anchors[r + 1]["y0"] - 6 if (r + 1) in row_anchors else top["y1"] + 46
                left = min(h["x0"] for h in row_anchors.values())
                a_cell = _crop(ap_img, (left - 10, y0, ap_img.width - 150, y1), 0)
            side_by_side(a_cell or _blank(), o_cell,
                         crops / f"table2-cell-{name}.png",
                         f"Apple (Table 2 row {r})", f"ours (Table 2 {name})")
            written.append(crops / f"table2-cell-{name}.png")

        # merged region: Apple "Merged" anchor vs our r1 c1..c2 area
        merged_cell = cell_of.get((1, 1)) or cell_of.get((1, 2))
        if merged_cell:
            o_img = _crop(ours, (merged_cell["x"] * dsf, merged_cell["y"] * dsf,
                                 (merged_cell["x"] + merged_cell["w"] * 2.2) * dsf,
                                 (merged_cell["y"] + merged_cell["h"]) * dsf), 16)
        else:
            fr = (tb["x"] + tb["w"] / 4, tb["y"] + tb["h"] / 5,
                  tb["x"] + tb["w"] * 3 / 4, tb["y"] + tb["h"] * 2 / 5)
            o_img = _crop(ours, tuple(v * dsf for v in fr), margin=24 * dsf)
        mhits = hits.get("Merged", [])
        if mhits:
            h0 = mhits[0]
            a_img = _crop(apple_pages[h0["page"]],
                          (h0["x0"] - 10, h0["y0"] - 8, h0["x1"] + 420, h0["y1"] + 56), 8)
        side_by_side(a_img or _blank(), o_img,
                     crops / "table2-merged-r1.png",
                     "Apple (Table 2 merged r1)", "ours (Table 2 merged r1)")
        written.append(crops / "table2-merged-r1.png")

    # --- inline image spot: Apple shows it, ours is expected to NOT have it -
    hits = find_anchors(pdf, ["Here is a small inline image"], dpi) if pdf else {}
    a_img = apple_around(hits.get("Here is a small inline image", []), extra_w=460, extra_h=90)
    o_img = ours_bbox_crop(bboxes.get("imageParagraph"), margin=30 * dsf)
    if a_img or o_img:
        side_by_side(a_img or _blank(), o_img or _blank(),
                     crops / "inline-image-spot.png",
                     "Apple (inline image)", "ours (inline image — expected ABSENT)")
        written.append(crops / "inline-image-spot.png")

    log(f"wrote {len(written)} region crops")
    return written


# ---------------------------------------------------------------- summary

def write_summary(work: Path, fixture: Path, mode: str, n_pages: int,
                  shot: Path, bboxes: dict, model: dict, composites: list[Path],
                  crops: list[Path], bands_by_page: dict, dpi: int) -> Path:
    warnings = [w.get("message", w.get("code", "?")) for w in model.get("warnings", [])]
    media = model.get("media", [])
    lines = [
        "# Visual diff — Apple ground truth vs our viewer render",
        "",
        f"- Fixture: `{fixture}`",
        f"- Apple side mode: **{mode}**"
        + ("" if mode.endswith("export") else
           " (app automation unavailable → embedded QuickLook preview, page 1 only)"),
        f"- Apple pages rasterized: {n_pages} at {dpi}dpi",
        f"- Our render: `{shot}` (continuous flow; per-page composites slice it "
        "proportionally — approximate alignment, for locating differences, not pixel verdicts)",
        f"- Converter warnings: {len(warnings)}",
    ]
    lines += [f"  - {w}" for w in warnings]
    lines += [
        f"- Converter `media` array: {len(media)} item(s).",
        "",
        "## Pages compared",
    ]
    for c in composites:
        page_no = c.stem.split("-")[-1]
        lines.append(f"- `{c.name}` (Apple page {page_no} | proportional ours slice)")
        for y0, y1 in bands_by_page.get(page_no, [])[:8]:
            lines.append(f"  - high-diff row band y={y0}–{y1} (locator heuristic, noisy by design)")
    lines += ["", "## Regions cropped"]
    for c in crops:
        lines.append(f"- `crops/{c.name}`")
    table_names = {t.get("caption") for t in bboxes.get("tables", [])}
    if {"Table 1", "Table 2"} <= table_names:
        lines += [
        "",
        "## Known differences to inspect",
        "",
        "1. **Table 2 number formats** (`table-degraded` warnings: malformed number format dropped):",
        f"  - r1c3: Apple shows hex-style `4D2` for value 1234; ours shows raw `1234`.",
        f"  - r2c3: Apple `12.5%` (percent); ours raw `0.125`.",
        f"  - r3c2: Apple `$1234.56` (currency); ours `USD 1234.56` (prefix style, not Apple's).",
        f"  - r3c3: Apple `1/4` (fraction); ours raw `0.25`.",
        f"  - r4c2: Apple `Fri, Aug 28, 2026` (date style); ours ISO `2026-08-28`.",
        f"  - r4c3: Apple `6.0221408E+23`; ours lowercase `6.0221408e+23` (cosmetic).",
        f"  - r2c1 (2468, formula-unparsed): both sides show plain `2468` — low severity.",
        "2. **Table 2 merge rendering**: Apple spans the 'Merged' cell horizontally across c1–c2 "
        "(and renders a second 'Merged' block down r2–r4 c1–c2); ours draws cell borders between "
        "c1/c2, shows 'Merged' text twice (r1c1, r3c1) and leaves r4c1 EMPTY where Apple shows "
        "'Merged' — merge spans are not reconstructed in our render. See `table2-merged-r1.png` "
        "and `table2-cell-r4c3.png` (Apple row 4).",
        "3. **Inline image** (Data/rsz_under_const-27.png): Apple renders the 🚧 image INLINE "
        "within the sentence 'Here is a small inline image 🚧 next to text'. Our render now "
        "includes the image (converter media fixed mid-run), but renders it as a BLOCK on its "
        "own line, breaking the sentence into three lines — inline text-flow is not preserved. "
        "See `inline-image-spot.png`.",
        "4. **Table 1 (3x3) page split**: Apple splits Table 1 across a page break (rows A1–C2 on "
        "one page, A3–C3 on the next); our continuous flow renders the whole table in one place — "
        "acceptable for a flow viewer, visible in `table1-3x3.png` and page composites.",
        "5. **Alignment**: Apple centers cell text (e.g. A1/B1/C1) and right-aligns numbers; ours "
        "left-aligns text and numbers. Cosmetic. Visible in `table1-3x3.png`.",
        ]
    else:
        lines += [
        "",
        "## Findings",
        "",
        "- Inspect the per-page composites above and the crop regions for layout, "
        "formatting, and content drift between Apple's render and ours. Findings are "
        "recorded in the campaign summary for this format.",
        ]
    out = work / "summary.md"
    out.write_text("\n".join(lines) + "\n")
    return out


# ---------------------------------------------------------------- main

def run_one(app: str, fixture: Path, work: Path, args, log) -> bool:
    work.mkdir(parents=True, exist_ok=True)
    _unguard_pil()

    # Apple side
    pdf = preview = None
    mode = "fallback-preview"
    prior_pdf = work / "apple" / "export.pdf"
    if args.skip_apple:
        log("skipping Apple side (--skip-apple)")
    elif args.reuse_apple and prior_pdf.exists() and prior_pdf.stat().st_size > 0:
        pdf, mode = prior_pdf, f"{app}-export (reused)"
        log(f"reusing Apple export {prior_pdf}")
    else:
        try:
            pdf, mode = export_via_app(APP_NAMES[app], fixture, work, log)
        except Exception as e:
            log(f"Apple side aborted: {e}")
    if pdf is not None:
        pages = rasterize_pdf(pdf, work / "apple", dpi=args.dpi)
        log(f"rasterized {len(pages)} Apple pages at {args.dpi}dpi")
    else:
        preview = extract_preview(fixture, work / "apple")
        if preview and preview.suffix == ".pdf":
            rasterize_pdf(preview, work / "apple", dpi=args.dpi, max_pages=1)
        elif preview is not None:
            # jpg/png QuickLook preview: normalize to page-1.png so composites
            # still get an Apple-side image
            from PIL import Image
            Image.open(preview).convert("RGB").save(work / "apple" / "page-1.png")
        if preview is None:
            log("no embedded preview either; Apple side will be empty")
        else:
            log(f"fallback: preview.{preview.suffix}, page 1 only")

    apple_pages = sorted((work / "apple").glob("page-*.png"), key=lambda p: int(p.stem.split("-")[-1]))
    log(f"apple pages: {len(apple_pages)}")

    # Our side
    server = ensure_viewer_server(args.base_url, log)
    try:
        rendered = render_ours(fixture, work, args.base_url, log)
    finally:
        if server:
            server.terminate()
    if rendered is None:
        print("[visual_diff] FATAL: could not render our side", file=sys.stderr)
        return False
    shot, ctx = rendered
    bboxes, model = ctx["bboxes"], ctx["model"]

    # Composites + heuristic bands
    composites = composite_rows(shot, work, log) if apple_pages else []
    bands_by_page: dict[str, list] = {}
    for i, page in enumerate(apple_pages):
        bands_by_page[str(i + 1)] = row_diff_bands(page, shot)

    crops = crop_regions(pdf, preview, shot, bboxes, work, args.dpi, log)
    summary = write_summary(work, fixture, mode, len(apple_pages), shot, bboxes,
                            model, composites, crops, bands_by_page, args.dpi)

    print(f"[visual_diff] artifacts: {work}")
    print(f"[visual_diff] summary: {summary}")
    return True


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--fixture", action="append", default=[], type=Path,
                    help="fixture path; repeatable. Mutually exclusive with --batch.")
    ap.add_argument("--batch", type=Path, metavar="TSV",
                    help="success.tsv (or a plain text file of fixture paths); one doc "
                         "per row, app inferred from the extension. "
                         "--out is the campaign base dir (<out>/<ext>/<stem>/).")
    ap.add_argument("--app", choices=("pages", "numbers", "keynote"), default="pages",
                    help="iWork app used for the Apple-side PDF export (single mode)")
    ap.add_argument("--out", required=True, type=Path, help="artifact dir (or base dir with --batch)")
    ap.add_argument("--dpi", type=int, default=150)
    ap.add_argument("--base-url", default="http://127.0.0.1:8123")
    ap.add_argument("--skip-apple", action="store_true", help="use embedded preview fallback")
    ap.add_argument("--reuse-apple", action="store_true",
                    help="reuse <out>/apple/export.pdf from a previous run (iterate on our side only)")
    args = ap.parse_args()

    def log(msg: str) -> None:
        print(f"[visual_diff] {msg}", flush=True)

    jobs: list[tuple[str, Path, Path]] = []
    if args.batch:
        if args.fixture:
            ap.error("--batch and --fixture are mutually exclusive")
        ext_to_app = {".pages": "pages", ".numbers": "numbers", ".key": "keynote"}
        for line in open(args.batch):
            line = line.strip()
            if not line or line.startswith("local_id"):
                continue
            if "\t" in line:
                cols = line.split("\t")
                sha, ext = cols[1], cols[2]
                p = REPO / "fixtures/crawl" / f"{sha}.{ext}"
            else:
                p = Path(line)
            app = ext_to_app.get(p.suffix)
            if app is None:
                log(f"skip (no app for {p.suffix}): {p}")
                continue
            stem = p.name.rsplit(".", 1)[0][:60] or p.stem[:60]
            jobs.append((app, p, args.out.resolve() / p.suffix.lstrip(".") / stem))
    else:
        if not args.fixture:
            ap.error("one or more --fixture paths (or --batch) are required")
        for f in args.fixture:
            f = f.resolve()
            jobs.append((args.app, f, args.out.resolve()))

    failures = 0
    for i, (app, fixture, work) in enumerate(jobs, start=1):
        if len(jobs) > 1:
            log(f"=== [{i}/{len(jobs)}] {fixture.name} ({app}) -> {work}")
        if not fixture.exists():
            log(f"fixture missing: {fixture}")
            failures += 1
            continue
        try:
            if not run_one(app, fixture, work, args, log):
                failures += 1
        except Exception as e:
            log(f"FAILED {fixture.name}: {e}")
            failures += 1
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
