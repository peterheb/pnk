//! TSS style resolution: walk `TSS.StyleArchive.parent` chains and flatten the
//! property payloads into resolved `CharStyle` / `ParaStyle` / `ListFormat`
//! (docs/format/styles.md, docs/model-design.md §3.1).
//!
//! Merge rule: child overrides parent property-by-property; a `*_null = true`
//! flag clears the inherited value and stops the walk for that property.

use crate::ctx::Ctx;
use crate::model::*;
use crate::pb::Msg;

/// One style object in the global id space.
pub struct Style {
    pub name: Option<String>,
    pub parent: Option<u64>,
    pub char_properties: Option<Msg>,
    pub para_properties: Option<Msg>,
}

/// Resolve the style archive at `id` into its base + property payloads.
pub fn style_of(ctx: &Ctx, id: u64) -> Option<Style> {
    let m = ctx.loaded.msg(id)?;
    let base = m.msg(1)?;
    // CharacterStyleArchive.parent rides on the embedded TSS.StyleArchive
    // (field 3); some writers duplicate it on the wrapper.
    let parent = base.reference(3).or_else(|| m.reference(3));
    Some(Style {
        name: base.string(1),
        parent,
        char_properties: m.msg(11),
        para_properties: m.msg(12),
    })
}

/// Walk the parent chain collecting property payload `field` (11 = char,
/// 12 = para), most-derived first. Guards against cycles.
pub fn chain(ctx: &Ctx, id: u64, field: u32) -> Vec<Msg> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cur = Some(id);
    while let Some(sid) = cur {
        if !seen.insert(sid) {
            break;
        }
        match style_of(ctx, sid) {
            Some(style) => {
                let p = if field == 11 {
                    style.char_properties
                } else {
                    style.para_properties
                };
                if let Some(p) = p {
                    out.push(p);
                }
                cur = style.parent;
            }
            None => break,
        }
    }
    out
}

/// First property value along the chain, honoring the null-flag pattern:
/// `Some(Some(m))` = the payload carrying the field; `Some(None)` = explicitly
/// cleared; `None` = keep walking (absent everywhere → truly unset).
fn prop<'a>(msgs: &'a [Msg], field: u32, null_field: Option<u32>) -> Option<Option<&'a Msg>> {
    for m in msgs {
        if let Some(nf) = null_field {
            if m.boolean(nf) == Some(true) {
                return Some(None);
            }
        }
        if m.has(field) {
            return Some(Some(m));
        }
    }
    None
}

fn take(msgs: &[Msg], field: u32, null_field: Option<u32>) -> Option<&Msg> {
    prop(msgs, field, null_field).and_then(|r| r)
}

// ---------------------------------------------------------------------------
// CharStyle
// ---------------------------------------------------------------------------

pub fn resolve_char_style(ctx: &mut Ctx, style_id: u64) -> CharStyle {
    let msgs = chain(ctx, style_id, 11);
    char_style_from(ctx, &msgs)
}

/// Effective character style of a run: the char-style chain first, then the
/// PARAGRAPH style's char_properties chain as fallback. TSWP resolution
/// runs run → paragraph style → (its parents, up to theme presets) — a
/// paragraph style archive carries char_properties in the same slot 11
/// [proto: TSWP.ParagraphStyleArchive char_properties; fixture G5: the
/// Title/Heading look (HelveticaNeue-Bold 30/18pt) lives ONLY there, and
/// Keynote placeholder runs carry e.g. {fontColor} overrides whose size
/// rides on the paragraph chain]. Emitting the merged result keeps the
/// pooled styles fully RESOLVED per model-design §1.5 (omit-default relies
/// on it).
pub fn resolve_effective_char_style(
    ctx: &mut Ctx,
    char_sid: Option<u64>,
    para_sid: Option<u64>,
) -> CharStyle {
    let mut msgs = match char_sid {
        Some(id) => chain(ctx, id, 11),
        None => Vec::new(),
    };
    if let Some(pid) = para_sid {
        msgs.extend(chain(ctx, pid, 11));
    }
    char_style_from(ctx, &msgs)
}

