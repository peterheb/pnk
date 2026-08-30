//! Drawable conversion (TSD): dispatch on the registry type id of each
//! drawable object and convert it to the model's `Drawable` union
//! (docs/format/drawables.md, docs/model-design.md §2.4). Anything not
//! modeled becomes `UnknownDrawable` + warning — never a silent drop.

use std::collections::HashSet;

use crate::ctx::Ctx;
use crate::model::*;
use crate::pb::{ids, Msg};

/// Convert the drawable object at `id`. `depth` guards against runaway
/// recursion in malformed group graphs.
pub fn convert_drawable(ctx: &mut Ctx, id: u64) -> Drawable {
    convert_drawable_depth(ctx, id, 0, &mut HashSet::new())
}

pub fn convert_drawable_depth(
    ctx: &mut Ctx,
    id: u64,
    depth: usize,
    visiting: &mut HashSet<u64>,
) -> Drawable {
    if depth > 24 || !visiting.insert(id) {
        return unknown_drawable(
            ctx,
            id,
            "drawable graph nesting too deep or cyclic",
        );
    }
    let result = convert_drawable_inner(ctx, id, depth, visiting);
    visiting.remove(&id);
    result
}

fn unknown_drawable(ctx: &mut Ctx, id: u64, reason: impl Into<String>) -> Drawable {
    let rec = ctx.loaded.record(id);
    Drawable::Unknown {
        common: None,
        type_id: rec.map(|r| format!("0x{:x}", r.type_id)).unwrap_or_else(|| "0x0".into()),
        type_name: rec.and_then(|r| r.name.clone()),
        reason: reason.into(),
    }
}

fn convert_drawable_inner(
    ctx: &mut Ctx,
    id: u64,
    depth: usize,
    visiting: &mut HashSet<u64>,
) -> Drawable {
    let Some(rec) = ctx.loaded.record(id).cloned() else {
        ctx.warn_detail(
            WarningCode::UnresolvedReference,
            format!("drawable reference {id} points nowhere"),
            id.to_string(),
        );
        return Drawable::Unknown {
            common: None,
            type_id: format!("ref:{id}"),
            type_name: None,
            reason: "unresolved reference".into(),
        };
    };
    let Some(msg) = rec.msg.clone() else {
        return Drawable::Unknown {
            common: None,
            type_id: format!("0x{:x}", rec.type_id),
            type_name: rec.name.clone(),
            reason: "payload not decodable".into(),
        };
    };

    match rec.type_id {
        // TSWP.ShapeInfoArchive / KN.PlaceholderArchive / TP.PlaceholderArchive
        2011 | 7 | 12 => shape_info_drawable(ctx, &msg, id, rec.type_id, rec.name.clone()),
        // TSD.ShapeArchive
        ids::SHAPE => shape_drawable(ctx, &msg, None, None),
        ids::IMAGE => image_drawable(ctx, &msg),
        ids::MOVIE => movie_drawable(ctx, &msg),
        ids::GROUP => group_drawable(ctx, &msg, depth, visiting),
        ids::CONNECTION_LINE => connection_line_drawable(ctx, &msg),
        // TST.TableInfoArchive / TST.WPTableInfoArchive
        6000 | 6007 => table_drawable(ctx, &msg),
        ids::CHART_DRAWABLE => chart_drawable(ctx, &msg),
        // TSD.MaskArchive as a top-level drawable: unexpected, but keep it.
        ids::MASK => mask_drawable(ctx, &msg),
        _ => {
            let type_name = rec.name.clone();
            ctx.warn_detail(
                WarningCode::UnsupportedFeature,
                format!(
                    "drawable of type {} could not be modeled",
                    type_name.clone().unwrap_or_else(|| format!("type id {}", rec.type_id))
                ),
                format!("0x{:x}", rec.type_id),
            );
            Drawable::Unknown {
                common: drawable_common(ctx, &msg).ok(),
                type_id: format!("0x{:x}", rec.type_id),
                type_name,
                reason: "recognized object type has no drawable model".into(),
            }
        }
    }
}

