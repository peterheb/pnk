// StyledText -> DOM. Paragraphs become <p> (or <h1>-<h5> when the hydrated
// paragraph style's outlineLevel says heading); runs resolve their char style
// through the document pool (charStyleIndex); smart fields render their
// stored value; inline attachments recurse into the drawable renderer.
import type {
  CharStyle,
  ParaStyle,
  Paragraph,
  ParagraphItem,
  StyledText,
  TextRun,
} from "../../model/src/shared";
import type { ViewerCtx } from "./ctx";
import { renderFlowDrawable } from "./drawables";
import { charStyleOf, paraStyleOf, type HydratedDoc } from "./hydrate";

export function applyCharStyle(el: HTMLElement, cs: CharStyle | undefined): void {
  if (!cs) return;
  const s = el.style;
  if (cs.fontName) s.fontFamily = `"${cs.fontName}", sans-serif`;
  if (cs.fontSizePt) s.fontSize = `${cs.fontSizePt}pt`;
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
  if (cs.baselineShiftPt) s.verticalAlign = `${cs.baselineShiftPt}pt`;
  if (cs.trackingPt) s.letterSpacing = `${cs.trackingPt}pt`;
  if (cs.fontColor) s.color = cs.fontColor;
  if (cs.backgroundColor) s.backgroundColor = cs.backgroundColor;
}

export function applyParaStyle(el: HTMLElement, ps: ParaStyle): void {
  const s = el.style;
  const align = ps.horizontalAlignment;
  if (align === "center" || align === "right" || align === "justify") s.textAlign = align;
  if (ps.leftIndentPt) s.marginLeft = `${ps.leftIndentPt}pt`;
  if (ps.rightIndentPt) s.marginRight = `${ps.rightIndentPt}pt`;
  if (ps.firstLineIndentPt) s.textIndent = `${ps.firstLineIndentPt}pt`;
  if (ps.spaceBeforePt) s.marginTop = `${ps.spaceBeforePt}pt`;
  if (ps.spaceAfterPt) s.marginBottom = `${ps.spaceAfterPt}pt`;
  if (ps.lineSpacingMultiple) s.lineHeight = String(ps.lineSpacingMultiple);
  else if (ps.lineSpacingExactPt) s.lineHeight = `${ps.lineSpacingExactPt}pt`;
  if (ps.backgroundColor) s.backgroundColor = ps.backgroundColor;
  if (ps.border) {
    const b = ps.border;
    s.border = `${b.widthPt}pt ${b.dash?.length ? "dashed" : "solid"} ${b.color}`;
  }
  if (ps.writingDirection === "right-to-left") s.direction = "rtl";
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
    // numbering: same list key as the previous paragraph continues its
    // counter; a new key (or an explicit start) restarts it
    const key = `${list!.level}:${list!.markerKind}:${list!.markerKind === "number" ? list!.numberKind : list!.markerText}`;
    if (listState.lastKey !== key) {
      listState.counters.set(key, list!.start ?? 1);
      listState.lastKey = key;
    } else {
      listState.counters.set(key, (listState.counters.get(key) ?? 0) + 1);
    }
    const n = listState.counters.get(key) ?? 1;
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
    const marker = document.createElement("span");
    marker.className = "list-marker";
    marker.textContent = markerText;
    marker.style.minWidth = `${Math.max(16, (list!.markerIndentPt ?? 0) + 12)}px`;
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
      // inline attachment (U+FFFC): render the embedded drawable in-flow
      el.appendChild(renderFlowDrawable(item.drawable, doc, ctx));
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
