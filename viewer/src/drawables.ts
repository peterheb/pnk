// Drawable rendering. Two contexts:
//   - canvas: absolutely positioned inside a scaled .canvas-inner (Keynote
//     slides, Pages layout pages, Numbers sheet canvases) — geometry in
//     points, rendered 1pt = 1px and scaled by the parent.
//   - flow: in-stream content (inline attachments, floating groups rendered
//     after a word-processing body).
//
// Shapes become inline SVG paths from the flattened curve data; images come
// from ViewerCtx object URLs; tables/charts delegate to their renderers.

import type {
  ChartModel,
  CurveElement,
  Drawable,
  DrawableCommon,
  Fill,
  ShapeGeometry,
  Stroke,
} from "../../model/src/shared";
import type { ViewerCtx } from "./ctx";
import { renderTable } from "./tables";
import { renderStyledText } from "./text";
import type { HydratedDoc } from "./hydrate";

function el(tag: string, className?: string): HTMLElement {
  const e = document.createElement(tag);
  if (className) e.className = className;
  return e;
}

// ---------------------------------------------------------------------------
// Fills / strokes -> CSS + SVG
// ---------------------------------------------------------------------------

/** CSS background for a fill; image fills degrade to a neutral tone. */
export function fillToCss(f: Fill | undefined): string | undefined {
  if (!f) return undefined;
  if (f.type === "solid") return f.color;
  if (f.type === "gradient") {
    const stops = f.gradient.stops
      .map((s) => `${s.color} ${(s.fraction * 100).toFixed(1)}%`)
      .join(", ");
    // model: angle 0 = left→right CCW; CSS: 0deg = to top
    const a = f.gradient.kind === "linear" ? (f.gradient.angleDeg ?? 0) : 0;
    return `linear-gradient(${90 - a}deg, ${stops})`;
  }
  // A tinted image fill (tile/pattern textures): the tint is the visible
  // color modulating a near-white texture — paint it when bytes are absent.
  return f.tint ?? "#d9d9de";
}

function svgStrokeAttrs(e: SVGElement, stroke: Stroke | undefined, scale: number): void {
  if (!stroke) return;
  e.setAttribute("stroke", stroke.color);
  e.setAttribute("stroke-width", String(Math.max(0.5, stroke.widthPt * scale)));
  e.setAttribute("stroke-linecap", stroke.cap);
  e.setAttribute("stroke-linejoin", stroke.join);
  // Apple emits placeholder dash arrays of all zeros for solid strokes; SVG
  // would render those as invisible zero-length dashes.
  if (stroke.dash?.some((d) => d > 0)) e.setAttribute("stroke-dasharray", stroke.dash.map((d) => d * scale).join(" "));
}

function svgGradientDefs(svg: SVGSVGElement, style: DrawableCommon["style"]): void {
  const g = style?.fill;
  if (!g || g.type !== "gradient") return;
  const defs = document.createElementNS("http://www.w3.org/2000/svg", "defs");
  const lg = document.createElementNS("http://www.w3.org/2000/svg", "linearGradient");
  lg.id = "grad-" + Math.random().toString(36).slice(2, 8);
  const a = ((g.gradient.angleDeg ?? 0) * Math.PI) / 180;
  lg.setAttribute("x1", String(0.5 - Math.cos(a) / 2));
  lg.setAttribute("y1", String(0.5 + Math.sin(a) / 2));
  lg.setAttribute("x2", String(0.5 + Math.cos(a) / 2));
  lg.setAttribute("y2", String(0.5 - Math.sin(a) / 2));
  for (const s of g.gradient.stops) {
    const stop = document.createElementNS("http://www.w3.org/2000/svg", "stop");
    stop.setAttribute("offset", String(s.fraction));
    stop.setAttribute("stop-color", s.color);
    lg.appendChild(stop);
  }
  defs.appendChild(lg);
  svg.appendChild(defs);
  svg.dataset.fillRef = lg.id;
}

// ---------------------------------------------------------------------------
// Shape geometry -> SVG path data
// ---------------------------------------------------------------------------

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));

