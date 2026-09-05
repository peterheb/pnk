// PDF media (Keynote equations are stored as PDFs with no raster twin;
// pasted vector art often is too). This module is the synchronous face the
// renderers import; the pdf.js rasterizer lives in pdfmedia-core.ts and is
// fetched on first use, so documents without PDF media never load it.

/** `%PDF` magic: .ai files saved with PDF compatibility qualify too. */
export function isPdfBytes(b: Uint8Array): boolean {
  return b.length > 4 && b[0] === 0x25 && b[1] === 0x50 && b[2] === 0x44 && b[3] === 0x46;
}

// One in-flight load of the rasterizer chunk, shared by every caller.
let core: Promise<typeof import("./pdfmedia-core")> | null = null;
function loadCore(): Promise<typeof import("./pdfmedia-core")> {
  core ??= import("./pdfmedia-core");
  return core;
}

/**
 * A box that fills itself with the rasterized PDF once pdf.js is done;
 * `fallback()` replaces it if rendering fails. The `pending` class is on
 * until either happens (the visual_diff harness waits for it to clear).
 */
export function pdfMediaEl(bytes: Uint8Array, cssW: number, cssH: number, fallback: () => HTMLElement): HTMLElement {
  const host = document.createElement("div");
  host.className = "media-pdf pending";
  loadCore()
    .then((m) => m.renderPdfToCanvas(bytes, cssW, cssH))
    .then((canvas) => {
      canvas.className = "media-pdf-canvas";
      host.replaceChildren(canvas);
    })
    .catch(() => host.replaceChildren(fallback()))
    .finally(() => host.classList.remove("pending"));
  return host;
}
