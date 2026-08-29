// Pages (.pages): word-processing flavor = flowing body + footnotes;
// page-layout flavor = stacked positioned page canvases. Floating objects
// render as page-anchored canvases in both flavors.

import type { PagesDocument } from "../../model/src/pages";
import type { ViewerCtx } from "./ctx";
import type { Drawable } from "../../model/src/shared";
import { renderParagraph, renderStyledText } from "./text";
import { renderCanvasDrawable } from "./drawables";

function pageCanvas(
  doc: PagesDocument,
  ctx: ViewerCtx,
  drawables: Drawable[],
  pageIndex: number | undefined,
): HTMLElement {
  const frame = document.createElement("div");
  frame.className = "canvas-frame pages-page";
  if (doc.pageSize) {
    frame.style.aspectRatio = `${doc.pageSize.width} / ${doc.pageSize.height}`;
    const inner = document.createElement("div");
    inner.className = "canvas-inner";
    // scale: pages render at a fixed 720px logical width, scaled responsively
    const scale = 720 / doc.pageSize.width;
    inner.style.width = `${doc.pageSize.width}px`;
    inner.style.height = `${doc.pageSize.height}px`;
    inner.style.transform = `scale(${scale})`;
    inner.style.position = "absolute";
    inner.style.top = "0";
    inner.style.left = "50%";
    inner.style.marginLeft = `${-doc.pageSize.width * scale / 2}px`;
    for (const d of drawables) inner.appendChild(renderCanvasDrawable(d, ctx));
    frame.appendChild(inner);
    frame.style.height = `${doc.pageSize.height * scale}px`;
    frame.dataset.pageIndex = pageIndex === undefined ? "" : String(pageIndex);
  } else {
    const inner = document.createElement("div");
    inner.className = "canvas-inner";
    inner.style.position = "relative";
    inner.style.width = "720px";
    inner.style.minHeight = "400px";
    for (const d of drawables) inner.appendChild(renderCanvasDrawable(d, ctx));
    frame.appendChild(inner);
  }
  return frame;
}

function floatingSection(doc: PagesDocument, ctx: ViewerCtx, mount: HTMLElement): void {
  if (doc.floating.length === 0) return;
  const wrap = document.createElement("section");
  wrap.className = "floating-section";
  const h = document.createElement("h3");
  h.textContent = doc.flavor === "page-layout" ? "Pages" : "Floating objects";
  wrap.appendChild(h);
  doc.floating.forEach((group, i) => {
    const label = document.createElement("div");
    label.className = "muted canvas-caption";
    label.textContent = group.pageIndex !== undefined ? `Page ${group.pageIndex + 1}` : `Group ${i + 1}`;
    wrap.appendChild(label);
    wrap.appendChild(pageCanvas(doc, ctx, group.drawables, group.pageIndex));
  });
  mount.appendChild(wrap);
}

export function renderPages(doc: PagesDocument, ctx: ViewerCtx, mount: HTMLElement): void {
  const view = document.createElement("div");
  view.id = "pages-view";

  if (doc.flavor === "word-processing" && doc.body) {
    const flow = document.createElement("article");
    flow.className = "pages-flow";
    for (const p of doc.body.paragraphs) flow.appendChild(renderParagraph(p, ctx));
    view.appendChild(flow);

    if (doc.footnotes?.length) {
      const section = document.createElement("section");
      section.className = "footnotes-section";
      const h = document.createElement("h3");
      h.textContent = "Footnotes";
      section.appendChild(h);
      doc.footnotes.forEach((fn, i) => {
        const row = document.createElement("div");
        row.className = "footnote";
        const mark = document.createElement("span");
        mark.className = "mark";
        mark.textContent = `${i + 1}.`;
        row.appendChild(mark);
        row.appendChild(renderStyledText(fn.text, ctx));
        section.appendChild(row);
      });
      view.appendChild(section);
    }
  }

  floatingSection(doc, ctx, view);
  mount.appendChild(view);
}