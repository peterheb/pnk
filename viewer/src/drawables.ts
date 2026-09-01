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
  LineEnd,
  ShapeGeometry,
  Stroke,
  StyledText,
} from "../../model/src/shared";
import type { ViewerCtx } from "./ctx";
import { renderTable } from "./tables";
import { renderStyledText } from "./text";
import { paraStyleOf, type HydratedDoc } from "./hydrate";

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
    case "regular-polygon": {
      // scalar = number of sides (fixture: G2 pentagon stores 5.0), first
      // vertex at 12 o'clock, inscribed in the w x h box like Apple draws it
      const n = Math.round(clamp(g.scalar ?? 5, 3, 24));
      const pts: string[] = [];
      for (let i = 0; i < n; i++) {
        const a = ((Math.PI * 2) / n) * i - Math.PI / 2;
        pts.push(`${f(w / 2 + (Math.cos(a) * w) / 2)},${f(h / 2 + (Math.sin(a) * h) / 2)}`);
      }
      return `M${pts.join(" L")} Z`;
    }
    case "double-arrow": {
      const head = w * 0.25;
      const bar = h * 0.45;
      const p: [number, number][] = [
        [0, h / 2], [head, 0], [head, (h - bar) / 2], [w - head, (h - bar) / 2], [w - head, 0],
        [w, h / 2], [w - head, h], [w - head, (h + bar) / 2], [head, (h + bar) / 2], [head, h],
      ];
      return "M" + p.map(([x, y]) => `${f(x)},${f(y)}`).join(" L") + " Z";
    }
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
// Line-end glyphs (arrowheads)
//
// TSD.LineEndArchive stores a canonical glyph path (e.g. "simple arrow" =
// triangle (0,0)-(3,6)-(6,0)) with +y pointing OUTWARD past the line tip.
// Placement fixture-verified against Apple's own PDF export (cdx-00243-21):
//   - the glyph's bbox top row (max y) sits exactly AT the path tip, apex on
//     the tip (arrow triangle (120.52,467.42)-(130.12,472.22)-(120.52,477.02)
//     for a line geometrically ending at x=130.12);
//   - glyph scale follows stroke width: 6x6 canonical renders 6.0pt at 1pt
//     stroke and 9.6pt at 2pt stroke -> s = 0.4 + 0.6*widthPt;
//   - the line's own stroke is cut back to the glyph base + width/2
//     (Apple strokes 98.14..121.52 under a 120.52..130.12 arrowhead);
//   - `head` decorates the path END point, `tail` the path START (the
//     "arrowhead" end is the one the user dragged to).
// ---------------------------------------------------------------------------

/** Tip position + outward unit direction at one end of an open path. */
interface PathTip {
  x: number;
  y: number;
  dx: number;
  dy: number;
}

/**
 * Start/end tips of an open explicit path, in the same (w x h) space the
 * path `d` is emitted in. Closed paths have no tips (line ends only apply
 * to open line/connection shapes).
 */