/** Path data for preset shape families (drawn directly in w×h space). */
function presetPathD(preset: string, g: ShapeGeometry, w: number, h: number): string {
  const f = (n: number) => +n.toFixed(2);
  switch (preset) {
    case "rect":
    case "rectangle":
    case "plain-rect":
      return `M0,0 L${f(w)},0 L${f(w)},${f(h)} L0,${f(h)} Z`;
    case "circle":
    case "ellipse":
    case "oval":
      return `M0,${f(h / 2)} A${f(w / 2)},${f(h / 2)} 0 1 1 ${f(w)},${f(h / 2)} A${f(w / 2)},${f(h / 2)} 0 1 1 0,${f(h / 2)} Z`;
    case "diamond":
      return `M${f(w / 2)},0 L${f(w)},${f(h / 2)} L${f(w / 2)},${f(h)} L0,${f(h / 2)} Z`;
    case "star": {
      // scalar = pointiness (0..1): inner radius shrinks as it grows
      const inner = 0.5 - 0.3 * clamp(g.scalar ?? 0.4, 0, 1);
      const pts: string[] = [];
      for (let i = 0; i < 10; i++) {
        const r = i % 2 === 0 ? 0.5 : inner;
        const a = (Math.PI / 5) * i - Math.PI / 2;
        pts.push(`${f(w / 2 + Math.cos(a) * r * w)},${f(h / 2 + Math.sin(a) * r * h)}`);
      }
      return `M${pts.join(" L")} Z`;
    }
    case "plus": {
      const t = w / 3, u = h / 3;
      return `M${f(t)},0 L${f(2 * t)},0 L${f(2 * t)},${f(u)} L${f(w)},${f(u)} L${f(w)},${f(2 * u)} L${f(2 * t)},${f(2 * u)} L${f(2 * t)},${f(h)} L${f(t)},${f(h)} L${f(t)},${f(2 * u)} L0,${f(2 * u)} L0,${f(u)} L${f(t)},${f(u)} Z`;
    }
    case "chevron": {
      const t = w * 0.25;
      return `M0,0 L${f(w - t)},0 L${f(w)},${f(h / 2)} L${f(w - t)},${f(h)} L0,${f(h)} L${f(t)},${f(h / 2)} Z`;
    }
    case "left-arrow":
    case "right-arrow":
    case "up-arrow":
    case "down-arrow": {
      // simple block arrow pointing in the preset's direction
      const head = preset.endsWith("left") || preset.endsWith("right") ? w * 0.35 : h * 0.35;
      const bar = (preset.endsWith("left") || preset.endsWith("right") ? h : w) * 0.45;
      const pts: Record<string, [number, number][]> = {
        "right-arrow": [[0, (h - bar) / 2], [w - head, (h - bar) / 2], [w - head, 0], [w, h / 2], [w - head, h], [w - head, (h + bar) / 2], [0, (h + bar) / 2]],
        "left-arrow": [[w, (h - bar) / 2], [head, (h - bar) / 2], [head, 0], [0, h / 2], [head, h], [head, (h + bar) / 2], [w, (h + bar) / 2]],
        "down-arrow": [[(w - bar) / 2, 0], [(w + bar) / 2, 0], [(w + bar) / 2, h - head], [w, h - head], [w / 2, h], [0, h - head], [(w - bar) / 2, h - head]],
        "up-arrow": [[(w - bar) / 2, h], [(w + bar) / 2, h], [(w + bar) / 2, head], [w, head], [w / 2, 0], [0, head], [(w - bar) / 2, head]],
      };
      return "M" + pts[preset].map(([x, y]) => `${f(x)},${f(y)}`).join(" L") + " Z";
    }
    case "callout": {
      // rounded rect body + a triangular tail toward tailPosition
      const tail = g.callout?.tailPosition;
      const tailW = g.callout ? g.callout.tailSize.width : 0;
      const tailH = g.callout ? g.callout.tailSize.height : 0;
      const base = presetPathD("rect", g, w, h);
      if (!tail || !tailH) return base;
      const tx = clamp(tail.x * (w / (g.naturalSize?.width ?? w)), 0, w - tailW);
      const ty = tail.y > (g.naturalSize?.height ?? h) / 2 ? h : 0;
      const dir = ty === 0 ? -1 : 1;
      return `${base} M${f(tx)},${f(ty)} L${f(tx + tailW)},${f(ty)} L${f(clamp(tail.x + tailW / 2, 0, w))},${f(ty + dir * tailH)} Z`;
    }
    default:
      // rounded-rect + every preset we don't specialize
      {
        const r = Math.min((g.scalar ?? 8) * (g.naturalSize ? w / g.naturalSize.width : 1), w / 2, h / 2);
        return `M${f(r)},0 L${f(w - r)},0 Q${f(w)},0 ${f(w)},${f(r)} L${f(w)},${f(h - r)} Q${f(w)},${f(h)} ${f(w - r)},${f(h)} L${f(r)},${f(h)} Q0,${f(h)} 0,${f(h - r)} L0,${f(r)} Q0,0 ${f(r)},0 Z`;
      }
  }
}

