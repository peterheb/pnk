// StyledText -> DOM. Paragraphs become <p> (or <h1>-<h5> when the hydrated
// paragraph style's outlineLevel says heading); runs resolve their char style
// through the document pool (charStyleIndex); smart fields render their
// stored value; inline attachments recurse into the drawable renderer.
import { isPdfBytes, pdfMediaEl } from "./pdfmedia";
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
/**
 * Fallback stacks for fonts that are usually NOT installed but have a
 * recognizable shape class: a condensed display face substituted by the
 * default sans overflows its layout badly (Labothek covers set 128px
 * BebasNeue titles that spilled off the page). Keyed by despaced lowercase
 * prefix; the real font still wins when present.
 */
const FONT_FALLBACKS: [RegExp, string][] = [
  [/^bebas|^oswald|^anton|^haettenschweiler|condensed|^impact|narrow/i, '"Arial Narrow", Impact, "Helvetica Neue", sans-serif'],
  // The Microsoft ClearType faces are the corpus's commonest missing fonts —
  // Calibri in 36 of 323 .pages documents, Cambria in 12 — and they have
  // metric-compatible free clones (Carlito, Caladea) that a reader may well
  // have installed. Behind those, a face of the same CLASS and roughly the
  // same width: Helvetica Neue for the humanist sans, Georgia for the screen
  // serif. Falling through to the generic default turned every one of them
  // into Helvetica, which is wider than Calibri and not a serif at all.
  [/^calibri|^candara|^corbel|^segoeui|^tahoma|^verdana/i, 'Carlito, "Helvetica Neue", Helvetica, sans-serif'],
  [/^cambria|^constantia|^bookantiqua|^palatino|^book/i, 'Caladea, Georgia, Palatino, "Times New Roman", serif'],
  [/^garamond|^ebgaramond|^minion|^goudy|^caslon|^baskerville/i, '"EB Garamond", Baskerville, Palatino, "Times New Roman", serif'],
  [/^consolas|^courier|^monaco|^menlo|^lucidaconsole|mono/i, 'Menlo, Consolas, "Courier New", monospace'],
  // any other name that READS as a serif still gets a serif
  [/times|serif|georgia|didot|charter|hoefler|century|rockwell|slab/i, 'Georgia, "Times New Roman", serif'],
];

/**
 * Weight carried by a PostScript name's suffix. iWork stores the FACE
 * ("HelveticaNeue-Light", "AvenirNext-DemiBold") and only sometimes a
 * separate bold flag, so the suffix is often the only weight information
 * there is — 65 of 323 corpus .pages documents style a run with a
 * weight-suffixed name and no bold flag, and Apple's export of 10a06959
 * draws its HelveticaNeue-Light 47pt title in the Light cut.
 */
const NAME_WEIGHTS: [RegExp, number][] = [
  [/(ultra|extra)black|ultra$/i, 900],
  [/black|heavy/i, 900],
  [/(ultra|extra)bold/i, 800],
  [/(semi|demi)bold|demi$/i, 600],
  [/bold/i, 700],
  [/medium/i, 500],
  [/(ultra|extra)light|thin|hairline/i, 100],
  [/light/i, 300],
];

/** Family without its weight/style suffix: Arial-BoldMT → Arial,
 *  TimesNewRomanPS-BoldMT → TimesNewRoman, HelveticaNeue-Light →
 *  HelveticaNeue. macOS resolves the full PostScript name, so the stripped
 *  family is a FALLBACK for browsers that only know families. */
function familyOf(name: string): string {
  const base = name.replace(
    /(PS)?-(Bold|Semi ?Bold|Demi ?Bold|Demi|Medium|Book|Light|Thin|Hairline|Heavy|Black|Ultra\w*|Extra\w*|Roman|Regular|Normal|Condensed|Oblique|Italic)+(MT|PSMT)?$/i,
    "",
  );
  return base && base !== name ? base : "";
}