/// Extract `DrawableCommon` from a `TSD.DrawableArchive` payload. `Err(())`
/// when the payload carries no geometry at all.
fn drawable_common(ctx: &mut Ctx, m: &Msg) -> Result<DrawableCommon, ()> {
    let mut c = DrawableCommon::default();
    let mut any = false;
    if let Some(g) = m.msg(1) {
        if let Some((x, y)) = g.point(1) {
            c.position = Some(Point { x, y });
            any = true;
        }
        if let Some((w, h)) = g.size(2) {
            c.size = Some(Size { width: w, height: h });
            any = true;
        }
        if let Some(deg) = g.f32v(4) {
            // Geometry angle is DEGREES, not radians — fixture-verified:
            // 24_Briefing master ticks store 90.0 and Apple's own PDF export
            // renders them vertical (a radians read would give 5156.6°≡117°,
            // the diagonal we used to draw). docs/format/drawables.md updated.
            c.angle_deg = Some(deg as f64);
            any = true;
        }
    }
    if let Some(w) = m.msg(3) {
        let kind = match w.varint(1).unwrap_or(0) {
            0 => TextWrapKind::None,
            1 => TextWrapKind::Around,
            2 => TextWrapKind::AboveBelow,
            3 => TextWrapKind::Left,
            4 => TextWrapKind::Right,
            5 => TextWrapKind::Largest,
            _ => TextWrapKind::Around,
        };
        c.text_wrap = Some(TextWrap { kind, margin_pt: w.f32v(4).map(|v| v as f64) });
        any = true;
    }
    if let Some(h) = m.string(4) {
        if !h.is_empty() {
            c.hyperlink = Some(h);
        }
        any = any || m.has(4);
    }
    if let Some(l) = m.boolean(5) {
        c.locked = Some(l);
        any = true;
    }
    if let Some(a) = m.string(8) {
        if !a.is_empty() {
            c.accessibility_description = Some(a);
        }
        any = any || m.has(8);
    }
    if any {
        Ok(c)
    } else {
        Err(())
    }
}

/// Style from TSD.ShapeStyleArchive (shape_properties = 11) or
/// TSD.MediaStyleArchive (media_properties = 11).
fn drawable_style(ctx: &mut Ctx, style_ref: Option<u64>, is_media: bool) -> (Option<DrawableStyle>, Option<DrawableCommon>) {
    let mut style = DrawableStyle::default();
    let mut extras = DrawableCommon::default();
    let mut any = false;
    if let Some(sid) = style_ref {
        if let Some(sm) = ctx.loaded.msg(sid).cloned() {
            // TSWP.ShapeStyleArchive (text shapes) wraps the TSD.ShapeStyleArchive
            // as `super` (1); the fill/stroke/opacity properties live on the TSD
            // level's field 11, while the wrapper's own field 11 holds text
            // layout properties. Prefer the super's properties when present.
            let tsd_super = sm.msg(1).filter(|s| s.has(11));
            let props_msg = tsd_super.as_ref().unwrap_or(&sm);
            // shape_properties / media_properties = 11
            if let Some(props) = props_msg.msg(11) {
                if !is_media {
                    style.fill = props.msg(1).and_then(|f| crate::tsd::fill_of(ctx, &f));
                    style.stroke = props.msg(2).and_then(|st| crate::tsd::stroke_of(ctx, &st));
                    let head = props.msg(6).and_then(|le| crate::tsd::line_end_of(ctx, &le));
                    let tail = props.msg(7).and_then(|le| crate::tsd::line_end_of(ctx, &le));
                    if head.is_some() || tail.is_some() {
                        style.line_ends = Some(LineEnds { head, tail });
                    }
                    extras.opacity = props.f32v(3).map(|v| v as f64);
                } else {
                    style.stroke = props.msg(1).and_then(|st| crate::tsd::stroke_of(ctx, &st));
                    extras.opacity = props.f32v(2).map(|v| v as f64);
                }
                extras.shadow = props.msg(if is_media { 3 } else { 4 })
                    .and_then(|sh| crate::tsd::shadow_of(ctx, &sh));
                extras.reflection = props
                    .msg(if is_media { 4 } else { 5 })
                    .and_then(|r| crate::tsd::reflection_of(&r));
                any = style.fill.is_some()
                    || style.stroke.is_some()
                    || style.line_ends.is_some()
                    || extras.opacity.is_some()
                    || extras.shadow.is_some()
                    || extras.reflection.is_some();
            }
        } else {
            ctx.warn_detail(
                WarningCode::UnresolvedReference,
                format!("style reference {sid} points nowhere"),
                sid.to_string(),
            );
        }
    }
    (if any { Some(style) } else { None }, if any { Some(extras) } else { None })
}