pub fn char_style_from(ctx: &mut Ctx, msgs: &[Msg]) -> CharStyle {
    let mut s = CharStyle::default();

    // font_name(5) with font_name_null(4)
    if let Some(m) = take(msgs, 5, Some(4)) {
        s.font_name = m.string(5);
        if let Some(name) = &s.font_name {
            crate::ctx::collect_font(&mut ctx.fonts, name);
        }
    }
    if let Some(m) = take(msgs, 3, None) {
        s.font_size_pt = m.f32v(3).map(|v| v as f64);
    }
    if let Some(m) = take(msgs, 1, None) {
        s.bold = m.boolean(1);
    }
    if let Some(m) = take(msgs, 2, None) {
        s.italic = m.boolean(2);
    }
    if let Some(m) = take(msgs, 11, None) {
        s.underline = match m.varint(11).unwrap_or(0) {
            1 => Some(UnderlineStyle::Single),
            2 => Some(UnderlineStyle::Double),
            3 => Some(UnderlineStyle::Wavy),
            _ => Some(UnderlineStyle::None),
        };
    }
    if let Some(m) = take(msgs, 12, None) {
        s.strikethrough = match m.varint(12).unwrap_or(0) {
            1 => Some(StrikethroughStyle::Single),
            2 => Some(StrikethroughStyle::Double),
            3 => Some(StrikethroughStyle::Triple),
            _ => Some(StrikethroughStyle::None),
        };
    }
    if let Some(m) = take(msgs, 13, None) {
        s.capitalization = match m.varint(13).unwrap_or(0) {
            1 => Some(Capitalization::AllCaps),
            2 => Some(Capitalization::SmallCaps),
            3 => Some(Capitalization::Title),
            _ => Some(Capitalization::None),
        };
    }
    if let Some(m) = take(msgs, 10, None) {
        s.baseline = match m.varint(10).unwrap_or(0) {
            1 => Some(BaselineScript::Superscript),
            2 => Some(BaselineScript::Subscript),
            _ => Some(BaselineScript::Normal),
        };
    }
    if let Some(m) = take(msgs, 14, None) {
        s.baseline_shift_pt = m.f32v(14).map(|v| v as f64);
    }
    if let Some(m) = take(msgs, 27, None) {
        s.tracking_pt = m.f32v(27).map(|v| v as f64);
    }
    if let Some(m) = take(msgs, 7, Some(6)) {
        s.font_color = crate::tsd::color_of(ctx, m, 7);
    }
    if let Some(m) = take(msgs, 26, Some(25)) {
        s.background_color = crate::tsd::color_of(ctx, m, 26);
    }
    if let Some(m) = take(msgs, 19, None) {
        if let Some(width_pt) = m.f32v(19).map(|v| v as f64) {
            let color = take(msgs, 18, Some(17)).and_then(|m| crate::tsd::color_of(ctx, m, 18));
            s.outline = Some(CharOutline { width_pt, color });
        }
    }
    if let Some(m) = take(msgs, 21, Some(20)) {
        s.shadow = m.msg(21).and_then(|sh| crate::tsd::shadow_of(ctx, &sh));
    }
    if let Some(m) = take(msgs, 9, Some(8)) {
        s.language = m.string(9);
    }
    if let Some(m) = take(msgs, 34, Some(33)) {
        let features: Vec<String> = m
            .msgs(34)
            .into_iter()
            .map(|f| format!("{}:{}", f.varint(1).unwrap_or(0), f.varint(2).unwrap_or(0)))
            .collect();
        if !features.is_empty() {
            s.font_features = Some(features);
        }
    }
    s
}

// ---------------------------------------------------------------------------
// ParaStyle
// ---------------------------------------------------------------------------

pub fn resolve_para_style(ctx: &mut Ctx, style_id: u64) -> ParaStyle {
    let msgs = chain(ctx, style_id, 12);
    para_style_from(ctx, &msgs)
}

