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

/** Marker text for a numbered list position. Surround per ListFormat
 *  numberSurround: period (default) "1.", paren "1)", double-paren "(1)",
 *  none bare "1" (G5: Apple renders the Harvard sub-level as "a)"). */
function numberMarker(n: number, kind: string | undefined, surround: string | undefined): string {
  let num: string;
  switch (kind) {
    case "alpha-upper": num = toAlpha(n, true); break;
    case "alpha-lower": num = toAlpha(n, false); break;
    case "roman-upper": num = toRoman(n, true); break;
    case "roman-lower": num = toRoman(n, false); break;
    default: num = String(n);
  }
  switch (surround) {
    case "paren": return `${num})`;
    case "double-paren": return `(${num})`;
    case "none": return num;
    default: return `${num}.`;
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
 * Unicode stand-ins for symbol-font marker glyphs addressed via the U+F0xx
 * private-use range (PowerPoint-import decks store e.g. Wingdings3 0x75).
 * Keyed "family:lowbyte" with the family lowercased/despaced; anything
 * unmapped degrades to a plain bullet. [inferred: standard Wingdings/Symbol
 * glyph charts; fixture 1249b390 stores wingdings3:0x75 for its ▶ lists]
 */
const PUA_MARKERS: Record<string, string> = {
  "wingdings:108": "●", // l
  "wingdings:109": "○", // m
  "wingdings:110": "■", // n
  "wingdings:111": "□", // o
  "wingdings:117": "◆", // u
  "wingdings:118": "❖", // v
  "wingdings:167": "▪", // §
  "wingdings:216": "➢", // Ø
  "wingdings:252": "✓", // ü
  "wingdings3:116": "◀", // t
  "wingdings3:117": "▶", // u
  "wingdings3:112": "▲", // p
  "wingdings3:113": "▼", // q
  "symbol:183": "•", // ·
  "symbol:45": "−",
};

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
  // Apple draws no marker on an EMPTY list paragraph (blank bullet lines
  // exist only while editing — 1249b390's preview shows clean gaps between
  // items where we drew lone ▶ glyphs); inline objects/fields still count
  // as content.
  const hasContent = p.items.some((it) =>
    typeof it === "string" ? it.length > 0 : "type" in it ? true : (it as TextRun).text.length > 0,
  );
  const hasMarker = !!list && hasContent && list.markerKind !== "none" &&
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
      ? numberMarker(n, list!.numberKind, list!.numberSurround)
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
    // bullets step 9/18pt per level, numbered 18/36/54pt. A NEGATIVE indent
    // (PowerPoint-import decks store -27 at level 0: marker hangs left of
    // the text origin) must not become a negative margin — that shifted the
    // whole row out of the box, clipping the first glyph of every line and
    // hiding the marker entirely (1249b390 'FileMaker Clipboard' deck).
    if (!style?.leftIndentPt && list!.markerIndentPt && list!.markerIndentPt > 0) {
      wrap.style.marginLeft = `${list!.markerIndentPt}px`;
    }
    const marker = document.createElement("span");
    marker.className = "list-marker";
    marker.textContent = markerText;
    marker.style.minWidth = "18px";
    // The marker inherits the first run's look (size + color): an unstyled
    // span rendered 15px near-black bullets INVISIBLE on dark decks (RIPE
    // slides 2/5: 28pt white body, default marker). Apple actually colors
    // markers from the list style's own font_color/scale — not yet in the
    // model (proposal sent) — so the run style is the faithful fallback.
    // Style source: the first run with visible text (writers prepend empty
    // runs whose styles carry no size — RIPE), preferring one that resolves
    // to an explicit font size.
    const runs = p.items.filter(
      (it): it is TextRun => typeof it !== "string" && !("type" in it) && it.text.length > 0,
    );
    const runCs =
      runs.map((r) => charStyleOf(doc, r.cStyle)).find((cs) => cs?.fontSizePt) ??
      (runs.length ? charStyleOf(doc, runs[0].cStyle) : undefined);
    if (runCs) applyCharStyle(marker, runCs);
    marker.style.paddingRight = "0.3em"; // marker-to-text gap, scales with size
    // The list style's OWN marker look wins over run inheritance when stored
    // (ListFormat markerColor/markerFontName/markerScale — RIPE orange dots).
    // markerScale multiplies the RUN size (LabelGeometry scale_with_text), so
    // resolve to px against it — an em here would key off the wrapper's
    // default 15px, not the paragraph's size.
    if (list!.markerColor) marker.style.color = list!.markerColor;
    if (list!.markerFontName) marker.style.fontFamily = `"${list!.markerFontName}", sans-serif`;
    // Symbol-font markers (Wingdings/Webdings/Symbol) address glyphs through
    // the U+F0xx private-use range; machines without the font draw tofu.
    // Substitute the Unicode equivalent and let any real font draw it.
    if (/^[-]$/.test(markerText)) {
      const uni = PUA_MARKERS[`${(list!.markerFontName ?? "").replace(/\s+/g, "").toLowerCase()}:${markerText.charCodeAt(0) & 0xff}`];
      marker.textContent = uni ?? "•";
      marker.style.fontFamily = "";
    }
    if (list!.markerScale) {
      const basePt = runCs?.fontSizePt;
      marker.style.fontSize = basePt
        ? `${basePt * list!.markerScale}px`
        : `${list!.markerScale}em`;
    }
    if (list!.markerBaselineOffsetPt) marker.style.verticalAlign = `${list!.markerBaselineOffsetPt}px`;
    wrap.appendChild(marker);
    wrap.appendChild(el);
    renderParagraphContent(el, p, doc, ctx, style?.dropCap);
    return wrap;
  }

  renderParagraphContent(el, p, doc, ctx, style?.dropCap);
  return el;
}