function pathTips(g: ShapeGeometry, w: number, h: number): { start?: PathTip; end?: PathTip } {
  const els = g.path?.elements;
  if (!els || els.length < 2) return {};
  // Same coordinate conventions as explicitPathD: a bare move+line pair is
  // already in drawable point space; everything else scales naturalSize->w,h.
  let sx = 1;
  let sy = 1;
  if (!(els.length === 2 && els[0].type === "move" && els[1].type === "line")) {
    const nw = g.naturalSize?.width || w || 1;
    const nh = g.naturalSize?.height || h || 1;
    sx = nw > 0 ? w / nw : 1;
    sy = nh > 0 ? (h > 0 ? h / nh : 1) : 1;
  }
  // Flatten to a vertex list (control points included — they carry the
  // tangent direction at the adjacent anchor, which is all we need).
  const pts: [number, number][] = [];
  for (const e of els) {
    if (e.type === "close") return {}; // closed shape: no free ends
    for (let i = 0; i < e.points.length; i += 2) pts.push([e.points[i] * sx, e.points[i + 1] * sy]);
  }
  if (pts.length < 2) return {};
  const unit = (from: [number, number], to: [number, number]): [number, number] | null => {
    const vx = to[0] - from[0];
    const vy = to[1] - from[1];
    const len = Math.hypot(vx, vy);
    return len > 1e-6 ? [vx / len, vy / len] : null;
  };
  // Outward at the start = away from the second vertex; at the end = away
  // from the second-to-last. Skip coincident control points.
  let startDir: [number, number] | null = null;
  for (let i = 1; i < pts.length && !startDir; i++) startDir = unit(pts[i], pts[0]);
  let endDir: [number, number] | null = null;
  for (let i = pts.length - 2; i >= 0 && !endDir; i--) endDir = unit(pts[i], pts[pts.length - 1]);
  const tips: { start?: PathTip; end?: PathTip } = {};
  if (startDir) tips.start = { x: pts[0][0], y: pts[0][1], dx: startDir[0], dy: startDir[1] };
  if (endDir) tips.end = { x: pts[pts.length - 1][0], y: pts[pts.length - 1][1], dx: endDir[0], dy: endDir[1] };
  return tips;
}

/** Fallback canonical glyph when the archive carried an identifier but no
 * path — the ubiquitous 6x6 arrow triangle. */
const DEFAULT_ARROW: [number, number][] = [
  [0, 0],
  [3, 6],
  [6, 0],
];

/**
 * Build the SVG element for one line-end glyph at `tip`, and report how far
 * the line's own stroke should be cut back (glyph height minus a half
 * stroke, per Apple's export).
 */
