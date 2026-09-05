/**
 * pnk JSON document models — primitives.
 *
 * Shared scalar types and style primitives used by every document model
 * (Pages / Numbers / Keynote). Design rules, enforced across all files:
 *
 * 1. UNITS — internet-friendly everywhere:
 *    - lengths/positions/sizes: points (pt), as plain `number`
 *    - angles: degrees (`angleDeg`), as plain `number`
 *    - colors: `#rrggbb` or `#rrggbbaa` hex strings
 *    - dates: ISO 8601 strings
 *    - durations: `{ seconds: number }` (plain SI seconds, not calendar)
 *
 * 2. OPTIONAL vs NULL — the convention for "not specified" vs "explicitly unset":
 *    - `field?: T`          → absent = the source never specified a value
 *      (protos: field absent, and no `*_null` flag).
 *    - `field: T | null`    → null = the source EXPLICITLY cleared the value
 *      (protos: `*_null = true` flag, or an intentional empty marker).
 *    Absent and null are therefore distinct and both meaningful: a viewer falls
 *    back to its own default for absent, and renders "no value" for null.
 *    In practice TSS style null-flags (`font_name_null` etc.) only survive on
 *    *unresolved* styles; this model ships RESOLVED styles, so `| null` fields
 *    are rare (table cell values, document-level explicit-unset spots).
 *
 * 3. NO INDIRECTION — every object is embedded; there are no object ids, no
 *    TSP.References, no style-archive pointers. What was a reference in the
 *    protobuf object graph is an inline object here.
 *
 * 4. RUST SERDE COMPATIBLE — camelCase fields, string-union enums (no numeric
 *    enums), tagged unions via a `type`/`kind` discriminant string. No TS-only
 *    types (no tuples-as-pairs, no branded types in serialized positions).
 *
 * Provenance for every mapping lives in docs/model-design.md; format facts in
 * docs/format/*.md.
 */

// ---------------------------------------------------------------------------
// Colors
// ---------------------------------------------------------------------------

/**
 * Color as a hex string: `#rrggbb`, or `#rrggbbaa` when the source color has
 * an alpha other than fully opaque. Alpha is in the last byte pair
 * (`00` = transparent, `ff` = opaque).
 *
 * Source colors are `TSP.Color` [proto: .scratch/otorp/Keynote/TSPMessages.proto
 * → TSP.Color]: model rgb/cmyk/white, float components 0..1, RGB color space
 * srgb (1) or p3 (2), plus `headroom` for HDR extension. Conversion rules:
 *  - rgb/srgb  → direct scale 0..1 → 0..255 per channel.
 *  - rgb/p3    → converted to the nearest sRGB approximation; when the result
 *    is visibly out of sRGB gamut the converter adds a
 *    `color-out-of-gamut` warning (see docs/model-design.md). [inferred]
 *  - cmyk/white→ converted to sRGB by the standard naive formulas
 *    (cmyk: r=(1-c)(1-k) etc.; white: r=g=b=w). [inferred: standard math;
 *    fixtures may need refinement]
 *  - headroom ≠ 1 (HDR) → clamp after conversion + `color-hdr-clamped` warning.
 */
export type HexColor = `#${string}`;

// ---------------------------------------------------------------------------
// Geometry (all in points, angles in degrees)
// ---------------------------------------------------------------------------

/** A position in points, origin at the canvas top-left, y increasing downward. */
export interface Point {
  x: number;
  y: number;
}

/** Width/height in points. */
export interface Size {
  width: number;
  height: number;
}

/** An axis-aligned rectangle in points. */
export interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Inset distances in points (top/left/bottom/right), from `TSD.EdgeInsetsArchive`. */
export interface EdgeInsets {
  top: number;
  left: number;
  bottom: number;
  right: number;
}

/**
 * Fills, from `TSD.FillArchive` [proto: .scratch/otorp/Keynote/TSDArchives.proto:158-163]:
 * exactly one of color / gradient / image is meaningful.
 * A fill property on the resolved style is `Fill | undefined`: undefined =
 * no fill specified (inherit chain exhausted / transparent).
 */
export type Fill = SolidFill | GradientFill | ImageFill;

export interface SolidFill {
  type: "solid";
  color: HexColor;
}

export interface GradientFill {
  type: "gradient";
  gradient: Gradient;
}

