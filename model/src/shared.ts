/**
 * pnk JSON document models — shared structures.
 *
 * Text (TSWP), drawables (TSD), tables (TST), charts (TSCH) and the calc
 * engine (TSCE) placeholder — used by all three document models.
 *
 * Conventions (see primitives.ts header + docs/model-design.md):
 *  - units: pt / degrees / #rrggbb / ISO 8601
 *  - `field?: T` = not specified; `field: T | null` = explicitly unset
 *  - no object ids, no TSP.References, no attribute-table offsets:
 *    styles are resolved and inlined, references are embedded objects
 *  - tagged unions with a `type`/`kind` string discriminant (serde-friendly)
 */


import type {
  CharStyle,
  CurvePath,
  Fill,
  HexColor,
  IsoDateString,
  LineEnd,
  MediaRef,
  ParaStyle,
  Point,
  Reflection,
  Shadow,
  Size,
  Stroke,
  VerticalAlignment,
} from "./primitives";
import type { KeynoteDocument } from "./keynote";
import type { NumbersDocument } from "./numbers";
import type { PagesDocument } from "./pages";

// Everything in primitives is part of the public model surface.
export * from "./primitives";
// ---------------------------------------------------------------------------
// Root envelope
// ---------------------------------------------------------------------------
export interface Warning {
  /** Stable machine code, e.g. "unknown-object-type". */
  code: WarningCode;
  /** Human-readable explanation, self-contained. */
  message: string;
  /**
   * Where the warning applies: a model path like `sheets[0].drawables[3]`
   * (converter-defined, best effort).
   */
  path?: string;
  /** Original object type id / registry name, when the warning is about one. */
  detail?: string;
  /**
   * Aggregation: total occurrences this row stands for; absent = 1. Warnings
   * differing only in embedded numbers (cell coords, object ids) collapse to
   * one row at emission — `message`/`path` are the first occurrence's.
   */
  count?: number;
  /** Up to 5 distinct example paths when count > 1. */
  paths?: string[];
}

/** Which app produced the document. */
export type AppKind = "pages" | "numbers" | "keynote";

export type WarningCode =
  /** A TSP.MessageInfo.type id had no trusted registry entry. */
  | "unknown-object-type"
  /** Known object type whose payload did not decode. */
  | "undecodable-object"
  /** A TSP.Reference / DataReference pointed nowhere. */
  | "unresolved-reference"
  /** Content exists but the viewer model cannot represent it faithfully. */
  | "unsupported-feature"
  /** Media bytes missing from Data/ or the registry. */
  | "media-missing"
  /** Color fell outside sRGB or HDR headroom was clamped. */
  | "color-degraded"
  /** Pre-UFF / legacy structures best-effort decoded. */
  | "legacy-variant"
  /** Table with pre-BNC storage or other degraded decode. */
  | "table-degraded"
  /** Formula AST present but not converted to text. */
  | "formula-unparsed";

/** Page/canvas orientation. [proto: orientation flag; numbers in_portrait_page_orientation] */
export type PageLayoutOrientation = "portrait" | "landscape";

/** One embedded media asset from the container's `Data/` store. */
export interface MediaAsset {
  /** DataInfo identifier (uint64 as decimal string). */
  dataId: string;
  /** Member name under `Data/` (or in the package directory). */
  fileName?: string;
  /** User-facing original file name. */
  preferredFileName?: string;
  kind: "image" | "movie" | "audio" | "pdf" | "other";
  /** Byte length when materialized. */
  byteLength?: number;
  /** Pixel dimensions for images. */
  pixelSize?: Size;
}

/**
 * Document-level metadata, resolved from `Metadata/Properties.plist`,
 * `Metadata/BuildVersionHistory.plist`, `Metadata/DocumentIdentifier` and
 * `TSP.PackageMetadata` (object id 2). See docs/format/container.md.
 */
export interface DocumentMeta {
  app: AppKind;
  /** App that last saved the file, e.g. "Pages" [proto/Properties.plist Application]. */
  application?: string;
  /** fileFormatVersion string from Properties.plist. */
  fileFormatVersion?: string;
  /**
   * Build version history: last entries of BuildVersionHistory.plist,
   * oldest → newest. Useful for feature gating.
   */
  buildVersionHistory?: string[];
  /** Document UUID (Metadata/DocumentIdentifier or PackageMetadata.revision). */
  documentId?: string;
  createdAt?: IsoDateString;
  modifiedAt?: IsoDateString;
  /** Author string if the source carries one. */
  author?: string;
  /** Locale identifier from TSK.DocumentArchive. [proto] */
  locale?: string;
}