/// Merge extra style-derived fields into the common block.
fn merge_extras(common: &mut DrawableCommon, extras: Option<DrawableCommon>) {
    if let Some(e) = extras {
        if common.opacity.is_none() {
            common.opacity = e.opacity;
        }
        if common.shadow.is_none() {
            common.shadow = e.shadow;
        }
        if common.reflection.is_none() {
            common.reflection = e.reflection;
        }
    }
}

/// `TSWP.ShapeInfoArchive` { super = 1 (TSD.ShapeArchive), text_flow = 3,
/// owned_storage = 4, is_text_box = 6 }.
///
/// KN.PlaceholderArchive / TP.PlaceholderArchive wrap one more level:
/// `{ super = 1 (ShapeInfoArchive), kind = 2 }` — so for those types the
/// ShapeInfoArchive lives at `m.msg(1)` and only the Kind is read off `m`.
fn shape_info_drawable(
    ctx: &mut Ctx,
    m: &Msg,
    id: u64,
    type_id: u32,
    type_name: Option<String>,
) -> Drawable {
    // Placeholder types: unwrap `{ super, kind }` → the ShapeInfoArchive.
    let super_info =
        if type_id == 7 || type_id == 12 { m.msg(1) } else { None };
    let info = super_info.as_ref().unwrap_or(m);
    let kind = if type_id == 7 || type_id == 12 { m.varint(2) } else { None };
    let Some(shape) = info.msg(1) else {
        return Drawable::Unknown {
            common: None,
            type_id: format!("0x{type_id:x}"),
            type_name,
            reason: "ShapeInfoArchive without a ShapeArchive super".into(),
        };
    };
    let placeholder_role = match type_id {
        // KN.PlaceholderArchive.Kind (KNArchives.proto:203-209)
        7 | 12 if type_name.as_deref().map(|n| n.starts_with("KN.")).unwrap_or(false) => {
            Some(
                match kind.unwrap_or(0) {
                    1 => "slide-number",
                    2 => "title",
                    3 => "body",
                    4 => "object",
                    _ => "placeholder",
                }
                .to_string(),
            )
        }
        7 | 12 => Some("placeholder".to_string()), // TP.PlaceholderArchive
        _ => None,
    };

    // Text: owned_storage (4) wins, else text_flow (3) → FlowInfo.text_storage (1),
    // else deprecated_storage (2) — older docs (pre-flow) reference the
    // StorageArchive there directly [proto: TSWPArchives.proto ShapeInfoArchive
    // field 2, deprecated=true; fixture 5008407355… stores its template text so].
    let storage_id = info
        .reference(4)
        .or_else(|| info.msg(3).and_then(|f| f.reference(1)))
        .or_else(|| info.reference(2));
    let mut text = storage_id.and_then(|sid| crate::text::extract(ctx, sid)).map(|e| e.text);
    // Keynote placeholders and title/body shapes keep their look on the
    // referenced paragraph style (their storage char-style tables hold null
    // overrides), so runs without their own character style inherit the
    // paragraph style's font name/size/color. Keynote-scoped: Pages storages
    // carry real character styles and are golden-pinned.
    if ctx.app_kind == crate::model::AppKind::Keynote {
        if let (Some(sid), Some(t)) = (storage_id, text.as_mut()) {
            promote_para_font(ctx, sid, t);
        }
    }
    let is_text_box = info.boolean(6).unwrap_or(false);

    let mut drawable = if is_text_box || type_id == 7 || type_id == 12 {
        // Textbox (or placeholder, which renders like a textbox).
        let mut common = common_from_shape(ctx, &shape);
        if let Some(role) = placeholder_role {
            common.placeholder = Some(PlaceholderInfo { role, inherited: None });
        }
        Drawable::Textbox {
            common,
            text: text.unwrap_or_default(),
            vertical_alignment: shape_vertical_alignment(ctx, &shape),
            text_insets: None,
        }
    } else {
        let mut d = shape_drawable(ctx, &shape, text, shape_vertical_alignment(ctx, &shape));
        if let Some(role) = placeholder_role {
            if let Drawable::Shape { common, .. } = &mut d {
                common.placeholder = Some(PlaceholderInfo { role, inherited: None });
            }
        }
        d
    };

    // Distinguish a bare empty textbox: if the shape has no pathsource and no
    // style at all it is still a textbox; nothing more to do here.
    let _ = id;
    drawable
}

