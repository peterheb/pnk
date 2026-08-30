// Pages (.pages): word-processing flavor = flowing body paginated into
// page-shaped frames (page size + margins from the document archive);
// page-layout flavor = stacked positioned page canvases. Floating objects
// render into their anchor page's canvas.

import type { PagesDocument, PageTemplate } from "../../model/src/pages";
import type { ViewerCtx } from "./ctx";
import type { Drawable, StyledText } from "../../model/src/shared";
import { newListNumberingState, renderParagraph, renderStyledText } from "./text";
import { applyTextFit, renderCanvasDrawable } from "./drawables";
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
    // scale: pages render at a 720px logical width, or the container's on
    // narrow (mobile) screens — display-only, layout metrics are in pt
    const scale = pageDisplayWidth / doc.pageSize.width;
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
    inner.style.width = `${pageDisplayWidth}px`;
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

/** Multi-column spec for a run of body paragraphs (PagesSection.columns). */
interface ColSpec {
  count: number;
  gapPt: number;
}

/** A run of paragraphs on one page sharing a column layout (null = flow). */
interface PageBlock {
  cols: ColSpec | null;
  /** Column block: printable height available when the block started. */
  heightPx: number;
  els: HTMLElement[];
}

/**
 * Word-processing pagination: paragraphs measured offscreen at the printable
 * width, then greedily packed into page frames of the printable height.
 * Paragraph-level breaks only (an oversized paragraph gets its own page and
 * clips) — a heuristic, not Apple's line-exact layout, but page count and
 * margins track the original closely. `pageBreakBefore` (paragraph style)
 * forces a break; each later section starts a new page (Pages' section break
 * behavior). Sections with `columns` render as a CSS multicol block filling
 * column-by-column (column-fill: auto), measured at the column width and
 * packed with a stacked budget of columns × printable height.
 * Returns the page of each paragraph (for page-bottom footnotes).
 */