/** Text-entry gate for viewers: deduped font names used in the document. */
export type FontList = string[];

/**
 * The root shape every converter emits — exactly one of the three flavors,
 * each carrying the same envelope fields.
 */
export type PnkDocument = PagesDocument | NumbersDocument | KeynoteDocument;

// Envelope field block shared by the three document roots (structurally;
// each concrete root redeclares these to stay JSON-flat).
export interface DocumentEnvelope {
  meta: DocumentMeta;
  /** Anything dropped/unknown/degraded — machine-readable, never silent. */
  warnings: Warning[];
  /** Deduped, sorted font names referenced anywhere in the document. */
  fonts: FontList;
  /** All embedded media assets (the `Data/` inventory), for one-pass fetching. */
  media: MediaAsset[];
  /**
   * Document-wide style pools, deduped and ordered first-use. Text nodes
   * reference entries by index (`Paragraph.pStyle` → `styles.para`,
   * `TextRun`/`FieldRun` `.cStyle` → `styles.char`); absent index =
   * unstyled/default. Drawable styles stay INLINE on purpose (measured:
   * pooling them is not worth the churn — docs/model-design.md §2).
   */
  styles: StylePools;
}

/** The two text-style pools. [proto payload: TSWP.ParagraphStylePropertiesArchive / CharacterStylePropertiesArchive] */
export interface StylePools {
  /** Resolved paragraph styles, deduped, first-use order. */
  para: ParaStyle[];
  /** Resolved character styles, deduped, first-use order. */
  char: CharStyle[];
}

// ---------------------------------------------------------------------------
// Text model (TSWP) — styled paragraphs/runs, fully resolved, no offsets
// ---------------------------------------------------------------------------

/**
 * A block of rich text. Source is a `TSWP.StorageArchive`: one character
 * buffer + attribute tables mapping UTF-16 offsets to styles/attachments.
 * The converter SPLITS the buffer at paragraph boundaries (newlines) and at
 * character-style entry offsets, resolving styles into the document's
 * `styles` pools — no character indexes and no inline style objects survive
 * (docs/model-design.md §Flattening).
 * [proto: .scratch/otorp/Keynote/TSWPArchives.proto → TSWP.StorageArchive;
 *  splitting verified in docs/format/text.md]
 */
export interface StyledText {
  paragraphs: Paragraph[];
}

export interface Paragraph {
  /** Index into the document's `styles.para` pool; absent = default/unstyled. */
  pStyle?: number;
  /** Content items in visual order. */
  items: ParagraphItem[];
  /**
   * A hard page break precedes this paragraph (U+0005 in the storage's
   * character buffer; the marker itself never prints). Distinct from the
   * paragraph STYLE's pageBreakBefore. [inferred: b31db822 / 155d6ba3]
   */
  pageBreakBefore?: boolean;
}

/**
 * One content item. A bare JSON string is a plain unstyled text run (the
 * common case); objects are used only when there is more to say:
 *  - `{ text, cStyle?, hyperlink?, language? }` — a styled run (no `type`
 *    key needed: the `text` key is self-evident);
 *  - `{ type: "inline-object", … }` / `{ type: "field", … }` — rare, tagged.
 */
export type ParagraphItem = string | TextRun | InlineObjectRun | FieldRun;

export interface TextRun {
  text: string;
  /** Index into the document's `styles.char` pool; absent = unstyled. */
  cStyle?: number;
  /** Hyperlink target when the run is a link. [proto: HyperlinkFieldArchive] */
  hyperlink?: string;
  /** Language override for this run (rare; usually on style). */
  language?: string;
}

/**
 * An inline attachment — the U+FFFC OBJECT REPLACEMENT CHARACTER position in
 * the source text [proto + parser: docs/format/text.md §Attachments]. The
 * drawable itself (image/shape/table/…) is embedded right here.
 */
