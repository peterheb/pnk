// TableModel -> HTML <table>: real cell values, merges spanned from the
// anchor, header/footer sections, per-cell resolved styles.
//
// Consumes the dense row-major `grid` + deduped `formats` pool (the
// amended TableModel contract).

import type {
  CellFormat,
  GridCell,
  TableCellStyle,
  TableCell,
  TableModel,
  TableMerge,
} from "../../model/src/shared";
import { fillToCss } from "./drawables";
import { applyCharStyle, naturalLineHeight, renderStyledText } from "./text";
import type { HydratedDoc } from "./hydrate";
import { cellStyleOf } from "./hydrate";
import type { ViewerCtx } from "./ctx";

// Document locale, set from meta.locale after parse. Comma-decimal
// separators follow the CLDR region when one is present (Apple renders
// "5,48" for en_EE documents — the region, not the language, decides);
// language-only locales fall back to the language's usual convention.
//
// DELIBERATE divergence from Numbers' PDF export: the app formats numbers
// in the MACHINE's locale, not the document's. maison-martos carries
// meta.locale = fr_FR and Numbers on an en_US Mac exports "€310,000" and
// "€1,722.22" (pymupdf over its export) where the document's own locale
// gives "€310.000" / "€1.722,22". A viewer has no machine locale worth
// speaking of — the same file would render differently for every reader —
// so we follow the DOCUMENT locale and stay consistent. Diffs against an
// Apple export from a differently-localised Mac are expected here.
const COMMA_DECIMAL_LANG = /^(de|fr|it|es|pt|nl|da|fi|nb|sv|el|pl|ru|tr)$/i;
const COMMA_DECIMAL_REGION = /^(ee|de|fr|it|es|pt|nl|dk|fi|no|gr|pl|ru|tr|br)$/i;
let decimalComma = false;
let docLocale = "en";

export function setTableLocale(locale: string | undefined): void {
  const [lang, region] = (locale ?? "").split(/[-_]/);
  decimalComma = COMMA_DECIMAL_LANG.test(lang) || COMMA_DECIMAL_REGION.test(region);
  docLocale = locale ? locale.replace(/_/g, "-") : "en";
  try {
    new Date().toLocaleString(docLocale);
  } catch {
    docLocale = "en";
  }
}

const cellKey = (r: number, c: number) => `${r}:${c}`;

interface MergeIndex {
  anchor: Map<string, TableMerge>;
  covered: Set<string>;
}

function indexMerges(merges: TableMerge[]): MergeIndex {
  const anchor = new Map<string, TableMerge>();
  const covered = new Set<string>();
  for (const m of merges) {
    anchor.set(cellKey(m.anchorRow, m.anchorColumn), m);
    for (let r = 0; r < m.rowSpan; r++)
      for (let c = 0; c < m.columnSpan; c++) {
        if (r === 0 && c === 0) continue;
        covered.add(cellKey(m.anchorRow + r, m.anchorColumn + c));
      }
  }
  return { anchor, covered };
}
/** Normalize a grid slot: plain scalars become unformatted cell views. */
function asCell(slot: NonNullable<GridCell>): TableCell {
  if (typeof slot === "object") return slot;
  return { v: slot };
}

/**
 * Number display in the document's locale. A number FORMAT's decimals is
 * the display contract (Apple shows 912558.880000000 for decimals=10) —
 * rendered exactly, no trimming. Unformatted bare numbers keep the noise
 * trim (12 significant digits).
 */
function formatNumber(v: number, decimals: number | undefined, exact: boolean, grouping?: boolean): string {
  let s: string;
  if (decimals !== undefined) {
    s = v.toFixed(Math.min(Math.max(decimals, 0), 20));
    if (!exact) s = s.replace(/(\.\d*?)0+$/, "$1").replace(/\.$/, "");
  } else {
    // unformatted cells: 12 significant digits kills double-repr noise
    // (388.59999999999997 -> 388.6) without hiding real precision
    s = Number(v.toPrecision(12)).toString();
  }
  if (decimalComma) s = s.replace(".", ",");
  if (grouping) {
    // format.grouping = show_thousands_separator: 5500 -> 5,500 (or
    // 5.500 in comma-decimal locales, matching Apple)
    const sep = decimalComma ? "." : ",";
    const dec = decimalComma ? "," : ".";
    const di = s.indexOf(dec);
    let int = di === -1 ? s : s.slice(0, di);
    const rest = di === -1 ? "" : s.slice(di);
    const neg = int.startsWith("-") ? "-" : "";
    if (neg) int = int.slice(1);
    int = int.replace(/\B(?=(\d{3})+(?!\d))/g, sep);
    s = neg + int + rest;
  }
  return s;
}

