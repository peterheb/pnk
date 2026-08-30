// StyledText -> DOM. Paragraphs become <p> (or <h1>-<h5> when the hydrated
// paragraph style's outlineLevel says heading); runs resolve their char style
// through the document pool (charStyleIndex); smart fields render their
// stored value; inline attachments recurse into the drawable renderer.
import type {
  CharStyle,
  ImageDrawable,
  ParaStyle,
  Paragraph,
  ParagraphItem,
  StyledText,
  TextRun,
} from "../../model/src/shared";
import type { ViewerCtx } from "./ctx";
import { renderFlowDrawable } from "./drawables";
import { charStyleOf, paraStyleOf, type HydratedDoc } from "./hydrate";

// Unit convention: canvases (slides, sheets, pages) size their inner frame at
// 1 CSS px per document POINT, so all point-valued style properties emit `px`
// numerically equal to the pt value. Emitting CSS `pt` here would inflate
// text 4/3 relative to the page geometry (browsers map 1pt = 4/3px) — the
// KVKK fixture paginated a 1-page doc onto 2 pages that way.
export function applyCharStyle(el: HTMLElement, cs: CharStyle | undefined): void {
  if (!cs) return;
  const s = el.style;
  if (cs.fontName) s.fontFamily = `"${cs.fontName}", sans-serif`;
  if (cs.fontSizePt) s.fontSize = `${cs.fontSizePt}px`;
  if (cs.bold) s.fontWeight = "700";
  if (cs.italic) s.fontStyle = "italic";
  if (cs.underline && cs.underline !== "none") s.textDecorationLine = "underline";
  if (cs.strikethrough && cs.strikethrough !== "none") {
    s.textDecorationLine = `${s.textDecorationLine === "underline" ? "underline " : ""}line-through`;
  }
  if (cs.capitalization === "all-caps") s.textTransform = "uppercase";
  else if (cs.capitalization === "small-caps") s.fontVariant = "small-caps";
  else if (cs.capitalization === "title") s.textTransform = "capitalize";
  if (cs.baseline === "superscript") s.verticalAlign = "super";
  else if (cs.baseline === "subscript") s.verticalAlign = "sub";
  if (cs.baselineShiftPt) s.verticalAlign = `${cs.baselineShiftPt}px`;
  if (cs.trackingPt) s.letterSpacing = `${cs.trackingPt}px`;
  if (cs.fontColor) s.color = cs.fontColor;
  if (cs.backgroundColor) s.backgroundColor = cs.backgroundColor;
}

export function applyParaStyle(el: HTMLElement, ps: ParaStyle): void {
  const s = el.style;
  const align = ps.horizontalAlignment;
  if (align === "center" || align === "right" || align === "justify") s.textAlign = align;
  // TSWP indents: first_line_indent is ABSOLUTE from the margin while
  // left_indent applies to continuation lines (G5 fixture: styles storing
  // left=36/first=0 render flush first lines in Apple's export; a hanging
  // style stores left=72/first=36). CSS text-indent is RELATIVE to
  // margin-left, so emit first - left; absent first means 0 (flush).
  const leftIndent = ps.leftIndentPt ?? 0;
  const firstIndent = ps.firstLineIndentPt ?? 0;
  if (leftIndent) s.marginLeft = `${leftIndent}px`;
  if (ps.rightIndentPt) s.marginRight = `${ps.rightIndentPt}px`;
  if (firstIndent - leftIndent) s.textIndent = `${firstIndent - leftIndent}px`;
  if (ps.spaceBeforePt) s.marginTop = `${ps.spaceBeforePt}px`;
  if (ps.spaceAfterPt) s.marginBottom = `${ps.spaceAfterPt}px`;
  if (ps.lineSpacingMultiple) s.lineHeight = String(ps.lineSpacingMultiple);
  else if (ps.lineSpacingExactPt) s.lineHeight = `${ps.lineSpacingExactPt}px`;
  if (ps.backgroundColor) s.backgroundColor = ps.backgroundColor;
  if (ps.border) {
    const b = ps.border;
    s.border = `${b.widthPt}px ${b.dash?.length ? "dashed" : "solid"} ${b.color}`;
  }
  if (ps.writingDirection === "right-to-left") s.direction = "rtl";
  // tabs render via white-space: pre-wrap (set in CSS for print areas);
  // tab-size approximates the default tab stop interval. Positioned
  // center/right/decimal stops are not modeled in CSS — heuristic only.
  if (ps.defaultTabStopPt) (s as CSSStyleDeclaration & { tabSize: string }).tabSize = `${ps.defaultTabStopPt}px`;
}

