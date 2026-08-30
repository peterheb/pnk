// Pages (.pages): word-processing flavor = flowing body paginated into
// page-shaped frames (page size + margins from the document archive);
// page-layout flavor = stacked positioned page canvases. Floating objects
// render into their anchor page's canvas.

import type { PagesDocument, PageTemplate } from "../../model/src/pages";
import type { ViewerCtx } from "./ctx";
import type { Drawable, StyledText } from "../../model/src/shared";
import { newListNumberingState, renderParagraph, renderStyledText } from "./text";
import { renderCanvasDrawable } from "./drawables";
import { paraStyleOf, type HydratedDoc } from "./hydrate";

/** Any visible text in a styled-text block? */
function hasText(t: StyledText): boolean {
  return t.paragraphs.some((p) =>
    p.items.some((it) =>
      typeof it === "string" ? it.length > 0 : !("type" in it) ? it.text.length > 0 : true,
    ),
  );
}

function templateHasHf(t: PageTemplate | undefined): boolean {
  return !!t && (t.headers.some(hasText) || t.footers.some(hasText));
}

/**
 * Header/footer template for page `i` (0-based): first-page template on page
 * 1 (honoring its hide flag), else the parity template, falling back to the
 * other parity when one side was left empty (Pages stores empty storages on
 * the unused templates when "different even/odd" is off).
 */
function hfTemplateFor(doc: PagesDocument, i: number): PageTemplate | undefined {
  const sec = doc.sections[0];
  if (!sec) return undefined;
  const byName = new Map<string, PageTemplate>();
  for (const t of doc.pageTemplates) if (t.name) byName.set(t.name, t);
  const first = sec.firstPageTemplate ? byName.get(sec.firstPageTemplate) : undefined;
  const even = sec.evenPageTemplate ? byName.get(sec.evenPageTemplate) : undefined;
  const odd = sec.oddPageTemplate ? byName.get(sec.oddPageTemplate) : undefined;
  if (i === 0 && first?.hideHeadersFooters) return undefined;
  const parity = (i + 1) % 2 === 0 ? [even, odd] : [odd, even];
  const candidates = i === 0 ? [first, ...parity] : parity;
  for (const c of candidates) if (templateHasHf(c)) return c;
  return undefined;
}

/** Three header/footer column storages as one absolutely-positioned row. */
function hfRow(
  cols: StyledText[],
  hdoc: HydratedDoc,
  ctx: ViewerCtx,
  cssClass: string,
): HTMLElement {
  const row = document.createElement("div");
  row.className = `pages-hf ${cssClass}`;
  const aligns = ["left", "center", "right"] as const;
  cols.slice(0, 3).forEach((t, i) => {
    const col = document.createElement("div");
    col.className = "pages-hf-col";
    col.style.textAlign = aligns[i] ?? "left";
    col.appendChild(renderStyledText(t, hdoc, ctx));
    row.appendChild(col);
  });
  return row;
}

/** Resolve page-number / page-count fields to the paginated reality. */
function fillPageFields(root: HTMLElement, pageNumber: number, pageCount: number): void {
  root.querySelectorAll<HTMLElement>('.field[data-field-kind="page-number"]').forEach((el) => {
    el.textContent = String(pageNumber);
  });
  root.querySelectorAll<HTMLElement>('.field[data-field-kind="page-count"]').forEach((el) => {
    el.textContent = String(pageCount);
  });
}

function pageCanvas(
  doc: PagesDocument,
  hdoc: HydratedDoc,
  ctx: ViewerCtx,
  drawables: Drawable[],
  pageIndex: number | undefined,
  templateDrawables?: Drawable[],
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
    // template underlay paints first, beneath the page's own drawables
    for (const d of templateDrawables ?? []) inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));
    for (const d of drawables) inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));
    frame.appendChild(inner);
    frame.style.height = `${doc.pageSize.height * scale}px`;
    frame.dataset.pageIndex = pageIndex === undefined ? "" : String(pageIndex);
  } else {
    const inner = document.createElement("div");
    inner.className = "canvas-inner";
    inner.style.position = "relative";
    inner.style.width = "720px";
    inner.style.minHeight = "400px";
    for (const d of templateDrawables ?? []) inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));
    for (const d of drawables) inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));
    frame.appendChild(inner);
  }
  return frame;
}