fn common_from_shape(ctx: &mut Ctx, shape: &Msg) -> DrawableCommon {
    // shape: super = 1 (TSD.DrawableArchive), style = 2, pathsource = 3
    let mut common = shape
        .msg(1)
        .and_then(|d| drawable_common(ctx, &d).ok())
        .unwrap_or_default();
    let (style, extras) = drawable_style(ctx, shape.reference(2), false);
    common.style = style;
    merge_extras(&mut common, extras);
    common
}

/// Rewrite runs that carry no character style of their own so they inherit
/// the referenced paragraph style's font attributes (Keynote placeholder
/// storages hold their look on the paragraph style, with null char-table
/// entries). Styled runs are left untouched.
fn promote_para_font(ctx: &mut Ctx, storage_id: u64, text: &mut StyledText) {
    // Paragraph-style refs in storage order (table_para_style = 5).
    let para_styles: Vec<u64> = {
        let storage = ctx.loaded.msg(storage_id).cloned();
        let Some(storage) = storage else { return };
        storage
            .msgs(5)
            .iter()
            .flat_map(|t| t.msgs(1))
            .filter_map(|e| e.reference(2))
            .collect()
    };
    if para_styles.is_empty() {
        return;
    }
    for (pi, para) in text.paragraphs.iter_mut().enumerate() {
        // One style per paragraph; extra paragraphs reuse the last entry.
        let sid = para_styles[pi.min(para_styles.len() - 1)];
        let cs = crate::styles::char_style_from(ctx, &crate::styles::chain(ctx, sid, 11));
        let Some(idx) = ctx.char_pool.intern(cs) else { continue };
        for item in para.items.iter_mut() {
            if let ParagraphItem::Plain(s) = item {
                *item = ParagraphItem::Text {
                    text: std::mem::take(s),
                    c_style: Some(idx),
                    hyperlink: None,
                    language: None,
                };
            }
        }
    }
}