/** Explicit CurvePath (naturalSize space) -> scaled path `d`, else null. */
function explicitPathD(g: ShapeGeometry, w: number, h: number): string | null {
  if (!g.path) return null;
  const els = g.path.elements;
  // standalone line fast path: exactly one move + one line, coordinates
  // already in the drawable's point space (no naturalSize, no scaling)
  if (els.length === 2 && els[0].type === "move" && els[1].type === "line") {
    const [x1, y1] = els[0].points;
    const [x2, y2] = els[1].points;
    return `M${x1},${y1} L${x2},${y2}`;
  }
  // Degenerate natural dimensions (0-height rules) must not scale to NaN.
  const nw = g.naturalSize?.width || w || 1;
  const nh = g.naturalSize?.height || h || 1;
  const sx = nw > 0 ? w / nw : 1;
  const sy = nh > 0 ? (h > 0 ? h / nh : 1) : 1;
  const d: string[] = [];
  for (const e of els) {
    // flat positional pairs: [x,y], [cx,cy,x,y], [c1x,c1y,c2x,c2y,x,y]
    const pts = e.type === "close" ? [] : e.points;
    const xy: string[] = [];
    for (let i = 0; i < pts.length; i += 2) {
      xy.push(`${(pts[i] * sx).toFixed(2)},${(pts[i + 1] * sy).toFixed(2)}`);
    }
    if (e.type === "move") d.push(`M${xy[0]}`);
    else if (e.type === "line") d.push(`L${xy[0]}`);
    else if (e.type === "quad") d.push(`Q${xy[0]} ${xy[1]}`);
    else if (e.type === "cubic") d.push(`C${xy[0]} ${xy[1]} ${xy[2]}`);
    else d.push("Z");
  }
  return d.join(" ");
}

/** Full path data for a shape: explicit bezier wins, else the preset. */
function shapePathD(g: ShapeGeometry, w: number, h: number): string {
  return explicitPathD(g, w, h) ?? presetPathD(g.preset ?? "rounded-rect", g, w, h);
}

// ---------------------------------------------------------------------------
// SVG shape
// ---------------------------------------------------------------------------

function shapeSvg(g: ShapeGeometry, w: number, h: number, style: DrawableCommon["style"]): SVGSVGElement {
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg") as SVGSVGElement;
  // Degenerate boxes are real: Keynote stores horizontal/vertical rules as
  // 0-height/0-width stroked line shapes. A 0 viewBox dimension disables SVG
  // rendering entirely, and the parent div's 0px also collapses the viewport —
  // pin both to >=1 and let the stroke paint outside via overflow.
  const vw = Math.max(w, 1);
  const vh = Math.max(h, 1);
  svg.setAttribute("viewBox", `0 0 ${vw} ${vh}`);
  svg.setAttribute("preserveAspectRatio", "none");
  svg.style.overflow = "visible";
  if (w < 1 || h < 1) {
    svg.style.width = `${vw}px`;
    svg.style.height = `${vh}px`;
  }
  // Stroke-width scale: average of the finite axis ratios (degenerate
  // 0-height/0-width rules would otherwise divide 0/0 into a NaN width).
  const ratios = [
    [w, g.naturalSize?.width] as const,
    [h, g.naturalSize?.height] as const,
  ]
    .map(([dim, nat]) => (nat && nat > 0 ? dim / nat : NaN))
    .filter((r) => Number.isFinite(r) && r > 0);
  const scale = ratios.length ? ratios.reduce((a, b) => a + b, 0) / ratios.length : 1;
  svgGradientDefs(svg, style);
  const path = document.createElementNS(NS, "path");
  path.setAttribute("d", shapePathD(g, w, h));
  const fill = style?.fill;
  path.setAttribute("fill", fill ? (fill.type === "gradient" ? `url(#${svg.dataset.fillRef})` : (fillToCss(fill) ?? "none")) : "none");
  const stroke = style?.stroke;
  if (stroke) svgStrokeAttrs(path, stroke, scale);
  svg.appendChild(path);
  return svg;
}

