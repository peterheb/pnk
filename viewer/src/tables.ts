// TableModel -> HTML <table>: real cell values, merges spanned from the
// anchor, header/footer sections, per-cell resolved styles.
//
// Consumes the dense row-major `grid` + deduped `formats` pool (the
// amended TableModel contract).

import type {
  CellFormat,
  CellStyle,
  CellValue,
  TableModel,
  TableMerge,
} from "../../model/src/shared";
import { applyCharStyle } from "./text";

// Document locale, set from meta.locale after parse: comma-decimal locales
// render "5,48" the way the source app would; others keep ".".
const COMMA_DECIMAL = /^(de|fr|it|es|pt|nl|da|fi|nb|sv|el|pl|ru|tr)(-|$)/i;
let decimalComma = false;

export function setTableLocale(locale: string | undefined): void {
  decimalComma = !!locale && COMMA_DECIMAL.test(locale);
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
/** Fixed-decimal display, trailing zeros trimmed, in the document's locale. */
function formatNumber(v: number, decimals: number | undefined): string {
  const n = decimals !== undefined ? Math.min(Math.max(decimals, 0), 8) : undefined;
  // unformatted cells: 12 significant digits kills double-repr noise
  // (388.59999999999997 -> 388.6) without hiding real precision
  let s = n !== undefined ? v.toFixed(n) : Number(v.toPrecision(12)).toString();
  if (decimalComma) s = s.replace(".", ",");
  return s;
}

/** Best-effort display string for a tagged cell value. */
function valueToText(value: CellValue, format: CellFormat | undefined): string {
  switch (value.type) {
    case "empty": return "";
    case "number": {
      if (format?.kind === "percent") {
        // iWork stores percent as a fraction; the format displays it ×100
        return `${formatNumber(value.value * 100, format.decimals)}%`;
      }
      const decimals = format && (format.kind === "number" || format.kind === "automatic")
        ? format.decimals : undefined;
      return formatNumber(value.value, decimals);
    }
    case "text": return value.value;
    case "bool": return value.value ? "true" : "false";
    case "date": return value.value.slice(0, 10);
    case "duration": return `${value.value}s`;
    case "currency": return `${value.currencyCode ?? "$"} ${formatNumber(value.value, format?.decimals)}`;
    case "error": return value.value;
    case "richtext": return value.text.paragraphs.map((p) => p.items.map((i) => (i.type === "text" ? i.text : i.type === "field" ? (i.value ?? "") : "")).join("")).join("\n");
  }
}

function applyCellStyle(td: HTMLTableCellElement, style: CellStyle | undefined, header: boolean, footer: boolean): void {
  const s = td.style;
  if (style?.fill) s.backgroundColor = style.fill.type === "solid" ? style.fill.color : "#e8e8ee";
  if (style?.borders) {
    const b = style.borders;
    if (b.top) s.borderTop = `${b.top.widthPt}px solid ${b.top.color}`;
    if (b.right) s.borderRight = `${b.right.widthPt}px solid ${b.right.color}`;
    if (b.bottom) s.borderBottom = `${b.bottom.widthPt}px solid ${b.bottom.color}`;
    if (b.left) s.borderLeft = `${b.left.widthPt}px solid ${b.left.color}`;
  }
  if (style?.text) applyCharStyle(td, style.text);
  if (style?.verticalAlignment) s.verticalAlign = style.verticalAlignment === "middle" ? "middle" : style.verticalAlignment === "bottom" ? "bottom" : "top";
  if (style?.padding) s.padding = `${style.padding.top ?? 4}px ${style.padding.right ?? 8}px ${style.padding.bottom ?? 4}px ${style.padding.left ?? 8}px`;
  if (style?.textWrap) s.whiteSpace = "normal";
  if (header) td.classList.add("cell-header");
  if (footer) td.classList.add("cell-footer");
}

/** One TableModel as a real table; hidden rows/columns are skipped. */
export function renderTable(model: TableModel): HTMLTableElement {
  const table = document.createElement("table");
  table.className = "sheet-table";
  if (model.name) {
    const cap = document.createElement("caption");
    cap.className = "table-caption";
    cap.style.captionSide = "top";
    cap.style.textAlign = "left";
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
  for (const c of visCols) {
    const col = document.createElement("col");
    const w = model.columns?.[c]?.sizePt ?? model.defaultColumnWidthPt;
    if (w) col.style.width = `${w}px`;
    cg.appendChild(col);
  }
  table.appendChild(cg);

  let sectionEl: HTMLTableSectionElement | null = null;
  let sectionKind: string | null = null;
  for (const r of visRows) {
    const kind = r < headEnd ? "thead" : r >= footStart ? "tfoot" : "tbody";
    if (kind !== sectionKind) {
      sectionKind = kind;
      sectionEl = document.createElement(kind) as HTMLTableSectionElement;
      table.appendChild(sectionEl);
    }
    const tr = document.createElement("tr");
    const info = model.rows?.[r];
    if (info?.sizePt) tr.style.height = `${info.sizePt}px`;
    for (const c of visCols) {
      if (covered.has(cellKey(r, c))) continue;
      const cell = grid[r]?.[c] ?? null;
      const header = r < headEnd || c < model.headerColumnCount;
      const footer = r >= footStart && !header;
      const td = document.createElement(header ? "th" : "td") as HTMLTableCellElement;
      const merge = anchor.get(cellKey(r, c));
      if (merge) {
        if (merge.rowSpan > 1) td.rowSpan = merge.rowSpan;
        if (merge.columnSpan > 1) td.colSpan = merge.columnSpan;
      }
      applyCellStyle(td, cell?.style, header, footer);
      if (cell) {
        const format = cell.formatIndex !== undefined ? formats[cell.formatIndex] : undefined;
        if (cell.value.type === "error") td.classList.add("cell-error");
        td.textContent = valueToText(cell.value, format);
      }
      td.dataset.row = String(r);
      td.dataset.col = String(c);
      tr.appendChild(td);
    }
    sectionEl!.appendChild(tr);
  }
  return table;
}