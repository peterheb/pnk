// Keynote (.key): slide list + per-slide canvas + presenter notes.

import type { Fill } from "../../model/src/shared";
import type { Drawable, DrawableCommon, KeynoteDocument, Slide } from "../../model/src/keynote";
import type { ViewerCtx } from "./ctx";
import { fillToCss, renderCanvasDrawable } from "./drawables";
import { renderStyledText } from "./text";
import type { HydratedDoc } from "./hydrate";

const THUMB_WIDTH = 168;

function buildCanvas(
  slide: Slide,
  doc: KeynoteDocument,
  hdoc: HydratedDoc,
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

  const master = doc.masters.find((m) => m.name === slide.masterName);
  applyStageBackground(inner, slide.background ?? null, master?.background ?? null, ctx);

  // master furniture first (decorations), skipping furniture the slide
  // overrides: same placeholder role, same rounded geometry, or — for master
  // text prompts stored as plain shapes (no placeholder identity) — a slide
  // text drawable covering the prompt's frame (Keynote bakes slide copies of
  // title/footer/subtitle prompts at (nearly) the master's frame).
  const slideRoles = new Set(slide.drawables.map(roleOf).filter((r): r is string => !!r));
  const slideGeoms = new Set(slide.drawables.map(geomKey).filter((k): k is string => !!k));
  const slideTextFrames = slide.drawables
    .filter((d) => textOf(d) !== null)
    .map(frameOf)
    .filter((f): f is Frame => !!f);
  if (master) {
    for (const d of master.drawables) {
      const role = roleOf(d);
      if (role && slideRoles.has(role)) continue;
      const key = geomKey(d);
      if (key && slideGeoms.has(key)) continue;
      if (textOf(d)) {
        const f = frameOf(d);
        if (f && slideTextFrames.some((sf) => covers(f, sf))) continue;
      }
      inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));
    }
  }
  for (const d of slide.drawables) inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));

  frame.appendChild(inner);
  return frame;
}

// -- slide background -------------------------------------------------------

/** Image extensions <img> cannot rasterize (Apple renders them natively). */
const VECTOR_FILL = /\.(pdf|ai|eps)$/i;

/** A stage-renderable image fill, or null (missing bytes / vector art). */
function renderableImageFill(
  f: Fill | null,
  ctx: ViewerCtx,
): { url: string; objectFit: string } | null {
  if (!f || f.type !== "image") return null;
  const name = f.image.preferredFileName ?? f.image.fileName ?? "";
  const url = ctx.url(f.image.dataId);
  if (!url || VECTOR_FILL.test(name)) return null;
  const objectFit =
    f.technique === "scale-to-fill" ? "cover"
    : f.technique === "scale-to-fit" ? "contain"
    : "fill";
  return { url, objectFit };
}

/**
 * Paint the stage background. The slide's fill wins; an unrenderable slide
 * fill (missing bytes / vector art) falls back to the master's fill — decks
 * often carry the raster twin of a slide's vector background there. A CSS
 * solid/gradient fill paints directly; when only an unrenderable image fill
 * resolves (e.g. a built-in theme asset whose bytes .key files do not ship),
 * approximate Keynote's default dark backdrop.
 */
function applyStageBackground(
  inner: HTMLElement,
  slideFill: Fill | null,
  masterFill: Fill | null,
  ctx: ViewerCtx,
): void {
  const img = renderableImageFill(slideFill, ctx) ?? renderableImageFill(masterFill, ctx);
  if (img) {
    const imgEl = document.createElement("img");
    imgEl.src = img.url;
    imgEl.alt = "slide background";
    imgEl.style.position = "absolute";
    imgEl.style.inset = "0";
    imgEl.style.width = "100%";
    imgEl.style.height = "100%";
    imgEl.style.objectFit = img.objectFit;
    inner.appendChild(imgEl);
    return;
  }
  const cssOf = (f: Fill | null): string | undefined =>
    f && f.type !== "image" ? fillToCss(f) : undefined;
  const css = cssOf(slideFill) ?? cssOf(masterFill);
  if (css) {
    inner.style.background = css;
    return;
  }
  if (slideFill || masterFill) {
    // Built-in theme art is not shipped in .key files (DataInfo names
    // survive, bytes don't); approximate the known Keynote backdrop assets
    // by name, else leave white.
    const name =
      (slideFill?.type === "image" ? slideFill.image.preferredFileName ?? slideFill.image.fileName : null)
      ?? (masterFill?.type === "image" ? masterFill.image.preferredFileName ?? masterFill.image.fileName : null)
      ?? "";
    const approx = THEME_BACKDROP_APPROX[name.toLowerCase()];
    if (approx) inner.style.background = approx;
  }
}