export function applyCharStyle(el: HTMLElement, cs: CharStyle | undefined): void {
  if (!cs) return;
  const s = el.style;
  if (cs.fontName) {
    const flat = cs.fontName.replace(/[\s-]+/g, "");
    const fb = FONT_FALLBACKS.find(([re]) => re.test(flat))?.[1] ?? "sans-serif";
    const family = familyOf(cs.fontName);
    const w = NAME_WEIGHTS.find(([re]) => re.test(cs.fontName!))?.[1];
    // An EXPLICIT `bold: false` over a bold-named face is Apple's way of
    // saying "regular": maison-martos stores font_name HelveticaNeue-Bold on
    // styles whose bold field is present and false, and Numbers draws them
    // regular (agent N). An ABSENT flag means the opposite — the stored face
    // stands, and Apple's export of 10a06959 draws that document's
    // HelveticaNeue-Light 47pt title in the Light cut. Only a bold-or-heavier
    // suffix is demoted: the same fixture keeps HelveticaNeue-Medium where
    // the bold field is present and false, so `false` contradicts "bold", not
    // "not the regular weight".
    const demote = cs.bold === false && !!family && !!w && w >= 600;
    s.fontFamily = demote
      ? `"${family}", ${fb}`
      : family
        ? `"${cs.fontName}", "${family}", ${fb}`
        : `"${cs.fontName}", ${fb}`;
    // the suffix's weight, so a Light/Medium/Bold cut still reads as one when
    // only the family resolves; an explicit bold flag still wins below
    if (demote) s.fontWeight = "400";
    else if (w) s.fontWeight = String(w);
  }
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
  // Superscript/subscript: Keynote sets the run at 2/3 size and raises it so
  // its top meets the cap height (handtracker b6b44046 cover, Arial 50pt:
  // the superscript "1" is 50px tall against 77px caps at 150dpi, its
  // bottom 26px above the baseline). CSS "super" alone kept full size.
  // The vertical-align length is in the run's OWN em (2/3 of the parent's).
  if (cs.baseline === "superscript") { s.fontSize = "66.7%"; s.verticalAlign = "0.5em"; }
  else if (cs.baseline === "subscript") { s.fontSize = "66.7%"; s.verticalAlign = "-0.25em"; }
  if (cs.baselineShiftPt) s.verticalAlign = `${cs.baselineShiftPt}px`;
  // A shifted run must not grow the LINE box (Apple's baseline shifts never
  // change line spacing; 0d5851c0 slide 19's raised red labels stretched
  // 40px-exact rows to 41.7px — visibly loose over 8 rows). line-height: 0
  // removes the shifted inline box from line-height calculation.
  if (cs.baseline === "superscript" || cs.baseline === "subscript" || cs.baselineShiftPt) s.lineHeight = "0";
  if (cs.trackingPt) s.letterSpacing = `${cs.trackingPt}px`;
  if (cs.fontColor) s.color = cs.fontColor;
  if (cs.backgroundColor) s.backgroundColor = cs.backgroundColor;
}

/**
 * Natural (single-spaced) line height per em for common faces: ascent +
 * descent + line gap from the font's hhea table. Apple's line-spacing
 * MULTIPLE scales THIS, not the font size — 16b4195d's Arial 12pt body at
 * 1.2× measures a 16.7pt pitch in Pages' export (1.2 × 1.15 × 12), where a
 * unitless CSS line-height of 1.2 gave 14.4pt and packed 8 pages into 6.
 * Keyed by despaced lowercase prefix; unknown faces use 1.2, the browser's
 * usual `normal`. [inferred: metrics from the fonts' hhea tables]
 */