export interface InlineObjectRun {
  type: "inline-object";
  /** The attached drawable, fully resolved. */
  drawable: Drawable;
  /** Anchor offsets in points. [proto: TSWP.DrawableAttachmentArchive h/v_offset] */
  offset?: { hPt?: number; vPt?: number };
  /**
   * "Move with Text" placement: the drawable floats on the page at
   * (text-area left + offset.hPt, anchor paragraph top + offset.vPt) and
   * body text wraps around it per `common.textWrap`; absent/false = inline
   * with text, sitting in the line like a glyph. Converter rule: non-zero
   * offset or an exterior wrap kind other than none [inferred, corpus
   * survey 2026-09-01: 370/374 zero-offset objects wrap none].
   */
  anchored?: boolean;
}

/**
 * A smart field that renders as text (page number, page count, footnote mark,
 * date). Source: TSWP smart-field archives
 * [proto: docs/format/text.md §Fields].
 */
export interface FieldRun {
  type: "field";
  /** Index into the document's `styles.char` pool; absent = unstyled. */
  cStyle?: number;
  /** Current rendered value as stored, when present. */
  value?: string;
  field:
    | { kind: "page-number" }
    | { kind: "page-count" }
    | { kind: "footnote-mark" }
    | { kind: "date"; updatePlan: "never" | "auto" | "once" }
    | { kind: "other"; detail?: string };
}

// ---------------------------------------------------------------------------
// Drawables (TSD) — everything placeable on a canvas
// ---------------------------------------------------------------------------

/**
 * Common geometry + styling of every drawable. Source: `TSD.DrawableArchive`
 * [proto: .scratch/otorp/Keynote/TSDArchives.proto:321-335] — position/size
 * from `TSD.GeometryArchive` (angle in RADIANS there, degrees here),
 * plus link/lock/wrap attributes.
 */
export interface DrawableCommon {
  /** Bounding position in canvas coordinates (points). */
  position?: Point;
  /** Natural size in points. */
  size?: Size;
  /** Rotation in degrees (converted from proto radians), counterclockwise. */
  angleDeg?: number;
  /** Horizontal/vertical mirroring of the shape source. [proto: PathSourceArchive flips] */
  flipped?: { horizontal?: boolean; vertical?: boolean };
  hyperlink?: string;
  locked?: boolean;
  accessibilityDescription?: string;
  /** Wrap text around this object's outline. [proto: TSD.ExteriorTextWrapArchive] */
  textWrap?: {
    kind: "none" | "around" | "above-below" | "left" | "right" | "largest";
    marginPt?: number;
  };
  /** Visual styling (resolved; undefined = no styling specified). */
  style?: DrawableStyle;
  /** Opacity 0..1. [proto: ShapeStylePropertiesArchive.opacity] */
  opacity?: number;
  shadow?: Shadow;
  reflection?: Reflection;
}

/** Resolved drawable styling (TSD.ShapeStylePropertiesArchive / MediaStylePropertiesArchive). */
export interface DrawableStyle {
  fill?: Fill;
  stroke?: Stroke;
  lineEnds?: { head?: LineEnd; tail?: LineEnd };
}

/**
 * The drawable union. `unknown` carries payloads the converter could not
 * decode — never silently dropped (see docs/model-design.md §Dropped).
 */
export type Drawable =
  | ShapeDrawable
  | TextboxDrawable
  | ImageDrawable
  | MovieDrawable
  | GroupDrawable
  | ConnectionLineDrawable
  | TableDrawable
  | ChartDrawable
  | UnknownDrawable;

export interface ShapeDrawable {
  type: "shape";
  common: DrawableCommon;
  /** Geometry as explicit curves / preset parameters — see ShapeGeometry. */
  geometry: ShapeGeometry;
  /** Text typed inside the shape, if any (resolved from the owned storage). */
  text?: StyledText;
  /** Vertical alignment of the shape's text. [proto: TSWP.ShapeStylePropertiesArchive] */
  verticalAlignment?: VerticalAlignment;
  /** Text insets. [proto: TSWP text insets] */
  textInsets?: { top?: number; left?: number; bottom?: number; right?: number };
  /** How hosted text relates to the box: "grow" = box grows vertically to fit,
   * "shrink" = text scales down to fit (Keynote placeholder shrink-to-fit).
   * Absent = fixed box, viewer clips. [proto: TSWP.ShapeStylePropertiesArchive
   * shrink_to_fit; auto-grow per text-box flags — resolved at emission] */
  textFit?: "grow" | "shrink";
}