function lineEndGlyph(
  le: LineEnd,
  tip: PathTip,
  stroke: Stroke,
  strokeScale: number,
): { el: SVGPathElement; cutback: number } | null {
  const NS = "http://www.w3.org/2000/svg";
  let d = "";
  let xs: number[] = [];
  let ys: number[] = [];
  if (le.path && le.path.elements.length >= 2) {
    const parts: string[] = [];
    for (const e of le.path.elements) {
      const pts = e.type === "close" ? [] : e.points;
      const xy: string[] = [];
      for (let i = 0; i < pts.length; i += 2) {
        xs.push(pts[i]);
        ys.push(pts[i + 1]);
        xy.push(`${pts[i]},${pts[i + 1]}`);
      }
      if (e.type === "move") parts.push(`M${xy[0]}`);
      else if (e.type === "line") parts.push(`L${xy[0]}`);
      else if (e.type === "quad") parts.push(`Q${xy[0]} ${xy[1]}`);
      else if (e.type === "cubic") parts.push(`C${xy[0]} ${xy[1]} ${xy[2]}`);
      else parts.push("Z");
    }
    d = parts.join(" ");
  } else if (le.identifier) {
    // identifier-only archive: synthesize the canonical arrow triangle
    d = "M" + DEFAULT_ARROW.map(([x, y]) => `${x},${y}`).join(" L") + " Z";
    xs = DEFAULT_ARROW.map((p) => p[0]);
    ys = DEFAULT_ARROW.map((p) => p[1]);
  }
  if (!d || xs.length === 0) return null;
  const cx = (Math.min(...xs) + Math.max(...xs)) / 2;
  const yMax = Math.max(...ys);
  const height = yMax - Math.min(...ys);
  const s = (0.4 + 0.6 * stroke.widthPt) * strokeScale;
  // Rotation mapping glyph +y onto the outward direction (screen y-down):
  // R(theta)*(0,1) = (dx,dy) => theta = atan2(-dx, dy).
  const deg = (Math.atan2(-tip.dx, tip.dy) * 180) / Math.PI;
  const p = document.createElementNS(NS, "path");
  p.setAttribute("d", d);
  p.setAttribute(
    "transform",
    `translate(${tip.x.toFixed(2)} ${tip.y.toFixed(2)}) rotate(${deg.toFixed(2)}) scale(${s.toFixed(4)}) translate(${-cx} ${-yMax})`,
  );
  if (le.isFilled !== false) {
    p.setAttribute("fill", stroke.color);
  } else {
    p.setAttribute("fill", "none");
    p.setAttribute("stroke", stroke.color);
    // keep the outline weight in glyph units: the transform scale multiplies
    p.setAttribute("stroke-width", String(stroke.widthPt / Math.max(s / strokeScale, 0.1)));
  }
  return { el: p, cutback: Math.max(0, s * height - (stroke.widthPt * strokeScale) / 2) };
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
    // An inline-level svg sits on the text BASELINE of its 0-height div, which
    // shoves the painted line ~13px down (24_Briefing master rules) — and for
    // rotated ticks that offset turns into a sideways shift. Block layout pins
    // it to the div's top-left, where the geometry says it belongs.
    svg.style.display = "block";
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
  let d = shapePathD(g, w, h);
  const stroke = style?.stroke;

  // Line-end glyphs (arrowheads): head decorates the path END, tail the
  // START; the straight-line fast path also gets its stroke cut back to the
  // glyph base like Apple draws it.
  const ends = style?.lineEnds;
  let glyphs: SVGPathElement[] = [];
  if (ends && stroke && g.path) {
    const tips = pathTips(g, w, h);
    const head = ends.head && tips.end ? lineEndGlyph(ends.head, tips.end, stroke, scale) : null;
    const tail = ends.tail && tips.start ? lineEndGlyph(ends.tail, tips.start, stroke, scale) : null;
    const els = g.path.elements;
    if ((head || tail) && els.length === 2 && els[0].type === "move" && els[1].type === "line" && tips.start && tips.end) {
      const s = tips.start;
      const e = tips.end;
      const x1 = s.x - s.dx * (tail?.cutback ?? 0);
      const y1 = s.y - s.dy * (tail?.cutback ?? 0);
      const x2 = e.x - e.dx * (head?.cutback ?? 0);
      const y2 = e.y - e.dy * (head?.cutback ?? 0);
      d = `M${x1.toFixed(2)},${y1.toFixed(2)} L${x2.toFixed(2)},${y2.toFixed(2)}`;
    }
    glyphs = [head?.el, tail?.el].filter((x): x is SVGPathElement => !!x);
  }

  path.setAttribute("d", d);
  const fill = style?.fill;
  path.setAttribute("fill", fill ? (fill.type === "gradient" ? `url(#${svg.dataset.fillRef})` : (fillToCss(fill) ?? "none")) : "none");
  if (stroke) svgStrokeAttrs(path, stroke, scale);
  svg.appendChild(path);
  for (const gl of glyphs) svg.appendChild(gl);
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
  if (vector && fileName) {
    // Placed vector art with no raster twin: a neutral gray shape with a
    // small filename caption — not an error card (the artwork exists, we
    // just cannot rasterize PDF/AI/EPS in-browser).
    const box = el("div", "media-vector");
    const cap = el("span", "media-vector-caption");
    cap.textContent = fileName.replace(/\.(pdf|ai|eps)$/i, "").replace(/-\d+$/, "");
    box.appendChild(cap);
    return box;
  }
  const miss = el("div", "media-missing");
  miss.textContent = fileName ? `${fileName} (media missing)` : "media missing";
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
// Text fit (model textFit: "grow" | "shrink"; absent = fixed box, clipped)
// ---------------------------------------------------------------------------

/**
 * "grow": a plain Keynote/Pages text box auto-sizes its height to its
 * content; the stored height is Apple's layout under Apple's font metrics,
 * so browsers (taller line boxes, fallback fonts) treat it as a MINIMUM and
 * let the box grow downward instead of clipping the last line.
 * "shrink": tag the box for the post-attach measurement pass (applyTextFit).
 */
function applyTextFitMode(
  div: HTMLElement,
  layer: HTMLElement,
  fit: "grow" | "shrink" | undefined,
  verticalAlignment?: string,
): void {
  if (fit === "grow") {
    // keep the stored height as a minimum so vertical alignment still works
    // when the content is shorter than the box
    const storedH = div.style.height;
    div.style.height = "auto";
    if (storedH && storedH !== "auto") div.style.minHeight = storedH;
    div.style.display = "flex";
    div.style.flexDirection = "column";
    div.style.justifyContent = verticalAlignStyle({ verticalAlignment });
    layer.style.position = "relative";
    layer.style.overflow = "visible";
    layer.style.height = "auto";
  } else if (fit === "shrink") {
    div.dataset.textFit = "shrink";
  } else {
    // Fixed box (no flag). Apple laid the stored frame out with ITS font
    // metrics; browser fallback faces + line boxes run taller, so text that
    // fits exactly in Keynote clips mid-line here (0f9df553 byline: 132px of
    // content in a 102pt box). Tolerance mode: the measurement pass may
    // shrink bounded (>=0.6) to absorb that drift — never past it, so truly
    // authored overflow still clips like Apple's fixed frames do.
    div.dataset.textFit = "tolerance";
  }
}

/**
 * Post-attach pass for boxes tagged "shrink" (Keynote's "shrink text on
 * overflow"): when the laid-out text is taller than its box, scale it down.
 * `scale(s)` with an inverse width (100/s %) reproduces a font-size
 * reduction — same wrap width in text space — and the transform origin
 * follows the box's vertical alignment. MUST run with `root` attached and
 * displayed (measurement). Idempotent: safe to re-run.
 */
export function applyTextFit(root: HTMLElement): void {
  for (const box of root.querySelectorAll<HTMLElement>("[data-text-fit]")) {
    const layer = box.querySelector<HTMLElement>(":scope > .drawable-text");
    const inner = layer?.querySelector<HTMLElement>(":scope > .drawable-text-inner");
    if (!layer || !inner) continue;
    if (box.dataset.textFit === "edge-clamp") {
      // Zero-size auto box: scale down (bounded) when the laid-out label
      // spills past the canvas' right edge — Apple's metrics fit it inside.
      const canvas = box.closest<HTMLElement>(".canvas-inner");
      if (!canvas) continue;
      inner.style.transform = "";
      const spill = box.offsetLeft + box.offsetWidth - canvas.clientWidth;
      if (spill > 1 && box.offsetWidth > 0) {
        const s = Math.max((box.offsetWidth - spill) / box.offsetWidth, 0.7);
        inner.style.transform = `scale(${s.toFixed(4)})`;
        inner.style.transformOrigin = "left top";
      }
      continue;
    }
    // "shrink" = Keynote's shrink-on-overflow (scale as far as needed);
    // "tolerance" = fixed box, bounded shrink for font-metric drift only.
    const minScale = box.dataset.textFit === "shrink" ? 0.35 : 0.6;
    inner.style.transform = "";
    inner.style.width = "";
    // The inner is a flex ITEM (.drawable-text is the flex container for
    // vertical alignment); its default flex-shrink:1 silently squashed the
    // compensated >100% width back to the container's, so text wrapped at
    // the ORIGINAL width and the scale left a right-side gap — centered
    // lines drifted left by (1−s)/2·width (0d5851c0 slide 9 subtitle,
    // ~45pt off-center at s=0.89).
    inner.style.flex = "0 0 auto";
    const boxH = layer.clientHeight;
    if (boxH <= 0) continue;
    const align = layer.style.alignItems;
    const origin = align === "flex-end" ? "left bottom" : align === "center" ? "left center" : "left top";
    let s = 1;
    // Rewrapping at the compensated width changes the height, so iterate;
    // s only ever decreases, which converges without oscillation.
    for (let i = 0; i < 3; i++) {
      const contentH = inner.offsetHeight;
      if (contentH * s <= boxH + 0.5) break;
      s = Math.max(Math.min(s, boxH / contentH), minScale);
      inner.style.width = `${(100 / s).toFixed(3)}%`;
      inner.style.transform = `scale(${s.toFixed(4)})`;
      inner.style.transformOrigin = origin;
    }
  }
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
  const lineKinds = ["line", "area", "stacked-area", "scatter"];
  if (lineKinds.includes(chart.type)) {
    // Line family: one polyline per series through the category centers,
    // 0-based y like Apple's default axis (Running Log pace chart), with
    // small circle markers; area variants also fill down to the baseline.
    chart.series.forEach((s, si) => {
      const color = colors[si % colors.length];
      const pts: [number, number][] = [];
      s.values.forEach((v, vi) => {
        if (typeof v !== "number") return;
        pts.push([(vi + 0.5) * groupW, h - 4 - (v / max) * (h - 12)]);
      });
      if (!pts.length) return;
      if (chart.type.endsWith("area")) {
        const area = document.createElementNS(NS, "path");
        area.setAttribute("d", `M${pts[0][0]},${h} L` + pts.map(([x, y]) => `${x},${y}`).join(" L") + ` L${pts[pts.length - 1][0]},${h} Z`);
        area.setAttribute("fill", color);
        area.setAttribute("opacity", "0.25");
        svg.appendChild(area);
      }
      if (chart.type !== "scatter") {
        const line = document.createElementNS(NS, "polyline");
        line.setAttribute("points", pts.map(([x, y]) => `${x},${y}`).join(" "));
        line.setAttribute("fill", "none");
        line.setAttribute("stroke", color);
        line.setAttribute("stroke-width", "2.5");
        line.setAttribute("stroke-linejoin", "round");
        svg.appendChild(line);
      }
      for (const [x, y] of pts) {
        const dot = document.createElementNS(NS, "circle");
        dot.setAttribute("cx", String(x));
        dot.setAttribute("cy", String(y));
        dot.setAttribute("r", "3");
        dot.setAttribute("fill", "#fff");
        dot.setAttribute("stroke", color);
        dot.setAttribute("stroke-width", "2");
        svg.appendChild(dot);
      }
    });
    return svg;
  }
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

/**
 * A 0-size box is a point ANCHOR: text laid out in it overflows per its
 * alignment, so centered paragraphs center ON the stored position and
 * right-aligned ones end there; same vertically via the box's own vertical
 * alignment. Monster deck fixture: the centered "DGLAP, ERBL" tag stores
 * (888, 477) and Apple draws its box spanning 767..1016 x 456..507 —
 * dead-center on the point; the left-aligned "Transverse" label anchors
 * top-left as before. Applies to 0×0 textboxes and 0×0 shapes alike.
 */
function anchorZeroSizeText(
  div: HTMLElement,
  layer: HTMLElement,
  text: StyledText | undefined,
  verticalAlignment: string | undefined,
  doc: HydratedDoc,
): void {
  div.style.width = "auto";
  div.style.height = "auto";
  div.style.overflow = "visible";
  layer.style.overflow = "visible";
  layer.style.position = "relative";
  layer.style.whiteSpace = "nowrap";
  layer.style.width = "max-content"; // percentage of an auto box is meaningless
  layer.style.height = "auto";
  const paras = text?.paragraphs;
  const firstPara = paras?.find((p) => typeof p !== "string" && p.items.length > 0) ?? paras?.[0];
  const hAlign = paraStyleOf(doc, typeof firstPara === "string" ? undefined : firstPara?.pStyle)?.horizontalAlignment;
  const tx = hAlign === "center" ? "-50%" : hAlign === "right" ? "-100%" : "0%";
  const ty = verticalAlignment === "middle" ? "-50%" : verticalAlignment === "bottom" ? "-100%" : "0%";
  if (tx !== "0%" || ty !== "0%") {
    div.style.transform = `${div.style.transform ?? ""} translate(${tx}, ${ty})`.trim();
  } else {
    // Auto-sized boxes lay out at Apple's metrics; browser faces run
    // wider, so a label Apple fits to the slide edge can spill past it
    // ("James 3:13-18" bottom-right badges). The measurement pass
    // clamps (offset-based, so only valid untransformed).
    div.dataset.textFit = "edge-clamp";
  }
}

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
  if (c.shadow && c.shadow.kind === "drop") {
    // Angle convention fixture-verified on G2's pentagon (angle 45, offset 5
    // renders down-right in Apple's raster): dx = cos, dy = sin, CSS y-down.
    const a = (c.shadow.angleDeg * Math.PI) / 180;
    const dx = Math.cos(a) * c.shadow.offsetPt;
    const dy = Math.sin(a) * c.shadow.offsetPt;
    const [r, g, b] = hexRgb(c.shadow.color);
    s.filter = `drop-shadow(${dx.toFixed(1)}px ${dy.toFixed(1)}px ${c.shadow.radiusPt}px rgba(${r},${g},${b},${c.shadow.opacity}))`;
  }
  if (c.reflection) {
    // Chromium/WebKit only; other engines just skip the reflection.
    // Mask orientation, probe-verified on Chromium 151 (scratchpad
    // reflect-probe.html): the gradient is flipped WITH the mirrored copy,
    // so gradient-BOTTOM lands on the contact line. Apple's reflection is
    // strongest at contact (~opacity) and dead ~55% out (measured on
    // 3c844ac1 logos vs Keynote's export), so: transparent through the far
    // 45%, ramping to opaque at the bottom. The previous opaque-to-
    // transparent ramp painted the ghost far from the shape and nothing at
    // contact (G2's pentagon reflection appeared under the diamond).
    s.setProperty(
      "-webkit-box-reflect",
      `below 0px linear-gradient(transparent 45%, rgba(0,0,0,${c.reflection.opacity}))`,
    );
  }
}

function hexRgb(hex: string): [number, number, number] {
  const v = parseInt(hex.replace("#", "").slice(0, 6), 16);
  return [(v >> 16) & 255, (v >> 8) & 255, v & 255];
}

/** Soft elliptical blob under the drawable for kind="contact" shadows. */
function contactShadowEl(c: DrawableCommon): HTMLElement | null {
  const sh = c.shadow;
  if (!sh || sh.kind !== "contact" || !c.size) return null;
  const hFrac = sh.contact?.height ?? 0.25;
  const blob = el("div", "contact-shadow");
  const bh = Math.max(c.size.height * hFrac * 0.5, 4);
  blob.style.left = "5%";
  blob.style.width = "90%";
  blob.style.height = `${bh}px`;
  blob.style.top = `${c.size.height - bh / 2 + (sh.offsetPt ?? 0) / 2}px`;
  const [r, g, b] = hexRgb(sh.color);
  blob.style.background = `radial-gradient(ellipse closest-side, rgba(${r},${g},${b},${sh.opacity}), transparent)`;
  blob.style.filter = `blur(${Math.max(sh.radiusPt / 4, 2)}px)`;
  return blob;
}

/**
 * Box-shaped drawables (textboxes, images) paint their stroke as a CSS
 * border. A picture FRAME replaces the stroke: Pages' presets are a white
 * mat with a soft drop shadow (10a06959's "Formal Shadow" name-tag box —
 * the 2pt black stroke stored beneath it never shows). Mat width scales
 * with the frame's asset scale; the exact bitmap frames are not modeled.
 */
function applyBoxStroke(div: HTMLElement, stroke: Stroke | undefined): void {
  if (!stroke) return;
  if (stroke.frame) {
    const mat = Math.round(4 + 16 * (stroke.frame.assetScale ?? 0.5));
    div.style.border = `${mat}px solid #fff`;
    div.style.boxShadow = "0 2px 7px rgba(0,0,0,0.38)";
    div.style.boxSizing = "content-box";
    div.style.margin = `${-mat}px 0 0 ${-mat}px`;
    return;
  }
  if (stroke.widthPt > 0) div.style.border = `${stroke.widthPt}px solid ${stroke.color}`;
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

  const contact = contactShadowEl(c);
  if (contact) div.appendChild(contact);

  if (d.type === "textbox") {
    // Textboxes carry shape styling too: a filled label box loses its
    // background if only the text layer paints (monster deck: lavender
    // "Transverse density" tags, dark-red banner boxes rendered as ghost
    // text on nothing).
    const bg = fillToCss(c.style?.fill);
    if (bg) div.style.background = bg;
    applyBoxStroke(div, c.style?.stroke);
    const layer = textLayer(d, doc, ctx);
    if (layer) {
      // Zero-size textboxes (Keynote emits some badge labels at 0×0) carry
      // their text unclipped: let the content size the box instead.
      if (!c.size || (c.size.width === 0 && c.size.height === 0)) {
        anchorZeroSizeText(div, layer, d.text, d.verticalAlignment, doc);
      } else {
        applyTextFitMode(div, layer, d.textFit, d.verticalAlignment);
      }
      div.appendChild(layer);
    } else div.textContent = "";
  } else if (d.type === "shape") {
    // A 0-height shape whose PATH carries a real natural height is a full
    // box stored degenerate (proteger-les-donnees red banner: size 471x0,
    // path 471x32) — adopt the path height so its white caption gets the
    // band as its layout/fit box instead of spilling invisibly below it.
    const naturalH = d.geometry.naturalSize?.height ?? 0;
    const effH = h === 0 && d.geometry.path && naturalH > 1 ? naturalH : h;
    if (effH !== h) div.style.height = `${effH}px`;
    const svg = shapeSvg(d.geometry, w, effH, c.style);
    div.appendChild(svg);
    const layer = textLayer({ ...d, text: d.text, verticalAlignment: d.verticalAlignment, common: c }, doc, ctx);
    if (layer) {
      if (w === 0 && effH === 0) {
        // 0×0 shape carrying text: a point anchor exactly like the 0×0
        // textbox labels (0d5851c0 slide 29's 51pt quote — Apple lays it
        // out natural-width from the anchor; our 0-width box wrapped it
        // into a 4-line sliver).
        anchorZeroSizeText(div, layer, d.text, d.verticalAlignment, doc);
      } else if (effH === 0) {
        // 0-height shape carrying text (RIPE ea785d2e subtitle): the box is
        // an anchor, not a clip — let the text flow down from it.
        layer.style.bottom = "auto";
        layer.style.height = "auto";
        layer.style.overflow = "visible";
      } else {
        // Shapes keep their geometry: a shape never grows for its text, so
        // "grow" degrades to the fixed-box tolerance mode.
        applyTextFitMode(div, layer, d.textFit === "grow" ? undefined : d.textFit, d.verticalAlignment);
      }
      div.appendChild(layer);
    }
  } else if (d.type === "image") {
    const img = imageEl(d.image.dataId, d.image.preferredFileName ?? d.image.fileName, ctx, d.image.preferredFileName, d.thumbnail);
    // photo borders / picture frames (the stroke lives on the image style)
    applyBoxStroke(div, c.style?.stroke);
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
    // NOTE: shapeSvg pins the viewBox to >=1 per axis — vertical/horizontal
    // connection lines have a 0-width/0-height frame, and a zero (or 1e-7)
    // viewBox dimension either disables rendering or blows the stroke up by
    // the reciprocal (a 6.8e-7pt-wide frame painted a full-slide black band).
    div.appendChild(shapeSvg({ path: d.path }, w, h, c.style));
  } else if (d.type === "table") {
    const wrap = el("div", "canvas-table-wrap");
    wrap.appendChild(renderTable(d.table, ctx, doc));
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
    wrap.appendChild(renderTable(d.table, ctx, doc));
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