const FONT_LINE_HEIGHTS: [RegExp, number][] = [
  // measured with CoreText on macOS 26.6:
  //   (CTFontGetAscent + CTFontGetDescent + CTFontGetLeading) / size
  // Several of the old guesses were a face's number applied to a different
  // face — Helvetica carried Arial's 1.15 where its real leading is 1.00
  // (zero line gap), Palatino 1.35 against a real 1.10, Hoefler Text 1.37
  // against 1.00 — and Menlo (1.164) and Monaco (1.334) shared one entry.
  [/^helveticaneue/, 1.193],
  [/^helvetica/, 1.0],
  [/^arialnarrow/, 1.1475],
  [/^arialblack/, 1.41],
  [/^arial/, 1.1499],
  [/^timesnewroman/, 1.1499],
  [/^times/, 1.0],
  [/^georgia/, 1.1362],
  [/^verdana/, 1.2153],
  [/^tahoma/, 1.207],
  [/^trebuchet/, 1.1611],
  // Calibri / Cambria / Garamond are not installed on macOS: these are the
  // substitutes FONT_FALLBACKS names, so the multiple scales what is drawn.
  [/^calibri|^candara|^corbel|^segoeui/, 1.193],
  [/^cambria|^constantia/, 1.1362],
  [/^garamond/, 1.144],
  [/^avenir/, 1.366],
  [/^sfpro|^sf-|^\.sf|^sfns/, 1.193],
  [/^palatino/, 1.10],
  [/^baskerville/, 1.144],
  [/^gillsans/, 1.1484],
  [/^futura/, 1.328],
  [/^charter/, 1.2202],
  [/^couriernew|^courier/, 1.0],
  [/^menlo/, 1.164],
  [/^monaco/, 1.334],
  [/^lucida/, 1.178],
  [/^impact/, 1.2197],
  [/^rockwell/, 1.2002],
  [/^americantypewriter/, 1.154],
  [/^copperplate/, 1.03],
  [/^chalkboard/, 1.276],
  [/^markerfelt/, 1.086],
  [/^bradleyhand/, 1.249],
  [/^snellroundhand/, 1.261],
  [/^zapfino/, 3.378],
  [/^noteworthy/, 1.615],
  [/^optima/, 1.212],
  [/^hoefler/, 1.0],
  [/^didot/, 1.264],
  [/^seravek/, 1.227],
  [/^wingdings|^webdings/, 1.11],
  [/^symbol/, 1.0],
  [/^hirakaku|^hiragino|^hiramin|^yugothic|^osaka/, 1.5],
];

export function naturalLineHeight(fontName: string | undefined): number {
  if (!fontName) return 1.2;
  const flat = fontName.replace(/[\s-]+/g, "").toLowerCase();
  return FONT_LINE_HEIGHTS.find(([re]) => re.test(flat))?.[1] ?? 1.2;
}

