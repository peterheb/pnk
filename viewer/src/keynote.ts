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
  slideNumber: number,
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

  // Resolved-inheritance contract (docs/model-review.md §3b): the converter
  // emits Slide.background already master-resolved and masterDrawables as the
  // filtered underlay — paint background, underlay, drawables, verbatim.
  applyStageBackground(inner, slide.background ?? null, ctx);
  for (const d of slide.masterDrawables ?? []) inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));
  for (const d of slide.drawables) {
    // The slide-number placeholder paints only when the slide shows numbers
    // (Apple hides it otherwise); its page-number field bakes to the real
    // index so it reads "2", not a field label.
    if (roleOf(d) === "slide-number") {
      if (!slide.slideNumberVisible) continue;
      inner.appendChild(renderCanvasDrawable(bakePageNumber(d, slideNumber), hdoc, ctx));
      continue;
    }
    // Empty placeholders are editor chrome: Keynote's own export paints
    // nothing for them (their theme para styles can carry stray borders).
    if (roleOf(d) && !hasVisibleText(d)) continue;
    inner.appendChild(renderCanvasDrawable(d, hdoc, ctx));
  }

  frame.appendChild(inner);
  return frame;
}

// -- slide background -------------------------------------------------------

/** Image extensions <img> cannot rasterize (Apple renders them natively). */
const VECTOR_FILL = /\.(pdf|ai|eps)$/i;

/**
 * Best-effort CSS gradient from PDF-based vector art (.ai/.pdf background
 * fills). Modern .ai files are PDF-compatible, and their shading DICTS are
 * plain text even when content streams are Flate-compressed: an axial
 * gradient carries `/ShadingType 2` with `/C0 [...]` / `/C1 [...]` function
 * endpoints (PDF 32000-1 §8.7.4.5.3). The axis direction lives in the
 * compressed stream's transform, so we assume top-to-bottom — the common
 * orientation for slide backdrops [inferred]. Returns null when no shading
 * is found (fully-compressed or raster-only art).
 */
async function vectorArtGradientCss(url: string): Promise<string | null> {
  try {
    const buf = await (await fetch(url)).arrayBuffer();
    const text = new TextDecoder("latin1").decode(buf);
    if (!/\/ShadingType\s*2/.test(text)) return null;
    const comp = (name: string): number[] | null => {
      const m = text.match(new RegExp(`\\/${name}\\s*\\[([^\\]]*)\\]`));
      if (!m) return null;
      const nums = m[1].trim().split(/\s+/).map(Number).filter((n) => !Number.isNaN(n));
      return nums.length ? nums : null;
    };
    const rgb = (c: number[]): string | null => {
      const to255 = (v: number) => Math.round(Math.min(1, Math.max(0, v)) * 255);
      if (c.length === 3) return `rgb(${to255(c[0])},${to255(c[1])},${to255(c[2])})`;
      if (c.length === 1) return `rgb(${to255(c[0])},${to255(c[0])},${to255(c[0])})`;
      if (c.length === 4) {
        // DeviceCMYK -> naive RGB
        const [cy, mg, ye, k] = c;
        return `rgb(${to255((1 - cy) * (1 - k))},${to255((1 - mg) * (1 - k))},${to255((1 - ye) * (1 - k))})`;
      }
      return null;
    };
    const c0 = comp("C0"), c1 = comp("C1");
    if (!c0 || !c1) return null;
    const a = rgb(c0), b = rgb(c1);
    return a && b ? `linear-gradient(180deg, ${a}, ${b})` : null;
  } catch {
    return null;
  }
}

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
 * Paint the stage background (the fill arrives master-resolved from the
 * converter, model-review §3b). A CSS solid/gradient fill paints directly;
 * a raster image fill paints via <img>; an unrenderable image fill (vector
 * art / theme assets whose bytes .key files do not ship) degrades to a
 * name-keyed approximation, upgraded async by PDF shading-dict sniffing.
 */
function applyStageBackground(
  inner: HTMLElement,
  slideFill: Fill | null,
  ctx: ViewerCtx,
): void {
  const img = renderableImageFill(slideFill, ctx);
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
  const css = slideFill && slideFill.type !== "image" ? fillToCss(slideFill) : undefined;
  if (css) {
    inner.style.background = css;
    return;
  }
  if (slideFill?.type === "image") {
    // Built-in theme art is not shipped in .key files (DataInfo names
    // survive, bytes don't); approximate the known Keynote backdrop assets
    // by name, else leave white.
    const name = slideFill.image.preferredFileName ?? slideFill.image.fileName ?? "";
    const approx = THEME_BACKDROP_APPROX[name.toLowerCase()];
    if (approx) inner.style.background = approx;
    // Vector art (.ai/.pdf) backgrounds whose bytes DID ship: sniff an axial
    // shading gradient out of the PDF dictionaries and paint it (async —
    // upgrades the backdrop as soon as the bytes parse).
    const url = ctx.url(slideFill.image.dataId);
    if (url && VECTOR_FILL.test(name)) {
      void vectorArtGradientCss(url).then((grad) => {
        if (grad) inner.style.background = grad;
      });
    }
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

/** Placeholder role of a drawable, when it has one. Master-underlay
 * filtering itself lives in the converter now (model-review §3b). */
function roleOf(d: Drawable): string | null {
  const c = "common" in d && d.common ? (d.common as DrawableCommon) : null;
  return c?.placeholder?.role ?? null;
}

/** Replace page-number fields with the slide's real number, inheriting the
 * char style of a neighboring styled run (Keynote pairs the field with an
 * empty styled run carrying its look). */
function bakePageNumber(d: Drawable, n: number): Drawable {
  if (!("text" in d) || !d.text) return d;
  let changed = false;
  const paragraphs = d.text.paragraphs.map((p) => {
    const cStyle = p.items.reduce<number | undefined>(
      (acc, it) =>
        acc ?? (typeof it === "object" && it && "cStyle" in it ? (it as { cStyle?: number }).cStyle : undefined),
      undefined,
    );
    const items = p.items.map((it) => {
      const isPageField =
        typeof it === "object" && it !== null && "type" in it
        && (it as { type?: string }).type === "field"
        && (it as { field?: { kind?: string } }).field?.kind === "page-number";
      if (!isPageField) return it;
      changed = true;
      return cStyle === undefined ? { text: String(n) } : { text: String(n), cStyle };
    });
    return { ...p, items };
  });
  return changed ? ({ ...d, text: { ...d.text, paragraphs } } as Drawable) : d;
}

/** True when the drawable carries at least one non-whitespace text run. */
function hasVisibleText(d: Drawable): boolean {
  if (!("text" in d) || !d.text) return false;
  for (const p of d.text.paragraphs) {
    for (const item of p.items) {
      if (typeof item === "string") {
        if (item.trim()) return true;
      } else if ("text" in item && typeof item.text === "string" && item.text.trim()) {
        return true;
      } else if ("type" in item && item.type === "field") {
        return true; // fields (page number etc.) render content
      }
    }
  }
  return false;
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

  const frame = buildCanvas(slide, doc, hdoc, ctx, widthPx, index + 1);
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

  // Corner badge only when no slide-number placeholder painted the number
  // on the canvas itself.
  if (slide.slideNumberVisible && !slide.drawables.some((d) => roleOf(d) === "slide-number")) {
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
    item.appendChild(buildCanvas(slide, doc, hdoc, ctx, THUMB_WIDTH, i + 1));
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