/**
 * Approximations for Apple-shipped theme backdrop assets whose bytes .key
 * files omit (Keynote renders them from its app-bundled theme store).
 */
const THEME_BACKDROP_APPROX: Record<string, string> = {
  // Keynote default template spotlight backdrop (a9d4f68c "Home" deck).
  "spotlight_10x7.jpeg":
    "radial-gradient(ellipse 95% 80% at 50% 32%, #8f8f8f 0%, #6d6d6d 38%, #4b4b4b 68%, #3a3a3a 100%)",
  // News/photo-deck cream paper texture (5089b9c7 deck).
  "cotton_paper_hd.jpeg": "radial-gradient(ellipse 120% 100% at 50% 40%, #f7f3e8 0%, #efe9d8 60%, #e7dfc9 100%)",
};

// -- master furniture suppression ------------------------------------------

/** Placeholder role of a drawable, when it has one. */
function roleOf(d: Drawable): string | null {
  const c = commonOf(d);
  return c?.placeholder?.role ?? null;
}

/** Rounded position+size signature of a drawable, when fully placed. */
function geomKey(d: Drawable): string | null {
  const c = commonOf(d);
  if (!c?.position || !c.size) return null;
  const r = (v: number) => Math.round(v);
  return `${r(c.position.x)},${r(c.position.y)},${r(c.size.width)},${r(c.size.height)}`;
}

/** DrawableCommon of any modeled drawable (`unknown` may carry none). */
function commonOf(d: Drawable): DrawableCommon | null {
  if ("common" in d && d.common) return d.common;
  return null;
}

/** Positioned frame of a drawable, when fully placed. */
interface Frame {
  x: number;
  y: number;
  w: number;
  h: number;
}

function frameOf(d: Drawable): Frame | null {
  const c = commonOf(d);
  if (!c?.position || !c.size) return null;
  return { x: c.position.x, y: c.position.y, w: c.size.width, h: c.size.height };
}

/** First non-empty text run of a drawable, when it carries visible text. */
function textOf(d: Drawable): string | null {
  if (!("text" in d) || !d.text) return null;
  for (const p of d.text.paragraphs) {
    for (const item of p.items) {
      if (typeof item === "string") {
        if (item.trim()) return item;
      } else if ("text" in item && typeof item.text === "string" && item.text.trim()) {
        return item.text;
      }
    }
  }
  return null;
}

/** True when `outer` covers at least 60% of `inner`'s area. */
function covers(outer: Frame, inner: Frame): boolean {
  const ix = Math.max(0, Math.min(outer.x + outer.w, inner.x + inner.w) - Math.max(outer.x, inner.x));
  const iy = Math.max(0, Math.min(outer.y + outer.h, inner.y + inner.h) - Math.max(outer.y, inner.y));
  const area = inner.w * inner.h;
  return area > 0 && (ix * iy) / area >= 0.6;
}

function renderStage(
  doc: KeynoteDocument,
  hdoc: HydratedDoc,
  ctx: ViewerCtx,
  slide: Slide,
  index: number,
  widthPx: number,
): HTMLElement {
  const stage = document.createElement("div");
  stage.className = "slide-stage";

  const frame = buildCanvas(slide, doc, hdoc, ctx, widthPx);
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
    ? renderStyledText(slide.notes, hdoc, ctx)
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

export function renderKeynote(
  doc: KeynoteDocument,
  hdoc: HydratedDoc,
  ctx: ViewerCtx,
  mount: HTMLElement,
): void {
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
    stageSlot.appendChild(renderStage(doc, hdoc, ctx, doc.slides[index], index, stageSlot.clientWidth || 800));
    for (const item of list.children) {
      item.classList.toggle("active", (item as HTMLElement).dataset.slideIndex === String(index));
    }
  };

  doc.slides.forEach((slide, i) => {
    const item = document.createElement("div");
    item.className = "slide-list-item";
    item.dataset.slideIndex = String(i);
    item.appendChild(buildCanvas(slide, doc, hdoc, ctx, THUMB_WIDTH));
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