// ---------------------------------------------------------------------------
// Content pieces
// ---------------------------------------------------------------------------

function imageEl(
  dataId: string | undefined,
  fileName: string | undefined,
  ctx: ViewerCtx,
  alt?: string,
  thumbnail?: { dataId: string; fileName?: string; preferredFileName?: string },
): HTMLElement {
  const url = dataId ? ctx.url(dataId) : undefined;
  const vector = /\.(pdf|ai|eps)$/i.test(fileName ?? "");
  if (url && !vector) {
    const img = document.createElement("img");
    img.src = url;
    img.alt = alt ?? fileName ?? "image";
    img.style.width = "100%";
    img.style.height = "100%";
    img.style.objectFit = "fill";
    return img;
  }
  // <img> cannot rasterize vector art (PDF/AI/EPS) — fall back to the
  // converter-emitted raster thumbnail (Keynote stores one alongside vector
  // media, e.g. -small-*.png twins) before degrading to a labeled placeholder.
  const thumbUrl = thumbnail?.dataId ? ctx.url(thumbnail.dataId) : undefined;
  const thumbName = thumbnail ? thumbnail.fileName ?? thumbnail.preferredFileName : undefined;
  const thumbRaster = thumbUrl && !/\.(pdf|ai|eps)$/i.test(thumbName ?? "");
  if (thumbRaster) {
    const img = document.createElement("img");
    img.src = thumbUrl;
    img.alt = alt ?? fileName ?? "image";
    img.style.width = "100%";
    img.style.height = "100%";
    img.style.objectFit = "fill";
    return img;
  }
  const miss = el("div", "media-missing");
  miss.textContent = fileName
    ? vector ? `vector art: ${fileName.replace(/\.(pdf|ai|eps)$/i, "")}` : `${fileName} (media missing)`
    : "media missing";
  return miss;
}

function movieEl(d: Extract<Drawable, { type: "movie" }>, ctx: ViewerCtx): HTMLElement {
  if (d.poster && ctx.url(d.poster.dataId)) return imageEl(d.poster.dataId, d.poster.preferredFileName, ctx, "movie poster");
  if (d.remoteUrl) {
    const box = el("div", "media-missing");
    box.textContent = `linked movie: ${d.remoteUrl}`;
    return box;
  }
  const box = el("div", "media-missing");
  box.textContent = "movie (no bytes available)";
  return box;
}

function verticalAlignStyle(d: { verticalAlignment?: string }): string {
  return d.verticalAlignment === "middle" ? "center" : d.verticalAlignment === "bottom" ? "flex-end" : "flex-start";
}

/** Text content of a textbox/shape, filling the positioned container. */
function textLayer(d: Drawable & { text?: unknown; common?: DrawableCommon }, doc: HydratedDoc, ctx: ViewerCtx): HTMLElement | null {
  if (!("text" in d) || !d.text || (d.text as { paragraphs?: unknown[] }).paragraphs === undefined) return null;
  const layer = el("div", "drawable-text");
  layer.style.alignItems = verticalAlignStyle(d as { verticalAlignment?: string });
  // NOTE on units: canvas geometry renders 1 document-point = 1px, and
  // text.ts emits sizes in CSS `px` for the same reason (commit c94861a) —
  // together they keep 1pt of text = 1px of canvas with no extra scaling.
  // (An earlier 0.75 layer transform compensated the old CSS-`pt` emission;
  // both fixes active would double-shrink — keep exactly one.)
  const inner = el("div", "drawable-text-inner");
  inner.appendChild(renderStyledText(d.text as never, doc, ctx));
  layer.appendChild(inner);
  return layer;
}

// ---------------------------------------------------------------------------
// Chart (minimal: inline numeric series -> SVG bars, else a summary card)
// ---------------------------------------------------------------------------