export interface ImageFill {
  type: "image";
  /** The fill image, resolved to a `Data/` asset. */
  image: MediaRef;
  /** How the image is fitted. [proto: TSD.ImageFillArchive.ImageFillTechnique] */
  technique:
    | "natural-size"
    | "stretch"
    | "tile"
    | "scale-to-fill"
    | "scale-to-fit";
  /** Tint color, if the source carries one. [proto: TSD.ImageFillArchive.tint] */
  tint?: HexColor;
}

/**
 * Gradient, from `TSD.GradientArchive`
 * [proto: .scratch/otorp/Keynote/TSDArchives.proto:121-137].
 * Stops are in source order; `fraction` is the position 0..1 along the
 * gradient axis, `inflection` the midpoint bias between neighbors.
 */
export interface Gradient {
  kind: "linear" | "radial";
  stops: GradientStop[];
  /**
   * Linear gradient angle in degrees (0 = left→right, measuring
   * counterclockwise). [proto: TSD.AngleGradientArchive.gradientangle]
   */
  angleDeg?: number;
  /**
   * Explicit start/end for gradients that carry a transform instead of an
   * angle, in the filled object's coordinate space.
   * [proto: TSD.TransformGradientArchive start/end/baseNaturalSize]
   */
  startPoint?: Point;
  endPoint?: Point;
}

export interface GradientStop {
  color: HexColor;
  /** Position along the gradient axis, 0..1. */
  fraction: number;
  /** Midpoint bias between this stop and the next, 0..1. */
  inflection?: number;
}

/**
 * Shadow, from `TSD.ShadowArchive`
 * [proto: .scratch/otorp/Keynote/TSDArchives.proto:218-234].
 * Defaults shown are the proto defaults; the converter bakes them in.
 */
export interface Shadow {
  color: HexColor;
  /** Light angle in degrees (proto default 315). */
  angleDeg: number;
  /** Offset distance in points (proto default 5). */
  offsetPt: number;
  /** Blur radius in points (proto default 1). */
  radiusPt: number;
  /** 0..1 (proto default 1). */
  opacity: number;
  kind: "drop" | "contact" | "curved";
  /** Contact-shadow extras. [proto: TSD.ContactShadowArchive height/offset] */
  contact?: { height?: number; offset?: number };
  /** Curved-shadow bend factor. [proto: TSD.CurvedShadowArchive curve] */
  curved?: { curve?: number };
}

/** Stroke/outline, from `TSD.StrokeArchive`
 * [proto: .scratch/otorp/Keynote/TSDArchives.proto:177-192]. */
export interface Stroke {
  color: HexColor;
  /** Stroke width in points. */
  widthPt: number;
  cap: "butt" | "round" | "square";
  join: "miter" | "round" | "bevel";
  /** Miter limit when `join` is "miter". */
  miterLimit?: number;
  /**
   * Dash pattern in points; empty/undefined = solid.
   * [proto: TSD.StrokePatternArchive.pattern/phase/count]
   */
  dash?: number[];
  /** Dash phase offset in points. */
  dashPhase?: number;
  /**
   * Picture frame drawn INSTEAD of the plain stroke: Pages' frame presets
   * ("Formal Shadow", "Simple White", …) are a white mat with a soft
   * shadow around the box. [proto: TSD.StrokeArchive.frame = 8 →
   * TSD.FrameArchive { frameName = 2, assetScale = 3 } (dunhamsteve proto;
   * unlisted in the 15.3.1 extraction, fixture-verified on 10a06959)]
   */
  frame?: { name: string; assetScale?: number };
  /**
   * Hand-drawn ("sketch") stroke preset name when the stroke is one of
   * Keynote's smart strokes ("Pencil", "Marker", ...): the line is drawn
   * with a textured brush the viewer approximates with a plain stroke.
   * [proto: TSD.StrokeArchive.smart_stroke = 7 → TSD.SmartStrokeArchive.stroke_name = 2]
   */
  smartStroke?: string;
}

/** Decorative line ends (arrowheads etc.).
 * [proto: TSD.LineEndArchive — path/identifier/is_filled]. */
export interface LineEnd {
  /** Preset identifier of the end decoration (e.g. a named arrowhead). */
  identifier?: string;
  isFilled?: boolean;
  /** The decoration outline as explicit curves, if the source carried a path. */
  path?: CurvePath;
}

/** Reflection under the object. [proto: TSD.ReflectionArchive — opacity 0.5 default] */
export interface Reflection {
  opacity: number;
}