pub fn para_style_from(ctx: &mut Ctx, msgs: &[Msg]) -> ParaStyle {
    let mut s = ParaStyle::default();

    if let Some(m) = take(msgs, 1, None) {
        // TATvalue mapping anchored by numbers-parser (docs/format/text.md):
        // 0=left, 1=right, 2=center, 3=justify, 4=auto — do not "fix" it.
        s.horizontal_alignment = match m.varint(1).unwrap_or(4) {
            0 => Some(HorizontalAlignment::Left),
            1 => Some(HorizontalAlignment::Right),
            2 => Some(HorizontalAlignment::Center),
            3 => Some(HorizontalAlignment::Justify),
            4 => Some(HorizontalAlignment::Auto),
            _ => None,
        };
    }
    if let Some(m) = take(msgs, 11, None) {
        s.left_indent_pt = m.f32v(11).map(|v| v as f64);
    }
    if let Some(m) = take(msgs, 19, None) {
        s.right_indent_pt = m.f32v(19).map(|v| v as f64);
    }
    if let Some(m) = take(msgs, 7, None) {
        s.first_line_indent_pt = m.f32v(7).map(|v| v as f64);
    }
    if let Some(m) = take(msgs, 21, None) {
        s.space_before_pt = m.f32v(21).map(|v| v as f64);
    }
    if let Some(m) = take(msgs, 20, None) {
        s.space_after_pt = m.f32v(20).map(|v| v as f64);
    }
    if let Some(m) = take(msgs, 13, Some(12)) {
        if let Some(ls) = m.msg(13) {
            let amount = ls.f32v(2).map(|v| v as f64);
            match ls.varint(1).unwrap_or(0) {
                0 => s.line_spacing_multiple = amount,
                2 => s.line_spacing_exact_pt = amount,
                1 => {
                    s.line_spacing_exact_pt = amount;
                    s.line_spacing_mode = Some(LineSpacingMode::Min);
                }
                3 => {
                    s.line_spacing_exact_pt = amount;
                    s.line_spacing_mode = Some(LineSpacingMode::Max);
                }
                4 => {
                    s.line_spacing_exact_pt = amount;
                    s.line_spacing_mode = Some(LineSpacingMode::SpaceBetween);
                }
                _ => {}
            }
        }
    }
    if let Some(m) = take(msgs, 25, Some(24)) {
        let tabs: Vec<TabStop> = m
            .msgs(25)
            .into_iter()
            .filter_map(|t| {
                Some(TabStop {
                    position_pt: t.f32v(1)? as f64,
                    alignment: match t.varint(2).unwrap_or(0) {
                        1 => TabAlignment::Center,
                        2 => TabAlignment::Right,
                        3 => TabAlignment::Decimal,
                        _ => TabAlignment::Left,
                    },
                    leader: t.string(3),
                })
            })
            .collect();
        if !tabs.is_empty() {
            s.tabs = Some(tabs);
        }
    }
    if let Some(m) = take(msgs, 4, None) {
        s.default_tab_stop_pt = m.f32v(4).map(|v| v as f64);
    }
    // list_style ref (40, null 39): resolved into ListFormat at level 0; the
    // per-paragraph nesting level rides on the storage's para-data table.
    if let Some(m) = take(msgs, 40, Some(39)) {
        if let Some(list_id) = m.reference(40) {
            s.list = resolve_list_format(ctx, list_id, 0);
        }
    }
    if let Some(m) = take(msgs, 27, None) {
        // Some writers store -1 (int32 sentinel) for "no outline level";
        // that arrives as u32::MAX and must be treated as unset
        // (contract: 0 = body, 1..5 = heading depth).
        s.outline_level = m.varint(27).map(|v| v as u32).filter(|v| *v != u32::MAX);
    }
    if let Some(m) = take(msgs, 9, None) {
        s.keep_lines_together = m.boolean(9);
    }
    if let Some(m) = take(msgs, 10, None) {
        s.keep_with_next = m.boolean(10);
    }
    if let Some(m) = take(msgs, 8, None) {
        s.hyphenate = m.boolean(8);
    }
    if let Some(m) = take(msgs, 14, None) {
        s.page_break_before = m.boolean(14);
    }
    if let Some(m) = take(msgs, 6, Some(5)) {
        s.background_color = crate::tsd::color_of(ctx, m, 6);
    }
    if let Some(m) = take(msgs, 32, Some(31)) {
        s.border = m.msg(32).and_then(|st| crate::tsd::stroke_of(ctx, &st));
    }
    if let Some(m) = take(msgs, 38, None) {
        s.writing_direction = match m.int(38) {
            Some(0) => Some(WritingDirection::LeftToRight),
            Some(1) => Some(WritingDirection::RightToLeft),
            _ => None, // -1 = natural: not an override
        };
    }
    s
}

