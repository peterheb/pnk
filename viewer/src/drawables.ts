// Drawable rendering. Two contexts:
//   - canvas: absolutely positioned inside a scaled .canvas-inner (Keynote
//     slides, Pages layout pages, Numbers sheet canvases) — geometry in
//     points, rendered 1pt = 1px and scaled by the parent.
//   - flow: in-stream content (inline attachments, floating groups rendered
//     after a word-processing body).
//
// Shapes become inline SVG paths from the flattened curve data; images come
// from ViewerCtx object URLs; tables/charts delegate to their renderers.

import { isPdfBytes, pdfMediaEl } from "./pdfmedia";
import type {
  CurvePath,
  ChartModel,
  ChartNumberFormat,
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
import { layoutTabs, naturalLineHeight, renderStyledText } from "./text";
import { charStyleOf, paraStyleOf, type HydratedDoc } from "./hydrate";
import { substituteFamily } from "./webfonts";

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

/** A CurvePath (any coordinate space) -> SVG path data, scaled by sx/sy. */
function curvePathToD(path: CurvePath, sx: number, sy: number): string {
  const f = (v: number) => (Math.round(v * 100) / 100).toString();
  const parts: string[] = [];
  for (const e of path.elements) {
    if (e.type === "close") { parts.push("Z"); continue; }
    const p = e.points;
    const xy = (i: number) => `${f(p[i] * sx)} ${f(p[i + 1] * sy)}`;
    if (e.type === "move" && p.length >= 2) parts.push(`M ${xy(0)}`);
    else if (e.type === "line" && p.length >= 2) parts.push(`L ${xy(0)}`);
    else if (e.type === "quad" && p.length >= 4) parts.push(`Q ${xy(0)} ${xy(2)}`);
    else if (e.type === "cubic" && p.length >= 6) parts.push(`C ${xy(0)} ${xy(2)} ${xy(4)}`);
  }
  return parts.length ? parts.join(" ") : "";
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
  // Hand-drawn ("smart") strokes: Keynote textures the line with a brush
  // preset (Pencil, Chalk2, Crayon, Dry Brush, ...). The brush parameters
  // are not in the model; a document-wide displacement filter gives the
  // edge wobble and a lighter, uneven ink, which is what reads as
  // hand-drawn at slide scale. Applied per element; the shape's fill goes
  // through the same filter, so a filled shape's edge wobbles with its
  // stroke, as Keynote's does.
  if (stroke.smartStroke && /chalk|crayon|pencil|dry brush/i.test(stroke.smartStroke)) {
    e.setAttribute("filter", `url(#${sketchStrokeFilterId(stroke.smartStroke)})`);
    // Keynote's chalk texture reads as a pale, near-white line whatever the
    // stroke colour: ripe76 (9d5dcf60) stores the circles' fill colour as
    // the Chalk2 stroke colour and the export draws a white speckled ring.
    // Lighten chalk 60% toward white. [inferred from one deck]
    if (/chalk/i.test(stroke.smartStroke)) e.setAttribute("stroke", lightenHex(stroke.color, 0.6));
  }
  // Pen, Feathered Brush and the other smooth presets draw as plain strokes:
  // at slide scale Keynote's export of them (greenberg 40c5f2ef, slide 12)
  // differs from a plain stroke only by a slight taper.
}

/** Mix a #rrggbb colour toward white by `t` (0..1). */
function lightenHex(hex: string, t: number): string {
  const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})/i.exec(hex);
  if (!m) return hex;
  const mix = (h: string) => Math.round(parseInt(h, 16) + (255 - parseInt(h, 16)) * t).toString(16).padStart(2, "0");
  return `#${mix(m[1])}${mix(m[2])}${mix(m[3])}`;
}