// ---------------------------------------------------------------------------
// Curves — the universal shape language
// ---------------------------------------------------------------------------

/**
 * One path element — `TSP.Path` translated to explicit primitives with
 * COMPACT FLAT positional point arrays [proto: .scratch/otorp/Keynote/
 * TSPMessages.proto → TSP.Path { ElementType: moveTo=1, lineTo=2,
 * quadCurveTo=3, curveTo=4, closeSubpath=5; each Element carries repeated
 * TSP.Point }] — `points` is `[x1,y1,x2,y2,…]` (SVG-style pairs), not
 * `{x,y}` objects:
 *  - "move"  → points = [x, y] (subpath start).
 *  - "line"  → points = [x, y] (line target).
 *  - "quad"  → points = [cx, cy, x, y] (control, target).
 *  - "cubic" → points = [c1x, c1y, c2x, c2y, x, y] (two controls, target).
 *  - "close" → closes the current subpath (no points).
 */
export type CurveElement =
  | { type: "move"; points: [number, number] }
  | { type: "line"; points: [number, number] }
  | { type: "quad"; points: [number, number, number, number] }
  | { type: "cubic"; points: [number, number, number, number, number, number] }
  | { type: "close" };

/**
 * An explicit vector path in the shape's own coordinate space (points).
 * A straight LINE is the minimal form: exactly one "move" + one "line"
 * element (2 sharp nodes, stroke-only, no fill — fixture-verified:
 * G5 acid line, editable_bezier_path_source with 2 nodes). Its coordinates
 * are already in the drawable's point space — no naturalSize scaling.
 */
export interface CurvePath {
  elements: CurveElement[];
}

// ---------------------------------------------------------------------------
// Alignment enums
// ---------------------------------------------------------------------------

/**
 * Horizontal text alignment. Source is the opaque
 * `TSWP.ParagraphStylePropertiesArchive.TextAlignmentType` (TATvalue0..4);
 * the value mapping is anchored by numbers-parser
 * [parser: masaccio/numbers-parser@32387958 src/numbers_parser/cell.py:145-149]:
 * TATvalue0=LEFT, TATvalue1=RIGHT, TATvalue2=CENTER, TATvalue3=JUSTIFIED,
 * TATvalue4=AUTO. Note the surprising 1=right / 2=center order.
 */
export type HorizontalAlignment = "left" | "right" | "center" | "justify" | "auto";

/**
 * Vertical text alignment in a frame/cell. Sources:
 * `TSWP.ShapeStylePropertiesArchive.VerticalAlignmentType`
 * (kFrameAlignTop=0/Middle=1/Bottom=2/Justify=3) and the int32
 * `TST.CellStylePropertiesArchive.vertical_alignment` (same 0..3 order).
 * [proto: .scratch/otorp/Keynote/TSWPArchives.proto:496-503;
 *  TSTStylePropertyArchiving.proto:24] [inferred: TST int32 uses the same enum]
 */
export type VerticalAlignment = "top" | "middle" | "bottom" | "justify";

// ---------------------------------------------------------------------------
// Dates & durations
// ---------------------------------------------------------------------------

/** ISO 8601 date-time string, always UTC (`...Z`).
 * Source dates are doubles = seconds since 2001-01-01T00:00:00Z
 * [proto: TSP.Date.seconds; parser: masaccio/numbers-parser@32387958
 * src/numbers_parser/cell.py (EPOCH + timedelta)]. */
export type IsoDateString = string;

/** A duration as plain seconds (iWork stores durations as double seconds). */
export interface Duration {
  seconds: number;
}

// ---------------------------------------------------------------------------
// Resolved text styles (no inheritance, no attribute-table offsets)
// ---------------------------------------------------------------------------

/** Underline style. Source: `TSWP.CharacterStylePropertiesArchive.UnderlineType`
 * (kNoUnderline=0, kSingle=1, kDouble=2, kWavy=3) [proto]. */
export type UnderlineStyle = "none" | "single" | "double" | "wavy";

/** Strikethrough style. Source: `StrikethruType` (kNo=0, kSingle=1, kDouble=2, kTriple=3) [proto]. */
export type StrikethroughStyle = "none" | "single" | "double" | "triple";

/** Capitalization. Source: `CapitalizationType` (kNoCaps=0, kAllCaps=1, kSmallCaps=2, kTitled=3) [proto]. */
export type Capitalization = "none" | "all-caps" | "small-caps" | "title";