/// `TSWP.ListStyleArchive` resolved for one nesting level
/// (docs/format/text.md §List styles; exotic locale kinds degrade to "other").
///
/// List styles chain like other TSS styles (super = 1 → StyleArchive with
/// parent at 3): overrides carry only the changed arrays — G5's custom
/// bullet stores its "❏" in `strings` (16) while `label_types` (11) rides
/// on the parent — so each property array is taken from the FIRST message
/// along the chain that has it. A per-level lookup falls back to the last
/// entry (arrays are per-level, padded by writers to ~9 entries).
pub fn resolve_list_format(ctx: &mut Ctx, list_id: u64, level: u32) -> Option<ListFormat> {
    // Collect the parent chain, most-derived first.
    let mut msgs: Vec<Msg> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cur = Some(list_id);
    while let Some(id) = cur {
        if !seen.insert(id) {
            break;
        }
        let Some(m) = ctx.loaded.msg(id) else { break };
        let m = m.clone();
        cur = m.msg(1).and_then(|b| b.reference(3)).or_else(|| m.reference(3));
        msgs.push(m);
    }
    if msgs.is_empty() {
        return None;
    }
    fn varints(msgs: &[Msg], field: u32) -> Vec<u64> {
        for m in msgs {
            let v: Vec<u64> = m
                .all(field)
                .into_iter()
                .filter_map(|v| match v {
                    iwadump::proto::Value::Varint(v) => Some(*v),
                    _ => None,
                })
                .collect();
            if !v.is_empty() {
                return v;
            }
        }
        Vec::new()
    }
    fn at_level<T: Copy>(v: &[T], level: u32) -> Option<T> {
        v.get(level as usize).or_else(|| v.last()).copied()
    }

    let label_types = varints(&msgs, 11);
    let label = at_level(&label_types, level).unwrap_or(0);
    let mut number_surround: Option<NumberSurround> = None;
    let (marker_kind, number_kind, marker_text, marker_image) = match label {
        0 => (ListMarkerKind::None, None, None, None),
        1 => {
            let images = msgs.iter().map(|m| m.msgs(17)).find(|v| !v.is_empty()).unwrap_or_default();
            let image_ref = images
                .get(level as usize)
                .or_else(|| images.last())
                .and_then(|img| img.reference(3).or_else(|| img.reference(1)));
            let image = image_ref.map(|id| ctx.media_ref(id));
            (ListMarkerKind::Image, None, None, image)
        }
        2 => {
            let text: Vec<String> = msgs
                .iter()
                .map(|m| {
                    m.all(16)
                        .into_iter()
                        .filter_map(|v| match v {
                            iwadump::proto::Value::Bytes(b) => {
                                Some(String::from_utf8_lossy(b).into_owned())
                            }
                            _ => None,
                        })
                        .collect::<Vec<String>>()
                })
                .find(|v| !v.is_empty())
                .unwrap_or_default();
            let text = text
                .get(level as usize)
                .or_else(|| text.last())
                .cloned();
            (ListMarkerKind::String, None, text, None)
        }
        3 => {
            let numbers = varints(&msgs, 15);
            let kind = at_level(&numbers, level).unwrap_or(0);
            number_surround = number_surround_of(kind);
            let number_kind = match kind {
                0..=2 => Some(NumberKind::Decimal),
                3..=5 => Some(NumberKind::RomanUpper),
                6..=8 => Some(NumberKind::RomanLower),
                9..=11 => Some(NumberKind::AlphaUpper),
                12..=14 => Some(NumberKind::AlphaLower),
                _ => {
                    // ~50 locale-specific kinds: degrade per policy
                    // (docs/format/text.md §List styles), warn once per value.
                    ctx.warn_detail(
                        WarningCode::UnsupportedFeature,
                        format!("locale list numbering kind {kind} degraded to a generic marker"),
                        kind.to_string(),
                    );
                    Some(NumberKind::Other)
                }
            };
            (ListMarkerKind::Number, number_kind, None, None)
        }
        _ => (ListMarkerKind::None, None, None, None),
    };
    let indents: Vec<f32> = msgs
        .iter()
        .map(|m| m.packed_f32s(13))
        .find(|v| !v.is_empty())
        .unwrap_or_default();
    Some(ListFormat {
        number_surround,
        level,
        marker_kind,
        marker_text,
        number_kind,
        marker_image,
        start: None,
        marker_indent_pt: at_level(&indents, level).map(|v| v as f64),
    })
}