/** One shared <filter> per brush family, in a 0x0 <svg> on the body. */
function sketchStrokeFilterId(preset: string): string {
  const chalky = /chalk|crayon|pencil/i.test(preset);
  const id = chalky ? "pnk-sketch-chalk" : "pnk-sketch-brush";
  if (!document.getElementById(id)) {
    const NS = "http://www.w3.org/2000/svg";
    const holder = document.createElementNS(NS, "svg");
    holder.setAttribute("width", "0");
    holder.setAttribute("height", "0");
    holder.setAttribute("aria-hidden", "true");
    holder.style.position = "absolute";
    const filter = document.createElementNS(NS, "filter");
    filter.id = id;
    filter.setAttribute("x", "-5%");
    filter.setAttribute("y", "-5%");
    filter.setAttribute("width", "110%");
    filter.setAttribute("height", "110%");
    const noise = document.createElementNS(NS, "feTurbulence");
    noise.setAttribute("type", "fractalNoise");
    noise.setAttribute("baseFrequency", chalky ? "0.08" : "0.03");
    noise.setAttribute("numOctaves", "2");
    noise.setAttribute("seed", "7");
    noise.setAttribute("result", "noise");
    const wobble = document.createElementNS(NS, "feDisplacementMap");
    wobble.setAttribute("in", "SourceGraphic");
    wobble.setAttribute("in2", "noise");
    wobble.setAttribute("scale", chalky ? "3" : "2");
    wobble.setAttribute("xChannelSelector", "R");
    wobble.setAttribute("yChannelSelector", "G");
    filter.appendChild(noise);
    filter.appendChild(wobble);
    if (chalky) {
      // Chalk and pencil leave gaps: modulate the alpha with the noise.
      const grain = document.createElementNS(NS, "feComposite");
      grain.setAttribute("in2", "noise");
      grain.setAttribute("operator", "arithmetic");
      grain.setAttribute("k1", "0");
      grain.setAttribute("k2", "0.85");
      grain.setAttribute("k3", "0");
      grain.setAttribute("k4", "0");
      filter.appendChild(grain);
    }
    const defs = document.createElementNS(NS, "defs");
    defs.appendChild(filter);
    holder.appendChild(defs);
    document.body.appendChild(holder);
  }
  return id;
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
      // point.x = head length (natural-size pt), point.y = shaft top edge as
      // a fraction of the height (see ShapeGeometry.point); defaults are
      // Keynote's stored values for a fresh arrow (64pt on 174, 0.34).
      const sx = g.naturalSize?.width ? w / g.naturalSize.width : 1;
      const head = g.point ? clamp(g.point.x * sx, 0, w / 2) : w * 0.25;
      const bar = g.point ? h * (1 - 2 * clamp(g.point.y, 0, 0.5)) : h * 0.45;
      const p: [number, number][] = [
        [0, h / 2], [head, 0], [head, (h - bar) / 2], [w - head, (h - bar) / 2], [w - head, 0],
        [w, h / 2], [w - head, h], [w - head, (h + bar) / 2], [head, (h + bar) / 2], [head, h],
      ];
      return "M" + p.map(([x, y]) => `${f(x)},${f(y)}`).join(" L") + " Z";
    }
    case "star": {
      // point.x = number of points, point.y = inner radius as a fraction of
      // the outer (Keynote stores 5 × 0.498 for its default star); without
      // it, scalar = pointiness (0..1): inner radius shrinks as it grows
      const n = g.point && g.point.x >= 3 ? Math.round(g.point.x) : 5;
      const inner = g.point ? 0.5 * clamp(g.point.y, 0.05, 0.95) : 0.5 - 0.3 * clamp(g.scalar ?? 0.4, 0, 1);
      const pts: string[] = [];
      for (let i = 0; i < 2 * n; i++) {
        const r = i % 2 === 0 ? 0.5 : inner;
        const a = (Math.PI / n) * i - Math.PI / 2;
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
      // Block arrow from the stored control point: point.x is the head
      // length along the arrow (natural-size pt, scaled to the box) and
      // point.y the shaft edge as a fraction of the cross size. Keynote's
      // export of a default 174×100 arrow (atnf.csiro.au Bayesian deck)
      // measures a 63pt head and a 31pt shaft, from a stored 64 × 0.34;
      // the old 0.35/0.45 guesses drew a chevron with a fat shaft.
      const horizontal = preset.endsWith("left") || preset.endsWith("right");
      const along = horizontal ? w : h;
      const across = horizontal ? h : w;
      const nAlong = horizontal ? g.naturalSize?.width : g.naturalSize?.height;
      const head = g.point ? clamp(g.point.x * (nAlong ? along / nAlong : 1), 0, along) : along * 0.35;
      const bar = g.point ? across * (1 - 2 * clamp(g.point.y, 0, 0.5)) : across * 0.45;
      const pts: Record<string, [number, number][]> = {
        "right-arrow": [[0, (h - bar) / 2], [w - head, (h - bar) / 2], [w - head, 0], [w, h / 2], [w - head, h], [w - head, (h + bar) / 2], [0, (h + bar) / 2]],
        "left-arrow": [[w, (h - bar) / 2], [head, (h - bar) / 2], [head, 0], [0, h / 2], [head, h], [head, (h + bar) / 2], [w, (h + bar) / 2]],
        "down-arrow": [[(w - bar) / 2, 0], [(w + bar) / 2, 0], [(w + bar) / 2, h - head], [w, h - head], [w / 2, h], [0, h - head], [(w - bar) / 2, h - head]],
        "up-arrow": [[(w - bar) / 2, h], [(w + bar) / 2, h], [(w + bar) / 2, head], [w, head], [w / 2, 0], [0, head], [(w - bar) / 2, head]],
      };
      return "M" + pts[preset].map(([x, y]) => `${f(x)},${f(y)}`).join(" L") + " Z";
    }
    case "callout": {
      // Rounded-rect body plus a wedge whose apex is tailPosition (in
      // naturalSize space, often far outside the body: kcsrk slide 7 points
      // a 244x100 callout at (281, -277)) and whose base, tailSize wide,
      // sits on the body edge facing the apex. [proto: TSD.CalloutPathSourceArchive]
      const tail = g.callout?.tailPosition;
      const nw = g.naturalSize?.width || w || 1;
      const nh = g.naturalSize?.height || h || 1;
      const rBody = Math.min((g.callout?.cornerRadius ?? 8) * (w / nw), w / 2, h / 2);
      const body = `M${f(rBody)},0 L${f(w - rBody)},0 Q${f(w)},0 ${f(w)},${f(rBody)} L${f(w)},${f(h - rBody)} Q${f(w)},${f(h)} ${f(w - rBody)},${f(h)} L${f(rBody)},${f(h)} Q0,${f(h)} 0,${f(h - rBody)} L0,${f(rBody)} Q0,0 ${f(rBody)},0 Z`;
      if (!tail) return body;
      const ax = tail.x * (w / nw);
      const ay = tail.y * (h / nh);
      if (ax >= 0 && ax <= w && ay >= 0 && ay <= h) return body; // apex inside: no visible tail
      const tailW = Math.max(4, (g.callout?.tailSize.width || 10) * (w / nw));
      // Base centre: the ray from the body centre to the apex, clipped to the border.
      const cx = w / 2;
      const cy = h / 2;
      const dx = ax - cx;
      const dy = ay - cy;
      const t = Math.min(dx !== 0 ? Math.abs(cx / dx) : Infinity, dy !== 0 ? Math.abs(cy / dy) : Infinity);
      const bx = cx + dx * t;
      const by = cy + dy * t;
      const len = Math.hypot(dx, dy) || 1;
      const px = (-dy / len) * (tailW / 2);
      const py = (dx / len) * (tailW / 2);
      // Pull the base slightly inside so the wedge fuses with the body fill.
      const ix = bx - (dx / len) * 2;
      const iy = by - (dy / len) * 2;
      return `${body} M${f(ix + px)},${f(iy + py)} L${f(ax)},${f(ay)} L${f(ix - px)},${f(iy - py)} Z`;
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
    // A near-zero natural axis (38a7da36 stores a 0-height rule as
    // 8.9e-17 x 24pt) is degenerate too: 1e-8 / 9e-17 painted a 1.9e8pt
    // stroke and the whole page black. (Pages agent, 2026-09-05)
    .map(([dim, nat]) => (nat && nat > 1e-3 && dim > 1e-3 ? dim / nat : NaN))
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

export function imageEl(
  dataId: string | undefined,
  fileName: string | undefined,
  ctx: ViewerCtx,
  alt?: string,
  thumbnail?: { dataId: string; fileName?: string; preferredFileName?: string },
  cssSize?: { width: number; height: number },
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
    // Placed vector art with no raster twin: pdf.js draws PDFs (Keynote
    // equations, pasted PDF art) into a canvas; anything it cannot open
    // gets a neutral gray shape with a small filename caption — not an
    // error card (the artwork exists, we just cannot show it).
    const placeholder = () => {
      const box = el("div", "media-vector");
      const cap = el("span", "media-vector-caption");
      cap.textContent = fileName.replace(/\.(pdf|ai|eps)$/i, "").replace(/-\d+$/, "");
      box.appendChild(cap);
      return box;
    };
    const bytes = dataId ? ctx.bytes(dataId) : undefined;
    if (bytes && isPdfBytes(bytes)) return pdfMediaEl(bytes, cssSize?.width ?? 0, cssSize?.height ?? 0, placeholder);
    return placeholder();
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
  // Keynote applies a paragraph's space-before only BETWEEN paragraphs
  // (and space-after likewise): the Dyalog deck's 59pt bullets start
  // 3.7pt under the box top in the export, then step 116pt apart.
  const paras = inner.querySelector(".styled-text")?.children;
  if (paras && paras.length) {
    (paras[0] as HTMLElement).style.marginTop = "0";
    (paras[paras.length - 1] as HTMLElement).style.marginBottom = "0";
  }
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
    // ...but bounded: the growth absorbs font-metric drift, not content the
    // app itself clips. 7b8e38ed's title box is stored 66pt tall with two
    // lines and five empty 24pt paragraphs after them; Pages prints the
    // 66pt box, and unbounded growth made it 200pt and covered the box
    // below. Half again the stored height covers the metric drift seen
    // across the corpus (Keynote boxes run 10-20% taller here).
    const storedPx = parseFloat(storedH || "0");
    if (storedPx > 0) {
      div.style.maxHeight = `${(storedPx * 1.5).toFixed(2)}px`;
      div.style.overflow = "hidden";
    }
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
  // positioned tab stops first: they change line widths and heights
  layoutTabs(root);
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
    // Width counts too: an unbreakable word wider than the box (b31db822's
    // DRAFT watermark, 247pt Trebuchet in a 412pt box — Pages shrinks it to
    // 141pt) overflows sideways, which a height-only check never sees.
    // Measured as the widest LINE from the text nodes' client rects, not
    // scrollWidth: centred text that overflows spills both ways and
    // scrollWidth reports box + twice the spill (greenberg's 400pt
    // "powerful" shrank to 0.7 where Keynote draws it at full size).
    const boxW = layer.clientWidth;
    const widest = widestLine(inner);
    const sW = boxW > 0 && widest > boxW + 0.5 ? boxW / widest : 1;
    const layoutAt = (scale: number) => {
      inner.style.width = `${(100 / scale).toFixed(3)}%`;
      inner.style.transform = `scale(${scale.toFixed(4)})`;
      inner.style.transformOrigin = origin;
    };
    const fits = (scale: number) => inner.offsetHeight * scale <= boxH + 0.5 && scale <= sW + 1e-6;
    for (let i = 0; i < 3; i++) {
      const contentH = inner.offsetHeight;
      if (fits(s)) break;
      const need = Math.min(s, boxH / contentH, sW);
      s = Math.max(need, minScale);
      layoutAt(s);
    }
    // Wrapping is discrete: a line a few percent wider than the box (Chrome
    // sets Helvetica Neue Medium ~2.5% wider than Keynote — greenberg's
    // 228pt "i'm interested in") wraps, the block doubles in height and the
    // first pass lands at 0.7. Laid out a little wider it does not wrap at
    // all, so bisect back up toward 1 for the largest scale that still fits.
    if (s < 1 && s > minScale) {
      let lo = s;
      let hi = Math.min(1, sW);
      for (let i = 0; i < 6 && hi - lo > 0.01; i++) {
        const mid = (lo + hi) / 2;
        layoutAt(mid);
        if (fits(mid)) lo = mid; else hi = mid;
      }
      s = lo;
      layoutAt(s);
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

/**
 * A chart number per its stored TSK format. `total` turns a value into its
 * share when the format is a percentage (pie labels); a percent format with
 * no total scales by 100 like a cell would. Apple's charts leave the
 * thousands separator OFF unless the format says otherwise — 1,855 of the
 * corpus's 1,868 value axes store `show_thousands_separator = false`, which
 * is why the RIPE waiting-list axis reads "1200" and not "1,200".
 */
function fmtNumber(v: number, f: ChartNumberFormat | undefined, total?: number): string {
  const dec = f?.decimals;
  let n = v;
  let suffix = "";
  if (f?.kind === "percent") {
    n = total ? (v / total) * 100 : v * 100;
    suffix = "%";
  }
  const body =
    dec !== undefined
      ? n.toFixed(dec)
      : String(Math.round(n * 1e6) / 1e6);
  const grouped = f?.thousandsSeparator
    ? body.replace(/\B(?=(\d{3})+(?!\d))/, "").replace(/^(-?\d+)/, (m) => m.replace(/\B(?=(\d{3})+(?!\d))/g, ","))
    : body;
  return (f?.kind === "currency" && f.currencyCode ? currencySymbol(f.currencyCode) : "") + grouped + suffix;
}

function currencySymbol(code: string): string {
  const map: Record<string, string> = { USD: "$", EUR: "€", GBP: "£", JPY: "¥" };
  return map[code] ?? code + " ";
}

/**
 * Text width in points for chart furniture. Chart labels are laid out by hand
 * (svg has no flow), and guessing "0.55 em per character" both ellipsized
 * legends Apple fits whole and thinned category rows Apple prints in full.
 * One shared 2d context measures the real string in the real face.
 */
let measureCtx: CanvasRenderingContext2D | null | undefined;
const measureCache = new Map<string, number>();
function textWidth(str: string, size: number, family: string): number {
  const key = `${size}|${family}|${str}`;
  const hit = measureCache.get(key);
  if (hit !== undefined) return hit;
  if (measureCtx === undefined) measureCtx = document.createElement("canvas").getContext("2d");
  // headless/oddball contexts fall back to the old estimate
  const w = measureCtx ? ((measureCtx.font = `${size}px ${family}`), measureCtx.measureText(str).width) : str.length * size * 0.55;
  if (measureCache.size < 4000) measureCache.set(key, w);
  return w;
}

function chartSvg(chart: ChartModel, w: number, h: number, numbersAxis = false): SVGSVGElement | null {
  const numeric = chart.series.every((s) => s.values.every((v) => v === null || typeof v === "number"));
  if (!numeric || chart.series.length === 0 || chart.categories.length === 0) return null;
  const NS = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(NS, "svg") as SVGSVGElement;
  svg.setAttribute("viewBox", `0 0 ${w} ${h}`);
  svg.style.overflow = "visible"; // slice labels sit outside the rim
  // Chart text is its own typeface and its own sizes: RIPE 85's slides are
  // Helvetica Neue but every chart in the deck is ArialMT at sizes the chart's
  // paragraph styles name outright (32pt axes, 38pt legend, 50pt ring labels).
  const ts = chart.textSizes;
  const face = ts?.fontName ? `"${ts.fontName.replace(/MT$|-.*$/, "")}", ` : "";
  const sub = ts?.fontName ? substituteFamily(ts.fontName) : null;
  const family = `${face}${sub ? `"${sub}", ` : ""}"Helvetica Neue", Helvetica, Arial, sans-serif`;
  svg.setAttribute("font-family", family);
  const colors = chart.seriesColors ?? ["#4a90d9", "#e0762e", "#7bb662", "#b0578d", "#5b6abf"];
  // Furniture scales with the chart (Keynote's 1920-wide slides set 24pt
  // legends where a 160pt Numbers chart sets 9pt) and takes currentColor,
  // so a dark slide backdrop gets light labels.
  const base = ts?.axisPt ?? Math.max(9, Math.min(30, Math.min(w, h) / 22));
  const legendSize = ts?.legendPt ?? base * 0.95;
  const titleSize = ts?.titlePt ?? base * 1.25;
  const labelSize = ts?.labelPt ?? base * 0.9;
  const text = (x: number, y: number, str: string, opts: { size?: number; anchor?: string; weight?: string; fill?: string; rotate?: number } = {}) => {
    const t = document.createElementNS(NS, "text");
    t.setAttribute("x", x.toFixed(1));
    t.setAttribute("y", y.toFixed(1));
    t.setAttribute("font-size", String(opts.size ?? base));
    t.setAttribute("text-anchor", opts.anchor ?? "middle");
    t.setAttribute("fill", opts.fill ?? "currentColor");
    if (!opts.fill) t.setAttribute("opacity", "0.85");
    if (opts.weight) t.setAttribute("font-weight", opts.weight);
    if (opts.rotate) t.setAttribute("transform", `rotate(${opts.rotate} ${x.toFixed(1)} ${y.toFixed(1)})`);
    t.textContent = str;
    svg.appendChild(t);
    return t;
  };
  const line = (x1: number, y1: number, x2: number, y2: number, stroke: string, width = 1) => {
    const l = document.createElementNS(NS, "line");
    l.setAttribute("x1", x1.toFixed(1)); l.setAttribute("y1", y1.toFixed(1));
    l.setAttribute("x2", x2.toFixed(1)); l.setAttribute("y2", y2.toFixed(1));
    l.setAttribute("stroke", stroke); l.setAttribute("stroke-width", String(width));
    svg.appendChild(l);
  };

  // --- layout: title on top, legend at the bottom, axis titles, plot box.
  // Apple's chart furniture: 9pt labels, hairline gridlines, legend swatches
  // centred under the plot. Fixture-verified on benmatselby's burndown deck.
  const pieLike = chart.type === "pie" || chart.type === "donut";
  const names = chart.series.map((s, i) => s.name ?? `Series ${i + 1}`);
  const pieBySeries = pieLike && chart.categories.length === 1 && chart.series.length > 1;
  const showLegend = chart.legendVisible === true || (chart.legendVisible !== false && !pieBySeries && (chart.series.length > 1 || pieLike || chart.series[0]?.name !== undefined));
  let top = 0;
  if (chart.title) {
    // Keynote hangs a pie's title ABOVE its frame (RIPE 85 slide 9: the title
    // baseline is at y=172 while the chart box starts at 234, and the pie
    // still fills the whole box); Numbers keeps a plot title inside the frame
    // (benmatselby's burndown "User Stories").
    if (pieLike) {
      text(w / 2, -titleSize * 0.35, chart.title, { size: titleSize, weight: "600" });
    } else {
      text(w / 2, top + titleSize * 0.9, chart.title, { size: titleSize, weight: "600" });
      top += titleSize * 1.7;
    }
  }
  let bottom = h;
  if (showLegend) {
    const items = pieLike ? (pieBySeries ? names : chart.categories) : names;
    const widths = items.map((name) => textWidth(name, legendSize, family) + legendSize * 2.0);
    // The stored legend frame is relative to the chart frame's CENTRE, and it
    // routinely sits OUTSIDE the frame: RIPE 85's waiting-list chart stores
    // (-809.5, -368.0) in a 1663x574 frame, putting the legend row 81pt ABOVE
    // the chart's top edge, which is where Apple's export draws it. The svg
    // has overflow visible, so an out-of-box legend just works; only a legend
    // that lands inside the box steals height from the plot.
    const lf = chart.legendFrame;
    const boxX = lf ? w / 2 + lf.x : 0;
    const boxY = lf ? h / 2 + lf.y : bottom - legendSize * 2;
    const boxW = lf ? lf.width : w;
    const boxH = lf ? lf.height : legendSize * 2;
    const rowW = Math.min(boxW - 4, widths.reduce((a, b) => a + b, 0));
    const shrink = rowW / widths.reduce((a, b) => a + b, 0);
    const y = boxY + boxH / 2 + legendSize * 0.35;
    let x = boxX + boxW / 2 - rowW / 2;
    items.forEach((name, i) => {
      const itemW = widths[i] * shrink;
      // Apple keys a line chart's legend with a short stroke, an area/bar/pie
      // chart's with a filled square (dns-oarc's cache-hit charts, RIPE 85's
      // waiting list).
      const dash = chart.type === "line" || chart.type === "scatter";
      const sw = document.createElementNS(NS, "rect");
      sw.setAttribute("x", (x + 2).toFixed(1));
      sw.setAttribute("y", (y - (dash ? legendSize * 0.45 : legendSize * 0.8)).toFixed(1));
      sw.setAttribute("width", (legendSize * (dash ? 1.1 : 0.9)).toFixed(1));
      sw.setAttribute("height", (legendSize * (dash ? 0.18 : 0.9)).toFixed(1)); sw.setAttribute("rx", "1.5");
      sw.setAttribute("fill", colors[i % colors.length]);
      svg.appendChild(sw);
      const label = text(x + legendSize * 1.4, y, name, { anchor: "start", size: legendSize });
      const room = itemW - legendSize * 1.6;
      if (textWidth(name, legendSize, family) > room) {
        let cut = name;
        while (cut.length > 3 && textWidth(cut + "…", legendSize, family) > room) cut = cut.slice(0, -1);
        label.textContent = cut + "…";
      }
      x += itemW;
    });
    // only a legend drawn over the plot costs the plot its bottom strip
    if (boxY + boxH > bottom - legendSize * 0.5) bottom = Math.min(bottom, boxY - legendSize * 0.4);
  }
  if (pieLike) {
    // Wedges: one series across the categories, or — Keynote's usual pie
    // layout, one row with a column per slice — one value per series, the
    // series names being the slice labels (RIPE 85: "Allocations 264 /
    // PI assignments 162" is a "Region 1" row with two columns).
    const bySeries = chart.categories.length === 1 && chart.series.length > 1;
    const vals = bySeries
      ? chart.series.map((s) => (typeof s.values[0] === "number" && s.values[0] > 0 ? s.values[0] : 0))
      : chart.series[0].values.map((v) => (typeof v === "number" && v > 0 ? v : 0));
    const labels = bySeries ? names : chart.categories;
    const total = vals.reduce((a, b) => a + b, 0) || 1;
    // The pie fills its own frame: Apple's export puts slide 9's rim at
    // 304.7pt in a 611.1pt box and slide 7's at 337.7pt in a 677.0pt box —
    // exactly half the box, with title and labels drawn over/outside it.
    const cx = w / 2, cy = h / 2, r = Math.min(w, h) / 2;
    // Slice labels. Keynote stores them on the series non-style: whether the
    // name and the value show, the value's number format, and — the piece
    // that decides inside-vs-outside — `pielabelexplosion`, the label centre
    // as a PERCENT of the pie radius. RIPE 85 slide 9 stores 59 (inside the
    // wedge, percentages); slide 7's inner ring stores 144, which puts its
    // labels clear of the OUTER ring it is nested in.
    const pl = chart.pieLabels;
    const labelR = pl?.radiusPct !== undefined ? r * (pl.radiusPct / 100) : r + labelSize * 0.8;
    const outside = labelR > r * 0.98;
    // Labels are queued and drawn AFTER every wedge: an inside label belongs
    // on top of its own slice, and svg paints in document order.
    const pending: (() => void)[] = [];
    let a0 = -Math.PI / 2;
    vals.forEach((v, i) => {
      if (v / total > 0.03) {
        const mid = a0 + (v / total) * Math.PI;
        const lx = cx + labelR * Math.cos(mid), ly = cy + labelR * Math.sin(mid);
        const anchor = !outside ? "middle" : Math.cos(mid) < -0.2 ? "end" : Math.cos(mid) > 0.2 ? "start" : "middle";
        const lines: string[] = [];
        if (!pl || pl.showSeriesName) lines.push((labels[i] ?? "").trim());
        if (!pl || pl.showValue) lines.push(pl?.valueFormat ? fmtNumber(v, pl.valueFormat, total) : v.toLocaleString("en-US"));
        const rows = lines.filter((t) => t.length > 0);
        pending.push(() => {
          // name over value, 1.125 em apart — Apple's export stacks them
          // ("10+ LIRs" at 294.4 and "12%" at 345.0 for a 45pt label).
          const lead = labelSize * 1.125;
          const y0 = ly - ((rows.length - 1) * lead) / 2;
          rows.forEach((t, ri) => text(lx, y0 + ri * lead, t, { anchor, size: labelSize }));
        });
        // leader line from the rim out to a label that sits off it
        if (pl?.leaderLines && outside) {
          const gap = labelSize * 0.35;
          line(cx + (r + gap) * Math.cos(mid), cy + (r + gap) * Math.sin(mid), lx - Math.sign(Math.cos(mid)) * gap, ly, "currentColor", 1);
        }
      }
      const a1 = a0 + (v / total) * Math.PI * 2;
      const p = document.createElementNS(NS, "path");
      const large = a1 - a0 > Math.PI ? 1 : 0;
      const [x0, y0, x1, y1] = [cx + r * Math.cos(a0), cy + r * Math.sin(a0), cx + r * Math.cos(a1), cy + r * Math.sin(a1)];
      p.setAttribute("d", `M${cx},${cy} L${x0.toFixed(1)},${y0.toFixed(1)} A${r},${r} 0 ${large} 1 ${x1.toFixed(1)},${y1.toFixed(1)} Z`);
      p.setAttribute("fill", colors[i % colors.length]);
      p.setAttribute("stroke", "#fff");
      svg.appendChild(p);
      a0 = a1;
    });
    // a hole cut with a mask (not a painted disc) keeps the slide's
    // background visible through the ring
    const holeFrac = chart.innerRadius ?? (chart.type === "donut" ? 0.5 : 0);
    if (holeFrac > 0) {
      const defs = document.createElementNS(NS, "defs");
      const mask = document.createElementNS(NS, "mask");
      const id = `ring-${Math.random().toString(36).slice(2, 8)}`;
      mask.setAttribute("id", id);
      const full = document.createElementNS(NS, "rect");
      full.setAttribute("x", "0"); full.setAttribute("y", "0"); full.setAttribute("width", String(w)); full.setAttribute("height", String(h)); full.setAttribute("fill", "#fff");
      const hole = document.createElementNS(NS, "circle");
      hole.setAttribute("cx", String(cx)); hole.setAttribute("cy", String(cy)); hole.setAttribute("r", (r * holeFrac).toFixed(1)); hole.setAttribute("fill", "#000");
      mask.appendChild(full); mask.appendChild(hole); defs.appendChild(mask); svg.insertBefore(defs, svg.firstChild);
      for (const el of Array.from(svg.querySelectorAll("path"))) el.setAttribute("mask", `url(#${id})`);
    }
    for (const draw of pending) draw();
    return svg;
  }

  const horizontal = chart.type === "bar" || chart.type === "stacked-bar";
  const stacked = chart.type.startsWith("stacked");
  const n = chart.categories.length;
  if (chart.valueAxisTitle && horizontal) bottom -= base * 1.5;
  if (chart.categoryAxisTitle) bottom -= base * 1.5;
  // value range + nice ticks
  const allVals: number[] = [];
  if (stacked) {
    chart.categories.forEach((_, vi) => {
      let pos = 0, neg = 0;
      for (const s of chart.series) { const v = s.values[vi]; if (typeof v === "number") { if (v >= 0) pos += v; else neg += v; } }
      allVals.push(pos, neg);
    });
  } else {
    for (const s of chart.series) for (const v of s.values) if (typeof v === "number") allVals.push(v);
  }
  let vmin = Math.min(0, ...allVals);
  let vmax = Math.max(0, ...allVals);
  if (chart.valueAxisMin !== undefined) vmin = chart.valueAxisMin;
  if (chart.valueAxisMax !== undefined) vmax = chart.valueAxisMax;
  if (vmax === vmin) vmax = vmin + 1;
  // Keynote's stored gridline count is a count of INTERVALS, not of lines: the
  // RIPE waiting-list axis stores 4 and Apple's export draws 0/300/600/900/
  // 1200 — five labels, four gaps, with the top rounded up to a nice number
  // that divides evenly. So pick the smallest nice step whose `g` intervals
  // still cover the data (300 = 3x10^2 here, which a 1/2/2.5/5 ladder can
  // never reach), then let the axis end exactly on it.
  const g = chart.valueAxisMajorGridlines && chart.valueAxisMajorGridlines > 1 ? chart.valueAxisMajorGridlines : 0;
  const targetTicks = g || 4;
  const rawStep = (vmax - vmin) / targetTicks;
  const mag = Math.pow(10, Math.floor(Math.log10(rawStep)));
  // Apple's ladder is finer than the classic 1/2/2.5/5: RIPE's waiting list
  // lands on 3x10^2 and ENOG's TLD chart on 7.5 (0..30 in four steps over data
  // that peaks at 28.1).
  const ladder = g ? [1, 1.5, 2, 2.5, 3, 4, 5, 6, 7.5, 8, 10] : [1, 2, 2.5, 5, 10];
  let step = ladder.map((m) => m * mag).find((st) => (vmax - vmin) / st <= targetTicks + (g ? 1e-9 : 0.5)) ?? mag * 10;
  if (numbersAxis && chart.valueAxisMax === undefined && vmin >= 0) {
    // Numbers rounds the TOP, not the step: its exports label the axis at
    // top x k/N whatever that gives (baabe23e067f "User Stories": 0, 5.25,
    // 10.5, 15.75, 21 for a maximum of 21; 5a89929253a1: 0..19,000 in steps
    // of 4,750). Measured on 40 exported charts (baabe23e067f, 5a89929253a1,
    // plus the two Keynote cases above): the maximum rounds up to a
    // multiple of 10^k (k = floor(log10 max)) when its leading digits are
    // 2.7 or more, else to a multiple of 10^(k-1) — 56 -> 60, 37 -> 40,
    // 4060 -> 5000, but 21 -> 21, 1798 -> 1800, 12539 -> 13000. 34 of the
    // 40 match; the ladder rule above matched 15. The six misses land one
    // unit higher (1650 -> 1800, 2097 -> 2200); the exact rule is not in
    // the archives (docs/JUDGE.md, Numbers round 4). [inferred]
    const k = Math.floor(Math.log10(Math.max(vmax, 1e-9)));
    const u = vmax / Math.pow(10, k) >= 2.7 ? Math.pow(10, k) : Math.pow(10, k - 1);
    vmax = Math.ceil(vmax / u - 1e-9) * u;
    vmin = 0;
    step = vmax / targetTicks;
  } else {
    if (chart.valueAxisMin === undefined) vmin = Math.floor(vmin / step) * step;
    if (chart.valueAxisMax === undefined) vmax = g ? vmin + step * g : Math.ceil(vmax / step) * step;
  }
  const ticks: number[] = [];
  for (let v = vmin; v <= vmax + step / 1000; v += step) ticks.push(Math.round(v * 1e6) / 1e6);
  // Axis decimals stay automatic: the stored decimal_places is 1 on 720 of
  // the corpus's axes with no sign in Apple's exports that ticks carry it.
  const tickFmt = chart.valueAxisFormat ? { ...chart.valueAxisFormat, decimals: undefined } : undefined;
  const fmtTick = (v: number) => fmtNumber(v, tickFmt);
  // The plot box IS the chart's frame; every piece of furniture is drawn
  // OUTSIDE it. Apple's slide-4 export puts the first category label's centre
  // at x=152 and the last at 1809 for a frame spanning 150.2 -> 1813.5, and
  // its value labels ("1200" at x=55.7) sit left of the frame entirely, with
  // the category row's baseline at 868 below a frame that ends at 851.6.
  const left = 0;
  const right = w;
  const plotTop = top;
  const plotBottom = bottom;
  const plotW = right - left, plotH = plotBottom - plotTop;
  if (plotW <= 10 || plotH <= 10) return svg;
  const vy = (v: number) => plotBottom - ((v - vmin) / (vmax - vmin)) * plotH;
  const vx = (v: number) => left + ((v - vmin) / (vmax - vmin)) * plotW;

  // gridlines + value labels
  const gridColor = "currentColor";
  const gridLine = (x1: number, y1: number, x2: number, y2: number, strong: boolean) => {
    const l = document.createElementNS(NS, "line");
    l.setAttribute("x1", x1.toFixed(1)); l.setAttribute("y1", y1.toFixed(1));
    l.setAttribute("x2", x2.toFixed(1)); l.setAttribute("y2", y2.toFixed(1));
    l.setAttribute("stroke", gridColor); l.setAttribute("stroke-width", "1");
    l.setAttribute("opacity", strong ? "0.6" : "0.18");
    svg.appendChild(l);
  };
  // Furniture the file asks for. matija.pretnar.info uses bare column charts
  // as illustrations — value axis off, gridlines off, tick labels off — and
  // we were drawing a stray "0" and a rule Keynote does not draw.
  const ax = chart.axes;
  const showValueGrid = ax ? ax.valueGridlines : true;
  const showValueLabels = ax ? ax.valueLabels : true;
  const showCatLabels = ax ? ax.categoryLabels : true;
  for (const t of ticks) {
    if (horizontal) {
      if (showValueGrid) gridLine(vx(t), plotTop, vx(t), plotBottom, false);
      if (showValueLabels) text(vx(t), plotBottom + base * 1.1, fmtTick(t));
    } else {
      if (showValueGrid) gridLine(left, vy(t), right, vy(t), false);
      if (showValueLabels) text(left - base * 0.4, vy(t) + base * 0.35, fmtTick(t), { anchor: "end" });
    }
  }
  // the "strong" rule along a bar chart's baseline is the CATEGORY axis
  if (!ax || ax.categoryAxisLine) {
    if (horizontal) gridLine(left, plotTop, left, plotBottom, true);
    else gridLine(left, vy(Math.max(vmin, Math.min(0, vmax))), right, vy(Math.max(vmin, Math.min(0, vmax))), true);
  }
  if (ax?.valueAxisLine) {
    if (horizontal) gridLine(left, plotBottom, right, plotBottom, true);
    else gridLine(left, plotTop, left, plotBottom, true);
  }
  if (chart.valueAxisTitle) {
    if (horizontal) text((left + right) / 2, bottom - 2, chart.valueAxisTitle);
    else text(base, (plotTop + plotBottom) / 2, chart.valueAxisTitle, { rotate: -90 });
  }
  // A legend stored OUTSIDE the frame (below it) leaves the category title
  // its usual place under the tick labels; only an in-frame legend pushes
  // the title up (burndown's sprint charts: legend at y = +179 in a 300pt
  // frame, "Days" sits under the ticks).
  const legendInside = showLegend && (!chart.legendFrame || h / 2 + chart.legendFrame.y < h);
  if (chart.categoryAxisTitle) text(horizontal ? base : (left + right) / 2, horizontal ? (plotTop + plotBottom) / 2 : legendInside ? h - base * 2.6 : plotBottom + base * 2.4, chart.categoryAxisTitle, { rotate: horizontal ? -90 : 0 });
  // category labels: at most ~one per 5 glyph widths of plot; a 343-day
  // series (RIPE's waiting list) shows a dozen dates, not all of them.
  // Date categories (ISO strings from date cells) read as "Jun 2022" like
  // Keynote's default date axis format.
  const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
  const catLabel = (c: string): string => {
    const m = /^(\d{4})-(\d{2})-(\d{2})/.exec(c);
    return m ? `${MONTHS[Math.max(0, Math.min(11, +m[2] - 1))]} ${m[1]}` : c;
  };
  // Thin the category row by what the widest label actually MEASURES: the
  // dns-oarc cache charts print 21 numeric labels where a per-character guess
  // showed 8, and RIPE's 343-day series still shows a dozen dates.
  const catSpan = (horizontal ? plotH : plotW) / Math.max(1, n);
  // a quarter-em of air: Apple lets adjacent category labels nearly touch
  // (ENOG's ten TLDs fill their slots edge to edge) and only drops one when
  // they would actually collide.
  const catW = horizontal ? base * 1.6 : Math.max(...chart.categories.map((c) => textWidth(catLabel(c), base, family))) + base * 0.25;
  const labelEvery = Math.max(1, Math.ceil(catW / Math.max(0.001, catSpan)));

  const lineKinds = ["line", "area", "stacked-area", "scatter"];
  if (lineKinds.includes(chart.type)) {
    // points spread edge to edge like Apple's line charts (first category
    // at the axis origin, last at the right edge)
    const px = (i: number) => (n === 1 ? left + plotW / 2 : left + (i / (n - 1)) * plotW);
    if (showCatLabels) chart.categories.forEach((c, i) => { if (i % labelEvery === 0) text(px(i), plotBottom + base * 1.1, catLabel(c)); });
    // category gridlines when the file asks for them (burndown's sprint
    // charts draw a full grid; the value gridlines alone read as ruled paper)
    if (ax?.categoryGridlines) chart.categories.forEach((_c, i) => gridLine(px(i), plotTop, px(i), plotBottom, false));
    const markers = n <= 40; // Keynote drops point markers on dense series
    const running: number[] = new Array(n).fill(0);
    // Apple paints the FIRST series on top: burndown's "Planned Left" (blue)
    // covers "Actual Left" (red) where the two coincide. Stacked kinds keep
    // the natural order so the running totals compose.
    const order = chart.series.map((_s, i) => i);
    if (!stacked) order.reverse();
    for (const si of order) {
      const s = chart.series[si];
      const color = colors[si % colors.length];
      const pts: [number, number][] = [];
      s.values.forEach((v, vi) => {
        if (typeof v !== "number") return;
        const base = stacked ? running[vi] : 0;
        if (stacked) running[vi] += v;
        pts.push([px(vi), vy(base + v)]);
      });
      if (!pts.length) continue;
      if (chart.type.endsWith("area")) {
        const area = document.createElementNS(NS, "path");
        area.setAttribute("d", `M${pts[0][0]},${vy(0)} L` + pts.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" L") + ` L${pts[pts.length - 1][0]},${vy(0)} Z`);
        area.setAttribute("fill", color);
        area.setAttribute("opacity", "0.35");
        svg.appendChild(area);
      }
      if (chart.type !== "scatter") {
        const pl = document.createElementNS(NS, "polyline");
        pl.setAttribute("points", pts.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" "));
        pl.setAttribute("fill", "none");
        pl.setAttribute("stroke", color);
        pl.setAttribute("stroke-width", (base * 0.22).toFixed(1));
        pl.setAttribute("stroke-linejoin", "round");
        svg.appendChild(pl);
      }
      if (markers) for (const [x, y] of pts) {
        const dot = document.createElementNS(NS, "circle");
        dot.setAttribute("cx", x.toFixed(1)); dot.setAttribute("cy", y.toFixed(1)); dot.setAttribute("r", (base * 0.28).toFixed(1));
        dot.setAttribute("fill", "#fff"); dot.setAttribute("stroke", color); dot.setAttribute("stroke-width", (base * 0.17).toFixed(1));
        svg.appendChild(dot);
      }
    }
    return svg;
  }
  // column / bar family, grouped or stacked
  const groupSpan = horizontal ? plotH / n : plotW / n;
  const groupInner = groupSpan * 0.7;
  const barW = stacked ? groupInner : groupInner / chart.series.length;
  if (showCatLabels) chart.categories.forEach((c, i) => {
    if (i % labelEvery !== 0) return;
    if (horizontal) text(left - base * 0.4, plotTop + (i + 0.5) * groupSpan + base * 0.35, catLabel(c), { anchor: "end" });
    else text(left + (i + 0.5) * groupSpan, plotBottom + base * 1.1, catLabel(c));
  });
  const stackPos: number[] = new Array(n).fill(0);
  const stackNeg: number[] = new Array(n).fill(0);
  chart.series.forEach((s, si) => {
    s.values.forEach((v, vi) => {
      if (typeof v !== "number") return;
      let base = 0;
      if (stacked) { base = v >= 0 ? stackPos[vi] : stackNeg[vi]; if (v >= 0) stackPos[vi] += v; else stackNeg[vi] += v; }
      const rect = document.createElementNS(NS, "rect");
      const off = stacked ? 0 : si * barW;
      if (horizontal) {
        const x0 = vx(Math.min(base, base + v)), x1 = vx(Math.max(base, base + v));
        rect.setAttribute("x", x0.toFixed(1)); rect.setAttribute("y", (plotTop + vi * groupSpan + (groupSpan - groupInner) / 2 + off).toFixed(1));
        rect.setAttribute("width", Math.max(0.5, x1 - x0).toFixed(1)); rect.setAttribute("height", barW.toFixed(1));
      } else {
        const y0 = vy(Math.max(base, base + v)), y1 = vy(Math.min(base, base + v));
        rect.setAttribute("x", (left + vi * groupSpan + (groupSpan - groupInner) / 2 + off).toFixed(1)); rect.setAttribute("y", y0.toFixed(1));
        rect.setAttribute("width", barW.toFixed(1)); rect.setAttribute("height", Math.max(0.5, y1 - y0).toFixed(1));
      }
      rect.setAttribute("fill", colors[si % colors.length]);
      svg.appendChild(rect);
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
/** Vertical anchoring of a 0-height text box: "middle" centres the laid-out
 *  block on the stored y, "bottom" stacks it above; "top" (default) flows
 *  down as before. Composed after any rotation so the shift is in the
 *  box's own frame. */
function anchorLineVertical(div: HTMLElement, layer: HTMLElement | null, verticalAlignment: string | undefined): void {
  const ty = verticalAlignment === "middle" ? "-50%" : verticalAlignment === "bottom" ? "-100%" : null;
  if (!ty) return;
  // The translate is a fraction of the box's own height, so the box must
  // take its content's height: a 0-height div with an absolutely placed
  // text layer stayed 0 tall and the shift was a no-op (icecube c3582f31
  // slide 1: the bottom-aligned slide-number placeholder at y=1060 hung
  // its "1" below the slide's edge). Grow boxes already sized this way.
  div.style.height = "auto";
  div.style.overflow = "visible";
  if (layer) {
    layer.style.position = "relative";
    layer.style.height = "auto";
    layer.style.overflow = "visible";
  }
  div.style.transform = `${div.style.transform ?? ""} translate(0, ${ty})`.trim();
}

function anchorZeroSizeText(
  div: HTMLElement,
  layer: HTMLElement,
  text: StyledText | undefined,
  verticalAlignment: string | undefined,
  doc: HydratedDoc,
  naturalSize?: { width: number; height: number },
): void {
  div.style.width = "auto";
  div.style.height = "auto";
  div.style.overflow = "visible";
  layer.style.overflow = "visible";
  layer.style.position = "relative";
  layer.style.whiteSpace = "nowrap";
  layer.style.width = "max-content"; // percentage of an auto box is meaningless
  layer.style.height = "auto";
  let multiLine = false;
  if (naturalSize && naturalSize.width > 0 && (doc as { kind?: string }).kind === "numbers") {
    // Numbers content-sized box (stored 0×0, path natural size present):
    // the box is at least its natural width and grows with its text, but
    // long text wraps rather than running across the sheet. The wrap
    // width is not stored; 6914f46e51ab's intro paragraph wraps at about
    // 355pt in Numbers' own export, and that is the cap used here.
    // Keynote decks store hundreds of 0×0 labels with natural sizes and
    // render correctly on the nowrap path above, so this is Numbers-only.
    // [inferred from one document]
    layer.style.minWidth = `${naturalSize.width}px`;
    layer.style.maxWidth = `${Math.max(naturalSize.width, 355)}px`;
    layer.style.whiteSpace = "normal";
  } else if (naturalSize && naturalSize.width > 0 && naturalSize.height > 0) {
    // Keynote 0×0 box whose natural size is TALLER than one line: the text
    // was laid out wrapped at that width (pre-trib.org's 124pt cover title
    // stores 1821×370 and Keynote breaks it into two centred lines); on
    // the nowrap path it ran off both slide edges. Single-line labels keep
    // nowrap — the natural width is Apple's metrics and would re-wrap a
    // label whose browser face runs a few px wider.
    const first = text?.paragraphs.find((p) => p.items.length > 0);
    let sizePt = 0;
    let font: string | undefined;
    for (const it of first?.items ?? []) {
      if (typeof it === "string" || "type" in it) continue;
      const cs = charStyleOf(doc, (it as { cStyle?: number }).cStyle);
      if (cs?.fontSizePt && cs.fontSizePt > sizePt) { sizePt = cs.fontSizePt; font = cs.fontName; }
    }
    const lineH = sizePt ? sizePt * naturalLineHeight(font) : 0;
    if (lineH && naturalSize.height > 1.5 * lineH) {
      // 3% wider than Apple's laid-out width: browser faces run a hair
      // wider, and at the exact width a line that just fit in Keynote
      // wraps its last word (pre-trib slide 9, Helvetica Bold 42pt).
      layer.style.width = `${(naturalSize.width * 1.03).toFixed(1)}px`;
      layer.style.whiteSpace = "normal";
      // The natural height caps the box (and triggers the bounded shrink)
      // only when it can hold the paragraphs at all: pre-trib.org's
      // section list stores 305pt for seven 42pt paragraphs at 1.6 leading,
      // a stale size that shrank the block to 65% where Keynote draws it
      // full size below the anchor.
      const paraCount = text?.paragraphs.filter((p) => p.items.length > 0).length ?? 1;
      const leading = paraStyleOf(doc, typeof first === "string" ? undefined : first?.pStyle)?.lineSpacingMultiple ?? 1;
      if (naturalSize.height >= 0.9 * paraCount * lineH * leading) {
        layer.style.height = `${naturalSize.height}px`;
        multiLine = true;
      }
    }
  }
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
  // A wrapped box keeps Apple's natural height as its box and takes the
  // bounded font-drift shrink of fixed boxes: pre-trib.org's title wraps to
  // two lines in Keynote's Franklin Gothic and to three in the browser's
  // wider fallback face, the third over the red banner beneath it.
  if (multiLine) div.dataset.textFit = "tolerance";
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
  // rotation applies to the already-mirrored shape (CSS composes right to
  // left, so the scale comes last in the list)
  const flip = c.flipped
    ? ` scale(${c.flipped.horizontal ? -1 : 1}, ${c.flipped.vertical ? -1 : 1})`
    : "";
  if (c.angleDeg || flip) s.transform = `${c.angleDeg ? `rotate(${-c.angleDeg}deg)` : ""}${flip}`.trim();
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

/** Widest laid-out line inside `root`, in layout px (rects divided by the
 *  root's own CSS scale so a transformed ancestor does not distort it). */
function widestLine(root: HTMLElement): number {
  const own = root.getBoundingClientRect();
  const scale = root.offsetWidth > 0 ? own.width / root.offsetWidth : 1;
  if (!(scale > 0)) return 0;
  let widest = 0;
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  const range = document.createRange();
  let node: Node | null = walker.nextNode();
  while (node) {
    if ((node.textContent ?? "").trim()) {
      range.selectNodeContents(node);
      for (const r of Array.from(range.getClientRects())) widest = Math.max(widest, r.width / scale);
    }
    node = walker.nextNode();
  }
  return widest;
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
    // Linked text boxes (Pages): tag the chain for the post-attach pass that
    // moves each box's overflow into the next one (pages.ts flowLinkedText).
    // A box with a successor keeps its stored frame — the overflow belongs
    // to the next box, not below this one — so it neither grows nor shrinks.
    const flowLast = !d.flow || d.flow.index >= d.flow.count - 1;
    if (d.flow) {
      div.dataset.flowId = String(d.flow.id);
      div.dataset.flowIndex = String(d.flow.index);
    }
    if (layer) {
      // Zero-size textboxes (Keynote emits some badge labels at 0×0) carry
      // their text unclipped: let the content size the box instead.
      if (!c.size || (c.size.width === 0 && c.size.height === 0)) {
        anchorZeroSizeText(div, layer, d.text, d.verticalAlignment, doc, d.naturalSize);
      } else {
        applyTextFitMode(div, layer, flowLast ? d.textFit : undefined, d.verticalAlignment);
        if (!flowLast) delete div.dataset.textFit;
        // A 0-height box with a real width is an anchor LINE: the text
        // wraps at the width and its vertical alignment is relative to the
        // stored y — "middle" centres the block on it, "bottom" stacks it
        // above. Keynote's export of RIPE 75's "Questions?" (613×0, middle,
        // y=286) paints the 97pt line spanning 250–327; ours hung it below
        // the anchor, over the email link. Same for kcsrk's 368×0 code box.
        if (c.size.height === 0 && c.size.width > 0) anchorLineVertical(div, layer, d.verticalAlignment);
      }
      div.appendChild(layer);
    } else div.textContent = "";
  } else if (d.type === "shape") {
    // A 0-height shape whose PATH carries a real natural height is a full
    // box stored degenerate (proteger-les-donnees red banner: size 471x0,
    // path 471x32) — adopt the path height so its white caption gets the
    // band as its layout/fit box instead of spilling invisibly below it.
    // A 0x0 shape is different: a content-sized text anchor whose path
    // natural size is the laid-out text (deeplearningbook 2bb490dc: the
    // master's "(Goodfellow 2016)" footer, 0x0 at (969, 746.5), path
    // 105x21.6, centred text; adopting the height alone left a 0-wide box
    // that painted nothing).
    const naturalH = d.geometry.naturalSize?.height ?? 0;
    const effH = h === 0 && w > 0 && d.geometry.path && naturalH > 1 ? naturalH : h;
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
        anchorZeroSizeText(div, layer, d.text, d.verticalAlignment, doc, d.geometry.naturalSize);
      } else if (effH === 0) {
        // 0-height shape carrying text (RIPE ea785d2e subtitle): the box is
        // an anchor, not a clip — let the text flow down from it.
        layer.style.bottom = "auto";
        layer.style.height = "auto";
        layer.style.overflow = "visible";
        anchorLineVertical(div, layer, d.verticalAlignment);
      } else {
        // Shapes keep their geometry: a shape never grows for its text, so
        // "grow" degrades to the fixed-box tolerance mode.
        applyTextFitMode(div, layer, d.textFit === "grow" ? undefined : d.textFit, d.verticalAlignment);
      }
      div.appendChild(layer);
    }
  } else if (d.type === "image") {
    const img = imageEl(d.image.dataId, d.image.preferredFileName ?? d.image.fileName, ctx, d.image.preferredFileName, d.thumbnail, { width: w, height: h });
    // Instant Alpha: clip the image to the kept region (naturalSize space,
    // scaled to the box). Keynote's export draws only the inside of that
    // path; without the clip a cut-out photo shows its original rectangle
    // (icecube c3582f31 slide 1: a map on a white screenshot).
    if (d.instantAlphaPath && d.naturalSize?.width && d.naturalSize?.height) {
      const dPath = curvePathToD(d.instantAlphaPath, w / d.naturalSize.width, h / d.naturalSize.height);
      if (dPath) img.style.clipPath = `path("${dPath}")`;
    }
    const m = d.mask?.common;
    if (m?.position && m.size && m.size.width > 0 && m.size.height > 0) {
      // TSD.ImageArchive.mask: the mask frame is in the image drawable's own
      // space — show only that window, keeping the full-size image behind it
      // (ppd deck cover photo: 1770x1508 image cropped to a 1770x577 band).
      // The photo border and drop shadow belong to the WINDOW, not the
      // full image box: Keynote's export of smp.org's wave photo (459×307
      // image, 459×245 window) draws its 7pt white stroke centred on the
      // window's edge; on the outer box it painted a white mat over the
      // hidden strip and shadowed the caption below. A CSS border sits
      // outside the content box, so the window is widened by the stroke
      // and shifted by half of it to keep the stroke centred on the edge.
      const stroke = c.style?.stroke;
      const sw = stroke && !stroke.frame && stroke.widthPt > 0 ? stroke.widthPt : 0;
      const wrap = el("div");
      wrap.style.position = "absolute";
      wrap.style.left = `${m.position.x - sw / 2}px`;
      wrap.style.top = `${m.position.y - sw / 2}px`;
      wrap.style.width = `${m.size.width + sw}px`;
      wrap.style.height = `${m.size.height + sw}px`;
      wrap.style.boxSizing = "border-box";
      wrap.style.overflow = "hidden";
      applyBoxStroke(wrap, stroke);
      if (div.style.filter) { wrap.style.filter = div.style.filter; div.style.filter = ""; }
      img.style.position = "absolute";
      img.style.left = `${-m.position.x - sw / 2}px`;
      img.style.top = `${-m.position.y - sw / 2}px`;
      img.style.width = `${w}px`;
      img.style.height = `${h}px`;
      img.style.maxWidth = "none";
      wrap.appendChild(img);
      div.appendChild(wrap);
    } else {
      // photo borders / picture frames (the stroke lives on the image style)
      applyBoxStroke(div, c.style?.stroke);
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
    wrap.appendChild(renderTable(d.table, ctx, doc, d.common?.size?.width));
    div.appendChild(wrap);
    // Numbers keeps a stale frame on tables (a 3628pt-tall box on a 43-row
    // budget sheet); the rendered rows set the layer's height instead so
    // the sheet canvas fits the drawn table.
    if ((doc as { kind?: string }).kind === "numbers") div.style.height = "auto";
  } else if (d.type === "chart") {
    const svg = chartSvg(d.chart, w, h, (doc as { kind?: string }).kind === "numbers");
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
    wrap.appendChild(renderTable(d.table, ctx, doc, d.common?.size?.width));
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