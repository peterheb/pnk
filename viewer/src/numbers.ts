// Numbers (.numbers): sheet tabs; each sheet is a free-form canvas holding
// tables, charts, images and shapes in absolute point coordinates.

import type { NumbersDocument, Sheet } from "../../model/src/numbers";
import type { ViewerCtx } from "./ctx";
import { renderCanvasDrawable } from "./drawables";
import type { HydratedDoc } from "./hydrate";

function drawableExtent(
  d: { type: string; common?: { position?: { x: number; y: number }; size?: { width: number; height: number } }; children?: unknown[] },
  cur: { x: number; y: number },
): void {
  if (d.common?.position && d.common.size) {
    cur.x = Math.max(cur.x, d.common.position.x + d.common.size.width);
    cur.y = Math.max(cur.y, d.common.position.y + d.common.size.height);
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