function chartSummary(d: Extract<Drawable, { type: "chart" }>): HTMLElement {
  const card = el("div", "unknown-drawable");
  const c = d.chart;
  const seriesDesc = c.series.map((s) => s.name ?? "series").join(", ");
  card.textContent = `${c.type}${c.threeD ? " (3D)" : ""} chart — ${c.series.length} series, ${c.categories.length} categories${seriesDesc ? `: ${seriesDesc}` : ""}`;
  return card;
}

function chartSvg(chart: ChartModel, w: number, h: number): SVGSVGElement | null {
  const numeric = chart.series.every((s) => s.values.every((v) => v === null || typeof v === "number"));
  if (!numeric || chart.series.length === 0 || chart.categories.length === 0) return null;
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg") as SVGSVGElement;
  svg.setAttribute("viewBox", `0 0 ${w} ${h}`);
  const max = Math.max(...chart.series.flatMap((s) => s.values.map((v) => (typeof v === "number" ? v : 0))), 1e-9);
  const colors = chart.seriesColors ?? ["#4a90d9", "#e0762e", "#7bb662", "#b0578d", "#5b6abf"];
  const groupW = w / chart.categories.length;
  const barW = (groupW * 0.7) / chart.series.length;
  chart.series.forEach((s, si) => {
    s.values.forEach((v, vi) => {
      if (typeof v !== "number") return;
      const bar = document.createElementNS(NS, "rect");
      const bh = (v / max) * (h - 8);
      bar.setAttribute("x", String(vi * groupW + groupW * 0.15 + si * barW));
      bar.setAttribute("y", String(h - bh));
      bar.setAttribute("width", String(barW));
      bar.setAttribute("height", String(bh));
      bar.setAttribute("fill", colors[si % colors.length]);
      svg.appendChild(bar);
    });
  });
  return svg;
}

// ---------------------------------------------------------------------------
// Public entry points
// ---------------------------------------------------------------------------

/** Position + rotate + opacity from DrawableCommon, 1pt = 1px. */
export function applyCommonGeometry(div: HTMLElement, c: DrawableCommon): void {
  const s = div.style;
  if (c.position) {
    s.left = `${c.position.x}px`;
    s.top = `${c.position.y}px`;
  }
  if (c.size) {
    s.width = `${c.size.width}px`;
    s.height = `${c.size.height}px`;
  }
  if (c.angleDeg) s.transform = `rotate(${-c.angleDeg}deg)`;
  if (c.opacity !== undefined) s.opacity = String(c.opacity);
}

