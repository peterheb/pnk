// Keynote (.key): slide list + per-slide canvas + presenter notes.

import type { KeynoteDocument, Slide } from "../../model/src/keynote";
import type { ViewerCtx } from "./ctx";
import { renderCanvasDrawable } from "./drawables";
import { renderStyledText } from "./text";

const THUMB_WIDTH = 168;

function buildCanvas(
  slide: Slide,
  doc: KeynoteDocument,
  ctx: ViewerCtx,
  widthPx: number,
): HTMLElement {
  const { width, height } = doc.slideSize;
  const scale = widthPx / width;

  const frame = document.createElement("div");
  frame.className = "canvas-frame";
  frame.style.aspectRatio = `${width} / ${height}`;

  const inner = document.createElement("div");
  inner.className = "canvas-inner";
  inner.style.width = `${width}px`;
  inner.style.height = `${height}px`;
  inner.style.transform = `scale(${scale})`;

  // master furniture first (background, placeholders), then slide content
  const master = doc.masters.find((m) => m.name === slide.masterName);
  if (master) for (const d of master.drawables) inner.appendChild(renderCanvasDrawable(d, ctx));
  for (const d of slide.drawables) inner.appendChild(renderCanvasDrawable(d, ctx));

  frame.appendChild(inner);
  return frame;
}

function renderStage(doc: KeynoteDocument, ctx: ViewerCtx, slide: Slide, index: number, widthPx: number): HTMLElement {
  const stage = document.createElement("div");
  stage.className = "slide-stage";

  const frame = buildCanvas(slide, doc, ctx, widthPx);
  frame.dataset.slideIndex = String(index);
  stage.appendChild(frame);

  const caption = document.createElement("div");
  caption.className = "slide-caption muted";
  const bits = [`Slide ${index + 1}${slide.name ? ` — ${slide.name}` : ""}`];
  if (slide.masterName) bits.push(`master: ${slide.masterName}`);
  if (slide.transition?.effect) bits.push(`transition: ${slide.transition.effect}`);
  if (slide.skipped) bits.push("skipped");
  caption.textContent = bits.join("  ·  ");
  stage.appendChild(caption);

  const notes = document.createElement("div");
  notes.className = "notes-panel";
  notes.dataset.hasNotes = slide.notes ? "true" : "false";
  const h = document.createElement("h3");
  h.textContent = "Presenter notes";
  notes.appendChild(h);
  notes.appendChild(slide.notes
    ? renderStyledText(slide.notes, ctx)
    : Object.assign(document.createElement("p"), { textContent: "No notes on this slide.", className: "muted" }));
  stage.appendChild(notes);

  if (slide.slideNumberVisible) {
    const num = document.createElement("div");
    num.className = "slide-number";
    num.textContent = String(index + 1);
    stage.appendChild(num);
  }
  return stage;
}

export function renderKeynote(doc: KeynoteDocument, ctx: ViewerCtx, mount: HTMLElement): void {
  const view = document.createElement("div");
  view.id = "keynote-view";

  const list = document.createElement("div");
  list.className = "slide-list";

  const stageSlot = document.createElement("div");
  stageSlot.className = "slide-stage-slot";

  let active = doc.slides.findIndex((s) => !s.skipped);
  if (active < 0) active = 0;

  const activate = (index: number) => {
    stageSlot.replaceChildren();
    stageSlot.appendChild(renderStage(doc, ctx, doc.slides[index], index, stageSlot.clientWidth || 800));
    for (const item of list.children) {
      item.classList.toggle("active", (item as HTMLElement).dataset.slideIndex === String(index));
    }
  };

  doc.slides.forEach((slide, i) => {
    const item = document.createElement("div");
    item.className = "slide-list-item";
    item.dataset.slideIndex = String(i);
    item.appendChild(buildCanvas(slide, doc, ctx, THUMB_WIDTH));
    const label = document.createElement("span");
    label.className = "label";
    label.textContent = `${i + 1}${slide.name ? ` · ${slide.name}` : ""}${slide.skipped ? " (skipped)" : ""}`;
    item.appendChild(label);
    item.addEventListener("click", () => activate(i));
    list.appendChild(item);
  });

  view.appendChild(list);
  view.appendChild(stageSlot);
  mount.appendChild(view);
  activate(active);
}