function floatingSection(
  doc: PagesDocument,
  hdoc: HydratedDoc,
  ctx: ViewerCtx,
  mount: HTMLElement,
  groups: PagesDocument["floating"],
  title: string,
): void {
  if (groups.length === 0) return;
  const wrap = document.createElement("section");
  wrap.className = "floating-section";
  const h = document.createElement("h3");
  h.textContent = title;
  wrap.appendChild(h);
  groups.forEach((group, i) => {
    const label = document.createElement("div");
    label.className = "muted canvas-caption";
    label.textContent = group.pageIndex !== undefined ? `Page ${group.pageIndex + 1}` : `Group ${i + 1}`;
    wrap.appendChild(label);
    wrap.appendChild(
      pageCanvas(doc, hdoc, ctx, group.drawables, group.pageIndex, group.templateDrawables),
    );
  });
  mount.appendChild(wrap);
}

/** Printable-area geometry in points, from the document archive
 *  (TP.DocumentArchive page_width/height 30/31, margins 32-35 [proto]). */
interface PageGeom {
  w: number;
  h: number;
  left: number;
  top: number;
  contentW: number;
  contentH: number;
}

function pageGeom(doc: PagesDocument): PageGeom | null {
  const ps = doc.pageSize;
  if (!ps || !(ps.width > 0) || !(ps.height > 0)) return null;
  const m = doc.pageMargins;
  const left = m?.left ?? 72;
  const right = m?.right ?? 72;
  const top = m?.top ?? 72;
  const bottom = m?.bottom ?? 72;
  const contentW = ps.width - left - right;
  const contentH = ps.height - top - bottom;
  if (contentW < 36 || contentH < 36) return null;
  return { w: ps.width, h: ps.height, left, top, contentW, contentH };
}

/**
 * Word-processing pagination: paragraphs measured offscreen at the printable
 * width, then greedily packed into page frames of the printable height.
 * Paragraph-level breaks only (an oversized paragraph gets its own page and
 * clips) — a heuristic, not Apple's line-exact layout, but page count and
 * margins track the original closely. `pageBreakBefore` (paragraph style)
 * forces a break.
 */