export interface TextboxDrawable {
  type: "textbox";
  common: DrawableCommon;
  text: StyledText;
  verticalAlignment?: VerticalAlignment;
  textInsets?: { top?: number; left?: number; bottom?: number; right?: number };
  /**
   * Path-source natural size, present only when `common.size` is 0×0
   * (Numbers content-sized text boxes). A hint, not the box: Numbers sizes
   * the box to its text; this is the size the box had when created
   * [inferred: 6914f46e51ab, verified against Numbers' export].
   */
  naturalSize?: Size;
  /** See ShapeDrawable.textFit — absent = fixed box, viewer clips. */
  textFit?: "grow" | "shrink";
}

/**
 * Shape geometry, flattened from the six `TSD.PathSourceArchive` variants
 * [proto: .scratch/otorp/Keynote/TSDArchives.proto:98-119 + 28-96].
 * Priority: explicit `path` (bezier/editable bezier) wins; otherwise a preset
 * shape is named and the viewer renders it; `naturalSize` is the preset's
 * design size (scale to the drawable's size).
 */
export interface ShapeGeometry {
  /** Preset shape identifier, e.g. "star", "plus", "left-arrow", "rounded-rect", "chevron", "callout". */
  preset?: string;
  /**
   * Preset parameter: corner radius for rounded-rect, pointiness for star
   * (source `ScalarPathSourceArchive.scalar`) [inferred: semantic per docs/format/drawables.md].
   */
  scalar?: number;
  /** Design size of the preset/path source. */
  naturalSize?: Size;
  /**
   * Explicit path when the source carried bezier data (converted to curves).
   * Coordinates are in `naturalSize` space (or drawable space when no
   * naturalSize was given).
   */
  path?: CurvePath;
  /** Callout tail parameters. [proto: TSD.CalloutPathSourceArchive] */
  callout?: {
    tailPosition: Point;
    tailSize: Size;
    cornerRadius?: number;
    centerTail?: boolean;
  };
  /**
   * Control point of a point preset, in `naturalSize` space. Arrows: x is
   * the head length in points and y the shaft's top edge as a fraction of
   * the height (Keynote's default arrow stores 64 × 0.34, verified against
   * the app's export of atnf.csiro.au's Bayesian deck). Star: x is the
   * number of points and y the inner radius as a fraction of the outer.
   * [proto: TSD.PointPathSourceArchive.point; semantics inferred]
   */
  point?: Point;
}

export interface ImageDrawable {
  type: "image";
  common: DrawableCommon;
  /** Primary image bytes (resolved DataReference). */
  image: MediaRef;
  /** Original (pre-adjustment) image when stored separately. */
  original?: MediaRef;
  /** Thumbnail when stored separately. */
  thumbnail?: MediaRef;
  /** SVG source when the "image" was imported from SVG. */
  svg?: MediaRef;
  /** Natural (untransformed) size in points. */
  naturalSize?: Size;
  /** Clipping mask as a drawable-shaped path. [proto: TSD.ImageArchive.mask] */
  mask?: { geometry: ShapeGeometry; common: DrawableCommon };
  /** Non-destructive image adjustments. [proto: TSD.ImageAdjustmentsArchive] */
  adjustments?: {
    exposure?: number;
    saturation?: number;
    contrast?: number;
    highlights?: number;
    shadows?: number;
    brightness?: number;
    [key: string]: number | undefined;
  };
  /**
   * Present when the image is a rendered equation (Insert > Equation in
   * Keynote/Pages/Numbers): `image` is the app's PDF rendering of
   * `equation.source`, and the source expression is the extractable
   * content. [proto: TSWP.EquationInfoArchive extends TSD.ImageArchive]
   */
  equation?: EquationInfo;
}

/**
 * The expression behind an equation image. [proto: TSWP.EquationInfoArchive
 * extension fields on TSD.ImageArchive: equation_source_text = 103 (older
 * files carry only equation_source_old = 100), equation_depth = 102,
 * equation_text_properties = 101]
 */
export interface EquationInfo {
  /** The expression as the author typed it: LaTeX, or MathML markup. */
  source: string;
  /** "mathml" when `source` starts with a `<math` element, else "latex" [inferred from the text]. */
  format: "latex" | "mathml";
  /**
   * Baseline depth in points: how far the rendered image extends below the
   * text baseline when placed inline (the image's bottom edge sits
   * `depthPt` under the baseline). [proto: equation_depth]
   */
  depthPt?: number;
  /** Font size the equation was set in, points. [proto: equation_text_properties.font_size] */
  fontSizePt?: number;
  /** Font family name. [proto: equation_text_properties.font_name] */
  fontName?: string;
  /** Text color. [proto: equation_text_properties.font_color] */
  color?: HexColor;
}