/**
 * Currency display symbol for an ISO code, Apple-style: "$100.00" not
 * "USD 100.00". Codes without a well-known symbol keep a "CODE " prefix.
 */
const CURRENCY_SYMBOL: Record<string, string> = {
  USD: "$", AUD: "$", CAD: "$", NZD: "$", HKD: "$", SGD: "$", MXN: "$",
  EUR: "€", GBP: "£", JPY: "¥", CNY: "¥", KRW: "₩",
  INR: "₹", RUB: "₽", TRY: "₺", ILS: "₪", PHP: "₱",
  THB: "฿", VND: "₫", UAH: "₴", NGN: "₦", BRL: "R$",
  ZAR: "R", CHF: "CHF ",
};

/**
 * Apple duration rendering, following numbers-parser's decode of
 * TSK.FormatStructArchive (cell.py _duration_format/_auto_units):
 * style 0 = compact positional ("28:40"), 1 = short units ("28m 40s"),
 * 2 = long units ("28 minutes 40 seconds"). Units enum: 1 week, 2 day,
 * 4 hour, 8 minute, 16 second, 32 millisecond.
 */
function formatDurationStyled(v: number, style: number, largest: number, smallest: number, auto: boolean): string {
  const WEEK = 604800, DAY = 86400, HOUR = 3600;
  if (auto) {
    if (v === 0) { largest = 2; smallest = 2; }
    else {
      largest = v >= WEEK ? 1 : v >= DAY ? 2 : v >= HOUR ? 4 : v >= 60 ? 8 : v >= 1 ? 16 : 32;
      if (Math.floor(v) !== v) smallest = 32;
      else if (v % 60) smallest = 16;
      else if (v % HOUR) smallest = 8;
      else if (v % DAY) smallest = 4;
      else if (v % WEEK) smallest = 2;
      smallest = Math.max(smallest, largest);
    }
  }
  const unit = (name: string, abbrev: string, value: number): string => {
    if (style === 0) return "";
    if (style === 1) return abbrev;
    return ` ${name}${value === 1 ? "" : "s"}`;
  };
  const inRange = (u: number) => largest <= u && smallest >= u;
  // Apple ROUNDS at the smallest displayed unit (28:39.7 -> "28m 40s");
  // pre-round so the carry propagates through the floors below.
  const SMALLEST_S: Record<number, number> = { 1: WEEK, 2: DAY, 4: HOUR, 8: 60, 16: 1, 32: 0.001 };
  const q = SMALLEST_S[smallest] ?? 1;
  let d = Math.round(v / q) * q;
  const parts: string[] = [];
  if (largest === 1) {
    const dd = Math.floor(d / WEEK);
    if (smallest !== 1) d -= WEEK * dd;
    parts.push(dd + unit("week", "w", dd));
  }
  if (inRange(2)) {
    const dd = Math.floor(d / DAY);
    if (smallest > 2) d -= DAY * dd;
    parts.push(dd + unit("day", "d", dd));
  }
  if (inRange(4)) {
    const dd = Math.floor(d / HOUR);
    if (smallest > 4) d -= HOUR * dd;
    parts.push(dd + unit("hour", "h", dd));
  }
  if (inRange(8)) {
    const dd = Math.floor(d / 60);
    if (smallest > 8) d -= 60 * dd;
    if (style === 0) {
      const pad = (largest === 8 && smallest === 8) || dd >= 10;
      parts.push((pad ? "" : "0") + dd);
    } else parts.push(dd + unit("minute", "m", dd));
  }
  if (inRange(16)) {
    const dd = Math.floor(d);
    if (smallest > 16) d -= dd;
    if (style === 0) {
      const pad = (largest === 16 && smallest === 16) || dd >= 10;
      parts.push((pad ? "" : "0") + dd);
    } else parts.push(dd + unit("second", "s", dd));
  }
  if (smallest >= 32) {
    const dd = Math.round(1000 * d);
    if (style === 0) {
      parts.push(dd >= 100 ? String(dd) : dd >= 10 ? `0${dd}` : `00${dd}`);
    } else parts.push(dd + unit("millisecond", "ms", dd));
  }
  let out = parts.join(style === 0 ? ":" : " ");
  if (style === 0) out = out.replace(/:(\d\d\d)$/, ".$1");
  return out;
}