/** Superscript/subscript. Source: `SuperscriptType` [proto]. */
export type BaselineScript = "normal" | "superscript" | "subscript";

/**
 * Resolved character formatting (from TSWP.CharacterStylePropertiesArchive
 * merged along the TSS.StyleArchive parent chain — see docs/model-design.md).
 * Every field optional = "not specified anywhere in the chain".
 * Font metrics are in points; colors are hex.
 */
export interface CharStyle {
  fontName?: string;
  fontSizePt?: number;
  bold?: boolean;
  italic?: boolean;
  underline?: UnderlineStyle;
  strikethrough?: StrikethroughStyle;
  capitalization?: Capitalization;
  baseline?: BaselineScript;
  /** Vertical baseline offset in points. [proto: baseline_shift] */
  baselineShiftPt?: number;
  /** Letter spacing in points. [proto: tracking (kerning is the legacy pair)] */
  trackingPt?: number;
  fontColor?: HexColor;
  /** Highlight color behind the glyphs. [proto: background_color] */
  backgroundColor?: HexColor;
  /** Text outline. [proto: outline (width) + outline_color] */
  outline?: { widthPt: number; color?: HexColor };
  /** Per-glyph text shadow. [proto: shadow (TSD.ShadowArchive)] */
  shadow?: Shadow;
  /** Language tag, e.g. "en-US". [proto: language] */
  language?: string;
  /** OpenType feature tags, e.g. ["tnum"]. [proto: font_features] */
  fontFeatures?: string[];
}

/**
 * Resolved paragraph formatting (from TSWP.ParagraphStylePropertiesArchive +
 * TSWP.LineSpacingArchive, merged along the style parent chain).
 * All indents/spacing in points.
 */
export interface ParaStyle {
  horizontalAlignment?: HorizontalAlignment;
  /** Left indent (hanging: first line outdents by firstLineIndentPt). */
  leftIndentPt?: number;
  rightIndentPt?: number;
  firstLineIndentPt?: number;
  spaceBeforePt?: number;
  spaceAfterPt?: number;
  /**
   * Line spacing. Source `TSWP.LineSpacingArchive` [proto]:
   * mode relative (multiple of line height) / minimum / exact / maximum /
   * space-between, with `amount`. `lineSpacingMultiple` carries the common
   * relative case; `lineSpacingExactPt` the exact case. Both may be absent.
   */
  lineSpacingMultiple?: number;
  lineSpacingExactPt?: number;
  lineSpacingMode?: "min" | "max" | "space-between";
  /** Explicit tab stops. [proto: TSWP.TabsArchive] */
  tabs?: TabStop[];
  /** Distance between default tab stops (points); absent = app default. */
  defaultTabStopPt?: number;
  /** List/bullet formatting if the paragraph is part of a list. */
  list?: ListFormat;
  /** Heading level from `outline_level` (0 = body text; 1..5 = heading depth). */
  outlineLevel?: number;
  keepLinesTogether?: boolean;
  keepWithNext?: boolean;
  hyphenate?: boolean;
  pageBreakBefore?: boolean;
  /** Paragraph background fill. [proto: fill] */
  backgroundColor?: HexColor;
  /** Paragraph border drawn around the paragraph block. [proto: stroke] */
  border?: Stroke;
  /** Writing direction override. [proto: writing_direction] */
  writingDirection?: "left-to-right" | "right-to-left";
  /**
   * Drop cap on the paragraph's leading characters. [proto: TSWP.DropCapArchive
   * via DropCapStyleArchive.drop_cap_properties; paragraphs keyed by
   * StorageArchive.table_drop_cap_style (field 28)] Shape/image caps degrade
   * to text rendering with an unsupported-feature warning (converter policy).
   */
  dropCap?: DropCap;
}

/** Drop-cap parameters, resolved. [proto: TSWP.DropCapArchive] */
export interface DropCap {
  /** Body lines the cap spans. [proto: number_of_lines, default 3] */
  lines?: number;
  /** Lines raised above the first baseline. [proto: number_of_raised_lines] */
  raisedLines?: number;
  /** Leading characters included in the cap. [proto: number_of_characters, default 1] */
  characters?: number;
  /** Glyph scale inside the cap box (0..1]. [proto: character_scale, default 1] */
  characterScale?: number;
  /** Hang into the margin, points. [proto: outdent] */
  outdentPt?: number;
  /** Gap between cap and adjacent text, points. [proto: padding] */
  paddingPt?: number;
  /** Resolved char overrides for the cap glyphs. [proto: DropCapStyleArchive.char_properties] */
  charStyle?: CharStyle;
}