export interface MovieDrawable {
  type: "movie";
  common: DrawableCommon;
  /** Movie bytes (resolved DataReference), when embedded. */
  movie?: MediaRef;
  /** Remote URL for linked/streaming movies. [proto: movieRemoteURL] */
  remoteUrl?: string;
  /** Poster frame image. */
  poster?: MediaRef;
  /** Audio-only movies keep a poster image. [proto: audioOnly] */
  audioOnly?: boolean;
  /** Trim range in seconds. [proto: startTime/endTime/posterTime] */
  trim?: { start?: number; end?: number; posterTime?: number };
  loop?: "none" | "repeat" | "back-and-forth";
  /** Playback volume 0..1. */
  volume?: number;
}

export interface GroupDrawable {
  type: "group";
  common: DrawableCommon;
  /**
   * Children with coordinates in the GROUP's coordinate space
   * (proto children carry absolute geometry; the converter re-bases them so
   * a group can be moved as one — docs/model-design.md §Flattening).
   */
  children: Drawable[];
  /** Freehand drawing metadata when this group is one. [proto: TSD.FreehandDrawingArchive ext 100] */
  freehand?: { opacity?: number; animation?: { duration?: number; loop?: boolean } };
}

/** A connector line between two drawables. [proto: TSD.ConnectionLineArchive] */
export interface ConnectionLineDrawable {
  type: "connection-line";
  common: DrawableCommon;
  /**
   * Routing as explicit curves (quadratic or orthogonal), in canvas space.
   * The proto stores the two endpoints as object references; the converter
   * resolves both anchors and bakes their positions into the path.
   */
  path: CurvePath;
  /** The shape this connector was attached to, as an embedded copy when the
   * target resolved; its `common.position/size` are the anchor facts. */
  from?: Pick<DrawableCommon, "position" | "size">;
  to?: Pick<DrawableCommon, "position" | "size">;
}

/** Table on a canvas — the wrapper (TST.TableInfoArchive) around a TableModel. */
export interface TableDrawable {
  type: "table";
  common: DrawableCommon;
  table: TableModel;
}

/** Chart on a canvas — TSCH.ChartDrawableArchive with its model resolved. */
export interface ChartDrawable {
  type: "chart";
  common: DrawableCommon;
  chart: ChartModel;
}

/** A drawable the converter recognized but could not model. */
export interface UnknownDrawable {
  type: "unknown";
  common?: DrawableCommon;
  /** Registry type id, hex string (e.g. "0x1a2b") — never guessed names. */
  typeId: string;
  /** Registry message name when a trusted table had one. */
  typeName?: string;
  reason: string;
}

// ---------------------------------------------------------------------------
// Tables (TST) — data resolved, styles inlined
// ---------------------------------------------------------------------------

/**
 * A table, resolved from TST.TableModelArchive + DataStore + tiles
 * (docs/format/tables.md). Dimensions and header counts are explicit; the
 * cell grid is DENSE row-major (`grid[row][column]`) — a viewer can walk it
 * 1:1 onto `<tr>` rendering — with `null` marking absent cells (sparse
 * sheets). Cell values are the LAST CALCULATED results; the model never
 * re-evaluates formulas (docs/format/calcengine.md).
 */
