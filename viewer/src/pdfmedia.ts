// In-browser rasterization of PDF media (Keynote equations are stored as
// PDFs with no raster twin; pasted vector art often is too) with pdf.js,
// bundled from viewer/node_modules — no CDN. The worker script is embedded
// as text and started from a blob: URL, so the page makes no request after
// load (the Playwright gate asserts that) and the CSP only needs
// `worker-src blob:`.
//
// pdfjs-dist 6.3.289, Apache-2.0 (Mozilla) — see docs/format/ATTRIBUTION.md.

import * as pdfjs from "pdfjs-dist/build/pdf.min.mjs";
import workerSource from "./gen/pdf.worker.txt";

let workerStarted = false;

function ensureWorker(): void {
  if (workerStarted) return;
  workerStarted = true;
  try {
    const blob = new Blob([workerSource], { type: "text/javascript" });
    pdfjs.GlobalWorkerOptions.workerPort = new Worker(URL.createObjectURL(blob), { type: "module" });
  } catch {
    // No worker (CSP or an old browser): pdf.js falls back to importing
    // workerSrc, which we do not serve; getDocument rejects and the caller
    // shows its placeholder.
  }
}

/** `%PDF` magic: .ai files saved with PDF compatibility qualify too. */
export function isPdfBytes(b: Uint8Array): boolean {
  return b.length > 4 && b[0] === 0x25 && b[1] === 0x50 && b[2] === 0x44 && b[3] === 0x46;
}

// One parsed document per media asset, shared by every drawable that shows it.
const docs = new WeakMap<Uint8Array, Promise<pdfjs.PDFDocumentProxy>>();

function loadDoc(bytes: Uint8Array): Promise<pdfjs.PDFDocumentProxy> {
  let p = docs.get(bytes);
  if (!p) {
    ensureWorker();
    p = pdfjs.getDocument({
      data: bytes.slice(), // pdf.js transfers the buffer to the worker
      disableFontFace: true, // glyphs drawn as paths: no font loading
      useSystemFonts: false,
    }).promise;
    docs.set(bytes, p);
  }
  return p;
}

/**
 * Rasterize page 1 to a canvas that fills a cssW x cssH box (device-pixel
 * resolution, capped at 4096px on the long side).
 */
export async function renderPdfToCanvas(bytes: Uint8Array, cssW: number, cssH: number): Promise<HTMLCanvasElement> {
  const doc = await loadDoc(bytes);
  const page = await doc.getPage(1);
  const base = page.getViewport({ scale: 1 });
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  let scale = Math.max(cssW / base.width, cssH / base.height) * dpr;
  if (!(scale > 0) || !Number.isFinite(scale)) scale = dpr;
  const longest = Math.max(base.width, base.height) * scale;
  if (longest > 4096) scale *= 4096 / longest;
  const viewport = page.getViewport({ scale });
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.ceil(viewport.width));
  canvas.height = Math.max(1, Math.ceil(viewport.height));
  await page.render({ canvas, viewport }).promise;
  page.cleanup();
  return canvas;
}

/**
 * A box that fills itself with the rasterized PDF once pdf.js is done;
 * `fallback()` replaces it if rendering fails. The `pending` class is on
 * until either happens (the visual_diff harness waits for it to clear).
 */
export function pdfMediaEl(bytes: Uint8Array, cssW: number, cssH: number, fallback: () => HTMLElement): HTMLElement {
  const host = document.createElement("div");
  host.className = "media-pdf pending";
  renderPdfToCanvas(bytes, cssW, cssH)
    .then((canvas) => {
      canvas.className = "media-pdf-canvas";
      host.replaceChildren(canvas);
    })
    .catch(() => host.replaceChildren(fallback()))
    .finally(() => host.classList.remove("pending"));
  return host;
}