/** One tab stop. [proto: TSWP.TabArchive position/alignment/leader] */
export interface TabStop {
  positionPt: number;
  alignment: "left" | "center" | "right" | "decimal";
  /** Leader character(s) filling the tab run (e.g. ".", "_"). */
  leader?: string;
}

/**
 * List formatting, from `TSWP.ListStyleArchive` resolved per level
 * [proto: .scratch/otorp/Keynote/TSWPArchives.proto → TSWP.ListStyleArchive:
 * label_types/number_types/strings/indents/…, one entry per nesting level].
 */
export interface ListFormat {
  /** Nesting level, 0-based. */
  level: number;
  /** What renders at the marker position. */
  markerKind: "none" | "string" | "number" | "image";
  /** Literal marker text when markerKind = "string" (e.g. "•", "→"). */
  markerText?: string;
  /**
   * Numbering scheme when markerKind = "number"
   * (source NumberType enum, ~65 locale variants; the converter maps the
   * latin/roman/alpha kinds by name and degrades exotic locale kinds to
   * "decimal" with a warning).
   */
  numberKind?: NumberKind;
  /** Marker image when markerKind = "image". */
  markerImage?: MediaRef;
  /**
   * Number surround when markerKind = "number": "1." (period, the default —
   * omitted per omit-default), "1)" (paren), "(1)" (double-paren), bare "1"
   * (none). [proto: TSWP.ListStyleArchive number_types encode scheme+surround
   * combos; split here so NumberKind stays a pure scheme]
   */
  numberSurround?: "period" | "paren" | "double-paren" | "none";
  /** Where the marker text starts (continues from previous list otherwise). */
  start?: number;
  /** Indent of the marker relative to the paragraph's left indent, in points. */
  markerIndentPt?: number;
  /**
   * Marker-origin-to-text-column distance as a multiple of the paragraph
   * font size. [proto: TSWP.ListStyleArchive text_indents = 12, per level;
   * inferred em semantics: Dyalog deck 1.0417 x 48pt = 50pt]
   */
  textIndentEm?: number;
  /** Marker color from the list style's own look; absent = inherit the first
   * run's style. [proto: TSWP.ListStyleArchive font_color (21, null 20)] */
  markerColor?: HexColor;
  /** Marker font override. [proto: ListStyleArchive font_name (23, null 22)] */
  markerFontName?: string;
  /** Marker glyph scale relative to the text size. [proto: LabelGeometry.scale (14), default 1] */
  markerScale?: number;
  /** Marker baseline offset in points. [proto: LabelGeometry.baseline_offset] */
  markerBaselineOffsetPt?: number;
}

export type NumberKind =
  | "decimal"
  | "alpha-upper"
  | "alpha-lower"
  | "roman-upper"
  | "roman-lower"
  | "other";

// ---------------------------------------------------------------------------
// Media references
// ---------------------------------------------------------------------------

/**
 * A pointer to an embedded media asset, resolved from the TSP.DataInfo
 * registry (see docs/format/media.md). The referenced bytes live in the
 * container; the root document envelope lists all assets in `media`.
 */
export interface MediaRef {
  /** DataInfo identifier (uint64, serialized as decimal string). */
  dataId: string;
  /** Actual member name under `Data/` in the container. */
  fileName?: string;
  /** User-facing original name. */
  preferredFileName?: string;
  /** Pixel dimensions when known (image assets). */
  pixelSize?: Size;
}

// ---------------------------------------------------------------------------
// Motion blur (builds only)
// ---------------------------------------------------------------------------

/**
 * Motion blur on Keynote build effects.
 * [proto: KN.TransitionAttributesArchive custom_motion_blur + custom_blur_amount;
 *  KN.BuildAttributesArchive equivalents]
 *
 * NOTE: there is NO static "blur" style property anywhere in the TSD/TSS
 * proto surface (verified by grep over .scratch/otorp, 2026-08-28) — blur in
 * iWork exists only as an animation parameter. Do not invent a static blur.
 */
export interface MotionBlur {
  /** 0..1 blur amount. */
  amount: number;
}