/// TSWP.ShapeStylePropertiesArchive.vertical_alignment (field 2, enum top=0/
/// middle=1/bottom=2/justify=3 — TSWPArchives.proto:495-513) read off the
/// text shape's TSWP.ShapeStyleArchive OWN `shape_properties` (11); the TSD
/// super's field 11 is fill/stroke and must not be misread (its field 2 is a
/// stroke). Theme presets keep the alignment on ancestor styles, so walk the
/// TSS.StyleArchive parent chain (field 3). Fixture: Home.key's title
/// placeholder is bottom-aligned in Apple's export.
fn shape_vertical_alignment(ctx: &Ctx, shape: &Msg) -> Option<VerticalAlignment> {
    let mut sid = shape.reference(2)?;
    for _ in 0..16 {
        let m = ctx.loaded.msg(sid)?;
        let is_tswp = ctx
            .loaded
            .record(sid)
            .and_then(|r| r.name.as_deref())
            .map(|n| n.starts_with("TSWP."))
            .unwrap_or(false);
        if is_tswp {
            if let Some(v) = m.msg(11).and_then(|p| p.varint(2)) {
                return Some(match v {
                    1 => VerticalAlignment::Middle,
                    2 => VerticalAlignment::Bottom,
                    3 => VerticalAlignment::Justify,
                    _ => VerticalAlignment::Top,
                });
            }
        }
        // TSS.StyleArchive.parent (3): the TSWP wrapper nests supers twice
        // (TSWP → TSD → TSS), a plain TSD style once.
        let tss = if is_tswp { m.msg(1).and_then(|t| t.msg(1)) } else { m.msg(1) };
        match tss.and_then(|t| t.reference(3)) {
            Some(p) => sid = p,
            None => return None,
        }
    }
    None
}

fn shape_drawable(
    ctx: &mut Ctx,
    shape: &Msg,
    text: Option<StyledText>,
    v_align: Option<VerticalAlignment>,
) -> Drawable {
    let common = common_from_shape(ctx, shape);
    let geometry = shape
        .msg(3)
        .map(|ps| crate::tsd::shape_geometry(&ps))
        .unwrap_or(ShapeGeometry {
            preset: None,
            scalar: None,
            natural_size: None,
            path: None,
            callout: None,
        });
    Drawable::Shape {
        common,
        geometry,
        text,
        vertical_alignment: v_align,
        text_insets: None,
    }
}

fn image_drawable(ctx: &mut Ctx, m: &Msg) -> Drawable {
    let mut common = m
        .msg(1)
        .and_then(|d| drawable_common(ctx, &d).ok())
        .unwrap_or_default();
    let (style, extras) = drawable_style(ctx, m.reference(3), true);
    common.style = style;
    merge_extras(&mut common, extras);

    // Display data pick (agent P): prefer the primary data, but when its
    // bytes are absent from the package fall back to a materialized
    // alternative — template packages ship only the `-small` preview
    // (00C Textbook: cover references the full-size jpg that is not in
    // Data/, only its -small sibling is).
    let main_id = m.reference(11).or_else(|| m.reference(2));
    let thumb_id = m.reference(12).or_else(|| m.reference(6));
    let orig_id = m.reference(13).or_else(|| m.reference(8));
    let display_id = [main_id, thumb_id, orig_id]
        .into_iter()
        .flatten()
        .find(|id| ctx.data_available(*id))
        .or(main_id);
    let image = display_id.map(|id| ctx.media_ref(id));
    let image = match image {
        Some(r) => r,
        None => {
            ctx.warn(
                WarningCode::UnresolvedReference,
                "image drawable without a resolvable data reference".to_string(),
            );
            MediaRef {
                data_id: "0".into(),
                file_name: None,
                preferred_file_name: None,
                pixel_size: None,
            }
        }
    };
    let original = m.reference(13).or_else(|| m.reference(8)).map(|id| ctx.media_ref(id));
    let thumbnail = m.reference(12).or_else(|| m.reference(6)).map(|id| ctx.media_ref(id));
    let svg = m.reference(23).map(|id| ctx.media_ref(id));
    let natural_size = m.size(9).map(|(w, h)| Size { width: w, height: h });
    let mask = m.reference(5).and_then(|mid| {
        let mm = ctx.loaded.msg(mid)?.clone();
        let common = mm
            .msg(1)
            .and_then(|d| drawable_common(ctx, &d).ok())
            .unwrap_or_default();
        let geometry = mm
            .msg(2)
            .map(|ps| crate::tsd::shape_geometry(&ps))
            .unwrap_or(ShapeGeometry {
                preset: None,
                scalar: None,
                natural_size: None,
                path: None,
                callout: None,
            });
        Some(ImageMask { geometry, common })
    });
    let adjustments = m.msg(14).map(|adj| ImageAdjustments {
        exposure: adj.f32v(1).map(|v| v as f64),
        saturation: adj.f32v(2).map(|v| v as f64),
        contrast: adj.f32v(3).map(|v| v as f64),
        highlights: adj.f32v(4).map(|v| v as f64),
        shadows: adj.f32v(5).map(|v| v as f64),
        brightness: None,
    });
    Drawable::Image { common, image, original, thumbnail, svg, natural_size, mask, adjustments }
}