function paginatedBody(
  doc: PagesDocument,
  hdoc: HydratedDoc,
  ctx: ViewerCtx,
  view: HTMLElement,
): void {
  const g = pageGeom(doc)!;
  const body = doc.body!;

  // 1. render all paragraphs once, in order (list numbering is stateful)
  const listState = newListNumberingState();
  const els: HTMLElement[] = [];
  const forceBreak: boolean[] = [];
  for (const p of body.paragraphs) {
    els.push(renderParagraph(p, hdoc, ctx, listState));
    forceBreak.push(!!paraStyleOf(hdoc, p.pStyle)?.pageBreakBefore);
  }

  // 2. measure at printable width in a hidden flow-root (transform scaling
  // does not affect layout metrics, so these heights match the page frames)
  const meas = document.createElement("div");
  meas.className = "pages-print";
  meas.style.cssText =
    `position:absolute;visibility:hidden;left:-100000px;top:0;width:${g.contentW}px;`;
  for (const el of els) meas.appendChild(el);
  document.body.appendChild(meas);
  const pagesEls: HTMLElement[][] = [[]];
  let pageStartY = 0;
  els.forEach((el, i) => {
    const top = el.offsetTop;
    const bottom = top + el.offsetHeight;
    const overflows = bottom - pageStartY > g.contentH && i > 0;
    if ((forceBreak[i] && i > 0 && pagesEls[pagesEls.length - 1].length > 0) || overflows) {
      pagesEls.push([]);
      pageStartY = top;
    }
    pagesEls[pagesEls.length - 1].push(el);
  });
  meas.remove();

  // 3. page frames: printable area positioned at the margins; floating
  // drawables anchored to page i render into the same canvas
  const floatingByPage = new Map<number, Drawable[]>();
  let maxFloatPage = -1;
  for (const gr of doc.floating) {
    const idx = gr.pageIndex ?? 0;
    floatingByPage.set(idx, [...(floatingByPage.get(idx) ?? []), ...gr.drawables]);
    if (idx > maxFloatPage) maxFloatPage = idx;
  }
  const pageCount = Math.max(pagesEls.length, maxFloatPage + 1);

  const scale = 720 / g.w;
  for (let i = 0; i < pageCount; i++) {
    const frame = document.createElement("div");
    frame.className = "canvas-frame pages-page pages-wp-page";
    frame.style.aspectRatio = `${g.w} / ${g.h}`;
    frame.style.height = `${g.h * scale}px`;
    frame.dataset.pageIndex = String(i);
    const inner = document.createElement("div");
    inner.className = "canvas-inner";
    inner.style.width = `${g.w}px`;
    inner.style.height = `${g.h}px`;
    inner.style.transform = `scale(${scale})`;
    inner.style.position = "absolute";
    inner.style.top = "0";
    inner.style.left = "50%";
    inner.style.marginLeft = `${-g.w * scale / 2}px`;

    // floating drawables first: behind the body text, like Apple's default
    for (const d of floatingByPage.get(i) ?? []) {
      inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));
    }

    const content = document.createElement("div");
    content.className = "pages-print";
    content.style.position = "absolute";
    content.style.left = `${g.left}px`;
    content.style.top = `${g.top}px`;
    content.style.width = `${g.contentW}px`;
    content.style.height = `${g.contentH}px`;
    for (const el of pagesEls[i] ?? []) content.appendChild(el);
    inner.appendChild(content);

    // headers/footers from the section's page templates, at the header/
    // footer margins (TP.DocumentArchive fields 36/37)
    const hf = hfTemplateFor(doc, i);
    if (hf) {
      const m = doc.pageMargins;
      if (hf.headers.some(hasText)) {
        const h = hfRow(hf.headers, hdoc, ctx, "pages-header");
        h.style.top = `${m?.header ?? 36}px`;
        h.style.left = `${g.left}px`;
        h.style.width = `${g.contentW}px`;
        inner.appendChild(h);
      }
      if (hf.footers.some(hasText)) {
        const f = hfRow(hf.footers, hdoc, ctx, "pages-footer");
        f.style.bottom = `${m?.footer ?? 36}px`;
        f.style.left = `${g.left}px`;
        f.style.width = `${g.contentW}px`;
        inner.appendChild(f);
      }
    }

    fillPageFields(inner, i + 1, pageCount);
    frame.appendChild(inner);
    view.appendChild(frame);
  }

  // footnote marks number sequentially through the document (Apple shows
  // superscript indexes; the converter stores only the mark position)
  let fnIndex = 0;
  view.querySelectorAll<HTMLElement>('.field[data-field-kind="footnote-mark"]').forEach((el) => {
    fnIndex += 1;
    el.textContent = String(fnIndex);
  });
}

export function renderPages(doc: PagesDocument, hdoc: HydratedDoc, ctx: ViewerCtx, mount: HTMLElement): void {
  const view = document.createElement("div");
  view.id = "pages-view";

  const wordProcessing = doc.flavor === "word-processing";
  const geom = pageGeom(doc);

  if (wordProcessing && doc.body && geom) {
    // paginated word-processing render: page frames + margins
    paginatedBody(doc, hdoc, ctx, view);
    appendFootnotes(doc, hdoc, ctx, view);
    mount.appendChild(view);
    return;
  }

  // document order: floating groups anchored to page 1 (a cover) belong
  // above the flowing body; later pages trail it
  const leading = doc.floating.filter((g) => (g.pageIndex ?? 0) === 0);
  const trailing = doc.floating.filter((g) => (g.pageIndex ?? 0) !== 0);
  if (!wordProcessing || doc.body) {
    floatingSection(doc, hdoc, ctx, view, leading, wordProcessing ? "Cover page" : "Pages");
  }

  if (wordProcessing && doc.body) {
    const flow = document.createElement("article");
    flow.className = "pages-flow";
    const listState = newListNumberingState();
    for (const p of doc.body.paragraphs) flow.appendChild(renderParagraph(p, hdoc, ctx, listState));
    view.appendChild(flow);
    appendFootnotes(doc, hdoc, ctx, view);
  }

  if (trailing.length > 0) {
    floatingSection(doc, hdoc, ctx, view, trailing, "Floating objects");
  }
  mount.appendChild(view);
}

function appendFootnotes(doc: PagesDocument, hdoc: HydratedDoc, ctx: ViewerCtx, view: HTMLElement): void {
  if (!doc.footnotes?.length) return;
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
    row.appendChild(renderStyledText(fn.text, hdoc, ctx));
    section.appendChild(row);
  });
  view.appendChild(section);
}