export function applyParaStyle(el: HTMLElement, ps: ParaStyle, fontName?: string, fontSizePx?: number): void {
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
  // multiple × the face's natural line height (see FONT_LINE_HEIGHTS)
  if (ps.lineSpacingMultiple) s.lineHeight = (ps.lineSpacingMultiple * naturalLineHeight(fontName)).toFixed(3);
  else if (ps.lineSpacingExactPt) {
    // "min"/"max" bound the NATURAL line height rather than replace it
    // (TSWP.LineSpacingArchive mode 1/3): kcsrk's Menlo 24pt code blocks
    // store "at least 20pt" and Keynote lays them out at Menlo's natural
    // 28pt pitch; an exact 20px packed them 30% too tight. "space-between"
    // (mode 4) adds the amount to the natural height. Without a known
    // paragraph size the bound falls back to exact. [inferred from the
    // export; mode semantics per the proto's enum names]
    const natural = fontSizePx ? naturalLineHeight(fontName) * fontSizePx : undefined;
    const mode = ps.lineSpacingMode;
    let lh = ps.lineSpacingExactPt;
    if (natural !== undefined) {
      if (mode === "min") lh = Math.max(lh, natural);
      else if (mode === "max") lh = Math.min(lh, natural);
      else if (mode === "space-between") lh = natural + lh;
    }
    s.lineHeight = `${lh.toFixed(2)}px`;
  }
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

// Scheme allowlist for document-supplied hyperlinks, mirroring the converter
// policy (crates/pnk2json/src/text.rs valid_url): the document is untrusted,
// so javascript:/file:/custom schemes must never reach an anchor href even
// if a hand-edited JSON payload carries one.
function safeHref(u: string): boolean {
  if (u.startsWith("#")) return true;
  const lower = u.toLowerCase();
  return lower.startsWith("https://") || lower.startsWith("http://") || lower.startsWith("mailto:");
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
  /** Last number printed at each nesting level (tiered "1.1" labels). */
  levels: number[];
}

export function newListNumberingState(): ListNumberingState {
  return { counters: new Map(), lastKey: null, levels: [] };
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
/**
 * Paragraph base direction: the style's writing direction when stored, else
 * Pages' "natural" — the first strong character decides (Unicode bidi
 * paragraph level). No corpus document stores the direction: 77890685's and
 * ae1cc13b's Arabic paragraphs are all "natural", and without this they
 * ran left-to-right with right alignment — list markers on the left, the
 * sentence's period at the wrong end, justified last lines at the left.
 */
function paragraphDirection(p: Paragraph, style: ParaStyle | undefined): "rtl" | "ltr" | undefined {
  if (style?.writingDirection === "right-to-left") return "rtl";
  if (style?.writingDirection === "left-to-right") return "ltr";
  for (const it of p.items) {
    const text = typeof it === "string" ? it : "type" in it ? "" : (it as TextRun).text;
    for (const ch of text) {
      if (/[\u0590-\u08FF\uFB1D-\uFDFF\uFE70-\uFEFF]/.test(ch)) return "rtl";
      if (/\p{L}/u.test(ch)) return undefined;
    }
  }
  return undefined;
}

export function renderParagraph(
  p: Paragraph,
  doc: HydratedDoc,
  ctx: ViewerCtx,
  listState: ListNumberingState = newListNumberingState(),
): HTMLElement {
  const style = paraStyleOf(doc, p.pStyle);
  const list = style?.list;
  const dir = paragraphDirection(p, style);
  // Apple draws no marker on an EMPTY list paragraph (blank bullet lines
  // exist only while editing — 1249b390's preview shows clean gaps between
  // items where we drew lone ▶ glyphs); inline objects/fields still count
  // as content.
  const hasContent = p.items.some((it) =>
    typeof it === "string" ? it.length > 0 : "type" in it ? true : (it as TextRun).text.length > 0,
  );
  const hasMarker = !!list && hasContent && list.markerKind !== "none" &&
    (list.markerKind === "string"
      ? !!list.markerText
      : list.markerKind === "image"
        ? !!list.markerImage
        : list.markerKind === "number");

  const level = style?.outlineLevel ?? 0;
  const el = level >= 1 && level <= 5
    ? document.createElement(`h${level}`)
    : document.createElement("p");
  if (dir) {
    el.dir = dir;
    // "auto" alignment follows the direction (start); an explicit left
    // alignment stays at the left in a right-to-left paragraph
    if (style?.horizontalAlignment === "left") el.style.textAlign = "left";
  }

  // The block's own font-size feeds the line-box STRUT: run spans carry
  // their sizes but the <p> inherited the chrome's 15px, so every line of
  // smaller text was padded to ~18px pitch (G2's 9pt caption rendered with
  // double leading — visible even between the wrapped lines of one
  // paragraph). Size the block to its largest visible run: Apple derives
  // line height from the tallest run in the line.
  const runSizes = p.items
    .map((it) =>
      typeof it === "string" || "type" in it ? undefined : charStyleOf(doc, (it as TextRun).cStyle)?.fontSizePt,
    )
    .filter((n): n is number => !!n);
  if (runSizes.length) el.style.fontSize = `${Math.max(...runSizes)}px`;
  // the dominant face sets the natural line height the spacing multiple scales
  const paraFont = p.items
    .map((it) =>
      typeof it === "string" || "type" in it ? undefined : charStyleOf(doc, (it as TextRun).cStyle)?.fontName,
    )
    .find((n): n is string => !!n);

  const paraSizePx = runSizes.length ? Math.max(...runSizes) : undefined;
  if (!hasMarker) {
    listState.lastKey = null;
    if (style) applyParaStyle(el, style, paraFont, paraSizePx);
  } else {
    // numbering: the stored restart flag (surfaced as list.start on the
    // paragraph's pooled style) resets the counter; otherwise numbering
    // CONTINUES the counter for this key — even across intervening
    // paragraphs or nested levels, which is Pages' own "continue from
    // previous" semantics (G5: "Four (Numbered, continued)" resumes 4 after
    // a nested run; "Restart One" carries start=1).
    const key = `${list!.level}:${list!.markerKind}:${list!.markerKind === "number" ? list!.numberKind : list!.markerText}`;
    // The converter counts the numbers (Paragraph.listNumber); the counter
    // below is the fallback for models written before it did.
    const n = p.listNumber
      ?? (list!.start !== undefined ? list!.start : (listState.counters.get(key) ?? 0) + 1);
    listState.counters.set(key, n);
    listState.lastKey = key;
    listState.levels.length = Math.min(listState.levels.length, list!.level + 1);
    listState.levels[list!.level] = n;
    // Tiered numbering shows the path through the enclosing levels: "1.1",
    // "1.1.1" (48f5f124). Levels the walk never saw print as 1.
    const markerText = list!.markerKind === "number"
      ? list!.tiered
        ? Array.from({ length: list!.level + 1 }, (_, l) => numberMarker(listState.levels[l] ?? 1, list!.numberKind, "none")).join(".") + "."
        : numberMarker(n, list!.numberKind, list!.numberSurround)
      : (list!.markerText ?? "•");

    // marker hangs in a flex row; paragraph margins live on the wrapper
    const wrap = document.createElement("div");
    wrap.className = "list-item";
    if (dir) wrap.dir = dir; // the marker hangs on the paragraph's start side
    if (style) {
      applyParaStyle(wrap, style, paraFont, paraSizePx);
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
    if (list!.markerKind !== "image") marker.textContent = markerText;
    marker.style.minWidth = "18px";
    // 18px was a guess; the list style stores the real marker-to-text column
    // as an EM multiple of the paragraph font size (ListFormat.textIndentEm,
    // TSWP.ListStyleArchive text_indents = 12 [proto]). The em is the
    // PARAGRAPH's size, not the wrapper's inherited 15px, so resolve to px
    // when a run size is known.
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
    // ...but not its underline or strikethrough: Keynote draws a plain dot
    // beside a hyperlink bullet (RIPE 75 slide 6), not an underlined one.
    marker.style.textDecoration = "none";
    // marker-to-text gap, scales with size; logical so a right-to-left row
    // (direction set by the paragraph style) keeps the gap beside the text
    marker.style.paddingInlineEnd = "0.3em";
    if (list!.textIndentEm) {
      const emPt = runCs?.fontSizePt ?? runSizes[0];
      marker.style.minWidth = emPt
        ? `${(list!.textIndentEm * emPt).toFixed(2)}px`
        : `${list!.textIndentEm}em`;
      marker.style.paddingRight = "0";
    }
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
    if (list!.markerScale && list!.markerKind !== "image") {
      const basePt = runCs?.fontSizePt;
      marker.style.fontSize = basePt
        ? `${basePt * list!.markerScale}px`
        : `${list!.markerScale}em`;
    }
    if (list!.markerBaselineOffsetPt) marker.style.verticalAlign = `${list!.markerBaselineOffsetPt}px`;
    // The marker must not set the row's height: it is a flex item beside
    // the paragraph, baseline-aligned, and a 1.5× marker (RIPE's orange
    // dots, 63px beside 42px text) or a marker at normal leading beside a
    // paragraph with a tighter exact line height made every list row
    // taller than its text. Keynote sizes the line from the text and hangs
    // the marker on its baseline; zero line-height keeps the glyph and its
    // baseline while contributing no height (Keynote's export: RIPE 75
    // sub-bullets step 62pt = 25pt space-before + Arial 32pt's natural
    // 36.8pt; ours stepped 78pt).
    if (list!.markerKind !== "image") marker.style.lineHeight = "0";
    // Image marker (0d5851c0 rightArrow bullets): the PNG scales with the
    // text like a glyph would — markerScale × the run's size (same
    // scale_with_text rule as string markers), hung on the baseline.
    if (list!.markerKind === "image" && list!.markerImage) {
      const url = ctx.url(list!.markerImage.dataId);
      if (url) {
        const img = document.createElement("img");
        img.src = url;
        const basePt = runCs?.fontSizePt;
        const hPx = (list!.markerScale ?? 0.5) * (basePt ?? 15);
        img.style.height = `${hPx.toFixed(1)}px`;
        img.style.width = "auto";
        img.style.verticalAlign = "baseline";
        marker.appendChild(img);
      } else marker.textContent = "•"; // media bytes missing: glyph fallback
    }
    wrap.appendChild(marker);
    wrap.appendChild(el);
    renderParagraphContent(el, p, doc, ctx, style?.dropCap);
    tagTabs(el, style);
    return wrap;
  }

  renderParagraphContent(el, p, doc, ctx, style?.dropCap);
  tagTabs(el, style);
  return el;
}

/** Record the paragraph's tab stops for layoutTabs when it contains a tab. */
function tagTabs(el: HTMLElement, style: ParaStyle | undefined): void {
  if (!el.querySelector(".tab-gap")) return;
  const stops = (style?.tabs ?? []).map((t) => ({ pos: t.positionPt, align: t.alignment, leader: t.leader || undefined }));
  el.dataset.tabs = JSON.stringify(stops);
  if (style?.defaultTabStopPt) el.dataset.tabDefault = String(style.defaultTabStopPt);
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
  // Trailing whitespace hangs past the box edge in Keynote and paints no
  // background there (kcsrk's code blocks end every line in 25-80 spaces
  // carrying the code's white highlight, one of them red: Keynote's export
  // shows neither, ours painted white bars across the stack diagram and a
  // red block at the slide edge). CSS pre-wrap hangs the spaces the same
  // way but still paints their background, so runs after the last visible
  // character lose it.
  let lastVisible = -1;
  items.forEach((it, i) => {
    const t = typeof it === "string" ? it : "type" in it ? "\ufffc" : (it as TextRun).text;
    if (t.trim().length > 0) lastVisible = i;
  });
  const bareOfBackground = (cs: CharStyle | undefined): CharStyle | undefined => {
    if (!cs?.backgroundColor) return cs;
    const { backgroundColor: _bg, ...rest } = cs;
    return rest;
  };
  for (const [index, item] of items.entries()) {
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
      const linkable = run.hyperlink !== undefined && safeHref(run.hyperlink);
      const span = document.createElement(linkable ? "a" : "span");
      const cs = charStyleOf(doc, run.cStyle);
      applyCharStyle(span, index > lastVisible ? bareOfBackground(cs) : cs);
      if (linkable && run.hyperlink) {
        (span as HTMLAnchorElement).href = run.hyperlink;
        (span as HTMLAnchorElement).target = "_blank";
        (span as HTMLAnchorElement).rel = "noopener";
      }
      appendRunText(span, run.text, span);
      el.appendChild(span);
    }
  }
  // A blank paragraph (items: [] or only empty runs) is a blank LINE in
  // iWork, but an empty <p> collapses to zero height and the surrounding
  // text fuses together (1eb960ba: the gaps around the title and between
  // the attendee block and body vanished). A <br> gives the line box its
  // strut at the paragraph's inherited size, and keeps it measurable for
  // WP pagination.
  if (el.textContent === "" && !el.querySelector("br, img")) {
    el.appendChild(document.createElement("br"));
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
      const bytes = ctx.bytes(d.image.dataId);
      if (!bytes || !isPdfBytes(bytes)) return tile;
      // pdf.js paints the PDF (an equation, usually) into the tile's box.
      const host = pdfMediaEl(bytes, size?.width ?? 0, size?.height ?? 0, () => tile.cloneNode() as HTMLElement);
      host.classList.add("inline-pdf");
      if (size) {
        host.style.width = `${size.width}px`;
        host.style.height = `${size.height}px`;
      }
      // An equation's stored depth is how far its box hangs below the text
      // baseline [model: ImageDrawable.equation.depthPt].
      host.style.verticalAlign = d.equation?.depthPt !== undefined ? `${-d.equation.depthPt}px` : "text-bottom";
      return host;
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
    // A tab becomes a measurable gap element; layoutTabs sizes it to the
    // paragraph's stops once the text is in the document (a raw \t only
    // knows CSS tab-size, a fixed grid with no right/center/decimal stops).
    part.split("\t").forEach((seg, j) => {
      if (j > 0) {
        const gap = document.createElement("span");
        gap.className = "tab-gap";
        parent.appendChild(gap);
      }
      if (seg) parent.appendChild(document.createTextNode(seg));
    });
  });
  void styleHost;
}

interface TabStopSpec { pos: number; align: string; leader?: string }

/**
 * Positioned tab stops (TSWP.TabsArchive), resolved after layout: each
 * .tab-gap is sized so the text after it lands on the next stop past its
 * own x — at the stop for left, ending there for right, centred for
 * center, with the number's separator there for decimal. Past the last
 * explicit stop the default interval takes over. x is measured from the
 * text area's left edge (the paragraph's box minus its indent), in layout
 * px; canvases are CSS-scaled, so client rects are divided by the scale.
 * Gaps are sized in order because each width moves the ones after it.
 * b31db822's contents page: right stops at 446.5pt put the page numbers
 * flush right on one line instead of wrapping under a 36px tab grid.
 */
export function layoutTabs(root: HTMLElement): void {
  const paras = root.matches("[data-tabs]")
    ? [root]
    : Array.from(root.querySelectorAll<HTMLElement>("[data-tabs]"));
  for (const p of paras) {
    const gaps = Array.from(p.querySelectorAll<HTMLElement>(".tab-gap"));
    if (gaps.length === 0) continue;
    let stops: TabStopSpec[] = [];
    try { stops = JSON.parse(p.dataset.tabs || "[]") as TabStopSpec[]; } catch { stops = []; }
    stops.sort((a, b) => a.pos - b.pos);
    const dflt = parseFloat(p.dataset.tabDefault || "") || 36;
    const origin = (p.closest(".list-item") as HTMLElement | null) ?? p;
    const oRect = origin.getBoundingClientRect();
    if (oRect.width === 0 || origin.offsetWidth === 0) continue;
    const scale = oRect.width / origin.offsetWidth;
    const originX = oRect.left - (parseFloat(getComputedStyle(origin).marginLeft) || 0) * scale;
    for (const gap of gaps) gap.style.width = "0px";
    gaps.forEach((gap, gi) => {
      const x = (gap.getBoundingClientRect().left - originX) / scale;
      const range = document.createRange();
      range.selectNodeContents(p);
      range.setStartAfter(gap);
      const next = gaps[gi + 1];
      if (next) range.setEndBefore(next);
      // width of the segment's LAST line: a segment that wraps reports a
      // bounding box spanning both lines, which would collapse a right or
      // center stop; Pages aligns the wrapped text's final line to the stop
      const segW = lastLineWidth(range) / scale;
      const stop = stops.find((st) => st.pos > x + 0.5);
      const pos = stop ? stop.pos : (Math.floor(x / dflt) + 1) * dflt;
      let width: number;
      switch (stop?.align) {
        case "right": width = pos - x - segW; break;
        case "center": width = pos - x - segW / 2; break;
        case "decimal": width = pos - x - decimalOffset(range, scale); break;
        default: width = pos - x;
      }
      // content wider than the stop allows: Pages runs on to the next stop;
      // a hair of space keeps the words apart either way
      if (width < 2) {
        const later = stops.find((st) => st.pos > x + segW + 2);
        width = later ? Math.max(2, later.pos - x - (later.align === "right" ? segW : 0)) : 2;
      }
      gap.style.width = `${width.toFixed(2)}px`;
      if (stop?.leader) gap.dataset.leader = stop.leader;
    });
  }
}

/** Width of the last line box a range covers (all rects sharing its top). */
function lastLineWidth(range: Range): number {
  const rects = Array.from(range.getClientRects()).filter((r) => r.width > 0 || r.height > 0);
  if (rects.length === 0) return range.getBoundingClientRect().width;
  const last = rects[rects.length - 1];
  const onLine = rects.filter((r) => Math.abs(r.top - last.top) < r.height / 2 + 0.5);
  return Math.max(...onLine.map((r) => r.right)) - Math.min(...onLine.map((r) => r.left));
}

/** Width from the segment start to its first decimal separator. */
function decimalOffset(range: Range, scale: number): number {
  const walker = document.createTreeWalker(range.commonAncestorContainer, NodeFilter.SHOW_TEXT);
  let node: Node | null = walker.nextNode();
  while (node) {
    if (range.comparePoint(node, 0) >= 0) {
      const text = node.textContent ?? "";
      const i = text.search(/[.,]/);
      if (i >= 0) {
        const r = document.createRange();
        r.setStart(range.startContainer, range.startOffset);
        r.setEnd(node, i);
        return r.getBoundingClientRect().width / scale;
      }
    }
    node = walker.nextNode();
  }
  return range.getBoundingClientRect().width / scale;
}

/** A whole text block (body, notes, cell rich text…). */
export function renderStyledText(t: StyledText | undefined, doc: HydratedDoc, ctx: ViewerCtx): HTMLElement {
  const div = document.createElement("div");
  div.className = "styled-text";
  const listState = newListNumberingState();
  if (t) for (const p of t.paragraphs) div.appendChild(renderParagraph(p, doc, ctx, listState));
  return div;
}