fn movie_drawable(ctx: &mut Ctx, m: &Msg) -> Drawable {
    let mut common = m
        .msg(1)
        .and_then(|d| drawable_common(ctx, &d).ok())
        .unwrap_or_default();
    let (style, extras) = drawable_style(ctx, m.reference(19), true);
    common.style = style;
    merge_extras(&mut common, extras);

    let movie = m.reference(14).or_else(|| m.reference(2)).map(|id| ctx.media_ref(id));
    let poster = m.reference(15).or_else(|| m.reference(10)).map(|id| ctx.media_ref(id));
    let remote_url = m.string(17);
    let audio_only = m.boolean(9);
    let trim = if m.has(3) || m.has(4) || m.has(5) {
        Some(MovieTrim {
            start: m.f32v(3).map(|v| v as f64),
            end: m.f32v(4).map(|v| v as f64),
            poster_time: m.f32v(5).map(|v| v as f64),
        })
    } else {
        None
    };
    let r#loop = m.varint(24).or_else(|| m.varint(6)).map(|v| match v {
        1 => MovieLoop::Repeat,
        2 => MovieLoop::BackAndForth,
        _ => MovieLoop::None,
    });
    let volume = m.f32v(7).map(|v| v as f64);
    Drawable::Movie { common, movie, remote_url, poster, audio_only, trim, r#loop, volume }
}

fn group_drawable(
    ctx: &mut Ctx,
    m: &Msg,
    depth: usize,
    visiting: &mut HashSet<u64>,
) -> Drawable {
    let mut common = m
        .msg(1)
        .and_then(|d| drawable_common(ctx, &d).ok())
        .unwrap_or_default();
    let (style, extras) = drawable_style(ctx, None, false);
    common.style = style;
    merge_extras(&mut common, extras);

    // Freehand drawing metadata (ext field 100 on GroupArchive).
    let freehand = m.msg(100).map(|f| FreehandInfo {
        opacity: f.f64v(2),
        animation: f.msg(3).map(|a| FreehandAnimation {
            duration: a.f64v(1),
            r#loop: a.boolean(2),
        }),
    });

    let group_pos = common.position;
    let children: Vec<Drawable> = m
        .references(2)
        .into_iter()
        .map(|cid| {
            let mut d = convert_drawable_depth(ctx, cid, depth + 1, visiting);
            // Re-base child coordinates into group-local space (§3.4).
            if let Some(gp) = group_pos {
                rebase_child(&mut d, gp);
            }
            d
        })
        .collect();
    Drawable::Group { common, children, freehand }
}

fn connection_line_drawable(ctx: &mut Ctx, m: &Msg) -> Drawable {
    // ConnectionLineArchive { super = 1 (ShapeArchive), connected_from = 2,
    // connected_to = 3 } (drawables.md).
    let common = match m.msg(1) {
        Some(shape) => common_from_shape(ctx, &shape),
        None => DrawableCommon::default(),
    };
    // Routing path: super(1).pathsource(3).connection_line_path_source(7)
    //   .super(1).path(3)
    let path = m
        .msg(1)
        .and_then(|shape| shape.msg(3))
        .and_then(|ps| ps.msg(7))
        .and_then(|cl| cl.msg(1))
        .and_then(|bz| bz.msg(3))
        .as_ref()
        .and_then(crate::tsd::tsp_path)
        .unwrap_or(CurvePath { elements: Vec::new() });
    let anchor = |aid: Option<u64>| -> Option<AnchorFacts> {
        aid.and_then(|aid| ctx.loaded.msg(aid))
            .and_then(|am| am.msg(1)) // DrawableArchive
            .and_then(|d| d.msg(1)) // GeometryArchive
            .map(|g| AnchorFacts {
                position: g.point(1).map(|(x, y)| Point { x, y }),
                size: g.size(2).map(|(w, h)| Size { width: w, height: h }),
            })
    };
    let from = anchor(m.reference(2));
    let to = anchor(m.reference(3));
    Drawable::ConnectionLine { common, path, from, to }
}

