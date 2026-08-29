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
pub fn resolve_list_format(ctx: &mut Ctx, list_id: u64, level: u32) -> Option<ListFormat> {
    let m = ctx.loaded.msg(list_id)?.clone();
    let label_types: Vec<u64> = m
        .all(11)
        .into_iter()
        .filter_map(|v| match v {
            iwadump::proto::Value::Varint(v) => Some(*v),
            _ => None,
        })
        .collect();
    let label = label_types.get(level as usize).copied().unwrap_or(0);
    let (marker_kind, number_kind, marker_text, marker_image) = match label {
        0 => (ListMarkerKind::None, None, None, None),
        1 => {
            let image_ref = m
                .msgs(17)
                .get(level as usize)
                .and_then(|img| img.reference(3).or_else(|| img.reference(1)));
            let image = image_ref.map(|id| ctx.media_ref(id));
            (ListMarkerKind::Image, None, None, image)
        }
        2 => {
            let text: Vec<String> = m
                .all(16)
                .into_iter()
                .filter_map(|v| match v {
                    iwadump::proto::Value::Bytes(b) => {
                        Some(String::from_utf8_lossy(b).into_owned())
                    }
                    _ => None,
                })
                .collect();
            let text = text.get(level as usize).cloned();
            (ListMarkerKind::String, None, text, None)
        }
        3 => {
            let numbers: Vec<u64> = m
                .all(15)
                .into_iter()
                .filter_map(|v| match v {
                    iwadump::proto::Value::Varint(v) => Some(*v),
                    _ => None,
                })
                .collect();
            let kind = numbers.get(level as usize).copied().unwrap_or(0);
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
    let indents: Vec<f32> = m.packed_f32s(13);
    Some(ListFormat {
        level,
        marker_kind,
        marker_text,
        number_kind,
        marker_image,
        start: None,
        marker_indent_pt: indents.get(level as usize).map(|v| *v as f64),
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
    for p in &msgs {
        if s.fill.is_none() {
            s.fill = p.msg(1).and_then(|f| crate::tsd::fill_of(ctx, &f));
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
