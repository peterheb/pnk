// Pages (.pages): word-processing flavor = flowing body paginated into
// page-shaped frames (page size + margins from the document archive);
// page-layout flavor = stacked positioned page canvases. Floating objects
// render into their anchor page's canvas.

import type { PagesDocument, PageTemplate } from "../../model/src/pages";
import type { ViewerCtx } from "./ctx";
import type { Drawable, DrawableCommon, Paragraph, StyledText } from "../../model/src/shared";

type TextWrap = NonNullable<DrawableCommon["textWrap"]>;
import { newListNumberingState, renderParagraph, renderStyledText } from "./text";
import { applyTextFit, fillToCss, renderCanvasDrawable } from "./drawables";
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
function templateCandidates(
  doc: PagesDocument,
  sec: PagesDocument["sections"][number] | undefined,
  i: number,
): (PageTemplate | undefined)[] {
  if (!sec) return [];
  const byName = new Map<string, PageTemplate>();
  for (const t of doc.pageTemplates) if (t.name) byName.set(t.name, t);
  const first = sec.firstPageTemplate ? byName.get(sec.firstPageTemplate) : undefined;
  const even = sec.evenPageTemplate ? byName.get(sec.evenPageTemplate) : undefined;
  const odd = sec.oddPageTemplate ? byName.get(sec.oddPageTemplate) : undefined;
  const parity = (i + 1) % 2 === 0 ? [even, odd] : [odd, even];
  return i === 0 ? [first, ...parity] : parity;
}

/** `i` = 0-based page index WITHIN the section. */
function hfTemplateFor(
  doc: PagesDocument,
  sec: PagesDocument["sections"][number] | undefined,
  i: number,
): PageTemplate | undefined {
  const candidates = templateCandidates(doc, sec, i);
  if (i === 0 && candidates[0]?.hideHeadersFooters) return undefined;
  for (const c of candidates) if (templateHasHf(c)) return c;
  return undefined;
}

/** Template furniture (master drawables) under a section's page `i`: the
 *  first/parity template that carries any, same fallback as headers
 *  (b31db822: a Pages 5.x doc keeps its rotated "DRAFT" watermark on the
 *  odd master only, and Apple paints it on every page). */
function templateDrawablesFor(
  doc: PagesDocument,
  sec: PagesDocument["sections"][number] | undefined,
  i: number,
): Drawable[] {
  for (const c of templateCandidates(doc, sec, i)) if (c && c.drawables.length) return c.drawables;
  return [];
}

/** A "Move with Text" object split out of its paragraph (InlineObjectRun.anchored). */
interface Anchor {
  drawable: Drawable;
  hPt: number;
  vPt: number;
  wrap: TextWrap | undefined;
}

/**
 * The float that stands in for a paragraph's anchored objects. Apple places
 * each at (text-area left + hPt, anchor paragraph top + vPt) and wraps the
 * body around it per its exterior wrap; here one float per paragraph spans
 * the union of the wrapping objects' boxes (+ wrap margin) on the side the
 * wrap names, and `shape-outside: inset(...)` keeps the gap between the
 * paragraph top and the first object open for text. Objects that do not
 * wrap ride along in a 0×0 float — positioned, no exclusion. The drawables
 * themselves paint absolutely inside the float at their offsets.
 */