fn table_drawable(ctx: &mut Ctx, m: &Msg) -> Drawable {
    let mut common = m
        .msg(1)
        .and_then(|d| drawable_common(ctx, &d).ok())
        .unwrap_or_default();
    merge_extras(&mut common, None);
    let table = match m.reference(2) {
        Some(mid) => crate::tables::convert_table(ctx, mid),
        None => {
            ctx.warn(
                WarningCode::UnresolvedReference,
                "table drawable without a tableModel reference".to_string(),
            );
            empty_table()
        }
    };
    Drawable::Table { common, table }
}

fn empty_table() -> TableModel {
    TableModel {
        name: None,
        row_count: 0,
        column_count: 0,
        header_row_count: 0,
        header_column_count: 0,
        footer_row_count: 0,
        header_rows_frozen: None,
        header_columns_frozen: None,
        rows: None,
        columns: None,
        default_row_height_pt: None,
        default_column_width_pt: None,
        grid: Vec::new(),
        formats: Vec::new(),
        cell_styles: Vec::new(),
        merges: Vec::new(),
        style: None,
    }
}

fn chart_drawable(ctx: &mut Ctx, m: &Msg) -> Drawable {
    let common = m
        .msg(1)
        .and_then(|d| drawable_common(ctx, &d).ok())
        .unwrap_or_default();
    let chart = m
        .msg(10000) // TSCH unity extension (charts.md)
        .map(|ca| crate::charts::convert_chart(ctx, &ca))
        .unwrap_or_else(|| {
            ctx.warn(
                WarningCode::UnsupportedFeature,
                "chart drawable without a decodable ChartArchive payload".to_string(),
            );
            ChartModel {
                r#type: ChartType::Other,
                three_d: false,
                data_status: ChartDataStatus::Unavailable,
                categories: Vec::new(),
                series: Vec::new(),
                legend_frame: None,
                legend_visible: None,
                series_colors: None,
                data_binding: None,
                scatter_format: None,
            }
        });
    Drawable::Chart { common, chart }
}

fn mask_drawable(ctx: &mut Ctx, m: &Msg) -> Drawable {
    let common = m
        .msg(1)
        .and_then(|d| drawable_common(ctx, &d).ok())
        .unwrap_or_default();
    let geometry = m
        .msg(2)
        .map(|ps| crate::tsd::shape_geometry(&ps))
        .unwrap_or(ShapeGeometry {
            preset: None,
            scalar: None,
            natural_size: None,
            path: None,
            callout: None,
        });
    // A bare mask at top level: represent as a shape carrying the mask path.
    Drawable::Shape { common, geometry, text: None, vertical_alignment: None, text_insets: None }
}

/// Re-base one child drawable's position into group-local coordinates.
fn rebase_child(d: &mut Drawable, gp: Point) {
    let common = match d {
        Drawable::Shape { common, .. }
        | Drawable::Textbox { common, .. }
        | Drawable::Image { common, .. }
        | Drawable::Movie { common, .. }
        | Drawable::Group { common, .. }
        | Drawable::ConnectionLine { common, .. }
        | Drawable::Table { common, .. }
        | Drawable::Chart { common, .. } => common,
        _ => return,
    };
    if let Some(cp) = common.position {
        common.position = Some(Point { x: cp.x - gp.x, y: cp.y - gp.y });
    }
}