/**
 * Fraction display, Apple-style: "fraction-N" fixes the denominator
 * (halves/quarters/eighths/tenths/sixteenths/hundredths); bare "fraction"
 * finds the closest denominator up to 3 digits. Whole part splits off:
 * 1.375 -> "1 3/8"; 0 numerator -> just the whole part.
 */
function toFractionText(v: number, fs: string): string {
  const sign = v < 0 ? "-" : "";
  const av = Math.abs(v);
  const whole = Math.floor(av);
  const frac = av - whole;
  const fixed = fs.startsWith("fraction-") ? parseInt(fs.slice(9), 10) : undefined;
  let n = 0;
  let d = 1;
  if (fixed !== undefined && fixed >= 2) {
    d = fixed;
    n = Math.round(frac * d);
  } else {
    let err = Infinity;
    for (let den = 1; den <= 999; den++) {
      const num = Math.round(frac * den);
      const e = Math.abs(frac - num / den);
      if (e < err - 1e-12) { err = e; n = num; d = den; if (e < 1e-9) break; }
    }
  }
  let w = whole;
  if (n === d) { w += 1; n = 0; }
  if (n === 0) return `${sign}${w}`;
  // reduce (fixed denominators stay as Apple shows them, e.g. 2/8)
  if (fixed === undefined) {
    const gcd = (a: number, b: number): number => (b === 0 ? a : gcd(b, a % b));
    const g = gcd(n, d);
    n /= g; d /= g;
  }
  return w > 0 ? `${sign}${w} ${n}/${d}` : `${sign}${n}/${d}`;
}

/**
 * Numeric custom-format pattern ("#,###", "'+'#,###", "0.00 'kg'"): the
 * digit run gives decimals and grouping, everything around it is a
 * literal (single-quoted in Apple's patterns). Returns undefined when the
 * pattern holds no digit token, so date/unknown patterns fall through.
 * The pattern already encodes the sign for the branch the converter
 * picked ("'-'#,###" for negatives), so the number is rendered ABSOLUTE
 * when the pattern carries a literal sign.
 */
