// The pdf.js half of pdfmedia.ts, loaded on demand: esbuild splits this
// module (and pdf.js with it, ~460 KB) into its own chunk that the page
// fetches the first time a document carries PDF media. The worker is a
// same-origin file the build copies next to main.js, so both loads are
// covered by CSP 'self' and only happen for the ~26% of corpus documents
// that embed a PDF.
//
// pdfjs-dist 6.3.289, Apache-2.0 (Mozilla) — see docs/format/ATTRIBUTION.md.

import * as pdfjs from "pdfjs-dist/build/pdf.min.mjs";

let workerConfigured = false;

function ensureWorker(): void {
  if (workerConfigured) return;
  workerConfigured = true;
  pdfjs.GlobalWorkerOptions.workerSrc = new URL("pdf.worker.min.mjs", document.baseURI).href;
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

