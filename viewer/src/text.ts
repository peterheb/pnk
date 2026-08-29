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

/** One paragraph, styled from the hydrated pools. Headings by outlineLevel. */
export function renderParagraph(p: Paragraph, doc: HydratedDoc, ctx: ViewerCtx): HTMLElement {
  const style = paraStyleOf(doc, p.paraStyleIndex);
  const level = style?.outlineLevel ?? 0;
  const el = level >= 1 && level <= 5
    ? document.createElement(`h${level}`)
    : document.createElement("p");
  if (style) applyParaStyle(el, style);
  for (const item of p.items) {
    if (item.type === "text") {
      const span = document.createElement(item.hyperlink ? "a" : "span");
      applyCharStyle(span, charStyleOf(doc, item.charStyleIndex));
      if (item.hyperlink) {
        (span as HTMLAnchorElement).href = item.hyperlink;
        (span as HTMLAnchorElement).target = "_blank";
        (span as HTMLAnchorElement).rel = "noopener";
      }
      // soft line breaks (U+2028 paragraph separator / U+2029 line
      // separator) are visible breaks in iWork but not in HTML text
      const parts = item.text.split(/[\u2028\u2029]/);
      parts.forEach((part, i) => {
        if (i > 0) span.appendChild(document.createElement("br"));
        if (part) span.appendChild(document.createTextNode(part));
      });
      el.appendChild(span);
    } else if (item.type === "field") {
      const span = document.createElement("span");
      span.className = "field";
      span.dataset.fieldKind = item.field.kind;
      span.textContent = item.value ?? fieldPlaceholderText(item);
      applyCharStyle(span, charStyleOf(doc, item.charStyleIndex));
      el.appendChild(span);
    } else {
      // inline attachment (U+FFFC): render the embedded drawable in-flow
      el.appendChild(renderFlowDrawable(item.drawable, doc, ctx));
    }
  }
  return el;
}

/** A whole text block (body, notes, cell rich text…). */
export function renderStyledText(t: StyledText | undefined, doc: HydratedDoc, ctx: ViewerCtx): HTMLElement {
  const div = document.createElement("div");
  div.className = "styled-text";
  if (t) for (const p of t.paragraphs) div.appendChild(renderParagraph(p, doc, ctx));
  return div;
}