/** One drawable on a canvas: absolutely positioned inside .canvas-inner. */
export function renderCanvasDrawable(d: Drawable, doc: HydratedDoc, ctx: ViewerCtx): HTMLElement {
  const div = el("div", "canvas-drawable");
  if (d.type === "unknown" || !d.common) {
    div.className = "canvas-drawable unknown-drawable";
    div.textContent = d.type === "unknown" ? `unmodeled ${d.typeName ?? `type ${d.typeId}`}: ${d.reason}` : "drawable";
    return div;
  }
  const c = d.common;
  applyCommonGeometry(div, c);
  if (c.accessibilityDescription) div.title = c.accessibilityDescription;

  const w = c.size?.width ?? 120;
  const h = c.size?.height ?? 60;

  if (d.type === "textbox") {
    const layer = textLayer(d, doc, ctx);
    if (layer) {
      // Zero-size textboxes (Keynote emits some badge labels at 0×0) carry
      // their text unclipped: let the content size the box instead.
      if (!c.size || (c.size.width === 0 && c.size.height === 0)) {
        div.style.width = "auto";
        div.style.height = "auto";
        div.style.overflow = "visible";
        layer.style.overflow = "visible";
        layer.style.position = "relative";
        layer.style.whiteSpace = "nowrap";
        layer.style.width = "max-content"; // percentage of an auto box is meaningless
        layer.style.height = "auto";
      }
      div.appendChild(layer);
    } else div.textContent = "";
  } else if (d.type === "shape") {
    const svg = shapeSvg(d.geometry, w, h, c.style);
    div.appendChild(svg);
    const layer = textLayer({ ...d, text: d.text, verticalAlignment: d.verticalAlignment, common: c }, doc, ctx);
    if (layer) div.appendChild(layer);
  } else if (d.type === "image") {
    const img = imageEl(d.image.dataId, d.image.preferredFileName ?? d.image.fileName, ctx, d.image.preferredFileName, d.thumbnail);
    const m = d.mask?.common;
    if (m?.position && m.size && m.size.width > 0 && m.size.height > 0) {
      // TSD.ImageArchive.mask: the mask frame is in the image drawable's own
      // space — show only that window, keeping the full-size image behind it
      // (ppd deck cover photo: 1770x1508 image cropped to a 1770x577 band).
      const wrap = el("div");
      wrap.style.position = "absolute";
      wrap.style.left = `${m.position.x}px`;
      wrap.style.top = `${m.position.y}px`;
      wrap.style.width = `${m.size.width}px`;
      wrap.style.height = `${m.size.height}px`;
      wrap.style.overflow = "hidden";
      img.style.position = "absolute";
      img.style.left = `${-m.position.x}px`;
      img.style.top = `${-m.position.y}px`;
      img.style.width = `${w}px`;
      img.style.height = `${h}px`;
      img.style.maxWidth = "none";
      wrap.appendChild(img);
      div.appendChild(wrap);
    } else {
      div.appendChild(img);
    }
  } else if (d.type === "movie") {
    div.appendChild(movieEl(d, ctx));
  } else if (d.type === "group") {
    // children are re-based into the group's space by the converter: render
    // them as canvas drawables inside this positioned container
    for (const child of d.children) div.appendChild(renderCanvasDrawable(child, doc, ctx));
  } else if (d.type === "connection-line") {
    const svg = shapeSvg({ path: d.path }, w, h, c.style);
    svg.setAttribute("viewBox", `0 0 ${w} ${h}`);
    div.appendChild(svg);
  } else if (d.type === "table") {
    const wrap = el("div", "canvas-table-wrap");
    wrap.appendChild(renderTable(d.table));
    div.appendChild(wrap);
  } else if (d.type === "chart") {
    const svg = chartSvg(d.chart, w, h);
    if (svg) {
      svgGradientDefs(svg, c.style);
      div.appendChild(svg);
    } else {
      div.appendChild(chartSummary(d));
    }
  }
  return div;
}

/** One drawable in a flow context (inline attachment / floating text stream). */
export function renderFlowDrawable(d: Drawable, doc: HydratedDoc, ctx: ViewerCtx): HTMLElement {
  if (d.type === "textbox") {
    const wrap = el("div", "flow-textbox");
    wrap.appendChild(renderStyledText(d.text, doc, ctx));
    return wrap;
  }
  if (d.type === "shape" && d.text) {
    const wrap = el("div", "flow-textbox");
    wrap.appendChild(renderStyledText(d.text, doc, ctx));
    return wrap;
  }
  if (d.type === "image") {
    const wrap = el("div", "flow-image");
    const url = ctx.url(d.image.dataId);
    if (url) {
      const img = document.createElement("img");
      img.src = url;
      img.alt = d.image.preferredFileName ?? "inline image";
      const nat = d.common?.size;
      if (nat) img.style.width = `${nat.width}px`;
      img.style.maxWidth = "100%";
      wrap.appendChild(img);
    } else {
      wrap.appendChild(imageEl(d.image.dataId, d.image.preferredFileName, ctx));
    }
    return wrap;
  }
  if (d.type === "table") {
    const wrap = el("div", "flow-table");
    wrap.appendChild(renderTable(d.table));
    return wrap;
  }
  if (d.type === "group") {
    const wrap = el("div", "flow-group");
    for (const child of d.children) wrap.appendChild(renderFlowDrawable(child, doc, ctx));
    return wrap;
  }
  if (d.type === "chart") return chartSummary(d);
  if (d.type === "shape") {
    const size = d.common?.size ?? { width: 60, height: 60 };
    const wrap = el("div", "flow-shape");
    wrap.style.width = `${size.width}px`;
    wrap.style.height = `${size.height}px`;
    wrap.appendChild(shapeSvg(d.geometry, size.width, size.height, d.common?.style));
    return wrap;
  }
  const card = el("div", "unknown-drawable");
  card.textContent = d.type === "unknown" ? `unmodeled drawable: ${d.reason}` : d.type;
  return card;
}