function fieldPlaceholderText(item: Extract<ParagraphItem, { type: "field" }>): string {
  switch (item.field.kind) {
    case "page-number": return "‹page number›";
    case "page-count": return "‹page count›";
    case "footnote-mark": return "†";
    case "date": return "‹date›";
    case "other": return item.field.detail ? `‹${item.field.detail}›` : "‹field›";
  }
}

/** Numbering counters shared by a consecutive run of list paragraphs. */
export interface ListNumberingState {
  counters: Map<string, number>;
  lastKey: string | null;
}

export function newListNumberingState(): ListNumberingState {
  return { counters: new Map(), lastKey: null };
}

function toRoman(n: number, upper: boolean): string {
  const pairs: [number, string][] = [[1000, "m"], [900, "cm"], [500, "d"], [400, "cd"], [100, "c"], [90, "xc"], [50, "l"], [40, "xl"], [10, "x"], [9, "ix"], [5, "v"], [4, "iv"], [1, "i"]];
  let out = "";
  let rest = n;
  for (const [v, sym] of pairs) while (rest >= v) { out += sym; rest -= v; }
  return upper ? out.toUpperCase() : out;
}

/** Marker text for a numbered list position. */
function numberMarker(n: number, kind: string | undefined): string {
  switch (kind) {
    case "alpha-upper": return `${toAlpha(n, true)}.`;
    case "alpha-lower": return `${toAlpha(n, false)}.`;
    case "roman-upper": return `${toRoman(n, true)}.`;
    case "roman-lower": return `${toRoman(n, false)}.`;
    default: return `${n}.`;
  }
}

function toAlpha(n: number, upper: boolean): string {
  let out = "";
  let rest = n;
  while (rest > 0) {
    rest -= 1;
    out = String.fromCharCode((upper ? 65 : 97) + (rest % 26)) + out;
    rest = Math.floor(rest / 26);
  }
  return out;
}

/**
 * One paragraph, styled from the hydrated pools. Headings by outlineLevel;
 * list membership renders a marker (• / 1. …) with restart-aware numbering
 * tracked in the shared ListNumberingState.
 */
export function renderParagraph(
  p: Paragraph,
  doc: HydratedDoc,
  ctx: ViewerCtx,
  listState: ListNumberingState = newListNumberingState(),
): HTMLElement {
  const style = paraStyleOf(doc, p.pStyle);
  const list = style?.list;
  const hasMarker = !!list && list.markerKind !== "none" &&
    (list.markerKind === "string" ? !!list.markerText : list.markerKind === "number");

  const level = style?.outlineLevel ?? 0;
  const el = level >= 1 && level <= 5
    ? document.createElement(`h${level}`)
    : document.createElement("p");

  if (!hasMarker) {
    listState.lastKey = null;
    if (style) applyParaStyle(el, style);
  } else {
    // numbering: the stored restart flag (surfaced as list.start on the
    // paragraph's pooled style) resets the counter; otherwise numbering
    // CONTINUES the counter for this key — even across intervening
    // paragraphs or nested levels, which is Pages' own "continue from
    // previous" semantics (G5: "Four (Numbered, continued)" resumes 4 after
    // a nested run; "Restart One" carries start=1).
    const key = `${list!.level}:${list!.markerKind}:${list!.markerKind === "number" ? list!.numberKind : list!.markerText}`;
    const n = list!.start !== undefined
      ? list!.start
      : (listState.counters.get(key) ?? 0) + 1;
    listState.counters.set(key, n);
    listState.lastKey = key;
    const markerText = list!.markerKind === "number"
      ? numberMarker(n, list!.numberKind)
      : (list!.markerText ?? "•");

    // marker hangs in a flex row; paragraph margins live on the wrapper
    const wrap = document.createElement("div");
    wrap.className = "list-item";
    if (style) {
      applyParaStyle(wrap, style);
      el.style.marginTop = "0";
      el.style.marginBottom = "0";
      el.style.marginLeft = "0";
    }
    // Nesting: the marker indent (absolute, per level) shifts the whole
    // row when the paragraph style itself has no left indent — G5's nested
    // bullets step 9/18pt per level, numbered 18/36/54pt.
    if (!style?.leftIndentPt && list!.markerIndentPt) {
      wrap.style.marginLeft = `${list!.markerIndentPt}px`;
    }
    const marker = document.createElement("span");
    marker.className = "list-marker";
    marker.textContent = markerText;
    marker.style.minWidth = "18px";
    wrap.appendChild(marker);
    wrap.appendChild(el);
    renderParagraphContent(el, p, doc, ctx);
    return wrap;
  }

  renderParagraphContent(el, p, doc, ctx);
  return el;
}