export interface TableModel {
  /**
   * Table name. [proto: TableModelArchive.table_name] Always carried when
   * stored: formula text (`TsceFormulaRef.sourceText`) names tables by it
   * ("Table 1::A1"). Whether the caption is DRAWN above the table is
   * `nameHidden` (absent = drawn).
   */
  name?: string;
  /** True when Numbers does not show the name above the table. [proto: table_name_enabled (22) not true] */
  nameHidden?: boolean;
  /**
   * Category grouping ("Organize by" a column), when enabled. `grid` stays
   * the ungrouped data; this is the group tree Numbers lays over it, with
   * the summary rules and the app's cached totals. [proto: TableModelArchive
   * category_owner (86) → CategoryOwnerRefArchive → GroupByArchive; see
   * crates/pnk2json/src/categories.rs]
   */
  grouping?: TableGrouping;
  rowCount: number;
  columnCount: number;
  headerRowCount: number;
  headerColumnCount: number;
  footerRowCount: number;
  /** Frozen header rows/cols (scroll behavior). */
  headerRowsFrozen?: boolean;
  headerColumnsFrozen?: boolean;
  /** Per-row sizes/hidden flags, length = rowCount (absent entries = default). */
  rows?: RowColInfo[];
  /** Per-column widths/hidden flags, length = columnCount. */
  columns?: RowColInfo[];
  /** Default row height / column width in points. [proto: fields 16/17] */
  defaultRowHeightPt?: number;
  defaultColumnWidthPt?: number;
  /**
   * Cell grid, row-major: exactly `rowCount` rows of `columnCount` entries.
   * `null` = no cell stored for that position (sparse tables). A present
   * cell is a plain JSON string/number/boolean (unformatted simple value) or
   * a `TableCell` object when there is more to say — see the glossary in
   * docs/model-design.md §Reading the envelope. Values are the LAST
   * CALCULATED results; the model never re-evaluates formulas
   * (docs/format/calcengine.md).
   */
  grid: (GridCell | null)[][];
  /**
   * Distinct number formats used by this table, deduped; cells reference a
   * format by index (`TableCell.fmt`), absent = unformatted.
   */
  formats: CellFormat[];
  /**
   * Distinct per-cell looks used by this table, deduped (same pooling pattern
   * as the document-wide text-style pools); cells reference by
   * `TableCell.cellStyleIndex`, absent = table default style.
   */
  cellStyles: TableCellStyle[];
  /** Merged regions; only the anchor cell carries content in `grid`. */
  merges: TableMerge[];
  /** Resolved table-level look. */
  style?: TableStyle;
  /**
   * Look of the table NAME drawn above the table (Numbers furniture): text
   * and paragraph alignment, resolved. [proto: TST.TableModelArchive
   * table_name_style = 30; the reserved height is table_name_height = 33]
   */
  nameStyle?: TableCellStyle;
}

/**
 * One grid slot: a plain unformatted value or an explicit cell object.
 * The position in `grid` implies row/column, so plain values need no keys.
 */
export type GridCell = string | number | boolean | TableCell;

/**
 * A grid cell that needs more than a bare value: formatted, typed
 * (date/duration/currency/richtext/error), styled, or formula-bearing.
 */
export interface TableCell {
  /** The cell's value; `null` = present-but-valueless (style/merge only). */
  v: string | number | boolean | StyledText | null;
  /**
   * Value type tag — REQUIRED when the JSON type of `v` is ambiguous
   * (an ISO string could be text, a number could be seconds), omitted for
   * plain text/number/bool. `date` v = ISO 8601 UTC string; `duration`
   * v = seconds; `currency` v = amount (+ optional `cur` code);
   * `richtext` v = StyledText; `error` v = stored error string.
   */
  type?: "date" | "duration" | "currency" | "richtext" | "error";
  /** Currency code when type = "currency" (e.g. "USD"). */
  cur?: string;
  /** Index into `TableModel.formats`; absent = unformatted. */
  fmt?: number;
  /** Index into `TableModel.cellStyles`; absent = table default look. */
  cellStyleIndex?: number;
  /** Formula placeholder when the cell computes its value. */
  formula?: TsceFormulaRef;
}

export interface RowColInfo {
  /** Size in points. */
  sizePt?: number;
  hidden?: boolean;
}


/** Resolved per-cell look (TST.CellStylePropertiesArchive + text style); pooled per table. */
export interface TableCellStyle {
  /**
   * Cell fill. `null` = explicitly NO fill (the style's chain sets an
   * empty cell_fill, overriding the table style's section default —
   * 0839b6d2's docx-imported cells over a blue-banded table style);
   * absent = unspecified, the section default (headerRowCellStyle /
   * bodyCellStyle) shows through (burndown's header rows).
   */
  fill?: Fill | null;
  /** Per-side borders; undefined side = no explicit border. */
  borders?: {
    top?: Stroke;
    right?: Stroke;
    bottom?: Stroke;
    left?: Stroke;
  };
  verticalAlignment?: VerticalAlignment;
  text?: CharStyle;
  paragraph?: ParaStyle;
  textWrap?: boolean;
  padding?: { top?: number; left?: number; bottom?: number; right?: number };
}

