// Keynote (.key): slide list + per-slide canvas + presenter notes.

import type { Fill } from "../../model/src/shared";
import type { Drawable, DrawableCommon, KeynoteDocument, Slide } from "../../model/src/keynote";
import type { ViewerCtx } from "./ctx";
import { applyTextFit, fillToCss, renderCanvasDrawable } from "./drawables";
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
  // The frame's 1px border eats into its border-box: scale to the CONTENT
  // box or the right/bottom edges of every canvas get clipped by
  // overflow:hidden (~12 doc-px on a thumbnail — visibly truncated).
  const scale = (widthPx - 2) / width;

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

/**
 * Second-chance backdrop sniff for vector art with no axial shading: inflate
 * the PDF's FlateDecode content streams (DecompressionStream "deflate" — PDF
 * streams are zlib-wrapped) and take the FIRST 3-component fill operator
 * (`r g b rg|sc|scn`) in a stream that draws paths — slide backdrops open
 * with a full-page fill (RIPE ea785d2e: `0.224 0.227 0.239 scn` charcoal
 * before the polygon texture) [inferred: single-page .ai backdrop layout].
 */
async function vectorArtFirstFillCss(url: string): Promise<string | null> {
  try {
    const bytes = new Uint8Array(await (await fetch(url)).arrayBuffer());
    const text = new TextDecoder("latin1").decode(bytes); // 1:1 byte mapping
    // Match `stream` but not `endstream` (Illustrator uses \r EOLs).
    const streamRe = /(^|[^d])stream\r?\n?/g;
    let mm: RegExpExecArray | null;
    let tries = 0;
    while ((mm = streamRe.exec(text)) && tries < 40) {
      const start = mm.index + mm[0].length;
      const end = text.indexOf("endstream", start);
      if (end < 0) break;
      tries++;
      let content: string | null = null;
      // zlib CMF: compression method nibble 8 (0x78 AND Illustrator's 0x48).
      if ((bytes[start] & 0x0f) === 8) {
        const chunks: number[] = [];
        try {
          const ds = new DecompressionStream("deflate");
          const reader = new Blob([bytes.slice(start, end)]).stream().pipeThrough(ds).getReader();
          // Tolerant read: PDF streams may carry trailing EOL junk that makes
          // DecompressionStream throw at the very end — keep what inflated.
          for (;;) {
            const r = await reader.read();
            if (r.done) break;
            for (const b of r.value) chunks.push(b);
          }
        } catch { /* partial output retained */ }
        if (chunks.length) content = new TextDecoder("latin1").decode(new Uint8Array(chunks));
      }
      if (content === null) content = text.slice(start, end); // plain-text form streams
      if (!/\d\s+[ml]\b/.test(content)) continue; // no path ops: not page content
      const m = content.match(/([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+(?:rg|scn|sc)\b/);
      if (!m) continue;
      const to255 = (v: string) => Math.round(Math.min(1, Math.max(0, Number(v))) * 255);
      return `rgb(${to255(m[1])},${to255(m[2])},${to255(m[3])})`;
    }
    return null;
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

/** Relative luminance below 0.35 for a solid fill, or for the average of a
 *  gradient's stops. */
function fillIsDark(fill: Fill): boolean {
  const lum = (hex: string): number => {
    const v = parseInt(hex.replace("#", "").slice(0, 6), 16);
    const c = [(v >> 16) & 255, (v >> 8) & 255, v & 255].map((x) => x / 255);
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
  };
  if (fill.type === "solid") return lum(fill.color) < 0.35;
  if (fill.type === "gradient" && fill.gradient.stops.length) {
    return fill.gradient.stops.reduce((a, st) => a + lum(st.color), 0) / fill.gradient.stops.length < 0.35;
  }
  return false;
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
    // Chart axes, labels and legends take currentColor: a dark backdrop
    // (RIPE's navy gradient) needs them light like Keynote draws them.
    if (fillIsDark(slideFill!)) inner.style.color = "#f2f2f4";
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
      void vectorArtGradientCss(url).then(async (grad) => {
        const css = grad ?? (await vectorArtFirstFillCss(url));
        if (css) inner.style.background = css;
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
  // Keynote '09 "Showroom" theme backdrop: light-gray studio-sweep gradient
  // (0f9df553 / b52c89c1 decks reference the jpeg, bytes not shipped).
  "showroom2_1024x768.jpeg":
    "linear-gradient(180deg, #e9eaec 0%, #dcdee1 45%, #c9cbd0 75%, #babcc2 100%)",
  "showroom_1024x768.jpeg":
    "linear-gradient(180deg, #e9eaec 0%, #dcdee1 45%, #c9cbd0 75%, #babcc2 100%)",
  "showroom_1024x768-1.jpeg":
    "linear-gradient(180deg, #e9eaec 0%, #dcdee1 45%, #c9cbd0 75%, #babcc2 100%)",
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

/** Fill one slide's stage holder: canvas, caption, notes strip, badge. */
function fillStage(
  stage: HTMLElement,
  doc: KeynoteDocument,
  hdoc: HydratedDoc,
  ctx: ViewerCtx,
  slide: Slide,
  index: number,
  widthPx: number,
): void {
  stage.replaceChildren();

  const frame = buildCanvas(slide, doc, hdoc, ctx, widthPx, index + 1);
  stage.appendChild(frame);

  const caption = document.createElement("div");
  caption.className = "slide-caption muted";
  // Caption label: the stored slide name, else the converter-derived
  // `slide.title` (first line, shortened) so the strip reads like an outline.
  const label = slide.name ?? slide.title?.split("\n")[0].slice(0, 80);
  const bits = [`Slide ${index + 1}${label ? ` — ${label}` : ""}`];
  if (slide.masterName) bits.push(`master: ${slide.masterName}`);
  if (slide.transition?.effect) bits.push(`transition: ${slide.transition.effect}`);
  if (slide.skipped) bits.push("skipped");
  caption.textContent = bits.join("  ·  ");
  stage.appendChild(caption);

  // Notes: a compact strip only when the slide HAS visible notes — nothing
  // to scroll past otherwise (a notes storage of empty paragraphs counts as
  // none). Typography is normalized to the strip (notes storages carry
  // 18pt+ editor styles that read as bloat here); bold/color survive.
  const hasNotes = slide.notes?.paragraphs.some((p) =>
    p.items.some((it) => (typeof it === "string" ? it.trim() : "type" in it ? true : it.text.trim())),
  );
  if (slide.notes && hasNotes) {
    const notes = document.createElement("div");
    notes.className = "notes-panel";
    const h = document.createElement("h3");
    h.textContent = "Notes";
    notes.appendChild(h);
    notes.appendChild(renderStyledText(slide.notes, hdoc, ctx));
    stage.appendChild(notes);
  }

  // Corner badge only when no slide-number placeholder painted the number
  // on the canvas itself.
  if (slide.slideNumberVisible && !slide.drawables.some((d) => roleOf(d) === "slide-number")) {
    const num = document.createElement("div");
    num.className = "slide-number";
    num.textContent = String(index + 1);
    frame.appendChild(num);
  }
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

  // One continuous scroll of every slide (like the Pages flow). Stages start
  // as aspect-ratio placeholders and render lazily as they approach the
  // viewport — the 645-slide monster deck must not pay for 645 full
  // canvases up front.
  const scroll = document.createElement("div");
  scroll.className = "slides-scroll";
  const stages: HTMLElement[] = [];
  const built = new Set<number>();

  const buildStage = (i: number): void => {
    if (built.has(i)) return;
    built.add(i);
    const stage = stages[i];
    fillStage(stage, doc, hdoc, ctx, doc.slides[i], i, stage.clientWidth || 800);
    // Shrink-to-fit measurement pass needs the stage attached and laid out.
    applyTextFit(stage);
  };

  // Skipped slides are what Keynote's player and its PDF export leave out
  // (greenberg.science: 28 slides, 25 exported pages); the deck shows the
  // same 25, numbered as stored so captions still match the file.
  const shown = doc.slides.map((slide, i) => ({ slide, i })).filter(({ slide }) => !slide.skipped);
  shown.forEach(({ i }) => {
    const stage = document.createElement("div");
    stage.className = "slide-stage";
    stage.dataset.slideIndex = String(i);
    const ph = document.createElement("div");
    ph.className = "canvas-frame";
    ph.style.aspectRatio = `${doc.slideSize.width} / ${doc.slideSize.height}`;
    stage.appendChild(ph);
    scroll.appendChild(stage);
    stages[i] = stage;
  });

  const io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) {
        if (!e.isIntersecting) continue;
        const i = Number((e.target as HTMLElement).dataset.slideIndex);
        io.unobserve(e.target);
        buildStage(i);
      }
    },
    { rootMargin: "1500px 0px" },
  );
  stages.forEach((s) => io.observe(s));

  const setActive = (index: number): void => {
    for (const item of list.children) {
      item.classList.toggle("active", (item as HTMLElement).dataset.slideIndex === String(index));
    }
  };

  shown.forEach(({ slide, i }) => {
    const item = document.createElement("div");
    item.className = "slide-list-item";
    item.dataset.slideIndex = String(i);
    item.appendChild(buildCanvas(slide, doc, hdoc, ctx, THUMB_WIDTH, i + 1));
    const label = document.createElement("span");
    label.className = "label";
    label.textContent = `${i + 1}${slide.name ? ` · ${slide.name}` : ""}`;
    item.appendChild(label);
    item.addEventListener("click", () => {
      buildStage(i);
      stages[i].scrollIntoView({ behavior: "smooth", block: "start" });
      setActive(i);
    });
    list.appendChild(item);
  });

  view.appendChild(list);
  view.appendChild(scroll);
  mount.appendChild(view);
  setActive(shown[0]?.i ?? 0);
  // Thumbnails were built detached; measure their shrink boxes now that the
  // whole view is attached.
  applyTextFit(list);
}