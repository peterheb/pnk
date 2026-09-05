// Numbers (.numbers): sheet tabs; each sheet is a free-form canvas holding
// tables, charts, images and shapes in absolute point coordinates.

import type { NumbersDocument, Sheet } from "../../model/src/numbers";
import type { TableModel } from "../../model/src/shared";
import type { ViewerCtx } from "./ctx";
import { applyTextFit, renderCanvasDrawable } from "./drawables";
import type { HydratedDoc } from "./hydrate";
import { spillUnwrappedCells, tableDrawnHeight, tableDrawnWidth } from "./tables";

function drawableExtent(
  d: { type: string; table?: TableModel; chart?: { legendFrame?: { x: number; y: number; width: number; height: number } }; common?: { position?: { x: number; y: number }; size?: { width: number; height: number } }; children?: unknown[] },
  cur: { x: number; y: number },
): void {
  if (d.common?.position && d.common.size) {
    // A chart's legend frame is relative to the frame's centre and often
    // hangs BELOW the frame (burndown's sprint charts: y = +179 in a 300pt
    // box); the canvas must reach it or the legend is clipped away.
    const lf = d.type === "chart" ? d.chart?.legendFrame : undefined;
    if (lf) {
      cur.x = Math.max(cur.x, d.common.position.x + d.common.size.width / 2 + lf.x + lf.width);
      cur.y = Math.max(cur.y, d.common.position.y + d.common.size.height / 2 + lf.y + lf.height + 4);
    }
    // A table draws at the sum of its column widths; its stored frame is a
    // stale cache (see tables.ts). Take the wider of the two so the canvas
    // never cuts a table off (cdrky's County Tax Rates: 1202pt of columns
    // in a 494pt frame).
    const w = d.type === "table" && d.table
      ? Math.max(d.common.size.width, tableDrawnWidth(d.table))
      : d.common.size.width;
    // The stored height can be stale-tall as well: a pre-BNC budget sheet
    // keeps a 3628pt frame on a 43-row table, which made the canvas (and
    // the screenshot) five times taller than the content. Rows decide; the
    // post-layout fit grows the canvas if wrapped rows run taller.
    const drawnH = d.type === "table" && d.table ? tableDrawnHeight(d.table) : 0;
    const h = drawnH > 0 ? drawnH : d.common.size.height;
    cur.x = Math.max(cur.x, d.common.position.x + w);
    cur.y = Math.max(cur.y, d.common.position.y + h);
  }
  if (d.type === "group") {
    for (const ch of (d.children ?? []) as Parameters<typeof drawableExtent>[0][]) drawableExtent(ch, cur);
  }
}

function sheetExtent(sheet: Sheet): { width: number; height: number } {
  const cur = { x: 720, y: 480 };
  for (const d of sheet.drawables) drawableExtent(d, cur);
  return { width: cur.x + 40, height: cur.y + 40 };
}

/** Grow the canvas to the drawn content. The stored table frame is a stale
 * cache of the table's size and unsized rows auto-fit in the DOM, so the
 * model-derived extent can be short by hundreds of points (a French
 * property listing lost its bottom 40% of rows to the element box). Must
 * run after the sheet is in the document, so offsets are laid out. */
function fitCanvasToContent(area: HTMLElement): void {
  const canvas = area.querySelector<HTMLElement>(".sheet-canvas");
  if (!canvas) return;
  let w = canvas.offsetWidth;
  let h = canvas.offsetHeight;
  for (const el of Array.from(canvas.children) as HTMLElement[]) {
    w = Math.max(w, el.offsetLeft + Math.max(el.offsetWidth, el.scrollWidth) + 40);
    h = Math.max(h, el.offsetTop + Math.max(el.offsetHeight, el.scrollHeight) + 40);
  }
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
}

function renderSheet(sheet: Sheet, hdoc: HydratedDoc, ctx: ViewerCtx, index: number): HTMLElement {
  const area = document.createElement("div");
  area.className = "sheet-area";
  area.dataset.sheetIndex = String(index);

  const canvas = document.createElement("div");
  canvas.className = "sheet-canvas";
  const ext = sheetExtent(sheet);
  canvas.style.width = `${ext.width}px`;
  canvas.style.height = `${ext.height}px`;

  for (const d of sheet.drawables) canvas.appendChild(renderCanvasDrawable(d, hdoc, ctx));

  area.appendChild(canvas);
  return area;
}

export function renderNumbers(doc: NumbersDocument, hdoc: HydratedDoc, ctx: ViewerCtx, mount: HTMLElement): void {
  const view = document.createElement("div");
  view.id = "numbers-view";

  const tabs = document.createElement("div");
  tabs.className = "sheet-tabs";
  const areaSlot = document.createElement("div");

  const activate = (index: number) => {
    areaSlot.replaceChildren();
    areaSlot.appendChild(renderSheet(doc.sheets[index], hdoc, ctx, index));
    // Font-metric tolerance shrink for shape/textbox text (measurement
    // pass; keynote has run it since c94861a — sheets clipped instead:
    // proteger-les-donnees red banner cut its own caption).
    applyTextFit(areaSlot);
    spillUnwrappedCells(areaSlot);
    fitCanvasToContent(areaSlot);
    for (const tab of tabs.children) {
      tab.classList.toggle("active", (tab as HTMLElement).dataset.sheetIndex === String(index));
    }
  };

  doc.sheets.forEach((sheet, i) => {
    const tab = document.createElement("button");
    tab.type = "button";
    tab.className = "sheet-tab";
    tab.dataset.sheetIndex = String(i);
    if (sheet.hidden) tab.classList.add("sheet-hidden");
    tab.textContent = sheet.name;
    tab.addEventListener("click", () => activate(i));
    tabs.appendChild(tab);
  });

  view.appendChild(tabs);
  view.appendChild(areaSlot);
  mount.appendChild(view);
  activate(0);
}