/// Number SURROUND from the same NumberType enum value: each scheme comes in
/// a triple ordered Decimal ("1."), DoubleParen ("(1)"), RightParen ("1)")
/// [proto: TSWPArchives.proto TSWP.ListStyleArchive.NumberType;
///  parser: numbers-parser bullets.py BULLET_PREFIXES/SUFFIXES]. The triples
/// run 0..=47 from base 0 and resume at 49 (Arabian) and 62 (Hebrew biblical
/// decimal); the two singleton kinds — circled (48) and Hebrew biblical
/// standard (61) — carry no punctuation. Period is the default → None (the
/// model omits it per omit-default).
fn number_surround_of(kind: u64) -> Option<NumberSurround> {
    let triple = |base: u64| match (kind - base) % 3 {
        1 => Some(NumberSurround::DoubleParen),
        2 => Some(NumberSurround::Paren),
        _ => None, // Decimal = period = the default
    };
    match kind {
        0..=47 => triple(0),
        48 | 61 => Some(NumberSurround::None),
        49..=60 => triple(49),
        62..=64 => triple(62),
        _ => None,
    }
}

/// TSWP.ColumnStyleArchive chain → SectionColumns. Column properties ride in
/// slot 11 like char properties (ColumnStylePropertiesArchive), with
/// `columns_null` (6) clearing and `columns` (7) carrying a ColumnsArchive:
/// equal_columns (1) { count = 1, gap = 2 } [proto: TSWPArchives.proto].
/// Non-equal columns degrade to their count with a warning (model contract).
/// The stored gap is a FRACTION of the printable width when <= 1 (fixture
/// 16b4195d: count=2, gap=0.05 on a 612pt page) [inferred]; multiply by
/// `content_width_pt` when available, else emit no gutter.
pub fn resolve_section_columns(
    ctx: &mut Ctx,
    style_id: u64,
    content_width_pt: Option<f64>,
) -> Option<SectionColumns> {
    let msgs = chain(ctx, style_id, 11);
    let cols = take(&msgs, 7, Some(6))?.msg(7)?;
    let (count, gap) = if let Some(eq) = cols.msg(1) {
        (eq.varint(1).unwrap_or(1) as u32, eq.f32v(2).map(|v| v as f64))
    } else if let Some(ne) = cols.msg(2) {
        // first (1) + following (2, repeated GapWidthArchive) — degrade.
        let count = 1 + ne.msgs(2).len() as u32;
        ctx.warn(
            WarningCode::UnsupportedFeature,
            format!("unequal-width columns degraded to {count} equal columns"),
        );
        (count, None)
    } else {
        return None;
    };
    if count < 2 {
        return None;
    }
    let gutter_pt = gap.map(|g| if g <= 1.0 { g * content_width_pt.unwrap_or(0.0) } else { g })
        .filter(|g| *g > 0.0);
    Some(SectionColumns { count, gutter_pt })
}

/// TSWP.DropCapStyleArchive → resolved DropCap [proto: TSWPArchives.proto:
/// drop_cap_properties (12) → DropCapStylePropertiesArchive.drop_cap (1) =
/// DropCapArchive { type=1, number_of_lines=2 (default 3),
/// number_of_raised_lines=3, number_of_characters=10 (default 1),
/// outdent=11, padding=12 (doubles, pt), character_scale=14 };
/// char_properties (11) carries the cap-glyph font overrides]. Shape/image
/// caps (type != text, or shape_enabled) degrade to the text rendering with
/// a warning (model policy).
pub fn resolve_drop_cap(ctx: &mut Ctx, id: u64) -> Option<DropCap> {
    let m = ctx.loaded.msg(id)?.clone();
    let props = m.msg(12)?;
    let dc = props.msg(1)?;
    if dc.varint(1).unwrap_or(0) != 0 || dc.varint(7) == Some(1) {
        ctx.warn(
            WarningCode::UnsupportedFeature,
            "shape/image drop cap degraded to plain text rendering".to_string(),
        );
    }
    let char_style = m.msg(11).map(|cp| {
        let msgs = vec![cp];
        char_style_from(ctx, &msgs)
    });
    Some(DropCap {
        lines: dc.varint(2).map(|v| v as u32),
        raised_lines: dc.varint(3).map(|v| v as u32).filter(|v| *v > 0),
        characters: dc.varint(10).map(|v| v as u32).filter(|v| *v > 1),
        character_scale: dc.f64v(14).filter(|v| *v != 1.0),
        outdent_pt: dc.f64v(11).filter(|v| *v != 0.0),
        padding_pt: dc.f64v(12).filter(|v| *v != 0.0),
        char_style: char_style.filter(|cs| *cs != CharStyle::default()),
    })
}