function anchorFloat(anchors: Anchor[], hdoc: HydratedDoc, ctx: ViewerCtx, contentW: number): HTMLElement {
  const fl = document.createElement("div");
  fl.className = "pages-anchor";
  const boxes = anchors.map((a) => {
    const sz = a.drawable.type !== "unknown" ? a.drawable.common?.size : undefined;
    return { a, x: a.hPt, y: a.vPt, w: sz?.width ?? 100, h: sz?.height ?? 100 };
  });
  const wrapping = boxes.filter((b) => b.a.wrap && b.a.wrap.kind !== "none");
  let side: "left" | "right" = "left";
  let width = 0;
  let height = 0;
  let insetTop = 0;
  if (wrapping.length) {
    const m = (b: (typeof boxes)[number]) => Math.max(0, b.a.wrap?.marginPt ?? 0);
    const ux0 = Math.min(...wrapping.map((b) => b.x - m(b)));
    const ux1 = Math.max(...wrapping.map((b) => b.x + b.w + m(b)));
    const uy0 = Math.min(...wrapping.map((b) => b.y - m(b)));
    const uy1 = Math.max(...wrapping.map((b) => b.y + b.h + m(b)));
    const kinds = new Set(wrapping.map((b) => b.a.wrap!.kind));
    // wrap "right" = text on the right (object left); "left" = the reverse;
    // largest/around = whichever side has more room; above-below = full width
    if (kinds.has("above-below")) {
      side = "left";
      width = contentW;
    } else if (kinds.has("right") && !kinds.has("left")) {
      side = "left";
      width = ux1;
    } else if (kinds.has("left") && !kinds.has("right")) {
      side = "right";
      width = contentW - ux0;
    } else {
      side = (ux0 + ux1) / 2 < contentW / 2 ? "left" : "right";
      width = side === "left" ? ux1 : contentW - ux0;
    }
    width = Math.max(0, Math.min(contentW, width));
    insetTop = Math.max(0, uy0);
    height = Math.max(0, uy1);
  }
  fl.style.cssFloat = side;
  fl.style.width = `${width.toFixed(2)}px`;
  fl.style.height = `${height.toFixed(2)}px`;
  if (height > 0 && insetTop > 0) fl.style.shapeOutside = `inset(${insetTop.toFixed(2)}px 0 0 0)`;
  const floatLeft = side === "left" ? 0 : contentW - width;
  for (const b of boxes) {
    const el = renderCanvasDrawable(b.a.drawable, hdoc, ctx);
    el.style.left = `${(b.x - floatLeft).toFixed(2)}px`;
    el.style.top = `${b.y.toFixed(2)}px`;
    fl.appendChild(el);
  }
  return fl;
}

/** Re-pin every anchor float to its paragraph's top (see paginatedBody). */
function fixAnchorDrift(root: HTMLElement): void {
  root.querySelectorAll<HTMLElement>(".pages-anchor").forEach((fl) => {
    const p = fl.nextElementSibling as HTMLElement | null;
    if (!p) return;
    const drift = fl.offsetTop - p.offsetTop;
    if (Math.abs(drift) < 0.5) return;
    const current = parseFloat(fl.style.marginTop || "0") || 0;
    fl.style.marginTop = `${(current - drift).toFixed(2)}px`;
  });
}