/** Number format descriptor (kept simple; custom formats degrade to a hint). */
export interface CellFormat {
  kind: "number" | "currency" | "percent" | "date" | "duration" | "text" | "custom" | "automatic";
  /** Decimal places for number-like kinds. */
  decimals?: number;
  /** Currency code for currency (e.g. "USD") when known. */
  currencyCode?: string;
  /** Thousands separators shown (locale-appropriate grouping). Absent = off.
   * [proto: TSK.FormatStructArchive.show_thousands_separator (field 5)] */
  grouping?: boolean;
  /** Accounting style: currency symbol at the cell's left edge, amount at
   * the right. Present only when true.
   * [proto: TSK.FormatStructArchive.use_accounting_style (field 6)] */
  accounting?: boolean;
  /** Raw custom format string when kind = "custom". */
  formatString?: string;
}

/** Category grouping of a table's rows. */
export interface TableGrouping {
  /** Model column indexes grouped by, outermost first. */
  columns: number[];
  /** Summary rules per column shown in group rows. */
  aggregates?: GroupAggregate[];
  /** Group tree, one level per entry of `columns`, in display order. */
  groups: TableGroup[];
  /** Whole-table cached summaries. */
  totals?: GroupTotal[];
}

export interface GroupAggregate {
  column: number;
  /** Stored rule code (ColumnAggregateArchive.agg_type): 2 = sum [inferred from one fixture]; other codes unnamed. */
  rule: number;
  level?: number;
}

export interface TableGroup {
  /** Group key: the grouped column's value; `null` = the blank group. */
  value: string | number | boolean | null;
  /** True when `value` is an ISO date string. */
  date?: boolean;
  /** Model row indexes (into `grid`) of the group's rows; leaf groups only. */
  rows?: number[];
  children?: TableGroup[];
  /** Cached summaries per aggregated column, from the app's accumulators. */
  totals?: GroupTotal[];
}

export interface GroupTotal {
  column: number;
  sum?: number;
  count?: number;
  min?: number;
  max?: number;
}

/** Merged region: anchor (top-left) + span. [proto: TST.MergeRegionMapArchive CellRange] */
export interface TableMerge {
  anchorRow: number;
  anchorColumn: number;
  rowSpan: number;
  columnSpan: number;
}

/** Resolved table-level styling (TST.TableStylePropertiesArchive subset a viewer needs). */
export interface TableStyle {
  bandedRows?: boolean;
  bandedFill?: Fill;
  /** Default look for body cells (per-cell style overrides this). */
  bodyCellStyle?: TableCellStyle;
  /** Default look for header-row / header-column cells. */
  headerRowCellStyle?: TableCellStyle;
  headerColumnCellStyle?: TableCellStyle;
  footerRowCellStyle?: TableCellStyle;
}

// ---------------------------------------------------------------------------
// Charts (TSCH) — type + inline data, rendering deferred
// ---------------------------------------------------------------------------

/**
 * A chart, resolved from TSCH.ChartArchive. Data is INLINED: when the chart
 * carries a private grid (`TSCH.ChartGridArchive`) the series values are
 * right here; when the chart is bound to a table (Numbers mediator), the
 * binding is recorded as a `dataBinding` placeholder and `dataStatus`
 * explains what the viewer can expect. Rendering (axes, ticks, legends) is
 * the viewer's job — the model carries type + data + minimal style hints.
 * [proto: docs/format/charts.md]
 */