// ---------------------------------------------------------------------------
// CellStyle (TST)
// ---------------------------------------------------------------------------

/// Resolve a `TST.CellStyleArchive` chain → CellStyle. The chain walk reuses
/// field 11 (cell_properties occupies the same slot as char_properties on
/// TSWP style wrappers).
pub fn resolve_cell_style(ctx: &mut Ctx, id: u64) -> Option<TableCellStyle> {
    let msgs = chain(ctx, id, 11);
    let mut s = TableCellStyle::default();
    // First-wins merge, most-derived first.
    let mut fill_decided = false;
    for p in &msgs {
        if !fill_decided {
            if let Some(fm) = p.msg(1) {
                // A PRESENT cell_fill decides the property even when the
                // FillArchive is empty — empty means "fill: none" and must
                // not fall through to an ancestor's fill (01_Running_Log:
                // the header style overrides the preset's blue with none;
                // Apple renders white).
                s.fill = crate::tsd::fill_of(ctx, &fm);
                fill_decided = true;
            }
        }
        if s.text_wrap.is_none() {
            s.text_wrap = p.boolean(3);
        }
        if s.vertical_alignment.is_none() {
            // 0..3 order per docs/format/text.md §Alignment.
            s.vertical_alignment = p.varint(8).map(|v| match v {
                1 => VerticalAlignment::Middle,
                2 => VerticalAlignment::Bottom,
                3 => VerticalAlignment::Justify,
                _ => VerticalAlignment::Top,
            });
        }
        if s.padding.is_none() {
            s.padding = p.msg(9).map(|pad| PaddingInsets {
                left: pad.f32v(1).map(|v| v as f64),
                top: pad.f32v(2).map(|v| v as f64),
                right: pad.f32v(3).map(|v| v as f64),
                bottom: pad.f32v(4).map(|v| v as f64),
            });
        }
        if s.borders.is_none() {
            // Modern per-side strokes (fields 10-13); deprecated per-side
            // table strokes (4-7) share the shape but are legacy.
            let top = p.msg(10).and_then(|st| crate::tsd::stroke_of(ctx, &st));
            let right = p.msg(11).and_then(|st| crate::tsd::stroke_of(ctx, &st));
            let bottom = p.msg(12).and_then(|st| crate::tsd::stroke_of(ctx, &st));
            let left = p.msg(13).and_then(|st| crate::tsd::stroke_of(ctx, &st));
            if top.is_some() || right.is_some() || bottom.is_some() || left.is_some() {
                s.borders = Some(CellBorders { top, right, bottom, left });
            }
        }
    }
    Some(s)
}

/// Minimal list format for storage-driven list membership (gotchas #14):
/// marker kind comes from the list style's `label_types` at the level —
/// without theme resolution the marker TEXT is not recoverable, so string
/// bullets degrade to `marker_text` from `strings` when present.
pub fn resolve_list_format_minimal(ctx: &mut Ctx, list_id: u64, level: u32) -> ListFormat {
    let full = resolve_list_format(ctx, list_id, level)
        .unwrap_or(ListFormat {
        number_surround: None,
            level,
            marker_kind: ListMarkerKind::None,
            marker_text: None,
            number_kind: None,
            marker_image: None,
            start: None,
            marker_indent_pt: None,
        });
    ListFormat { level, ..full }
}

/// Resolve char properties from a ParagraphStyleArchive's `char_properties`
/// (field 11). These are the "style-driven text props" that apply to ALL
/// runs in paragraphs using this style (Peter's header/heading font issue).
pub fn resolve_para_char_style(ctx: &mut Ctx, style_id: u64) -> CharStyle {
    let Some(m) = ctx.loaded.msg(style_id) else { return CharStyle::default() };
    let Some(cp) = m.msg(11) else { return CharStyle::default() };
    let msgs = vec![cp];
    char_style_from(ctx, &msgs)
}