/** Split a paragraph's anchored objects out of its item list. */
function splitAnchors(p: Paragraph): { para: Paragraph; anchors: Anchor[] } {
  const anchors: Anchor[] = [];
  const items = p.items.filter((it) => {
    if (typeof it === "string" || !("type" in it) || it.type !== "inline-object" || !it.anchored) return true;
    anchors.push({
      drawable: it.drawable,
      hPt: it.offset?.hPt ?? 0,
      vPt: it.offset?.vPt ?? 0,
      wrap: it.drawable.type !== "unknown" ? it.drawable.common?.textWrap : undefined,
    });
    return false;
  });
  return anchors.length ? { para: { ...p, items }, anchors } : { para: p, anchors };
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
  /** Paragraph index of each element in `els`. */
  paras: number[];
  /** Flow pages: the printable-area container the paragraphs were paginated
   *  INTO (page-sized, a block formatting context) — reused as the page's
   *  content element so layout is what was measured. */
  container?: HTMLElement;
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
  // "Move with Text" objects leave the line and become a float that
  // precedes their anchor paragraph wherever it lands (see anchorFloat)
  const anchorEls = new Map<number, HTMLElement>();
  // paragraphs that carried ONLY anchors (b31db822's cover: two empty
  // paragraphs holding the bubble image, the title shape and the logos)
  const anchorOnly = new Set<number>();
  body.paragraphs.forEach((p0, i) => {
    const { para: p, anchors } = splitAnchors(p0);
    if (anchors.length) {
      anchorEls.set(i, anchorFloat(anchors, hdoc, ctx, g.contentW));
      const visible = p.items.some((it) =>
        typeof it === "string" ? it.length > 0 : "type" in it ? true : it.text.length > 0,
      );
      if (!visible) anchorOnly.add(i);
    }
    const fb = pageSizedDrawables(p);
    fullBleed.push(fb);
    if (fb) {
      els.push(document.createElement("div")); // zero-height stand-in
      forceBreak.push(true);
      return;
    }
    els.push(renderParagraph(p, hdoc, ctx, listState));
    forceBreak.push(
      !!p.pageBreakBefore || !!paraStyleOf(hdoc, p.pStyle)?.pageBreakBefore || !!fullBleed[i - 1],
    );
  });
  /** A paragraph into a container, its anchor float first. */
  const place = (container: HTMLElement, k: number, el: HTMLElement) => {
    const fl = anchorEls.get(k);
    if (fl) container.appendChild(fl);
    container.appendChild(el);
  };

  // per-paragraph column spec from the sections' body ranges; a section
  // start (beyond paragraph 0) is a page break
  const spans = (doc.sections ?? [])
    .map((s, si) => ({
      start: s.bodyParagraphStart ?? 0,
      section: si as number | undefined,
      cols: s.columns && s.columns.count >= 2
        ? { count: s.columns.count, gapPt: s.columns.gutterPt ?? 36 }
        : null,
    }))
    .sort((a, b) => a.start - b.start);
  if (spans.length === 0 || spans[0].start > 0) spans.unshift({ start: 0, section: undefined, cols: null });
  const specOf: (ColSpec | null)[] = [];
  const sectionOfPara: (number | undefined)[] = [];
  {
    let si = 0;
    for (let i = 0; i < els.length; i++) {
      while (si + 1 < spans.length && spans[si + 1].start <= i) si++;
      specOf.push(spans[si].cols);
      sectionOfPara.push(spans[si].section);
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
    /** Bottom of the paragraph's anchor float, when it has one. */
    floatBottoms: (number | undefined)[];
  }
  const segments: Segment[] = [];
  const meas = document.createElement("div");
  meas.style.cssText = "position:absolute;visibility:hidden;left:-100000px;top:0;";
  // An anchor-only paragraph must stay where its anchors are: its empty
  // LINE box would be pushed under its own float (a cover image wider than
  // the column), dragging the next anchors a page down. A block with a
  // fixed height and no line box ignores floats — measure the natural line
  // height first, then freeze it.
  if (anchorOnly.size) {
    const probe = document.createElement("div");
    probe.className = "pages-print";
    probe.style.cssText = `position:relative;width:${g.contentW}px;`;
    for (const k of anchorOnly) probe.appendChild(els[k]);
    meas.appendChild(probe);
    document.body.appendChild(meas);
    const natural = new Map<number, number>();
    for (const k of anchorOnly) natural.set(k, els[k].offsetHeight);
    meas.remove();
    probe.remove();
    for (const k of anchorOnly) {
      const el = els[k];
      el.textContent = "";
      el.style.height = `${natural.get(k) ?? 0}px`;
      el.style.lineHeight = "0";
    }
  }
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
      const seg: Segment = { start: i, spec, els: els.slice(i, j), tops: [], heights: [], floatBottoms: [] };
      // flow segments paginate incrementally (step 3); only column
      // segments are pre-measured here, at their column width
      if (spec) {
        seg.els.forEach((el, k) => place(wrap, i + k, el));
        meas.appendChild(wrap);
      }
      segments.push(seg);
      i = j;
    }
    document.body.appendChild(meas);
    for (const seg of segments) {
      if (!seg.spec) continue;
      seg.els.forEach((el) => {
        seg.tops.push(el.offsetTop);
        seg.heights.push(el.offsetHeight);
        seg.floatBottoms.push(undefined);
      });
    }
    meas.remove();
  }

  /** A page-sized printable-area container (the page's content element). */
  const newPageContent = (): HTMLElement => {
    const content = document.createElement("div");
    content.className = "pages-print";
    content.style.position = "absolute";
    content.style.left = `${g.left}px`;
    content.style.top = `${g.top}px`;
    content.style.width = `${g.contentW}px`;
    content.style.height = `${g.contentH}px`;
    return content;
  };

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
  // Flow pagination is INCREMENTAL, in a live page-sized container: each
  // paragraph (with its anchor float) is appended and the page is read
  // back — so a page only ever sees its own floats. Measuring the whole
  // body in one tall flow let cover-page floats push the next page's
  // lines down (b31db822: the TOC paragraphs measured 700pt tall under the
  // cover image's exclusion and broke after two lines).
  document.body.appendChild(meas);
  const marginBelow = g.h - g.top - g.contentH;
  segments.forEach((seg, si) => {
    if (si > 0 && pageHasContent()) newPage();
    if (!seg.spec) {
      let blk: PageBlock | null = null;
      const startBlock = () => {
        const container = newPageContent();
        meas.appendChild(container);
        const b: PageBlock = { cols: null, heightPx: g.contentH, els: [], paras: [], container };
        pages[pages.length - 1].push(b);
        return b;
      };
      const currentBlock = (): PageBlock => {
        const page = pages[pages.length - 1];
        const last = page[page.length - 1];
        return last && !last.cols && last.container ? last : startBlock();
      };
      seg.els.forEach((el, j) => {
        const k = seg.start + j;
        if (forceBreak[k] && pageHasContent()) {
          newPage();
          blk = null;
        }
        if (fullBleed[k]) {
          // full-bleed cover page: the drawables own the page; the zero-
          // height stand-in element stays out of the flow entirely
          fullBleedByPage.set(pages.length - 1, fullBleed[k]!);
          pageOfPara[k] = pages.length - 1;
          return;
        }
        if (!blk) blk = currentBlock();
        const fl = anchorEls.get(k);
        const tryPlace = (b: PageBlock): boolean => {
          place(b.container!, k, el);
          // floats never overlap: a later full-width float lands BELOW an
          // earlier one — pin it back to its paragraph (exclusion lost lies
          // inside the earlier float's anyway)
          if (fl) fixAnchorDrift(b.container!);
          const bottom = el.offsetTop + el.offsetHeight;
          // the text must fit the printable area; an anchor float may hang
          // into the bottom margin (b31db822's cover logos do, by 27pt)
          const fb = fl ? fl.offsetTop + fl.offsetHeight : 0;
          return bottom <= g.contentH + 0.5 && fb <= g.contentH + marginBelow + 0.5;
        };
        if (!tryPlace(blk) && pageHasContent()) {
          el.remove();
          fl?.remove();
          newPage();
          blk = startBlock();
          tryPlace(blk); // an oversized paragraph keeps its own page, clipped
        }
        blk.els.push(el);
        blk.paras.push(k);
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
          blk = { cols: seg.spec, heightPx: g.contentH, els: [], paras: [] };
          pages[pages.length - 1].push(blk);
        }
        blk.els.push(el);
        blk.paras.push(k);
        pageOfPara[k] = pages.length - 1;
      });
    }
  });

  meas.remove();
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

  // section of each page: that of its first paragraph, else carried over;
  // page index within the section drives first/odd/even template choice
  const sectionOfPage: (number | undefined)[] = [];
  const pageInSection: number[] = [];
  {
    let cur: number | undefined = sectionOfPara[0];
    let n = 0;
    for (let i = 0; i < pageCount; i++) {
      const firstPara = pagesEls[i]?.find((b) => b.paras.length)?.paras[0];
      const sec = firstPara !== undefined ? sectionOfPara[firstPara] : cur;
      if (sec !== cur) n = 0;
      cur = sec;
      sectionOfPage.push(cur);
      pageInSection.push(n);
      n += 1;
    }
  }

  const scale = pageDisplayWidth / g.w;
  for (let i = 0; i < pageCount; i++) {
    const sec = sectionOfPage[i] !== undefined ? doc.sections[sectionOfPage[i]!] : doc.sections[0];
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
    // section background (10a06959: a workbook whose sections are solid
    // blue / teal / orange pages)
    const bg = fillToCss(sec?.backgroundFill);
    if (bg) inner.style.background = bg;

    // template furniture, then floating drawables: both behind the body text
    for (const d of templateDrawablesFor(doc, sec, pageInSection[i])) {
      inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));
    }
    for (const d of floatingByPage.get(i) ?? []) {
      inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));
    }
    // full-bleed cover drawables paint at page coordinates (not inset)
    for (const d of fullBleedByPage.get(i) ?? []) {
      inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));
    }

    const live = (pagesEls[i] ?? []).find((b) => b.container)?.container;
    const content = live ?? newPageContent();
    for (const blk of pagesEls[i] ?? []) {
      if (blk.container) continue; // paragraphs already live in it
      if (!blk.cols) {
        blk.els.forEach((el, k) => place(content, blk.paras[k], el));
      } else {
        const cols = document.createElement("div");
        cols.className = "pages-cols";
        cols.style.columnCount = String(blk.cols.count);
        cols.style.columnGap = `${blk.cols.gapPt}px`;
        cols.style.height = `${blk.heightPx}px`;
        blk.els.forEach((el, k) => place(cols, blk.paras[k], el));
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
    const hf = hfTemplateFor(doc, sec, pageInSection[i]);
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
    // page containers lay the floats out afresh: re-pin them once attached
    fixAnchorDrift(view);
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