/** Items of a paragraph into the given element. */
function renderParagraphContent(el: HTMLElement, p: Paragraph, doc: HydratedDoc, ctx: ViewerCtx): void {
  for (const item of p.items) {
    // bare string = plain unstyled run; object = styled/typed run
    if (typeof item === "string") {
      appendRunText(el, item, undefined);
    } else if ("type" in item && item.type === "field") {
      const span = document.createElement("span");
      span.className = "field";
      span.dataset.fieldKind = item.field.kind;
      span.textContent = item.value ?? fieldPlaceholderText(item);
      applyCharStyle(span, charStyleOf(doc, item.cStyle));
      el.appendChild(span);
    } else if ("type" in item && item.type === "inline-object") {
      // U+FFFC inline attachment: images flow WITH the sentence (Apple
      // renders them mid-text, baseline-ish); block drawables (tables)
      // keep the flow renderer
      const d = item.drawable;
      if (d.type === "image") {
        el.appendChild(inlineImageEl(d, ctx));
      } else {
        el.appendChild(renderFlowDrawable(d, doc, ctx));
      }
    } else {
      const run = item as TextRun;
      const span = document.createElement(run.hyperlink ? "a" : "span");
      applyCharStyle(span, charStyleOf(doc, run.cStyle));
      if (run.hyperlink) {
        (span as HTMLAnchorElement).href = run.hyperlink;
        (span as HTMLAnchorElement).target = "_blank";
        (span as HTMLAnchorElement).rel = "noopener";
      }
      appendRunText(span, run.text, span);
      el.appendChild(span);
    }
  }
}

/** An inline-attachment image: flows with the sentence, baseline-aligned. */
function inlineImageEl(d: ImageDrawable, ctx: ViewerCtx): HTMLElement {
  const url = ctx.url(d.image.dataId);
  const size = d.common?.size;
  if (!url) {
    const miss = document.createElement("span");
    miss.className = "media-missing";
    miss.textContent = d.image.preferredFileName ?? d.image.fileName ?? "inline image (media missing)";
    return miss;
  }
  const img = document.createElement("img");
  img.src = url;
  img.alt = d.image.preferredFileName ?? d.image.fileName ?? "inline image";
  if (size) {
    img.style.width = `${size.width}px`;
    img.style.height = `${size.height}px`;
  }
  img.style.verticalAlign = "text-bottom";
  img.className = "inline-image";
  return img;
}

/**
 * Run text into the given parent: soft line breaks (U+2028 paragraph
 * separator / U+2029 line separator) are visible breaks in iWork but not in
 * HTML text, so they split into nodes joined by <br>.
 */
function appendRunText(parent: HTMLElement, text: string, styleHost: HTMLElement | undefined): void {
  const parts = text.split(/[\u2028\u2029]/);
  parts.forEach((part, i) => {
    if (i > 0) parent.appendChild(document.createElement("br"));
    if (part) parent.appendChild(document.createTextNode(part));
  });
  void styleHost;
}

/** A whole text block (body, notes, cell rich text…). */
export function renderStyledText(t: StyledText | undefined, doc: HydratedDoc, ctx: ViewerCtx): HTMLElement {
  const div = document.createElement("div");
  div.className = "styled-text";
  const listState = newListNumberingState();
  if (t) for (const p of t.paragraphs) div.appendChild(renderParagraph(p, doc, ctx, listState));
  return div;
}