export interface ChartModel {
  /** Chart type, normalized from the ~27-value TSCH.ChartType enum. */
  type: ChartType;
  /** True when the type is a 3D variant. */
  threeD: boolean;
  /** Series values are present inline / referenced from a table / unavailable. */
  dataStatus: "inline" | "table-bound" | "unavailable";
  /** Category labels (x axis), after applying `seriesDirection`. */
  categories: string[];
  /** Series, values aligned with `categories` (same length when inline). */
  series: ChartSeries[];
  /** Legend frame in canvas points, when stored. */
  legendFrame?: { x: number; y: number; width: number; height: number };
  legendVisible?: boolean;
  /** Series colors as stored in per-series styles, best effort. */
  seriesColors?: HexColor[];
  /** Numbers-only: the table this chart reads from, as a placeholder. */
  dataBinding?: TsceFormulaRef;
  /** Scatter layout. [proto: TSCH.ScatterFormat] */
  scatterFormat?: "separate-x" | "shared-x";
  /** Chart title when shown. [proto: TSCH.Generated.ChartNonStyleArchive title 46 / showtitle 35] */
  title?: string;
  /** Axis titles when shown. [proto: TSCH.Generated.ChartAxisNonStyleArchive 15/16, show 13/14] */
  categoryAxisTitle?: string;
  valueAxisTitle?: string;
  /** Value-axis bounds when the author pinned them. [proto: usermin 18 / usermax 17] */
  valueAxisMin?: number;
  valueAxisMax?: number;
  /** Major gridline count on the value axis. [proto: field 5] */
  valueAxisMajorGridlines?: number;
  /** Pie/donut hole as a fraction of the radius, when stored (a pie with a hole is a ring). [proto: non-style innerradius 27] */
  innerRadius?: number;
  /**
   * Pie/donut slice labels, from series 0's non-style (Keynote edits all
   * series together). [proto: TSCH.Generated.ChartSeriesNonStyleArchive
   * pieshowserieslabels 31, pieshowvaluelabels 44, pienumberformat 99,
   * pielabelexplosion 147; ChartNonStyleArchive piecalloutlinetype 111]
   */
  pieLabels?: PieLabels;
  /** Value-axis tick number format. [proto: ChartAxisNonStyleArchive defaultnumberformat 42 / numberformattype 3] */
  valueAxisFormat?: ChartNumberFormat;
  /**
   * Chart furniture type sizes, resolved from ChartArchive.paragraph_styles
   * (20) via each sub-style's paragraph-style index (title: chart style 20;
   * legend: legend style 2; axes: axis style 6/7/8; slice labels: series
   * style 23/29/152/153, inherited along the TSS parent chain).
   */
  textSizes?: { titlePt?: number; legendPt?: number; axisPt?: number; labelPt?: number; fontName?: string };
  /**
   * Axis furniture visibility; absent = Keynote's defaults (value gridlines
   * and labels, category baseline and labels). [proto: Generated
   * ChartAxisStyleArchive showaxis 24/25, showmajorgridlines 27/28;
   * ChartAxisNonStyleArchive showlabels 9/10/11]
   */
  axes?: { valueGridlines: boolean; categoryGridlines: boolean; valueAxisLine: boolean; categoryAxisLine: boolean; valueLabels: boolean; categoryLabels: boolean };
}

export interface ChartNumberFormat {
  kind: "number" | "percent" | "currency";
  /** Absent = automatic (the 253 sentinel). */
  decimals?: number;
  thousandsSeparator: boolean;
  currencyCode?: string;
}

export interface PieLabels {
  showSeriesName: boolean;
  showValue: boolean;
  valueFormat?: ChartNumberFormat;
  /** Label centre as a percentage of the pie radius (> 100 = outside the rim). */
  radiusPct?: number;
  leaderLines: boolean;
}

export interface ChartSeries {
  /** Series (legend) label. */
  name?: string;
  /** Values aligned with ChartModel.categories; holes are `null`. */
  values: (number | IsoDateString | null)[];
}

/** Normalized chart types (from TSCH.ChartType, 2D and 3D variants collapsed). */
export type ChartType =
  | "column"
  | "stacked-column"
  | "bar"
  | "stacked-bar"
  | "line"
  | "area"
  | "stacked-area"
  | "pie"
  | "donut"
  | "scatter"
  | "bubble"
  | "radar"
  | "other";

// ---------------------------------------------------------------------------
// TSCE placeholder — formulas stay opaque
// ---------------------------------------------------------------------------

/**
 * A formula reference, deliberately OPAQUE (pnk does not decompile TSCE ASTs
 * — docs/format/calcengine.md). The last-calculated VALUE is already in the
 * cell/chart data; this placeholder only records that a formula existed.
 */
export interface TsceFormulaRef {
  /** Identity of the formula in the source (e.g. the TableDataList key). */
  id: string;
  /**
   * "decoded": `sourceText` holds the formula text re-synthesized from the
   * TSCE AST, as the app's formula editor shows it (relative references
   * resolved against the owning cell; `×`/`÷`/`≥`/`≤`/`≠` operators;
   * `Table::A1` / `Sheet::Table::A1` cross-table prefixes; `#REF!` for
   * broken references). "unparsed": kept opaque, see `warning`.
   */
  status: "unparsed" | "decoded";
  /** Formula text; present when status is "decoded". */
  sourceText?: string;
  /** Present when status is "unparsed": what to surface instead of a live formula. */
  warning?: Warning;
}
