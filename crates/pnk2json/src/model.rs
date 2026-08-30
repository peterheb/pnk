//! pnk JSON document model — Rust serde mirror of `model/src/*.ts`.
//!
//! Field names here ARE the JSON names (docs/model-design.md §7): every struct
//! carries `#[serde(rename_all = "camelCase")]`, optional = `Option` +
//! `skip_serializing_if`, and tagged unions use `#[serde(tag = "type")]`
//! (or `"kind"` / `"tag"` where the TS contract says so). The TS files remain
//! the contract; this module must stay 1:1 reviewable against them.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// primitives.ts — colors, geometry, fills, strokes, curves, text styles
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeInsets {
    pub top: f64,
    pub left: f64,
    pub bottom: f64,
    pub right: f64,
}

/// `TSP.Color` converted to `#rrggbb[aa]` per docs/model-design.md §2.3.
pub type HexColor = String;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum Fill {
    Solid { color: HexColor },
    Gradient { gradient: Gradient },
    Image {
        image: MediaRef,
        technique: ImageFillTechnique,
        #[serde(skip_serializing_if = "Option::is_none")]
        tint: Option<HexColor>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImageFillTechnique {
    NaturalSize,
    Stretch,
    Tile,
    ScaleToFill,
    ScaleToFit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Gradient {
    pub kind: GradientKind,
    pub stops: Vec<GradientStop>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle_deg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_point: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_point: Option<Point>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GradientKind {
    Linear,
    Radial,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GradientStop {
    pub color: HexColor,
    pub fraction: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inflection: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Shadow {
    pub color: HexColor,
    pub angle_deg: f64,
    pub offset_pt: f64,
    pub radius_pt: f64,
    pub opacity: f64,
    pub kind: ShadowKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<ContactShadow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curved: Option<CurvedShadow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShadowKind {
    Drop,
    Contact,
    Curved,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ContactShadow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurvedShadow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub curve: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Stroke {
    pub color: HexColor,
    pub width_pt: f64,
    pub cap: StrokeCap,
    pub join: StrokeJoin,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub miter_limit: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash: Option<Vec<f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dash_phase: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StrokeCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StrokeJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LineEnd {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_filled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<CurvePath>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reflection {
    pub opacity: f64,
}

// --- curves ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CurveElement {
    // points are compact flat pairs [x1,y1,x2,y2,…] (SVG-style), matching the
    // ratified TS contract in model/src/primitives.ts (2ed592d): move/line 2
    // numbers, quad 4, cubic 6, close none.
    Move { points: Vec<f64> },
    Line { points: Vec<f64> },
    Quad { points: Vec<f64> },
    Cubic { points: Vec<f64> },
    Close {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        points: Vec<f64>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CurvePath {
    pub elements: Vec<CurveElement>,
}

// --- alignment enums ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HorizontalAlignment {
    Left,
    Right,
    Center,
    Justify,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerticalAlignment {
    Top,
    Middle,
    Bottom,
    Justify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageLayoutOrientation {
    Portrait,
    Landscape,
}

// --- text style enums ---

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnderlineStyle {
    None,
    Single,
    Double,
    Wavy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StrikethroughStyle {
    None,
    Single,
    Double,
    Triple,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capitalization {
    None,
    AllCaps,
    SmallCaps,
    Title,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BaselineScript {
    Normal,
    Superscript,
    Subscript,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CharStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<UnderlineStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strikethrough: Option<StrikethroughStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capitalization: Option<Capitalization>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline: Option<BaselineScript>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_shift_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tracking_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_color: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_color: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline: Option<CharOutline>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow: Option<Shadow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_features: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CharOutline {
    pub width_pt: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<HexColor>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WritingDirection {
    LeftToRight,
    RightToLeft,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ParaStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub horizontal_alignment: Option<HorizontalAlignment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left_indent_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right_indent_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_line_indent_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_before_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_after_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_spacing_multiple: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_spacing_exact_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_spacing_mode: Option<LineSpacingMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tabs: Option<Vec<TabStop>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_tab_stop_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub list: Option<ListFormat>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outline_level: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_lines_together: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_with_next: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hyphenate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_break_before: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_color: Option<HexColor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub border: Option<Stroke>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub writing_direction: Option<WritingDirection>,
    /// Drop cap on the paragraph's leading characters (TSWP.DropCapArchive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop_cap: Option<DropCap>,
}

/// Drop-cap parameters, resolved. [proto: TSWP.DropCapArchive]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DropCap {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raised_lines: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub characters: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub character_scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outdent_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub char_style: Option<CharStyle>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LineSpacingMode {
    Min,
    Max,
    SpaceBetween,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TabStop {
    pub position_pt: f64,
    pub alignment: TabAlignment,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub leader: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TabAlignment {
    Left,
    Center,
    Right,
    Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ListFormat {
    pub level: u32,
    pub marker_kind: ListMarkerKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_kind: Option<NumberKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker_image: Option<MediaRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker_indent_pt: Option<f64>,
    /// Number surround: "1." (period, default/omitted), "1)", "(1)", bare "1".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number_surround: Option<NumberSurround>,
    /// Marker color from the list style's own look (ListStyleArchive font_color).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker_font_name: Option<String>,
    /// Marker glyph scale relative to text size (LabelGeometry.scale, default 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker_scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marker_baseline_offset_pt: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NumberSurround {
    Period,
    Paren,
    DoubleParen,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ListMarkerKind {
    None,
    String,
    Number,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NumberKind {
    Decimal,
    AlphaUpper,
    AlphaLower,
    RomanUpper,
    RomanLower,
    Other,
}

// --- media references ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaRef {
    /// DataInfo identifier as a decimal string (JS-safe u64).
    pub data_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pixel_size: Option<Size>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MotionBlur {
    pub amount: f64,
}

// ---------------------------------------------------------------------------
// shared.ts — envelope, warnings, text, drawables, tables, charts
// ---------------------------------------------------------------------------

pub type FontList = Vec<String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AppKind {
    #[default]
    Pages,
    Numbers,
    Keynote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WarningCode {
    UnknownObjectType,
    UndecodableObject,
    UnresolvedReference,
    UnsupportedFeature,
    MediaMissing,
    ColorDegraded,
    LegacyVariant,
    TableDegraded,
    FormulaUnparsed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    pub code: WarningCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Aggregation: total occurrences this row stands for; None = 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// Up to 5 distinct example paths when count > 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MediaAsset {
    pub data_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_file_name: Option<String>,
    pub kind: MediaKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub byte_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pixel_size: Option<Size>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MediaKind {
    Image,
    Movie,
    Audio,
    Pdf,
    Other,
}

/// Document-wide text-style pools (model contract: deduped, first-use
/// order; absent index = unstyled/default).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StylePools {
    pub para: Vec<ParaStyle>,
    pub char: Vec<CharStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMeta {
    pub app: AppKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub application: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_format_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_version_history: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub document_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

// --- text (TSWP) ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct StyledText {
    #[serde(default)]
    pub paragraphs: Vec<Paragraph>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Paragraph {
    /// Index into the document's `styles.para` pool; absent = default/unstyled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p_style: Option<u32>,
    pub items: Vec<ParagraphItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged, rename_all_fields = "camelCase")]
pub enum ParagraphItem {
    /// Plain unstyled text run — the common case.
    Plain(String),
    /// Styled run (no `type` key needed: the `text` key is self-evident).
    Text {
        text: String,
        /// Index into the document's `styles.char` pool; absent = unstyled.
        #[serde(skip_serializing_if = "Option::is_none")]
        c_style: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        hyperlink: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        language: Option<String>,
    },
    InlineObject {
        #[serde(rename = "type")]
        kind: InlineObjectTag,
        drawable: Drawable,
        #[serde(skip_serializing_if = "Option::is_none")]
        offset: Option<InlineOffset>,
    },
    Field {
        #[serde(rename = "type")]
        kind: FieldTag,
        /// Index into the document's `styles.char` pool; absent = unstyled.
        #[serde(skip_serializing_if = "Option::is_none")]
        c_style: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        value: Option<String>,
        field: FieldKind,
    },
}

/// Tag literals for the tagged variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InlineObjectTag {
    #[serde(rename = "inline-object")]
    InlineObject,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldTag {
    #[serde(rename = "field")]
    Field,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct InlineOffset {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub h_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub v_pt: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum FieldKind {
    PageNumber,
    PageCount,
    FootnoteMark,
    Date { update_plan: DateUpdatePlan },
    Other {
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DateUpdatePlan {
    Never,
    Auto,
    Once,
}

// --- drawables (TSD) ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct DrawableCommon {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle_deg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flipped: Option<Flips>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hyperlink: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locked: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accessibility_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_wrap: Option<TextWrap>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<DrawableStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadow: Option<Shadow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reflection: Option<Reflection>,
    /// Keynote build/animation attached to this drawable (keynote.ts module
    /// augmentation of DrawableCommon).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keynote_build: Option<BuildSpec>,
    /// Placeholder role when this drawable is a master/template placeholder
    /// (keynote.ts / pages.ts — `placeholder: { role, inherited }`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<PlaceholderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Flips {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub horizontal: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextWrap {
    pub kind: TextWrapKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margin_pt: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextWrapKind {
    None,
    Around,
    AboveBelow,
    Left,
    Right,
    Largest,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaceholderInfo {
    /// Role as stored: "title" | "body" | "object" | "slide-number" |
    /// "placeholder" (generic) | template tag text (Pages).
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherited: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DrawableStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<Fill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stroke: Option<Stroke>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_ends: Option<LineEnds>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LineEnds {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<LineEnd>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail: Option<LineEnd>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextFit {
    Grow,
    Shrink,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum Drawable {
    Shape {
        common: DrawableCommon,
        geometry: ShapeGeometry,
        #[serde(skip_serializing_if = "Option::is_none")]
        text: Option<StyledText>,
        #[serde(skip_serializing_if = "Option::is_none")]
        vertical_alignment: Option<VerticalAlignment>,
        #[serde(skip_serializing_if = "Option::is_none")]
        text_insets: Option<TextInsets>,
        /// "grow" = box grows to fit text; "shrink" = text scales down to fit;
        /// absent = fixed box, viewer clips. Resolved at emission.
        #[serde(skip_serializing_if = "Option::is_none")]
        text_fit: Option<TextFit>,
    },
    Textbox {
        common: DrawableCommon,
        text: StyledText,
        #[serde(skip_serializing_if = "Option::is_none")]
        vertical_alignment: Option<VerticalAlignment>,
        #[serde(skip_serializing_if = "Option::is_none")]
        text_insets: Option<TextInsets>,
        /// See Shape.text_fit.
        #[serde(skip_serializing_if = "Option::is_none")]
        text_fit: Option<TextFit>,
    },
    Image {
        common: DrawableCommon,
        image: MediaRef,
        #[serde(skip_serializing_if = "Option::is_none")]
        original: Option<MediaRef>,
        #[serde(skip_serializing_if = "Option::is_none")]
        thumbnail: Option<MediaRef>,
        #[serde(skip_serializing_if = "Option::is_none")]
        svg: Option<MediaRef>,
        #[serde(skip_serializing_if = "Option::is_none")]
        natural_size: Option<Size>,
        #[serde(skip_serializing_if = "Option::is_none")]
        mask: Option<ImageMask>,
        #[serde(skip_serializing_if = "Option::is_none")]
        adjustments: Option<ImageAdjustments>,
    },
    Movie {
        common: DrawableCommon,
        #[serde(skip_serializing_if = "Option::is_none")]
        movie: Option<MediaRef>,
        #[serde(skip_serializing_if = "Option::is_none")]
        remote_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        poster: Option<MediaRef>,
        #[serde(skip_serializing_if = "Option::is_none")]
        audio_only: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        trim: Option<MovieTrim>,
        #[serde(skip_serializing_if = "Option::is_none")]
        r#loop: Option<MovieLoop>,
        #[serde(skip_serializing_if = "Option::is_none")]
        volume: Option<f64>,
    },
    Group {
        common: DrawableCommon,
        children: Vec<Drawable>,
        #[serde(skip_serializing_if = "Option::is_none")]
        freehand: Option<FreehandInfo>,
    },
    ConnectionLine {
        common: DrawableCommon,
        path: CurvePath,
        #[serde(skip_serializing_if = "Option::is_none")]
        from: Option<AnchorFacts>,
        #[serde(skip_serializing_if = "Option::is_none")]
        to: Option<AnchorFacts>,
    },
    Table {
        common: DrawableCommon,
        table: TableModel,
    },
    Chart {
        common: DrawableCommon,
        chart: ChartModel,
    },
    Unknown {
        #[serde(skip_serializing_if = "Option::is_none")]
        common: Option<DrawableCommon>,
        type_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        type_name: Option<String>,
        reason: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AnchorFacts {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Point>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TextInsets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageMask {
    pub geometry: ShapeGeometry,
    pub common: DrawableCommon,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ImageAdjustments {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saturation: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contrast: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlights: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shadows: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MovieTrim {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster_time: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MovieLoop {
    None,
    Repeat,
    BackAndForth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FreehandInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation: Option<FreehandAnimation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FreehandAnimation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#loop: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShapeGeometry {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scalar: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub natural_size: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<CurvePath>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callout: Option<CalloutParams>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CalloutParams {
    pub tail_position: Point,
    pub tail_size: Size,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corner_radius: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub center_tail: Option<bool>,
}

// --- tables (TST) ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableModel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub row_count: u32,
    pub column_count: u32,
    pub header_row_count: u32,
    pub header_column_count: u32,
    pub footer_row_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_rows_frozen: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_columns_frozen: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<RowColInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<RowColInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_row_height_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_column_width_pt: Option<f64>,
    /// Dense row-major cell grid (`grid[row][column]`); `None` = no cell
    /// stored at that position (sparse tables).
    pub grid: Vec<Vec<Option<GridCell>>>,
    /// Distinct number formats used by this table, deduped; cells reference
    /// them by `TableCell.formatIndex`.
    pub formats: Vec<CellFormat>,
    /// Distinct per-cell looks used by this table, deduped (same pooling
    /// pattern as the document-wide text-style pools); cells reference by
    /// `TableCell.cellStyleIndex`, absent = table default style.
    pub cell_styles: Vec<TableCellStyle>,
    pub merges: Vec<TableMerge>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<TableStyle>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RowColInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_pt: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableCell {
    /// The cell's value; `None` = present-but-valueless (style/merge only).
    pub v: GridValue,
    /// Value type tag — REQUIRED when the JSON type of `v` is ambiguous
    /// (an ISO string could be text, a number could be seconds), omitted for
    /// plain text/number/bool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<CellTypeTag>,
    /// Currency code when type = "currency".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cur: Option<String>,
    /// Index into `TableModel.formats`; absent = unformatted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fmt: Option<u32>,
    /// Index into `TableModel.cellStyles`; absent = table default look.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell_style_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formula: Option<TsceFormulaRef>,
}

/// The `v` payload: plain scalar, ISO string, or rich text.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum GridValue {
    Scalar(String),
    Number(f64),
    Bool(bool),
    Richtext(StyledText),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CellTypeTag {
    Date,
    Duration,
    Currency,
    Richtext,
    Error,
}

/// One grid slot: a plain unformatted value or an explicit cell object.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum GridCell {
    /// Plain unformatted simple value.
    Plain(GridPlain),
    /// Cell that needs more than a bare value.
    Cell(TableCell),
}

/// Plain grid values (unformatted text/number/bool).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum GridPlain {
    Text(String),
    /// JSON number preserving integral-ness (7434 serializes as 7434, not
    /// 7434.0) — smaller envelopes, same value.
    Number(serde_json::Number),
    Bool(bool),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum CellValue {
    Empty,
    Number { value: f64 },
    Text { value: String },
    Bool { value: bool },
    Date { value: String },
    Duration { value: f64 },
    Currency {
        value: f64,
        #[serde(skip_serializing_if = "Option::is_none")]
        currency_code: Option<String>,
    },
    Richtext { text: StyledText },
    Error { value: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableCellStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<Fill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub borders: Option<CellBorders>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertical_alignment: Option<VerticalAlignment>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<CharStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paragraph: Option<ParaStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_wrap: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding: Option<PaddingInsets>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CellBorders {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<Stroke>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<Stroke>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<Stroke>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<Stroke>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PaddingInsets {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CellFormat {
    pub kind: CellFormatKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub currency_code: Option<String>,
    /// Thousands separators shown (TSK.FormatStructArchive.show_thousands_separator).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grouping: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format_string: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CellFormatKind {
    Number,
    Currency,
    Percent,
    Date,
    Duration,
    Text,
    Custom,
    Automatic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableMerge {
    pub anchor_row: u32,
    pub anchor_column: u32,
    pub row_span: u32,
    pub column_span: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banded_rows: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub banded_fill: Option<Fill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_cell_style: Option<TableCellStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_row_cell_style: Option<TableCellStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header_column_cell_style: Option<TableCellStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer_row_cell_style: Option<TableCellStyle>,
}

// --- charts (TSCH) ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChartModel {
    pub r#type: ChartType,
    pub three_d: bool,
    pub data_status: ChartDataStatus,
    pub categories: Vec<String>,
    pub series: Vec<ChartSeries>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legend_frame: Option<Rect>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub legend_visible: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub series_colors: Option<Vec<HexColor>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_binding: Option<TsceFormulaRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scatter_format: Option<ChartScatterFormat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChartDataStatus {
    Inline,
    TableBound,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChartScatterFormat {
    SeparateX,
    SharedX,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChartSeries {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub values: Vec<Option<ChartValue>>,
}

/// Grid values: numbers, dates (ISO), or a hole.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ChartValue {
    Number(f64),
    Date(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChartType {
    Column,
    StackedColumn,
    Bar,
    StackedBar,
    Line,
    Area,
    StackedArea,
    Pie,
    Donut,
    Scatter,
    Bubble,
    Radar,
    Other,
}

// --- TSCE placeholder ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TsceFormulaRef {
    pub id: String,
    /// Always "unparsed" (TSCE ASTs are never decompiled).
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_text: Option<String>,
    pub warning: Warning,
}

impl TsceFormulaRef {
    pub fn unparsed(id: impl Into<String>) -> TsceFormulaRef {
        TsceFormulaRef {
            id: id.into(),
            status: "unparsed".to_string(),
            source_text: None,
            warning: Warning {
                code: WarningCode::FormulaUnparsed,
                message: "TSCE formula kept opaque; the stored last-calculated value is in the cell/chart data".into(),
                path: None,
                detail: None,
                count: None,
                paths: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// keynote.ts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MasterSlide {
    pub name: String,
    pub drawables: Vec<Drawable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<StyledText>,
    /// Slide background fill [proto: KN.SlideStyleArchive.slide_properties.fill].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Fill>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildSpec {
    pub delivery: BuildDelivery,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automatic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub acceleration: Option<BuildAcceleration>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_delivery: Option<BuildTextDelivery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chunks: Option<Vec<BuildChunk>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub motion_blur: Option<MotionBlur>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum BuildDelivery {
    #[default]
    In,
    Out,
    Action,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildAcceleration {
    None,
    EaseIn,
    EaseOut,
    EaseBoth,
    Custom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildTextDelivery {
    ByObject,
    ByWord,
    ByCharacter,
    ByLine,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BuildChunk {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automatic: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TransitionSpec {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effect: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub animation_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delay_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub automatic: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Slide {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_name: Option<String>,
    /// Resolved master underlay painted before `drawables` (docs/model-review.md §3b).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub master_drawables: Option<Vec<Drawable>>,
    pub drawables: Vec<Drawable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<StyledText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition: Option<TransitionSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slide_number_visible: Option<bool>,
    /// Slide background fill, RESOLVED through the master chain at emission;
    /// absent = no effective fill (docs/model-review.md §3b).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<Fill>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeynoteDocument {
    /// Always "keynote".
    pub kind: String,
    pub meta: DocumentMeta,
    pub warnings: Vec<Warning>,
    pub fonts: FontList,
    pub media: Vec<MediaAsset>,
    pub styles: StylePools,
    pub slide_size: Size,
    pub slides: Vec<Slide>,
    pub masters: Vec<MasterSlide>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub theme_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playback: Option<KeynotePlayback>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub soundtrack: Option<Soundtrack>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recording: Option<RecordingInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct KeynotePlayback {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<KeynotePlayMode>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#loop: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autoplay_transition_delay_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autoplay_build_delay_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slide_numbers_visible: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KeynotePlayMode {
    Normal,
    AutoPlay,
    HyperlinksOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Soundtrack {
    pub tracks: Vec<MediaAsset>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repeat: Option<SoundtrackRepeat>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SoundtrackRepeat {
    None,
    One,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecordingInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<f64>,
}

// ---------------------------------------------------------------------------
// numbers.ts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SheetPrintSetup {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<PageLayoutOrientation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_page_numbers: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_order: Option<PageOrder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub margins: Option<EdgeInsets>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_page_number: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_custom_start_page_number: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_header_inset: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_footer_inset: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PageOrder {
    DownThenOver,
    OverThenDown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
    pub drawables: Vec<Drawable>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<StyledText>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footers: Option<Vec<StyledText>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uses_single_header_footer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<SheetStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub print: Option<SheetPrintSetup>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_direction_rtl: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SheetStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fill: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NumbersDocument {
    /// Always "numbers".
    pub kind: String,
    pub meta: DocumentMeta,
    pub warnings: Vec<Warning>,
    pub fonts: FontList,
    pub media: Vec<MediaAsset>,
    pub styles: StylePools,
    pub sheets: Vec<Sheet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forms: Option<Vec<NumbersForm>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NumbersForm {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sheet_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_table_name: Option<String>,
}

// ---------------------------------------------------------------------------
// pages.ts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageTemplate {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub drawables: Vec<Drawable>,
    pub placeholders: Vec<PagePlaceholder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_fill: Option<Fill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide_headers_footers: Option<bool>,
    #[serde(default)]
    pub headers: Vec<StyledText>,
    #[serde(default)]
    pub footers: Vec<StyledText>,
    pub headers_footers_match_previous_page: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PagePlaceholder {
    pub tag: String,
    pub drawable: Drawable,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub z_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct PagesSection {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_page_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub even_page_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub odd_page_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_numbering: Option<PageNumbering>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inherit_previous_header_footer: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_fill: Option<Fill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_paragraph_start: Option<u32>,
    /// Multi-column layout; absent = single column (docs/model-review dispatch).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub columns: Option<SectionColumns>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SectionColumns {
    pub count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gutter_pt: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageNumbering {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_at: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_page_number_kind: Option<FirstPageNumberKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FirstPageNumberKind {
    Continue,
    RestartAt,
    FromPrevious,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FootnotePlacement {
    SectionEndnotes,
    DocumentEndnotes,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Footnote {
    pub anchor_paragraph_index: u32,
    pub text: StyledText,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FloatingPage {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_index: Option<u32>,
    /// Page-layout flavor: resolved template underlay painted before
    /// `drawables` (docs/model-review.md §3c). Absent for word-processing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template_drawables: Option<Vec<Drawable>>,
    pub drawables: Vec<Drawable>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PagesDocument {
    /// Always "pages".
    pub kind: String,
    pub flavor: PagesFlavor,
    pub meta: DocumentMeta,
    pub warnings: Vec<Warning>,
    pub fonts: FontList,
    pub media: Vec<MediaAsset>,
    pub styles: StylePools,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_size: Option<Size>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_margins: Option<PageMargins>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<PageLayoutOrientation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_scale: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<StyledText>,
    /// Page-layout flavor only: the never-rendered body flow the file
    /// still carries (Convert-to-Layout "body discarded" is rendering-level).
    /// Omitted when the stored body is empty/absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hidden_body: Option<StyledText>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footnotes: Option<Vec<Footnote>>,
    /// Endnote collection mode; absent = page-bottom footnotes (the default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footnote_placement: Option<FootnotePlacement>,
    pub floating: Vec<FloatingPage>,
    pub page_templates: Vec<PageTemplate>,
    pub sections: Vec<PagesSection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub table_of_contents: Option<TableOfContents>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PagesFlavor {
    WordProcessing,
    PageLayout,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PageMargins {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bottom: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub left: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub right: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub header: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub footer: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TableOfContents {
    pub entries: Vec<TocEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TocEntry {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
}

/// The converter output: exactly one of the three document flavors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum PnkDocument {
    Pages(PagesDocument),
    Numbers(NumbersDocument),
    Keynote(KeynoteDocument),
}

impl PnkDocument {
    pub fn meta(&self) -> &DocumentMeta {
        match self {
            PnkDocument::Pages(d) => &d.meta,
            PnkDocument::Numbers(d) => &d.meta,
            PnkDocument::Keynote(d) => &d.meta,
        }
    }
}