function formatCustomNumber(v: number, pattern: string): string | undefined {
  const m = pattern.match(/[#0][#0,]*(\.[#0]+)?/);
  if (!m) return undefined;
  const token = m[0];
  const decimals = m[1] ? m[1].length - 1 : 0;
  const grouping = token.includes(",");
  const lit = (s: string) => s.replace(/'([^']*)'/g, "$1");
  const prefix = lit(pattern.slice(0, m.index));
  const suffix = lit(pattern.slice((m.index ?? 0) + token.length));
  const signed = /[-+]/.test(prefix) || /[-+]/.test(suffix);
  const body = formatNumber(signed ? Math.abs(v) : v, decimals, true, grouping);
  return `${prefix}${body}${suffix}`;
}

/**
 * ICU-ish date pattern renderer (TSK.FormatStructArchive date_time_format /
 * custom format strings such as "d", "M/d/yy", "d. MMMM yyyy"). UTC getters
 * only — cell dates are wall-clock values stored as ...T00:00:00Z.
 */
function formatDatePattern(d: Date, pattern: string): string {
  const pad = (v: number, w: number) => String(v).padStart(w, "0");
  const loc = (opts: Intl.DateTimeFormatOptions) => {
    try {
      return d.toLocaleString(docLocale, { ...opts, timeZone: "UTC" });
    } catch {
      return d.toLocaleString("en", { ...opts, timeZone: "UTC" });
    }
  };
  let out = "";
  let i = 0;
  while (i < pattern.length) {
    const ch = pattern[i];
    if (ch === "'") {
      // quoted literal; '' inside quotes = a single quote
      let j = i + 1;
      let lit = "";
      while (j < pattern.length) {
        if (pattern[j] === "'") {
          if (pattern[j + 1] === "'") { lit += "'"; j += 2; continue; }
          break;
        }
        lit += pattern[j++];
      }
      out += lit === "" ? "'" : lit;
      i = j + 1;
      continue;
    }
    if (!/[A-Za-z]/.test(ch)) { out += ch; i++; continue; }
    let n = 1;
    while (pattern[i + n] === ch) n++;
    switch (ch) {
      case "y": out += n === 2 ? pad(d.getUTCFullYear() % 100, 2) : String(d.getUTCFullYear()); break;
      case "M": out += n >= 4 ? loc({ month: "long" }) : n === 3 ? loc({ month: "short" }) : pad(d.getUTCMonth() + 1, n); break;
      case "d": out += pad(d.getUTCDate(), n); break;
      case "E": case "e": case "c": out += loc({ weekday: n >= 4 ? "long" : "short" }); break;
      case "H": case "k": out += pad(d.getUTCHours(), n); break;
      case "h": case "K": out += pad(d.getUTCHours() % 12 || 12, n); break;
      case "m": out += pad(d.getUTCMinutes(), n); break;
      case "s": out += pad(d.getUTCSeconds(), n); break;
      case "a": out += d.getUTCHours() < 12 ? "AM" : "PM"; break;
      case "G": out += "AD"; break;
      default: break; // unsupported token: drop rather than leak letters
    }
    i += n;
  }
  return out;
}

/** Display string for a normalized cell view. */
function valueToText(cell: TableCell, format: CellFormat | undefined): string {
  const { v } = cell;
  if (v === null || v === undefined) return "";
  // an untyped number takes its display type from the cell FORMAT: cdrky's
  // budget sheets store plain numbers under a USD/2-decimal format and
  // Numbers prints "$140,353.01", not 140353.008293
  const formatType = typeof v === "number" && format?.kind === "currency" ? "currency" : undefined;
  const type = cell.type ?? formatType ?? (typeof v === "number" ? "number" : typeof v === "boolean" ? "bool" : "text");
  switch (type) {
    case "number": {
      const n = typeof v === "number" ? v : Number(v);
      if (format?.kind === "percent") {
        // iWork stores percent as a fraction; the format displays it ×100
        return `${formatNumber(n * 100, format.decimals, true, format.grouping)}%`;
      }
      const fs = format?.formatString;
      if (fs === "scientific") {
        // Apple's scientific: E+NN with the format's decimals; auto
        // decimals keeps the full mantissa (G5: 6.0221408E+23)
        const s = format?.decimals !== undefined ? n.toExponential(format.decimals) : n.toExponential();
        return s.replace("e", "E");
      }
      if (fs !== undefined && fs.startsWith("base-")) {
        const radix = parseInt(fs.slice(5), 10);
        if (radix >= 2 && radix <= 36) {
          const i = Math.round(n);
          return (i < 0 ? "-" : "") + Math.abs(i).toString(radix).toUpperCase();
        }
      }
      if (fs !== undefined && fs.startsWith("fraction")) {
        return toFractionText(n, fs);
      }
      // "sign-plus" (uses_plus_sign): Numbers prints +2 for positives
      if (fs === "sign-plus" && n > 0) {
        return `+${formatNumber(n, format?.decimals, format?.decimals !== undefined, format?.grouping)}`;
      }
      if (format?.kind === "custom" && fs !== undefined) {
        const custom = formatCustomNumber(n, fs);
        if (custom !== undefined) return custom;
      }
      // a number FORMAT with explicit decimals is the display contract:
      // render exactly what it says (no trailing-zero trim)
      const exact = format !== undefined && format.decimals !== undefined;
      const decimals = format && (format.kind === "number" || format.kind === "automatic")
        ? format.decimals : undefined;
      return formatNumber(n, decimals, exact, format?.grouping);
    }
    case "bool": return typeof v === "boolean" ? (v ? "true" : "false") : String(v);
    case "date": {
      // date/custom formats carry an ICU-ish pattern in formatString
      // ("d" for calendar day numbers, "M/d/yy", ...)
      const pattern = format && (format.kind === "date" || format.kind === "custom")
        ? format.formatString : undefined;
      const d = new Date(String(v));
      if (!pattern || isNaN(d.getTime())) return String(v).slice(0, 10);
      return formatDatePattern(d, pattern);
    }
    case "duration": {
      const total = typeof v === "number" ? v : Number(v);
      const spec = format?.kind === "duration" ? format.formatString : undefined;
      const m = spec?.match(/^duration-(\d+)-(\d+)-(\d+)-([01])$/);
      if (m) {
        return formatDurationStyled(total, Number(m[1]), Number(m[2]), Number(m[3]), m[4] === "1");
      }
      // no stored format: Apple's compact h:mm:ss (h omitted when 0)
      const t = Math.round(total);
      const h = Math.floor(t / 3600);
      const mi = Math.floor((t % 3600) / 60);
      const s = t % 60;
      return h > 0 ? `${h}:${String(mi).padStart(2, "0")}:${String(s).padStart(2, "0")}` : `${String(mi).padStart(2, "0")}:${String(s).padStart(2, "0")}`;
    }
    case "currency": {
      // Apple style: "$5,500.00" — symbol prefix, no space, 2 decimals
      // unless the format says otherwise; unknown codes keep "CODE " prefix
      const n = typeof v === "number" ? v : Number(v);
      const code = cell.cur ?? format?.currencyCode ?? "";
      const decimals = format?.decimals ?? 2;
      // currency groups by default (Apple: $1,234.56); an explicit
      // grouping:false in the stored format turns it off
      const body = formatNumber(Math.abs(n), decimals, true, format?.grouping ?? true);
      const sym = CURRENCY_SYMBOL[code];
      const sign = n < 0 ? "-" : "";
      return sym !== undefined ? `${sign}${sym}${body}` : `${sign}${code ? code + " " : "$"}${body}`;
    }
    case "error": return String(v);
    case "richtext": {
      const st = v as TableModel["grid"] extends never ? never : NonNullable<TableCell["v"]> & { paragraphs?: { items?: { type?: string; text?: string }[] }[] };
      return (st as { paragraphs?: { items?: { type?: string; text?: string }[] }[] }).paragraphs
        ?.map((p) => (p.items ?? []).map((i) => (typeof i === "string" ? i : i.text ?? "")).join("")).join("\n") ?? String(v);
    }
    default: return String(v);
  }
}

function applyCellStyle(td: HTMLTableCellElement, style: TableCellStyle | undefined, header: boolean, footer: boolean, ctx?: ViewerCtx): void {
  const s = td.style;
  if (style?.fill) {
    if (style.fill.type === "solid") {
      s.backgroundColor = style.fill.color;
      s.backgroundImage = "";
    } else if (style.fill.type === "image") {
      // Cell image fills (LED price-list v3 doc: product photos live in the
      // cell style). technique maps onto background-size; without bytes the
      // tint or a neutral tone stands in.
      const url = ctx?.url(style.fill.image.dataId);
      if (url) {
        s.backgroundImage = `url("${url}")`;
        s.backgroundPosition = "center";
        const t = style.fill.technique;
        s.backgroundRepeat = t === "tile" ? "repeat" : "no-repeat";
        s.backgroundSize = t === "scale-to-fill" ? "cover"
          : t === "stretch" ? "100% 100%"
          : t === "tile" || t === "natural-size" ? "auto"
          : "contain";
      } else {
        s.backgroundColor = style.fill.tint ?? "#e8e8ee";
      }
    } else if (style.fill.type === "gradient") {
      // Numbers' highlight colours from the colour well's second row are
      // GRADIENT cell fills, not solid ones (maison-martos marks its
      // EYZAHUT row with a #fae232->#fffb00 vertical gradient and its
      // Charols rows with a cyan one). Painting them as the neutral
      // fallback lost every highlight in the sheet.
      s.backgroundImage = fillToCss(style.fill) ?? "";
    } else {
      s.backgroundColor = "#e8e8ee";
    }
  }
  if (style?.borders) {
    const b = style.borders;
    // width 0 = explicit "no line" (erases the base gridline); dash
    // patterns map to dotted (short) / dashed CSS lines
    // Hairlines: a 0.25pt stroke prints as a faint gray rule in Apple's
    // export, but CSS rounds sub-pixel borders up to a solid 1px black
    // (burndown's 0.25pt gridlines came out as a heavy black grid). Fade
    // thin strokes by their width instead of widening them.
    const faded = (color: string, widthPt: number): string => {
      if (widthPt >= 1 || !/^#[0-9a-f]{6}$/i.test(color)) return color;
      const v = parseInt(color.slice(1), 16);
      return `rgba(${(v >> 16) & 255},${(v >> 8) & 255},${v & 255},${Math.max(0.25, widthPt).toFixed(2)})`;
    };
    const css = (st: { widthPt: number; color: string; dash?: number[] }) =>
      st.widthPt <= 0 ? "none"
        : `${Math.max(st.widthPt, 1)}px ${st.dash ? (st.dash[0] <= 1.5 ? "dotted" : "dashed") : "solid"} ${faded(st.color, st.widthPt)}`;
    if (b.top) s.borderTop = css(b.top);
    if (b.right) s.borderRight = css(b.right);
    if (b.bottom) s.borderBottom = css(b.bottom);
    if (b.left) s.borderLeft = css(b.left);
  }
  if (style?.text) {
    applyCharStyle(td, style.text);
    // Apple auto-fits a row at the FACE's natural leading, not the
    // browser's `normal`: maison-martos' 12pt Helvetica Neue rows measure
    // 36pt for two lines and 50 for three in Numbers' export — 14pt a line
    // over an 8pt inset — where Chrome's default gave 15.
    if (style.text.fontName) s.lineHeight = String(naturalLineHeight(style.text.fontName));
    // A cell's text style OVERRIDES the section's, so an explicit
    // bold/italic false has to undo it — applyCharStyle only ever turns
    // them on. The PostScript family name has to give way too: Numbers
    // stores "HelveticaNeue-Bold" as the face name on styles whose bold
    // flag is false and draws them regular (maison-martos' body cells;
    // Apple's export uses the HelveticaNeue face there), but naming that
    // family in CSS picks the bold cut on macOS.
    const WEIGHT_SUFFIX = /[-\s](Bold|Black|Heavy|Semibold|Demibold|Medium)(MT|PS)?$/i;
    if (style.text.bold === false) {
      s.fontWeight = "400";
      const name = style.text.fontName;
      if (name && WEIGHT_SUFFIX.test(name)) {
        // Prepend the de-suffixed family rather than rewriting the string:
        // Chrome serializes font-family without quotes when the name is a
        // valid CSS identifier, so a quoted search-and-replace never matched.
        s.fontFamily = `"${name.replace(WEIGHT_SUFFIX, "")}", ${s.fontFamily}`;
      }
    }
    if (style.text.italic === false) s.fontStyle = "normal";
  }
  if (style?.paragraph?.horizontalAlignment && style.paragraph.horizontalAlignment !== "auto") {
    td.style.textAlign = style.paragraph.horizontalAlignment;
  }
  if (style?.verticalAlignment) s.verticalAlign = style.verticalAlignment === "middle" ? "middle" : style.verticalAlignment === "bottom" ? "bottom" : "top";
  if (style?.padding) s.padding = `${style.padding.top ?? 4}px ${style.padding.right ?? 8}px ${style.padding.bottom ?? 4}px ${style.padding.left ?? 8}px`;
  // Wrap is resolved through the style chain by the converter and omitted
  // when false, so absent means "one line": Numbers keeps unwrapped text on
  // a single line, clipped at the cell edge unless the cells to the right
  // are empty, in which case it spills over them (see spillUnwrappedCells).
  if (style?.textWrap) {
    s.whiteSpace = "normal";
    td.classList.add("cell-wrap");
  } else {
    s.whiteSpace = "nowrap";
    td.classList.add("cell-nowrap");
  }
  if (header) td.classList.add("cell-header");
  if (footer) td.classList.add("cell-footer");
}

/**
 * Let unwrapped cell text spill across empty neighbor cells, as Numbers
 * draws it: an unwrapped cell wider than its column extends over the cells
 * to its right while they are empty, and is clipped at the first cell with
 * content or at the table edge (MTD workbook: three lines of instructions
 * in a 40pt first column spanning the next two columns; a calendar
 * template's "S_LOCALIZABLE_Sunday" clipped to "S_L" because its neighbor
 * has content). Needs layout, so call after the table is in the document.
 */
export function spillUnwrappedCells(root: HTMLElement): void {
  const cells = root.querySelectorAll<HTMLTableCellElement>("table.sheet-table td.cell-nowrap, table.sheet-table th.cell-nowrap");
  for (const td of Array.from(cells)) {
    if (td.textContent?.trim() === "") continue;
    const align = getComputedStyle(td).textAlign;
    if (align === "right" || align === "end" || align === "center") continue;
    const need = td.scrollWidth - td.clientWidth;
    if (need <= 0) continue;
    let room = 0;
    let sib = td.nextElementSibling as HTMLTableCellElement | null;
    while (sib && room < need) {
      if (sib.textContent?.trim() !== "" || sib.querySelector("img, svg, table")) break;
      room += sib.getBoundingClientRect().width;
      sib = sib.nextElementSibling as HTMLTableCellElement | null;
    }
    if (room <= 0) continue;
    // Move the content into a clipping box that is as wide as the run of
    // empty cells allows; the cell itself lets it overflow.
    const box = document.createElement("div");
    box.className = "cell-spill";
    box.append(...Array.from(td.childNodes));
    td.appendChild(box);
    const cs = getComputedStyle(td);
    const inner = td.clientWidth - parseFloat(cs.paddingLeft) - parseFloat(cs.paddingRight);
    box.style.width = `${inner + room}px`;
    td.style.overflow = "visible";
  }
}

/**
 * Drawn width of a table: the sum of its visible column widths (a stored 0
 * means the table's default width). Returns 0 when a width is unknown, in
 * which case the renderer falls back to the auto table layout.
 */
export function tableDrawnWidth(model: TableModel): number {
  let total = 0;
  for (let c = 0; c < model.columnCount; c++) {
    const info = model.columns?.[c];
    if (info?.hidden) continue;
    const w = info?.sizePt || model.defaultColumnWidthPt;
    if (!w) return 0;
    total += w;
  }
  return total;
}

/** One TableModel as a real table; hidden rows/columns are skipped. */
export function renderTable(model: TableModel, ctx?: ViewerCtx, hdoc?: HydratedDoc, _frameWidth?: number): HTMLTableElement {
  const table = document.createElement("table");
  table.className = "sheet-table";
  if (model.name) {
    const cap = document.createElement("caption");
    cap.className = "table-caption";
    cap.style.captionSide = "top";
    // The table NAME carries a real style (TableModel.nameStyle, from
    // table_name_style / table_name_shape_style): every fixture in this
    // campaign resolves to centred text with 6pt of space under it, which
    // is exactly what Numbers' exports draw. Centre is the fallback when a
    // document stores no name style; left-aligned grey read as a caption.
    const ns = model.nameStyle;
    cap.style.textAlign = ns?.paragraph?.horizontalAlignment && ns.paragraph.horizontalAlignment !== "auto"
      ? ns.paragraph.horizontalAlignment : "center";
    if (ns?.text) applyCharStyle(cap, ns.text);
    if (ns?.paragraph?.spaceAfterPt) cap.style.marginBottom = `${ns.paragraph.spaceAfterPt}px`;
    cap.textContent = model.name;
    table.appendChild(cap);
  }

  const grid = model.grid;
  const formats = model.formats;
  const { anchor, covered } = indexMerges(model.merges);

  const visRows: number[] = [];
  for (let r = 0; r < model.rowCount; r++) if (!model.rows?.[r]?.hidden) visRows.push(r);
  const visCols: number[] = [];
  for (let c = 0; c < model.columnCount; c++) if (!model.columns?.[c]?.hidden) visCols.push(c);

  const headEnd = model.headerRowCount;
  const footStart = model.rowCount - model.footerRowCount;

  const cg = document.createElement("colgroup");
  let totalW = 0;
  let allWidthsKnown = visCols.length > 0;
  for (const c of visCols) {
    const col = document.createElement("col");
    // a stored 0 means "the default" (cdrky's pre-BNC sheet stores 0 for
    // ten of twelve columns and Numbers draws them at the default width)
    const w = model.columns?.[c]?.sizePt || model.defaultColumnWidthPt;
    if (w) {
      col.style.width = `${w}px`;
      totalW += w;
    } else allWidthsKnown = false;
    cg.appendChild(col);
  }
  table.appendChild(cg);
  // Stored column widths are exact: fixed layout + explicit table width,
  // otherwise the auto algorithm shrink-to-fits the container and every
  // column collapses toward min-content (lafs_playlist wrapped 3-6 lines
  // per cell inside its drawable box).
  if (allWidthsKnown && totalW > 0) {
    // The stored column widths — NOT the drawable frame — are the table's
    // drawn width. The TableInfo frame is a cache that goes stale: three
    // tables in cdrky's budget and three in maison-martos share the same
    // 494x233 "new table" default while their columns sum to 1202, 717,
    // 282, ... Measured in Apple's own PDF export (pymupdf, widest grid
    // stroke run): burndown 61.1..1060.2 = 999pt for a 998.1pt column sum
    // (frame 899.7); maison 72..1177 = 1105 for 1104.7 (frame 686);
    // 72..790 = 718 for 717.0 (frame 494). A table wider than the page is
    // clipped at the margin, not scaled.
    table.style.tableLayout = "fixed";
    table.style.width = `${totalW}px`;
    table.classList.add("exact-cols"); // lifts the base min-width guard
  }

  let sectionEl: HTMLTableSectionElement | null = null;
  let sectionKind: string | null = null;
  let bodyOrdinal = -1;
  const banded = model.style?.bandedRows && model.style.bandedFill?.type === "solid"
    ? model.style.bandedFill.color : undefined;
  // A table whose style carries real stroke info paints ONLY its own
  // borders — the base gray gridlines would add lines Apple doesn't draw
  // (02_Invoice has horizontal rules only).
  if (model.style?.bodyCellStyle?.borders) table.classList.add("own-strokes");
  for (const r of visRows) {
    const kind = r < headEnd ? "thead" : r >= footStart ? "tfoot" : "tbody";
    if (kind !== sectionKind) {
      sectionKind = kind;
      sectionEl = document.createElement(kind) as HTMLTableSectionElement;
      table.appendChild(sectionEl);
    }
    const tr = document.createElement("tr");
    const info = model.rows?.[r];
    // stored height wins (CSS height on a <tr> is a minimum — content can
    // still grow it); 0/absent rows auto-fit their content like Apple.
    // Do NOT fall back to defaultRowHeightPt: Apple auto-fits unsized rows
    // (mini-calendar rows render ~13px under a 25.9pt stored default).
    if (info?.sizePt) tr.style.height = `${info.sizePt}px`;
    if (kind === "tbody") bodyOrdinal++;
    for (const c of visCols) {
      if (covered.has(cellKey(r, c))) continue;
      const cell: GridCell | null = grid[r]?.[c] ?? null;
      const header = r < headEnd || c < model.headerColumnCount;
      const footer = r >= footStart && !header;
      const td = document.createElement(header ? "th" : "td") as HTMLTableCellElement;
      const merge = anchor.get(cellKey(r, c));
      if (merge) {
        if (merge.rowSpan > 1) td.rowSpan = merge.rowSpan;
        if (merge.columnSpan > 1) td.colSpan = merge.columnSpan;
      }
      const norm = cell !== null ? asCell(cell) : null;
      // Section default first (header-row/header-column/footer/body look
      // from the table style — Apple's templates keep the whole header
      // look there and no per-cell styles), then the per-cell style
      // overrides on top.
      const section = r < headEnd ? model.style?.headerRowCellStyle
        : c < model.headerColumnCount ? model.style?.headerColumnCellStyle
        : r >= footStart ? model.style?.footerRowCellStyle
        : model.style?.bodyCellStyle;
      // banded rows: every second BODY row takes the banded fill; section
      // and per-cell fills paint over it (G5 acid table, Apple pattern:
      // 2nd/4th/... body rows banded)
      if (banded !== undefined && !header && !footer && bodyOrdinal % 2 === 1) {
        td.style.backgroundColor = banded;
      }
      if (section) applyCellStyle(td, section, header, footer, ctx);
      const style = cellStyleOf(model, norm?.cellStyleIndex);
      // A cell with its OWN style resolves fill through that style's
      // parent chain, not the table's section default or banding: 0839b6d2
      // (docx import) stores fill-less cell styles over a blue-banded table
      // style and Pages paints white cells.
      if (style && style.fill === null) {
        td.style.backgroundColor = "";
        td.style.backgroundImage = "";
      }
      applyCellStyle(td, style, header, footer, ctx);
      if (norm) {
        const format = norm.fmt !== undefined ? formats[norm.fmt] : undefined;
        // Apple convention: numeric-formatted values right-align; an
        // explicit paragraph alignment (already applied) wins over the auto
        const formatAligns = format !== undefined && ["number", "currency", "percent", "date", "duration"].includes(format.kind);
        const typedAligns = norm.type === "date" || norm.type === "duration" || norm.type === "currency";
        const numeric = typeof norm.v === "number" || typedAligns || (norm.type === undefined && formatAligns && typeof norm.v !== "string" && typeof norm.v !== "boolean");
        if (!td.style.textAlign && numeric && norm.type !== "error") td.style.textAlign = "right";
        if (norm.type === "error") td.classList.add("cell-error");
        const text = valueToText(norm, format);
        const rich = norm.type === "richtext" && typeof norm.v === "object" && norm.v !== null && "paragraphs" in norm.v
          ? norm.v : null;
        if (rich && hdoc && ctx) {
          // Rich-text cells keep their runs: the cell style's text look is
          // only the base (0839b6d2, a docx import, stores a 1pt cell font
          // under 11pt runs — flattened, "Nome:" vanished into a 1px line).
          td.replaceChildren(renderStyledText(rich, hdoc, ctx));
        } else {
          td.textContent = text;
          // multi-paragraph cell text (rich-text cells join with \n) keeps
          // its line structure like Apple
          if (text.includes("\n")) td.style.whiteSpace = "pre-line";
        }
      }
      td.dataset.row = String(r);
      td.dataset.col = String(c);
      tr.appendChild(td);
    }
    sectionEl!.appendChild(tr);
  }
  return table;
}