/** Items of a paragraph into the given element. A dropCap (ParaStyle) carves
 *  the leading characters off the first text run into a floated cap glyph
 *  sized to span `lines` body lines (G5 page 5's big "T"). */
function renderParagraphContent(
  el: HTMLElement,
  p: Paragraph,
  doc: HydratedDoc,
  ctx: ViewerCtx,
  dropCap?: import("../../model/src/shared").ParaStyle["dropCap"],
): void {
  let items = p.items;
  if (dropCap) {
    const k = dropCap.characters ?? 1;
    const first = items[0];
    const text = typeof first === "string" ? first : !("type" in (first ?? {})) ? (first as TextRun).text : undefined;
    if (text && text.length >= k) {
      const capText = [...text].slice(0, k).join("");
      const rest = [...text].slice(k).join("");
      const cap = document.createElement("span");
      cap.className = "drop-cap";
      if (typeof first !== "string") applyCharStyle(cap, charStyleOf(doc, (first as TextRun).cStyle));
      applyCharStyle(cap, dropCap.charStyle);
      const lines = dropCap.lines ?? 3;
      const scale = dropCap.characterScale ?? 1;
      cap.style.fontSize = `${(lines * 1.2 * scale).toFixed(2)}em`;
      cap.style.lineHeight = "0.85";
      cap.style.cssFloat = "left";
      cap.style.paddingRight = `${dropCap.paddingPt ?? 4}px`;
      if (dropCap.outdentPt) cap.style.marginLeft = `${-dropCap.outdentPt}px`;
      cap.textContent = capText;
      el.appendChild(cap);
      items = [
        typeof first === "string" ? rest : { ...(first as TextRun), text: rest },
        ...items.slice(1),
      ];
    }
  }
  for (const item of items) {
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
  let url = ctx.url(d.image.dataId);
  const size = d.common?.size;
  // Vector art (PDF/AI/EPS) cannot be an <img> src: fall back to a raster
  // thumbnail twin when one ships, else a small neutral tile — a broken-image
  // icon per attachment turned the kcsrk equation-dense deck into noise.
  const isVec = (n?: string) => /\.(pdf|ai|eps)$/i.test(n ?? "");
  if (isVec(d.image.preferredFileName ?? d.image.fileName)) {
    const thumbUrl = d.thumbnail ? ctx.url(d.thumbnail.dataId) : undefined;
    if (thumbUrl && !isVec(d.thumbnail?.fileName ?? d.thumbnail?.preferredFileName)) {
      url = thumbUrl;
    } else {
      const tile = document.createElement("span");
      tile.className = "inline-vector-tile";
      if (size) {
        tile.style.width = `${size.width}px`;
        tile.style.height = `${size.height}px`;
      }
      return tile;
    }
  }
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