function paginatedBody(
  doc: PagesDocument,
  hdoc: HydratedDoc,
  ctx: ViewerCtx,
  view: HTMLElement,
): void {
  const g = pageGeom(doc)!;
  const body = doc.body!;

  // 1. render all paragraphs once, in order (list numbering is stateful).
  // A paragraph carrying a PAGE-SIZED inline group (template covers: 00C
  // Textbook stores the whole cover as one inline group) renders as a
  // full-bleed canvas layer on its own page instead of flowing inside the
  // printable area; its positioned inline siblings ride along.
  const pageSizedDrawables = (p: (typeof body.paragraphs)[number]): Drawable[] | null => {
    let pageSized = false;
    const positioned: Drawable[] = [];
    for (const it of p.items) {
      if (typeof it === "string" || !("type" in it) || it.type !== "inline-object") continue;
      const d = it.drawable;
      const sz = d.type !== "unknown" ? d.common?.size : undefined;
      if (!sz) continue;
      positioned.push(d);
      if (sz.width >= g.w * 0.85 && sz.height >= g.h * 0.85) pageSized = true;
    }
    return pageSized ? positioned : null;
  };
  const listState = newListNumberingState();
  const els: HTMLElement[] = [];
  const forceBreak: boolean[] = [];
  const fullBleed: (Drawable[] | null)[] = [];
  body.paragraphs.forEach((p, i) => {
    const fb = pageSizedDrawables(p);
    fullBleed.push(fb);
    if (fb) {
      els.push(document.createElement("div")); // zero-height stand-in
      forceBreak.push(true);
      return;
    }
    els.push(renderParagraph(p, hdoc, ctx, listState));
    forceBreak.push(!!paraStyleOf(hdoc, p.pStyle)?.pageBreakBefore || !!fullBleed[i - 1]);
  });

  // per-paragraph column spec from the sections' body ranges; a section
  // start (beyond paragraph 0) is a page break
  const spans = (doc.sections ?? [])
    .map((s) => ({
      start: s.bodyParagraphStart ?? 0,
      cols: s.columns && s.columns.count >= 2
        ? { count: s.columns.count, gapPt: s.columns.gutterPt ?? 36 }
        : null,
    }))
    .sort((a, b) => a.start - b.start);
  if (spans.length === 0 || spans[0].start > 0) spans.unshift({ start: 0, cols: null });
  const specOf: (ColSpec | null)[] = [];
  {
    let si = 0;
    for (let i = 0; i < els.length; i++) {
      while (si + 1 < spans.length && spans[si + 1].start <= i) si++;
      specOf.push(spans[si].cols);
      if (i > 0 && spans[si].start === i) forceBreak[i] = true;
    }
  }
  const colWidth = (c: ColSpec): number => (g.contentW - c.gapPt * (c.count - 1)) / c.count;

  // 2. measure offscreen: consecutive same-spec paragraphs share a wrapper at
  // their render width (flow = printable width, columns = column width);
  // transform scaling does not affect layout metrics, so these heights match
  // the page frames. Spec changes only happen at section starts, so every
  // segment after the first begins on a fresh page.
  interface Segment {
    start: number;
    spec: ColSpec | null;
    els: HTMLElement[];
    tops: number[];
    heights: number[];
  }
  const segments: Segment[] = [];
  const meas = document.createElement("div");
  meas.style.cssText = "position:absolute;visibility:hidden;left:-100000px;top:0;";
  {
    let i = 0;
    while (i < els.length) {
      let j = i;
      while (j < els.length && specOf[j] === specOf[i]) j++;
      const wrap = document.createElement("div");
      wrap.className = "pages-print";
      const spec = specOf[i];
      wrap.style.cssText =
        `position:relative;width:${spec ? colWidth(spec) : g.contentW}px;`;
      const seg: Segment = { start: i, spec, els: els.slice(i, j), tops: [], heights: [] };
      for (const el of seg.els) wrap.appendChild(el);
      meas.appendChild(wrap);
      segments.push(seg);
      i = j;
    }
    document.body.appendChild(meas);
    for (const seg of segments) {
      for (const el of seg.els) {
        seg.tops.push(el.offsetTop);
        seg.heights.push(el.offsetHeight);
      }
    }
    meas.remove();
  }

  // 3. pack into pages of blocks: page breaks where the cumulative bottom
  // exceeds the printable height (flow) or count × printable height
  // (column sections, stacked-column budget)
  const pages: PageBlock[][] = [[]];
  const pageOfPara: number[] = new Array(els.length).fill(0);
  const fullBleedByPage = new Map<number, Drawable[]>();
  const pageHasContent = () =>
    pages[pages.length - 1].some((b) => b.els.length > 0) ||
    fullBleedByPage.has(pages.length - 1);
  const newPage = () => pages.push([]);
  segments.forEach((seg, si) => {
    if (si > 0 && pageHasContent()) newPage();
    if (!seg.spec) {
      let pageTop = seg.tops[0] ?? 0;
      let blk: PageBlock | null = null;
      seg.els.forEach((el, j) => {
        const k = seg.start + j;
        const overflows = seg.tops[j] + seg.heights[j] - pageTop > g.contentH;
        if ((forceBreak[k] || overflows) && pageHasContent()) {
          newPage();
          pageTop = seg.tops[j];
          blk = null;
        }
        if (fullBleed[k]) {
          // full-bleed cover page: the drawables own the page; the zero-
          // height stand-in element stays out of the flow entirely
          fullBleedByPage.set(pages.length - 1, fullBleed[k]!);
          pageOfPara[k] = pages.length - 1;
          return;
        }
        if (!blk) {
          const page = pages[pages.length - 1];
          const last = page[page.length - 1];
          blk = last && !last.cols ? last : { cols: null, heightPx: 0, els: [] };
          if (blk !== last) page.push(blk);
        }
        blk.els.push(el);
        pageOfPara[k] = pages.length - 1;
      });
    } else {
      const cap = g.contentH * seg.spec.count;
      let colTop = seg.tops[0] ?? 0;
      let blk: PageBlock | null = null;
      seg.els.forEach((el, j) => {
        const k = seg.start + j;
        if (blk && seg.tops[j] + seg.heights[j] - colTop > cap) {
          newPage();
          colTop = seg.tops[j];
          blk = null;
        }
        if (!blk) {
          blk = { cols: seg.spec, heightPx: g.contentH, els: [] };
          pages[pages.length - 1].push(blk);
        }
        blk.els.push(el);
        pageOfPara[k] = pages.length - 1;
      });
    }
  });

  const pagesEls: PageBlock[][] = pages;

  // 4. page frames: printable area positioned at the margins; floating
  // drawables anchored to page i render into the same canvas
  const floatingByPage = new Map<number, Drawable[]>();
  let maxFloatPage = -1;
  for (const gr of doc.floating) {
    const idx = gr.pageIndex ?? 0;
    floatingByPage.set(idx, [...(floatingByPage.get(idx) ?? []), ...gr.drawables]);
    if (idx > maxFloatPage) maxFloatPage = idx;
  }
  const pageCount = Math.max(pagesEls.length, maxFloatPage + 1);

  // Footnotes render at the bottom of their anchor's page (Apple's default);
  // an explicit endnotes placement keeps the end-of-document section instead
  // (handled by the caller).
  const footnotesByPage = new Map<number, { text: StyledText; n: number }[]>();
  if (doc.footnotes?.length && !doc.footnotePlacement) {
    doc.footnotes.forEach((fn, i) => {
      const pg =
        pageOfPara[Math.min(fn.anchorParagraphIndex, pageOfPara.length - 1)] ?? 0;
      const list = footnotesByPage.get(pg) ?? [];
      list.push({ text: fn.text, n: i + 1 });
      footnotesByPage.set(pg, list);
    });
  }

  const scale = pageDisplayWidth / g.w;
  for (let i = 0; i < pageCount; i++) {
    const frame = document.createElement("div");
    frame.className = "canvas-frame pages-page pages-wp-page";
    frame.style.aspectRatio = `${g.w} / ${g.h}`;
    frame.style.height = `${g.h * scale}px`;
    // clip at the paper edge like Pages (full-bleed covers store images
    // taller than the page: 00C's is 1155pt on a 1024pt page)
    frame.style.overflow = "hidden";
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
    // full-bleed cover drawables paint at page coordinates (not inset)
    for (const d of fullBleedByPage.get(i) ?? []) {
      inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));
    }

    const content = document.createElement("div");
    content.className = "pages-print";
    content.style.position = "absolute";
    content.style.left = `${g.left}px`;
    content.style.top = `${g.top}px`;
    content.style.width = `${g.contentW}px`;
    content.style.height = `${g.contentH}px`;
    for (const blk of pagesEls[i] ?? []) {
      if (!blk.cols) {
        for (const el of blk.els) content.appendChild(el);
      } else {
        const cols = document.createElement("div");
        cols.className = "pages-cols";
        cols.style.columnCount = String(blk.cols.count);
        cols.style.columnGap = `${blk.cols.gapPt}px`;
        cols.style.height = `${blk.heightPx}px`;
        for (const el of blk.els) cols.appendChild(el);
        content.appendChild(cols);
      }
    }
    inner.appendChild(content);

    // page-bottom footnotes, pinned inside the printable area
    const pageFns = footnotesByPage.get(i);
    if (pageFns) {
      const area = document.createElement("div");
      area.className = "pages-footnote-area pages-print";
      area.style.left = `${g.left}px`;
      area.style.width = `${g.contentW}px`;
      area.style.bottom = `${g.h - g.top - g.contentH}px`;
      for (const { text, n } of pageFns) {
        const row = document.createElement("div");
        row.className = "footnote";
        const body = renderStyledText(text, hdoc, ctx);
        // the footnote body's own leading mark field (Pages stores one)
        // carries the footnote's number; add one if the body has none
        const marks = body.querySelectorAll<HTMLElement>(
          '.field[data-field-kind="footnote-mark"]',
        );
        if (marks.length > 0) {
          marks.forEach((m) => (m.textContent = String(n)));
        } else {
          const mark = document.createElement("span");
          mark.className = "mark";
          mark.textContent = `${n}`;
          row.appendChild(mark);
        }
        row.appendChild(body);
        area.appendChild(row);
      }
      inner.appendChild(area);
    }

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

  // body footnote marks number sequentially through the document (Apple
  // shows superscript indexes; the converter stores only the mark position).
  // Marks inside the page-bottom footnote areas already carry their number.
  let fnIndex = 0;
  view.querySelectorAll<HTMLElement>('.field[data-field-kind="footnote-mark"]').forEach((el) => {
    if (el.closest(".pages-footnote-area")) return;
    fnIndex += 1;
    el.textContent = String(fnIndex);
  });
}

/** Display width for a page frame: 720px logical, or the container width on
 *  narrow screens so pages default to full width on mobile. Set per render
 *  from the mount; pagination itself measures in page POINTS regardless. */
let pageDisplayWidth = 720;

export function renderPages(doc: PagesDocument, hdoc: HydratedDoc, ctx: ViewerCtx, mount: HTMLElement): void {
  pageDisplayWidth = Math.max(280, Math.min(720, mount.clientWidth || 720));
  const view = document.createElement("div");
  view.id = "pages-view";

  const wordProcessing = doc.flavor === "word-processing";
  const geom = pageGeom(doc);

  if (wordProcessing && doc.body && geom) {
    // paginated word-processing render: page frames + margins. Footnotes
    // render at their anchor pages' bottoms unless the document asks for
    // endnotes (footnotePlacement: section/document endnotes).
    paginatedBody(doc, hdoc, ctx, view);
    if (doc.footnotePlacement) appendFootnotes(doc, hdoc, ctx, view);
    mount.appendChild(view);
    applyTextFit(view);
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
  // measurement pass (attached): bounded shrink absorbs font-metric drift
  applyTextFit(view);
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
