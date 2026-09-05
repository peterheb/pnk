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
        return unknown_drawable(ctx, id, "drawable graph nesting too deep or cyclic");
    }
    let result = convert_drawable_inner(ctx, id, depth, visiting);
    visiting.remove(&id);
    result
}

fn unknown_drawable(ctx: &mut Ctx, id: u64, reason: impl Into<String>) -> Drawable {
    let rec = ctx.loaded.record(id);
    Drawable::Unknown {
        common: None,
        type_id: rec
            .map(|r| format!("0x{:x}", r.type_id))
            .unwrap_or_else(|| "0x0".into()),
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
        // TSWP.TOCInfoArchive { super = 1: ShapeInfoArchive }: the laid-out
        // table of contents is a text box (Pages agent, 2026-09-05).
        2240 => match msg.msg(1) {
            Some(sup) => shape_info_drawable(ctx, &sup, id, 2011, rec.name.clone()),
            None => Drawable::Unknown {
                common: None,
                type_id: "0x8c0".into(),
                type_name: rec.name.clone(),
                reason: "TOCInfoArchive without a ShapeInfoArchive super".into(),
            },
        },
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
                    type_name
                        .clone()
                        .unwrap_or_else(|| format!("type id {}", rec.type_id))
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
fn drawable_common(_ctx: &mut Ctx, m: &Msg) -> Result<DrawableCommon, ()> {
    let mut c = DrawableCommon::default();
    let mut any = false;
    if let Some(g) = m.msg(1) {
        if let Some((x, y)) = g.point(1) {
            c.position = Some(Point { x, y });
            any = true;
        }
        if let Some((w, h)) = g.size(2) {
            c.size = Some(Size {
                width: w,
                height: h,
            });
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
        // TSD.ExteriorTextWrapArchive: field 1 is the wrap TYPE, field 2 the
        // direction. P's survey of 323 Pages documents found direction = 2 on
        // all but six files while `type` carries every variation ((5,2) 2501
        // objects, (4,2) 963, (1,2) 155, (2,2) 49), so `type` is the field to
        // read. The 0/1/2 assignment is fixture-verified; 3 = left, 4 = right,
        // 5 = largest is [inferred] — otorp strips the enum names, and no
        // Keynote fixture in the corpus flows text around a drawable at all,
        // so K has no export that can tell left from right. A Pages fixture
        // with a visibly left-wrapped figure would settle it.
        let kind = match w.varint(1).unwrap_or(0) {
            0 => TextWrapKind::None,
            1 => TextWrapKind::Around,
            2 => TextWrapKind::AboveBelow,
            3 => TextWrapKind::Left,
            4 => TextWrapKind::Right,
            5 => TextWrapKind::Largest,
            _ => TextWrapKind::Around,
        };
        c.text_wrap = Some(TextWrap {
            kind,
            margin_pt: w.f32v(4).map(|v| v as f64),
        });
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
/// TSD.MediaStyleArchive (media_properties = 11), resolved through the
/// TSS.StyleArchive `parent = 3` chain (docs/format/styles.md): a shape
/// inserted with the theme's default look stores an EMPTY local property
/// bag — its fill/stroke live in the parent preset (fixture:
/// G2-golden-pages-layout.pages pentagon/oval/parallelogram, whose style
/// carries only shadow+reflection locally, fill+stroke on parent
/// "shape-2-shapestyle"). Field PRESENCE at a nearer level wins even when
/// the decoder yields None (present-but-empty = "explicitly cleared",
/// stopping inheritance).
fn drawable_style(
    ctx: &mut Ctx,
    style_ref: Option<u64>,
    is_media: bool,
) -> (Option<DrawableStyle>, Option<DrawableCommon>) {
    let mut style = DrawableStyle::default();
    let mut extras = DrawableCommon::default();
    // Per-property "seen at a nearer chain level" flags:
    // fill, stroke, opacity, shadow, reflection, head, tail
    let mut seen = [false; 7];
    let mut head: Option<LineEnd> = None;
    let mut tail: Option<LineEnd> = None;
    let mut cur = style_ref;
    let mut visited: HashSet<u64> = HashSet::new();
    while let Some(sid) = cur {
        if !visited.insert(sid) || visited.len() > 16 {
            break; // cycle / runaway chain guard
        }
        let Some(sm) = ctx.loaded.msg(sid).cloned() else {
            ctx.warn_detail(
                WarningCode::UnresolvedReference,
                format!("style reference {sid} points nowhere"),
                sid.to_string(),
            );
            break;
        };
        // TSWP.ShapeStyleArchive (text shapes) wraps the TSD.ShapeStyleArchive
        // as `super` (1); the fill/stroke/opacity properties live on the TSD
        // level's field 11, while the wrapper's own field 11 holds text
        // layout properties. Prefer the super's properties when present.
        let tsd_super = sm.msg(1).filter(|s| s.has(11));
        let props_msg = tsd_super.as_ref().unwrap_or(&sm);
        // Next hop: TSS.StyleArchive super (1) → parent (3).
        cur = props_msg.msg(1).and_then(|hdr| hdr.reference(3));
        // shape_properties / media_properties = 11
        let Some(props) = props_msg.msg(11) else {
            continue;
        };
        if !is_media {
            if !seen[0] && props.has(1) {
                seen[0] = true;
                style.fill = props.msg(1).and_then(|f| crate::tsd::fill_of(ctx, &f));
            }
            if !seen[1] && props.has(2) {
                seen[1] = true;
                style.stroke = props.msg(2).and_then(|st| crate::tsd::stroke_of(ctx, &st));
            }
            if !seen[2] && props.has(3) {
                seen[2] = true;
                extras.opacity = props.f32v(3).map(|v| v as f64);
            }
            if !seen[5] && props.has(6) {
                seen[5] = true;
                head = props
                    .msg(6)
                    .and_then(|le| crate::tsd::line_end_of(ctx, &le));
            }
            if !seen[6] && props.has(7) {
                seen[6] = true;
                tail = props
                    .msg(7)
                    .and_then(|le| crate::tsd::line_end_of(ctx, &le));
            }
        } else {
            if !seen[1] && props.has(1) {
                seen[1] = true;
                style.stroke = props.msg(1).and_then(|st| crate::tsd::stroke_of(ctx, &st));
            }
            if !seen[2] && props.has(2) {
                seen[2] = true;
                extras.opacity = props.f32v(2).map(|v| v as f64);
            }
        }
        let shadow_field = if is_media { 3 } else { 4 };
        if !seen[3] && props.has(shadow_field) {
            seen[3] = true;
            extras.shadow = props
                .msg(shadow_field)
                .and_then(|sh| crate::tsd::shadow_of(ctx, &sh));
        }
        let refl_field = if is_media { 4 } else { 5 };
        if !seen[4] && props.has(refl_field) {
            seen[4] = true;
            extras.reflection = props
                .msg(refl_field)
                .and_then(|r| crate::tsd::reflection_of(&r));
        }
    }
    if head.is_some() || tail.is_some() {
        style.line_ends = Some(LineEnds { head, tail });
    }
    let any = style.fill.is_some()
        || style.stroke.is_some()
        || style.line_ends.is_some()
        || extras.opacity.is_some()
        || extras.shadow.is_some()
        || extras.reflection.is_some();
    (
        if any { Some(style) } else { None },
        if any { Some(extras) } else { None },
    )
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
    let super_info = if type_id == 7 || type_id == 12 {
        m.msg(1)
    } else {
        None
    };
    let info = super_info.as_ref().unwrap_or(m);
    let kind = if type_id == 7 || type_id == 12 {
        m.varint(2)
    } else {
        None
    };
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
        7 | 12
            if type_name
                .as_deref()
                .map(|n| n.starts_with("KN."))
                .unwrap_or(false) =>
        {
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
    let mut text = storage_id
        .and_then(|sid| crate::text::extract(ctx, sid))
        .map(|e| e.text);
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
    let frame = shape_text_frame_props(ctx, &shape);
    // Text-fit semantics: "shrink text on overflow" (resolved flag) scales
    // text down to the stored box; a plain text box (is_text_box, not a
    // placeholder) auto-grows its height as content wraps — Keynote stores
    // the height laid out with Apple's font metrics, so renderers with
    // different metrics must treat it as a minimum, not a clip [inferred:
    // Keynote app behavior; placeholders keep layout-fixed frames].
    let text_fit = if frame.shrink_to_fit == Some(true) {
        Some(TextFit::Shrink)
    } else if is_text_box && placeholder_role.is_none() {
        Some(TextFit::Grow)
    } else {
        None
    };

    let mut drawable = if is_text_box || type_id == 7 || type_id == 12 {
        // Textbox (or placeholder, which renders like a textbox).
        let mut common = common_from_shape(ctx, &shape);
        if let Some(role) = placeholder_role {
            common.placeholder = Some(PlaceholderInfo {
                role,
                inherited: None,
            });
        }
        Drawable::Textbox {
            common,
            text: text.unwrap_or_default(),
            vertical_alignment: frame.vertical_alignment,
            text_insets: None,
            text_fit,
            natural_size: None,
        }
    } else {
        let mut d = shape_drawable(ctx, &shape, text, frame.vertical_alignment);
        if text_fit.is_some() {
            if let Drawable::Shape { text_fit: tf, .. } = &mut d {
                *tf = text_fit;
            }
        }
        if let Some(role) = placeholder_role {
            if let Drawable::Shape { common, .. } = &mut d {
                common.placeholder = Some(PlaceholderInfo {
                    role,
                    inherited: None,
                });
            }
        }
        d
    };

    // Classic-import anchored geometry: Keynote-'09-converted decks (format
    // 1.5) store some text shapes' geometry with flags == 0 and position =
    // the shape's CENTER, not its top-left. 0d5851c0 slide 1: the title
    // stores (512, 638) — the slide's horizontal center — and Apple lays the
    // 500×36 rect out at 262..762 with its centered text on x=512; modern
    // archives (G2, 0f9df553) always write flags 3 (7 when rotated).
    // Re-anchor to top-left here so the model's geometry contract holds and
    // the viewer never learns about the flag. A 0×0 anchored label is
    // unaffected (shift of half-zero), and rotation is left alone — no
    // rotated flags==0 sample exists to verify against. [inferred: flag-bit
    // semantics are undocumented; behavior verified against Apple's own
    // render of 0d5851c0 slides 1/27/28]
    if shape
        .msg(1)
        .and_then(|d| d.msg(1))
        .and_then(|g| g.varint(3))
        == Some(0)
    {
        if let Drawable::Shape { common, .. } | Drawable::Textbox { common, .. } = &mut drawable {
            if common.angle_deg.unwrap_or(0.0) == 0.0 {
                if let (Some(p), Some(s)) = (common.position.as_mut(), common.size.as_ref()) {
                    p.x -= s.width / 2.0;
                    p.y -= s.height / 2.0;
                }
            }
        }
    }

    // Zero-size geometry: Numbers text boxes can store 0×0 in the geometry
    // while the path source still carries a natural size (6914f46e51ab time
    // sheet: five boxes at 0×0, path natural sizes 183×36, 185×25, 30×21,
    // 146×25). Numbers sizes such a box to its content, so the natural size
    // is a hint, not the box: the title's 183pt matches its text, the
    // 30×21 box holds a three-line paragraph. Emit it as Textbox.natural_size
    // and leave the stored 0×0 alone; the viewer's zero-size path decides.
    // [inferred: verified against Numbers' export of 6914f46e51ab]
    if let Drawable::Textbox {
        common,
        natural_size,
        ..
    } = &mut drawable
    {
        let zero = common
            .size
            .as_ref()
            .is_none_or(|s| s.width == 0.0 && s.height == 0.0);
        if zero {
            if let Some(ns) = shape
                .msg(3)
                .map(|ps| crate::tsd::shape_geometry(&ps))
                .and_then(|g| g.natural_size)
            {
                if ns.width > 0.0 || ns.height > 0.0 {
                    *natural_size = Some(ns);
                }
            }
        }
    }

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
    // Mirroring lives in TSD.GeometryArchive.flags (super(1).geometry(1)
    // field 3), not in the PathSourceArchive flip booleans, which every
    // shape in the corpus stores as false. Corpus survey (120 decks,
    // 31,343 shapes): values 0/1/3 everywhere, 7 on 349 shapes and 11 on
    // two — bit 4 is the horizontal flip (greenberg's curved arrow at 180°
    // carries 7 and Keynote draws it as the vertical mirror of its twin
    // at 3), bit 8 the vertical one. Bits 1/2 are something else and are
    // set on most shapes. [inferred]
    let flags = shape
        .msg(1)
        .and_then(|d| d.msg(1))
        .and_then(|g| g.varint(3))
        .unwrap_or(0);
    let h = (flags & 4 != 0).then_some(true);
    let v = (flags & 8 != 0).then_some(true);
    if h.is_some() || v.is_some() {
        common.flipped = Some(Flips {
            horizontal: h,
            vertical: v,
        });
    }
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
        let Some(idx) = ctx.char_pool.intern(cs) else {
            continue;
        };
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

/// Text-frame properties resolved through the style parent chain.
struct TextFrameProps {
    vertical_alignment: Option<VerticalAlignment>,
    /// "Shrink text on overflow": TSWP.ShapeStylePropertiesArchive
    /// .shrink_to_fit (field 1) [proto: TSWPArchives.proto:502]; the older
    /// TSWP.ColumnStyleArchive keeps it in column_properties.shrink_to_fit
    /// (field 2) with vertical_alignment at field 5 [proto:
    /// TSWPArchives.proto:468-493].
    shrink_to_fit: Option<bool>,
}

/// TSWP.ShapeStylePropertiesArchive.vertical_alignment (field 2, enum top=0/
/// middle=1/bottom=2/justify=3 — TSWPArchives.proto:495-513) and
/// shrink_to_fit (field 1) read off the text shape's TSWP.ShapeStyleArchive
/// OWN `shape_properties` (11); the TSD super's field 11 is fill/stroke and
/// must not be misread (its field 2 is a stroke). Theme presets keep these
/// on ancestor styles, so walk the TSS.StyleArchive parent chain (field 3);
/// each property resolves independently at its nearest present level.
/// Fixture: Home.key's title placeholder is bottom-aligned in Apple's export.
fn shape_text_frame_props(ctx: &Ctx, shape: &Msg) -> TextFrameProps {
    let mut props = TextFrameProps {
        vertical_alignment: None,
        shrink_to_fit: None,
    };
    let Some(mut sid) = shape.reference(2) else {
        return props;
    };
    for _ in 0..16 {
        let Some(m) = ctx.loaded.msg(sid) else {
            return props;
        };
        let name = ctx
            .loaded
            .record(sid)
            .and_then(|r| r.name.as_deref())
            .unwrap_or("");
        let is_tswp = name.starts_with("TSWP.");
        if is_tswp {
            // Field slots differ between the two TSWP text-frame styles.
            let is_column = name == "TSWP.ColumnStyleArchive";
            let (va_field, fit_field) = if is_column { (5, 2) } else { (2, 1) };
            if props.vertical_alignment.is_none() {
                if let Some(v) = m.msg(11).and_then(|p| p.varint(va_field)) {
                    props.vertical_alignment = Some(match v {
                        1 => VerticalAlignment::Middle,
                        2 => VerticalAlignment::Bottom,
                        3 => VerticalAlignment::Justify,
                        _ => VerticalAlignment::Top,
                    });
                }
            }
            if props.shrink_to_fit.is_none() {
                if let Some(b) = m.msg(11).and_then(|p| p.boolean(fit_field)) {
                    props.shrink_to_fit = Some(b);
                }
            }
            if props.vertical_alignment.is_some() && props.shrink_to_fit.is_some() {
                return props;
            }
        }
        // TSS.StyleArchive.parent (3): the TSWP wrapper nests supers twice
        // (TSWP → TSD → TSS), a plain TSD style once.
        let tss = if is_tswp {
            m.msg(1).and_then(|t| t.msg(1))
        } else {
            m.msg(1)
        };
        match tss.and_then(|t| t.reference(3)) {
            Some(p) => sid = p,
            None => return props,
        }
    }
    props
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
            point: None,
        });
    Drawable::Shape {
        common,
        geometry,
        text,
        vertical_alignment: v_align,
        text_insets: None,
        text_fit: None,
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
    let original = m
        .reference(13)
        .or_else(|| m.reference(8))
        .map(|id| ctx.media_ref(id));
    let thumbnail = m
        .reference(12)
        .or_else(|| m.reference(6))
        .map(|id| ctx.media_ref(id));
    let svg = m.reference(23).map(|id| ctx.media_ref(id));
    let natural_size = m.size(9).map(|(w, h)| Size {
        width: w,
        height: h,
    });
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
                point: None,
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
    let equation = equation_info(ctx, m);
    Drawable::Image {
        common,
        image,
        original,
        thumbnail,
        svg,
        natural_size,
        mask,
        adjustments,
        equation,
    }
}

/// Equation images (Insert > Equation) are TSD.ImageArchives whose media is
/// a PDF the app rendered from the typed expression, which rides along as
/// TSWP.EquationInfoArchive extension fields: equation_source_text = 103
/// (equation_source_old = 100 in older files, identical when both exist),
/// equation_depth = 102 (baseline depth, pt), equation_text_properties =
/// 101 (an inline TSWP.CharacterStylePropertiesArchive: font_size = 3,
/// font_name = 5, font_color = 7). Fixture-verified on the atnf Bayesian
/// deck (0ddd627b): 55 equations, all LaTeX, e.g. "P(T \cap C) = P(C)P(T|C)".
fn equation_info(ctx: &mut Ctx, m: &Msg) -> Option<EquationInfo> {
    let source = m.string(103).or_else(|| m.string(100))?;
    let format = if source.trim_start().starts_with("<math") {
        EquationFormat::Mathml
    } else {
        EquationFormat::Latex
    };
    let props = m.msg(101);
    let font_size_pt = props.as_ref().and_then(|p| p.f32v(3)).map(|v| v as f64);
    let font_name = props.as_ref().and_then(|p| p.string(5)).filter(|n| !n.is_empty());
    let color = props.as_ref().and_then(|p| crate::tsd::color_of(ctx, p, 7));
    Some(EquationInfo {
        source,
        format,
        depth_pt: m.f32v(102).map(|v| v as f64),
        font_size_pt,
        font_name,
        color,
    })
}

fn movie_drawable(ctx: &mut Ctx, m: &Msg) -> Drawable {
    let mut common = m
        .msg(1)
        .and_then(|d| drawable_common(ctx, &d).ok())
        .unwrap_or_default();
    let (style, extras) = drawable_style(ctx, m.reference(19), true);
    common.style = style;
    merge_extras(&mut common, extras);

    let movie = m
        .reference(14)
        .or_else(|| m.reference(2))
        .map(|id| ctx.media_ref(id));
    let poster = m
        .reference(15)
        .or_else(|| m.reference(10))
        .map(|id| ctx.media_ref(id));
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
    Drawable::Movie {
        common,
        movie,
        remote_url,
        poster,
        audio_only,
        trim,
        r#loop,
        volume,
    }
}

fn group_drawable(ctx: &mut Ctx, m: &Msg, depth: usize, visiting: &mut HashSet<u64>) -> Drawable {
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

    // Child geometry is stored ALREADY group-local in the archive (verified
    // across G2-golden-pages-layout.pages and 6 crawl .key decks: every raw
    // child position lands inside [0, group size]); the former §3.4 re-base
    // here double-subtracted the group origin and threw grouped drawables to
    // the canvas corner. Emit children verbatim.
    let children: Vec<Drawable> = m
        .references(2)
        .into_iter()
        .map(|cid| convert_drawable_depth(ctx, cid, depth + 1, visiting))
        .collect();
    Drawable::Group {
        common,
        children,
        freehand,
    }
}

/// A connection anchor: the connected drawable's frame plus, when its
/// path source decodes, its outline as a polygon in slide coordinates (used
/// to trim the line where it leaves the shape).
struct Anchor {
    position: Point,
    size: Size,
    outline: Option<Vec<(f64, f64)>>,
}

impl Anchor {
    fn center(&self) -> (f64, f64) {
        (
            self.position.x + self.size.width / 2.0,
            self.position.y + self.size.height / 2.0,
        )
    }

    /// Point-in-shape test: even-odd against the outline polygon when one
    /// decoded, else the frame rectangle.
    fn contains(&self, p: (f64, f64)) -> bool {
        if let Some(poly) = &self.outline {
            let mut inside = false;
            let n = poly.len();
            let mut j = n - 1;
            for i in 0..n {
                let (xi, yi) = poly[i];
                let (xj, yj) = poly[j];
                if (yi > p.1) != (yj > p.1) {
                    let x = xj + (p.1 - yj) / (yi - yj) * (xi - xj);
                    if p.0 < x {
                        inside = !inside;
                    }
                }
                j = i;
            }
            return inside;
        }
        p.0 >= self.position.x
            && p.0 <= self.position.x + self.size.width
            && p.1 >= self.position.y
            && p.1 <= self.position.y + self.size.height
    }
}

/// Geometry of a connection anchor drawable, whatever its wrapper depth:
/// TSD.ShapeArchive nests DrawableArchive once, TSWP.ShapeInfoArchive twice,
/// KN/TP placeholders three times. Walk down `super = 1` until the level
/// whose field 1 parses as DrawableArchive.geometry (TSP.Point + TSP.Size):
/// that level is the DrawableArchive, the one above it the ShapeArchive
/// (`pathsource = 3`, `style = 2`), and the one above that — when there is
/// one — the TSWP.ShapeInfoArchive that owns the text storage.
fn anchor_shape(ctx: &mut Ctx, aid: u64) -> Option<Anchor> {
    let mut cur = ctx.loaded.msg(aid)?.clone();
    let mut levels: Vec<Msg> = Vec::new();
    for _ in 0..4 {
        if let Some(g) = cur.msg(1) {
            if let (Some((x, y)), Some((w, h))) = (g.point(1), g.size(2)) {
                let position = Point { x, y };
                let size = Size {
                    width: w,
                    height: h,
                };
                let shape = levels.last();
                let info = levels.len().checked_sub(2).map(|i| &levels[i]);
                let geo = shape
                    .and_then(|s| s.msg(3))
                    .map(|ps| crate::tsd::shape_geometry(&ps));
                if w == 0.0 && h == 0.0 {
                    // A content-sized text box stores a 0x0 frame; Keynote
                    // lays it out at the path source's natural size, placed
                    // against the anchor by the text's alignment (centred
                    // text is centred on the anchor, middle-aligned text
                    // straddles it). Keynote's export connects to the centre
                    // of THAT box and stops the line at its edge (kcsrk
                    // slide 6: "computation" label, natural 132x36, centred
                    // and middle-aligned; the export's arrow tip sits at the
                    // text's left edge, the stored endpoint at its centre).
                    if let (Some(shape), Some(nat)) = (shape, geo.as_ref().and_then(|g| g.natural_size)) {
                        if nat.width > 0.0 && nat.height > 0.0 {
                            let (ax, ay) = zero_box_anchor_fractions(ctx, shape, info);
                            return Some(Anchor {
                                position: Point {
                                    x: x - nat.width * ax,
                                    y: y - nat.height * ay,
                                },
                                size: nat,
                                outline: None,
                            });
                        }
                    }
                }
                let outline = geo.and_then(|geo| outline_polygon(&geo, &position, &size));
                return Some(Anchor {
                    position,
                    size,
                    outline,
                });
            }
        }
        levels.push(cur.clone());
        cur = cur.msg(1)?.clone();
    }
    None
}

/// Where a content-sized (0x0) text box hangs from its anchor point, as
/// fractions of its laid-out width and height: horizontal from the first
/// paragraph's alignment (centre 0.5, right 1, else 0), vertical from the
/// text frame's vertical alignment (middle 0.5, bottom 1, else 0). Mirrors
/// the viewer's zero-size text placement. `shape` is the ShapeArchive,
/// `info` the TSWP.ShapeInfoArchive above it (owner of the storage).
fn zero_box_anchor_fractions(ctx: &mut Ctx, shape: &Msg, info: Option<&Msg>) -> (f64, f64) {
    let ay = match shape_text_frame_props(ctx, shape).vertical_alignment {
        Some(VerticalAlignment::Middle) => 0.5,
        Some(VerticalAlignment::Bottom) => 1.0,
        _ => 0.0,
    };
    // Storage: owned_storage (4), else text_flow (3) -> FlowInfo.text_storage
    // (1), else deprecated_storage (2) — the same order as the text path.
    let storage_id = info.and_then(|i| {
        i.reference(4)
            .or_else(|| i.reference(3).and_then(|f| ctx.loaded.msg(f)).and_then(|f| f.reference(1)))
            .or_else(|| i.reference(2))
    });
    // First paragraph style: TSWP.StorageArchive.table_para_style (5) is an
    // ObjectAttributeTable whose entries (1) carry {character_index, object}.
    let para_style_id = storage_id
        .and_then(|sid| ctx.loaded.msg(sid))
        .and_then(|s| s.msg(5))
        .and_then(|t| t.msgs(1).into_iter().min_by_key(|e| e.varint(1).unwrap_or(0)))
        .and_then(|e| e.reference(2));
    let ax = match para_style_id.map(|id| crate::styles::resolve_para_style(ctx, id).horizontal_alignment) {
        Some(Some(HorizontalAlignment::Center)) => 0.5,
        Some(Some(HorizontalAlignment::Right)) => 1.0,
        _ => 0.0,
    };
    (ax, ay)
}

/// Flatten a shape's explicit path (in its naturalSize space) into a slide-
/// space polygon; presets (rounded rect, polygons) fall back to the frame.
fn outline_polygon(geo: &ShapeGeometry, position: &Point, size: &Size) -> Option<Vec<(f64, f64)>> {
    let path = geo.path.as_ref()?;
    let nat = geo.natural_size.as_ref()?;
    if nat.width <= 0.0 || nat.height <= 0.0 {
        return None;
    }
    let sx = size.width / nat.width;
    let sy = size.height / nat.height;
    let map = |x: f64, y: f64| (position.x + x * sx, position.y + y * sy);
    let mut poly: Vec<(f64, f64)> = Vec::new();
    let mut cur = (0.0, 0.0);
    for e in &path.elements {
        match e {
            CurveElement::Move { points } | CurveElement::Line { points } => {
                if points.len() >= 2 {
                    cur = (points[0], points[1]);
                    poly.push(map(cur.0, cur.1));
                }
            }
            CurveElement::Quad { points } if points.len() >= 4 => {
                let (c, p2) = ((points[0], points[1]), (points[2], points[3]));
                for i in 1..=8 {
                    let t = i as f64 / 8.0;
                    let u = 1.0 - t;
                    let x = u * u * cur.0 + 2.0 * u * t * c.0 + t * t * p2.0;
                    let y = u * u * cur.1 + 2.0 * u * t * c.1 + t * t * p2.1;
                    poly.push(map(x, y));
                }
                cur = p2;
            }
            CurveElement::Cubic { points } if points.len() >= 6 => {
                let (c1, c2, p3) = (
                    (points[0], points[1]),
                    (points[2], points[3]),
                    (points[4], points[5]),
                );
                for i in 1..=8 {
                    let t = i as f64 / 8.0;
                    let u = 1.0 - t;
                    let x = u * u * u * cur.0
                        + 3.0 * u * u * t * c1.0
                        + 3.0 * u * t * t * c2.0
                        + t * t * t * p3.0;
                    let y = u * u * u * cur.1
                        + 3.0 * u * u * t * c1.1
                        + 3.0 * u * t * t * c2.1
                        + t * t * t * p3.1;
                    poly.push(map(x, y));
                }
                cur = p3;
            }
            _ => {}
        }
    }
    (poly.len() >= 3).then_some(poly)
}

/// Ellipse radius of the box inscribed in `s`, along unit direction (ux,uy):
/// how far a connection line travels from the shape's center to its border.
/// Exact for circles/ovals (the dominant connected shape), close enough for
/// boxes (corners under-trim by <=sqrt(2), hidden under the shape itself).
fn border_trim(s: &Size, ux: f64, uy: f64) -> f64 {
    let hw = (s.width / 2.0).max(1e-6);
    let hh = (s.height / 2.0).max(1e-6);
    1.0 / ((ux / hw).powi(2) + (uy / hh).powi(2)).sqrt()
}

fn connection_line_drawable(ctx: &mut Ctx, m: &Msg) -> Drawable {
    // ConnectionLineArchive { super = 1 (ShapeArchive), connected_from = 2,
    // connected_to = 3 } (drawables.md).
    let mut common = match m.msg(1) {
        Some(shape) => common_from_shape(ctx, &shape),
        None => DrawableCommon::default(),
    };
    // A connection line is a stroke-only open path: resolved theme styles can
    // still carry a fill, and painting it floods giant polygons (kcsrk deck
    // slide 22 grew full-width black bands). Apple never fills these.
    if let Some(st) = common.style.as_mut() {
        st.fill = None;
    }
    // Routing path: super(1).pathsource(3).connection_line_path_source(7)
    //   .super(1).path(3); type = 2 (0 quadratic, 1 orthogonal).
    let cl_source = m
        .msg(1)
        .and_then(|shape| shape.msg(3))
        .and_then(|ps| ps.msg(7));
    let stored = cl_source
        .as_ref()
        .and_then(|cl| cl.msg(1))
        .and_then(|bz| bz.msg(3))
        .as_ref()
        .and_then(crate::tsd::tsp_path)
        .unwrap_or(CurvePath {
            elements: Vec::new(),
        });
    let quadratic = cl_source.as_ref().and_then(|cl| cl.varint(2)).unwrap_or(0) == 0;
    let outset_from = cl_source.as_ref().and_then(|cl| cl.f32v(3)).unwrap_or(0.0) as f64;
    let outset_to = cl_source.as_ref().and_then(|cl| cl.f32v(4)).unwrap_or(0.0) as f64;

    let from_anchor = m.reference(2).and_then(|aid| anchor_shape(ctx, aid));
    let to_anchor = m.reference(3).and_then(|aid| anchor_shape(ctx, aid));
    // A content-sized text box stores a 0x0 frame; `anchor_shape` turns it
    // into its laid-out box when the path source carries a natural size.
    // One without a natural size has no known centre or outline: such an
    // end keeps the stored endpoint.
    fn sized(a: &Option<Anchor>) -> Option<&Anchor> {
        a.as_ref()
            .filter(|a| a.size.width > 0.0 && a.size.height > 0.0)
    }
    let from_geo_anchor = sized(&from_anchor);
    let to_geo_anchor = sized(&to_anchor);

    let facts = |a: &Option<Anchor>| {
        a.as_ref().map(|a| AnchorFacts {
            position: Some(a.position),
            size: Some(a.size),
        })
    };
    let from = facts(&from_anchor);
    let to = facts(&to_anchor);

    // Quadratic routing (Keynote's default "curved" connection): the stored
    // path is move + line + line whose middle point is ON the curve, and the
    // ends are the connected shapes' CENTERS in the line's local frame
    // (fixture-verified: kcsrk deck slide 8, line 2892641 stores (-35.78,
    // 42.43)->(151.05, 42.43) at position (761.70, 334.80) = the centres of
    // "bar" and "baz"; Keynote's export draws the curve through the middle
    // point and clips it at each box's edge, peak measured at y=336 for a
    // stored middle y of 334.8). Free ends (no connected_from/to) stay where
    // stored; connected ends follow the shape's current centre, so a shape
    // moved after the path was baked still gets its line (slide 8's "k" box).
    if quadratic && !stored.elements.is_empty() {
        let origin = common.position.unwrap_or(Point { x: 0.0, y: 0.0 });
        let mut pts: Vec<(f64, f64)> = Vec::new();
        for e in &stored.elements {
            let p = match e {
                CurveElement::Move { points } | CurveElement::Line { points } => points,
                _ => continue,
            };
            if p.len() >= 2 {
                pts.push((p[0] + origin.x, p[1] + origin.y));
            }
        }
        if pts.len() == 2 || pts.len() == 3 {
            let s0 = pts[0];
            let s2 = pts[pts.len() - 1];
            let sm = if pts.len() == 3 {
                pts[1]
            } else {
                ((s0.0 + s2.0) / 2.0, (s0.1 + s2.1) / 2.0)
            };
            let p0 = from_geo_anchor.map(|a| a.center()).unwrap_or(s0);
            let p2 = to_geo_anchor.map(|a| a.center()).unwrap_or(s2);
            let mid = similarity_map(sm, s0, s2, p0, p2);
            let ctrl = (2.0 * mid.0 - (p0.0 + p2.0) / 2.0, 2.0 * mid.1 - (p0.1 + p2.1) / 2.0);
            let (path, bb_min, bb_max) = trimmed_quad(
                p0,
                ctrl,
                p2,
                from_geo_anchor.map(|a| (a, outset_from)),
                to_geo_anchor.map(|a| (a, outset_to)),
            );
            let mut path = path;
            offset_curve_path(&mut path, -bb_min.0, -bb_min.1);
            common.position = Some(Point {
                x: bb_min.0,
                y: bb_min.1,
            });
            common.size = Some(Size {
                width: bb_max.0 - bb_min.0,
                height: bb_max.1 - bb_min.1,
            });
            common.angle_deg = None;
            return Drawable::ConnectionLine {
                common,
                path,
                from,
                to,
            };
        }
    }

    // Orthogonal routing (and anything unexpected): REBAKE from the live
    // anchors. The stored baked path goes stale when the connected shapes are
    // moved after baking (fixture-verified: kcsrk deck slide 22 stores
    // fork-line endpoints (126.7, 39.0)pt away from where Apple's own PDF
    // export draws them; the export's segments are exactly center-to-center,
    // trimmed by each shape's border radius). The stored path still
    // contributes its SHAPE (elbows) via a similarity map from its endpoints
    // onto the recomputed ones.
    let mut path = stored;
    if let (Some(fa), Some(ta)) = (from_geo_anchor, to_geo_anchor) {
        let c1 = fa.center();
        let c2 = ta.center();
        let (dx, dy) = (c2.0 - c1.0, c2.1 - c1.1);
        let dist = (dx * dx + dy * dy).sqrt();
        if dist > 1e-6 {
            let (ux, uy) = (dx / dist, dy / dist);
            let t1 = border_trim(&fa.size, ux, uy) + outset_from;
            let t2 = border_trim(&ta.size, ux, uy) + outset_to;
            if t1 + t2 < dist - 1.0 {
                let start = (c1.0 + ux * t1, c1.1 + uy * t1);
                let end = (c2.0 - ux * t2, c2.1 - uy * t2);
                path = rebaked_connection_path(&path, start, end);
                // Rebase to a fresh local frame: position = path bbox origin.
                let (bb_min, bb_max) = curve_path_bounds(&path).unwrap_or(((0.0, 0.0), (0.0, 0.0)));
                offset_curve_path(&mut path, -bb_min.0, -bb_min.1);
                common.position = Some(Point {
                    x: bb_min.0,
                    y: bb_min.1,
                });
                common.size = Some(Size {
                    width: bb_max.0 - bb_min.0,
                    height: bb_max.1 - bb_min.1,
                });
                common.angle_deg = None;
            }
        }
    }

    Drawable::ConnectionLine {
        common,
        path,
        from,
        to,
    }
}

/// Map point `p` by the 2D similarity (rotate + uniform scale + translate)
/// that carries segment (s0, s2) onto (p0, p2); a pure translation when the
/// source segment is degenerate.
fn similarity_map(p: (f64, f64), s0: (f64, f64), s2: (f64, f64), p0: (f64, f64), p2: (f64, f64)) -> (f64, f64) {
    let vs = (s2.0 - s0.0, s2.1 - s0.1);
    let ls2 = vs.0 * vs.0 + vs.1 * vs.1;
    if ls2 < 1e-9 {
        return (p.0 - s0.0 + p0.0, p.1 - s0.1 + p0.1);
    }
    let vn = (p2.0 - p0.0, p2.1 - p0.1);
    let a = (vn.0 * vs.0 + vn.1 * vs.1) / ls2;
    let b = (vn.1 * vs.0 - vn.0 * vs.1) / ls2;
    let (x, y) = (p.0 - s0.0, p.1 - s0.1);
    (p0.0 + a * x - b * y, p0.1 + b * x + a * y)
}

/// The quadratic Bezier p0 -> ctrl -> p2, cut back to where it leaves the
/// `from` shape and enters the `to` shape (each extended by its outset), as
/// a move + quad path (move + line when the curve is straight), with its
/// exact bounding box.
fn trimmed_quad(
    p0: (f64, f64),
    ctrl: (f64, f64),
    p2: (f64, f64),
    from: Option<(&Anchor, f64)>,
    to: Option<(&Anchor, f64)>,
) -> (CurvePath, (f64, f64), (f64, f64)) {
    let at = |t: f64| {
        let u = 1.0 - t;
        (
            u * u * p0.0 + 2.0 * u * t * ctrl.0 + t * t * p2.0,
            u * u * p0.1 + 2.0 * u * t * ctrl.1 + t * t * p2.1,
        )
    };
    const N: usize = 512;
    let samples: Vec<(f64, f64)> = (0..=N).map(|i| at(i as f64 / N as f64)).collect();
    let dist = |a: (f64, f64), b: (f64, f64)| ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt();
    // First sample outside the from-shape, then `outset` further along.
    let mut t_in = 0.0;
    if let Some((a, outset)) = from {
        if let Some(i) = samples.iter().position(|p| !a.contains(*p)) {
            let mut j = i;
            let mut run = 0.0;
            while run < outset && j + 1 < samples.len() {
                run += dist(samples[j], samples[j + 1]);
                j += 1;
            }
            t_in = j as f64 / N as f64;
        }
    }
    let mut t_out = 1.0;
    if let Some((a, outset)) = to {
        if let Some(i) = samples.iter().rposition(|p| !a.contains(*p)) {
            let mut j = i;
            let mut run = 0.0;
            while run < outset && j > 0 {
                run += dist(samples[j], samples[j - 1]);
                j -= 1;
            }
            t_out = j as f64 / N as f64;
        }
    }
    if t_in >= t_out {
        // Overlapping shapes: nothing visible would remain; keep the whole curve.
        t_in = 0.0;
        t_out = 1.0;
    }
    let (a, b) = (t_in, t_out);
    let q0 = at(a);
    let q2 = at(b);
    let q1 = (
        p0.0 * (1.0 - a) * (1.0 - b) + ctrl.0 * (a + b - 2.0 * a * b) + p2.0 * a * b,
        p0.1 * (1.0 - a) * (1.0 - b) + ctrl.1 * (a + b - 2.0 * a * b) + p2.1 * a * b,
    );
    // Straight when the control point sits on the chord.
    let chord_mid = ((q0.0 + q2.0) / 2.0, (q0.1 + q2.1) / 2.0);
    let straight = dist(q1, chord_mid) < 0.05;
    let mut min = (q0.0.min(q2.0), q0.1.min(q2.1));
    let mut max = (q0.0.max(q2.0), q0.1.max(q2.1));
    if !straight {
        // Axis extrema of the sub-curve at t = (q0 - q1) / (q0 - 2 q1 + q2).
        for axis in 0..2 {
            let (v0, v1, v2) = if axis == 0 { (q0.0, q1.0, q2.0) } else { (q0.1, q1.1, q2.1) };
            let den = v0 - 2.0 * v1 + v2;
            if den.abs() > 1e-9 {
                let t = (v0 - v1) / den;
                if t > 0.0 && t < 1.0 {
                    let u = 1.0 - t;
                    let v = u * u * v0 + 2.0 * u * t * v1 + t * t * v2;
                    if axis == 0 {
                        min.0 = min.0.min(v);
                        max.0 = max.0.max(v);
                    } else {
                        min.1 = min.1.min(v);
                        max.1 = max.1.max(v);
                    }
                }
            }
        }
    }
    let elements = if straight {
        vec![
            CurveElement::Move {
                points: vec![q0.0, q0.1],
            },
            CurveElement::Line {
                points: vec![q2.0, q2.1],
            },
        ]
    } else {
        vec![
            CurveElement::Move {
                points: vec![q0.0, q0.1],
            },
            CurveElement::Quad {
                points: vec![q1.0, q1.1, q2.0, q2.1],
            },
        ]
    };
    (CurvePath { elements }, min, max)
}

fn points_of(e: &mut CurveElement) -> &mut Vec<f64> {
    match e {
        CurveElement::Move { points }
        | CurveElement::Line { points }
        | CurveElement::Quad { points }
        | CurveElement::Cubic { points }
        | CurveElement::Close { points } => points,
    }
}

fn curve_path_bounds(p: &CurvePath) -> Option<((f64, f64), (f64, f64))> {
    let mut min = (f64::INFINITY, f64::INFINITY);
    let mut max = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut any = false;
    for e in &p.elements {
        let pts = match e {
            CurveElement::Move { points }
            | CurveElement::Line { points }
            | CurveElement::Quad { points }
            | CurveElement::Cubic { points }
            | CurveElement::Close { points } => points,
        };
        for xy in pts.chunks(2) {
            if xy.len() == 2 {
                any = true;
                min.0 = min.0.min(xy[0]);
                min.1 = min.1.min(xy[1]);
                max.0 = max.0.max(xy[0]);
                max.1 = max.1.max(xy[1]);
            }
        }
    }
    any.then_some((min, max))
}

fn offset_curve_path(p: &mut CurvePath, dx: f64, dy: f64) {
    for e in p.elements.iter_mut() {
        let pts = points_of(e);
        for xy in pts.chunks_mut(2) {
            if xy.len() == 2 {
                xy[0] += dx;
                xy[1] += dy;
            }
        }
    }
}

/// Map the stored connection path onto recomputed endpoints with the 2D
/// similarity (rotate + uniform scale + translate) that carries its first
/// point to `start` and its last to `end` — a straight stored path stays
/// straight, an elbowed/curved one keeps its proportions. Degenerate stored
/// paths are replaced by a plain segment.
fn rebaked_connection_path(stored: &CurvePath, start: (f64, f64), end: (f64, f64)) -> CurvePath {
    let mut flat: Vec<(f64, f64)> = Vec::new();
    for e in &stored.elements {
        let pts = match e {
            CurveElement::Move { points }
            | CurveElement::Line { points }
            | CurveElement::Quad { points }
            | CurveElement::Cubic { points }
            | CurveElement::Close { points } => points,
        };
        for xy in pts.chunks(2) {
            if xy.len() == 2 {
                flat.push((xy[0], xy[1]));
            }
        }
    }
    let straight = || CurvePath {
        elements: vec![
            CurveElement::Move {
                points: vec![start.0, start.1],
            },
            CurveElement::Line {
                points: vec![end.0, end.1],
            },
        ],
    };
    let (Some(s0), Some(s2)) = (flat.first().copied(), flat.last().copied()) else {
        return straight();
    };
    let vs = (s2.0 - s0.0, s2.1 - s0.1);
    let ls2 = vs.0 * vs.0 + vs.1 * vs.1;
    if ls2 < 1e-9 {
        return straight();
    }
    let vn = (end.0 - start.0, end.1 - start.1);
    // complex ratio vn / vs
    let a = (vn.0 * vs.0 + vn.1 * vs.1) / ls2;
    let b = (vn.1 * vs.0 - vn.0 * vs.1) / ls2;
    let mut out = stored.clone();
    for e in out.elements.iter_mut() {
        let pts = points_of(e);
        for xy in pts.chunks_mut(2) {
            if xy.len() == 2 {
                let (px, py) = (xy[0] - s0.0, xy[1] - s0.1);
                xy[0] = start.0 + a * px - b * py;
                xy[1] = start.1 + b * px + a * py;
            }
        }
    }
    out
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
        name_hidden: None,
        grouping: None,
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
        name_style: None,
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
                title: None,
                category_axis_title: None,
                value_axis_title: None,
                value_axis_min: None,
                value_axis_max: None,
                value_axis_major_gridlines: None,
                inner_radius: None,
                pie_labels: None,
                value_axis_format: None,
                text_sizes: None,
                axes: None,
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
            point: None,
        });
    // A bare mask at top level: represent as a shape carrying the mask path.
    Drawable::Shape {
        common,
        geometry,
        text: None,
        vertical_alignment: None,
        text_insets: None,
        text_fit: None,
    }
}
