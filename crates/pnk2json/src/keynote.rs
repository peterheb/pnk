//! Keynote conversion: KN.DocumentArchive [1] → KN.ShowArchive [2] → slides,
//! masters, builds, transitions (docs/format/keynote.md, model-design §2.1).
//! Slide order comes from `KN.SlideTreeArchive.slides`; the deprecated
//! navigator tree only donates `skipped` / `slideNumberVisible` flags.
//! Builds attach to drawables via the object ids they reference.

use std::collections::HashMap;

use crate::ctx::Ctx;
use crate::model::*;
use crate::pb::Msg;

pub fn convert_document(ctx: &mut Ctx, root: &Msg) -> KeynoteDocument {
    let show_id = root.reference(2).unwrap_or(2);
    let Some(show) = ctx.loaded.msg(show_id).cloned() else {
        ctx.warn(
            WarningCode::UnresolvedReference,
            "KN.ShowArchive not resolvable from the document root; emitting an empty show",
        );
        return empty_keynote(ctx);
    };

    let locale = ctx.resolve_locale(&root);

    // Slide size (required field 4).
    let slide_size = show
        .size(4)
        .map(|(w, h)| Size { width: w, height: h })
        .unwrap_or(Size { width: 1280.0, height: 720.0 });

    // Masters: KN.ThemeArchive.templates (field 2).
    let theme_id = show.reference(2);
    let template_ids: Vec<u64> = theme_id
        .and_then(|t| ctx.loaded.msg(t))
        .map(|t| t.references(2))
        .unwrap_or_default();
    let theme_name = theme_id
        .and_then(|t| ctx.loaded.msg(t))
        .and_then(|t| t.msg(1))
        .and_then(|base| base.string(3)); // TSS.ThemeArchive.theme_identifier

    // Theme.templates entries are KN.SlideNodeArchive NAVIGATOR wrappers
    // (type 4) in current documents — the actual master slide is the node's
    // `slide` (field 2) target. Older documents point straight at
    // KN.SlideArchive; deref only when the target is a node.
    let template_ids: Vec<u64> = template_ids
        .into_iter()
        .map(|tid| {
            match ctx.loaded.record(tid).map(|r| r.type_id) {
                Some(4) => ctx.loaded.msg(tid).and_then(|n| n.reference(2)).unwrap_or(tid),
                _ => tid,
            }
        })
        .collect();

    let mut masters: Vec<MasterSlide> = Vec::new();
    let mut master_names: HashMap<u64, String> = HashMap::new();
    for (i, tid) in template_ids.iter().enumerate() {
        let (master, name) = convert_slide_raw(ctx, *tid, true);
        let name = name.unwrap_or_else(|| format!("Master {}", i + 1));
        master_names.insert(*tid, name.clone());
        masters.push(MasterSlide { name, drawables: master.drawables, notes: master.notes, background: master.background });
    }

    // Slide order: SlideTreeArchive.slides (field 2, authoritative);
    // fall back to slideList (19), then the deprecated navigator tree.
    let mut slide_ids: Vec<u64> = Vec::new();
    if let Some(tree) = show.msg(3) {
        slide_ids = tree.references(2);
    }
    if slide_ids.is_empty() {
        slide_ids = show.references(19);
    }
    if slide_ids.is_empty() {
        if let Some(tree) = show.msg(3) {
            slide_ids = walk_slide_nodes(ctx, tree.reference(1));
        }
    }
    // In practice, tree.slides may reference KN.SlideNodeArchive objects
    // rather than SlideArchives directly: deref node.slide (field 2).
    slide_ids = slide_ids
        .into_iter()
        .map(|sid| match ctx.loaded.record(sid).map(|r| r.type_id) {
            Some(4) => ctx.loaded.msg(sid).and_then(|n| n.reference(2)).unwrap_or(sid),
            _ => sid,
        })
        .collect();

    // Navigator flags: isSkipped (4), isSlideNumberVisible (18), by slide id.
    let mut node_flags: HashMap<u64, (Option<bool>, Option<bool>)> = HashMap::new();
    if let Some(tree) = show.msg(3) {
        collect_node_flags(ctx, tree.reference(1), &mut node_flags);
    }

    let master_index: HashMap<u64, usize> =
        template_ids.iter().enumerate().map(|(i, t)| (*t, i)).collect();

    let mut slides = Vec::new();
    for sid in &slide_ids {
        let (mut slide, _) = convert_slide_raw(ctx, *sid, false);
        if let Some((skipped, num_visible)) = node_flags.get(sid) {
            if slide.skipped.is_none() {
                slide.skipped = *skipped;
            }
            if slide.slide_number_visible.is_none() {
                slide.slide_number_visible = *num_visible;
            }
        }
        // Placeholder inheritance from the master (model-design §3.2); also
        // resolves masterName via the slide's template_slide (17).
        inherit_placeholders(ctx, *sid, &mut slide, &master_names);
        // Resolved-inheritance contract (model-review §3b): the slide carries
        // its effective background (master chain walked) and the filtered
        // master underlay — a viewer paints background, masterDrawables,
        // drawables, and never consults masters[].
        if let Some(mi) = resolve_template_id(ctx, *sid).and_then(|mid| master_index.get(&mid)) {
            let master = &masters[*mi];
            if slide.background.is_none() {
                slide.background = master.background.clone();
            }
            slide.master_drawables = Some(master_underlay(master, &slide));
        }
        slides.push(slide);
    }

    // Playback settings (KN.ShowArchive fields 6/8/9/10/11).
    let playback = {
        let any = show.has(9) || show.has(8) || show.has(10) || show.has(11) || show.has(6);
        if any {
            Some(KeynotePlayback {
                mode: show.varint(9).map(|v| match v {
                    1 => KeynotePlayMode::AutoPlay,
                    2 => KeynotePlayMode::HyperlinksOnly,
                    _ => KeynotePlayMode::Normal,
                }),
                r#loop: show.boolean(8),
                autoplay_transition_delay_sec: show.f64v(10),
                autoplay_build_delay_sec: show.f64v(11),
                slide_numbers_visible: show.boolean(6),
            })
        } else {
            None
        }
    };

    // Soundtrack: KN.Soundtrack { movie_media = 3, mode = 2 }.
    let soundtrack = show.reference(17).and_then(|stid| {
        let st = ctx.loaded.msg(stid)?.clone();
        Some({
            let tracks: Vec<MediaAsset> = st
                .references(3)
                .into_iter()
                .map(|d| {
                    let r = ctx.media_ref(d);
                    media_asset_from_ref(ctx, &r)
                })
                .collect();
            Soundtrack {
                tracks,
                repeat: match st.varint(2).unwrap_or(0) {
                    1 => Some(SoundtrackRepeat::All),
                    2 => Some(SoundtrackRepeat::None),
                    _ => Some(SoundtrackRepeat::One),
                },
            }
        })
    });

    // Recording: duration only (track machinery dropped per model-design §6).
    let recording = show
        .reference(7)
        .and_then(|rid| ctx.loaded.msg(rid))
        .map(|r| RecordingInfo { duration_sec: r.f64v(3) });

    let mut doc = KeynoteDocument {
        kind: "keynote".to_string(),
        meta: ctx.meta.clone(),
        warnings: Vec::new(),
        fonts: Vec::new(),
        media: Vec::new(),
        styles: StylePools::default(),
        slide_size,
        slides,
        masters,
        theme_name,
        playback,
        soundtrack,
        recording,
    };
    if doc.meta.locale.is_none() {
        doc.meta.locale = locale;
    }
    doc
}

fn empty_keynote(ctx: &mut Ctx) -> KeynoteDocument {
    let mut doc = KeynoteDocument {
        kind: "keynote".to_string(),
        meta: ctx.meta.clone(),
        warnings: Vec::new(),
        fonts: Vec::new(),
        media: Vec::new(),
        styles: StylePools::default(),
        slide_size: Size { width: 1280.0, height: 720.0 },
        slides: Vec::new(),
        masters: Vec::new(),
        theme_name: None,
        playback: None,
        soundtrack: None,
        recording: None,
    };
    doc
}

fn media_asset_from_ref(ctx: &Ctx, r: &MediaRef) -> MediaAsset {
    let entry = r.data_id.parse::<u64>().ok().and_then(|id| ctx.datas.get(&id));
    MediaAsset {
        data_id: r.data_id.clone(),
        file_name: r.file_name.clone(),
        preferred_file_name: r.preferred_file_name.clone(),
        kind: r
            .file_name
            .as_deref()
            .map(crate::ctx::media_kind)
            .unwrap_or(MediaKind::Audio),
        byte_length: entry.and_then(|e| e.materialized_length),
        pixel_size: r.pixel_size,
    }
}

/// Navigator tree walk: KN.SlideNodeArchive { children = 1, slide = 2 }.
fn walk_slide_nodes(ctx: &Ctx, node_id: Option<u64>) -> Vec<u64> {
    let mut out = Vec::new();
    if let Some(nid) = node_id {
        if let Some(n) = ctx.loaded.msg(nid) {
            if let Some(s) = n.reference(2) {
                out.push(s);
            }
            for c in n.references(1) {
                out.extend(walk_slide_nodes(ctx, Some(c)));
            }
        }
    }
    out
}

fn collect_node_flags(
    ctx: &Ctx,
    node_id: Option<u64>,
    flags: &mut HashMap<u64, (Option<bool>, Option<bool>)>,
) {
    if let Some(nid) = node_id {
        if let Some(n) = ctx.loaded.msg(nid) {
            if let Some(s) = n.reference(2) {
                flags.insert(s, (n.boolean(4), n.boolean(18)));
            }
            for c in n.references(1) {
                collect_node_flags(ctx, Some(c), flags);
            }
        }
    }
}

/// Convert one KN.SlideArchive [5]/[6]. Regular slides paint in
/// drawables_z_order (42) order when present, else owned_drawables (7);
/// masters use their owned list (keynote.md field notes).
fn convert_slide_raw(ctx: &mut Ctx, slide_id: u64, is_master: bool) -> (Slide, Option<String>) {
    let Some(m) = ctx.loaded.msg(slide_id).cloned() else {
        ctx.warn_detail(
            WarningCode::UnresolvedReference,
            format!("slide reference {slide_id} points nowhere"),
            slide_id.to_string(),
        );
        return (empty_slide(), None);
    };

    // name = field 10 (KN.SlideArchive.name). Guard the contract: a string
    // that decodes but contains binary junk (control/NUL bytes — e.g. a
    // geometry payload mis-read as a name) is treated as absent.
    let name = m
        .string(10)
        .filter(|s| {
            !s.is_empty()
                && !s.chars().any(|c| c.is_control() && c != '\t' && c != '\n')
        });

    let drawable_ids = if !is_master {
        let z = m.references(42);
        if !z.is_empty() {
            z
        } else {
            m.references(7)
        }
    } else {
        m.references(7)
    };

    // Convert with ids kept, attach builds by id, then drop the ids.
    let mut converted: Vec<(u64, Drawable)> = drawable_ids
        .into_iter()
        .map(|d| (d, crate::drawables::convert_drawable(ctx, d)))
        .collect();
    attach_builds(ctx, slide_id, &mut converted);

    // Slides may carry title/body/slide-number/object placeholders outside
    // the paint lists (SlideArchive fields 5/6/20/30) — decode those too,
    // appended after the painted drawables.
    let painted: std::collections::HashSet<u64> = converted.iter().map(|(id, _)| *id).collect();
    for pid in [5u32, 6, 20, 30].iter().flat_map(|f| m.references(*f)) {
        if !painted.contains(&pid) {
            converted.push((pid, crate::drawables::convert_drawable(ctx, pid)));
        }
    }
    let drawables: Vec<Drawable> = converted.into_iter().map(|(_, d)| d).collect();

    // Notes: KN.NoteArchive { containedStorage = 1 } → TSWP.StorageArchive.
    let notes = m
        .reference(27)
        .and_then(|nid| ctx.loaded.msg(nid))
        .and_then(|n| n.reference(1))
        .and_then(|stid| crate::text::extract(ctx, stid))
        .map(|e| e.text);

    // Background: KN.SlideStyleArchive.slide_properties(11).fill(1), walking
    // up the TSS.StyleArchive parent chain when the style itself sets none.
    let background = m.reference(1).and_then(|sid| slide_background_fill(ctx, sid, 0));
    // Transition: TransitionArchive.attributes(2).animationAttributes(8).
    let transition = m
        .msg(4)
        .and_then(|t| t.msg(2))
        .and_then(|ta| ta.msg(8))
        .map(|aa| TransitionSpec {
            effect: aa.string(2),
            animation_type: aa.string(1),
            duration_sec: aa.f64v(3),
            delay_sec: aa.f64v(5),
            automatic: aa.boolean(6),
            direction: aa.varint(4).map(|v| v.to_string()),
            color: aa.msg(7).and_then(|c| {
                let mut w = Vec::new();
                crate::colors::color_hex(&c, &mut |r| w.push(r))
            }),
        });

    (
        Slide {
        master_drawables: None,
            name: name.clone(),
            skipped: None,
            master_name: None,
            drawables,
            notes,
            transition,
            slide_number_visible: None,
            background,
        },
        name,
    )
}

fn empty_slide() -> Slide {
    Slide {
        master_drawables: None,
        name: None,
        skipped: None,
        master_name: None,
        drawables: Vec::new(),
        notes: None,
        transition: None,
        slide_number_visible: None,
        background: None,
    }
}

/// Attach KN.BuildArchive specs to the drawables they reference.
/// KN.BuildArchive { drawable = 1, delivery = 2 (string), attributes = 4 }.
fn attach_builds(ctx: &mut Ctx, slide_id: u64, converted: &mut [(u64, Drawable)]) {
    let Some(m) = ctx.loaded.msg(slide_id).cloned() else { return };
    let build_refs = m.references(2);
    if build_refs.is_empty() {
        return;
    }
    // Build chunks: slide.buildChunks (43), grouped by their build reference.
    let mut chunks_by_build: HashMap<u64, Vec<BuildChunk>> = HashMap::new();
    for cid in m.references(43) {
        if let Some(c) = ctx.loaded.msg(cid) {
            if let Some(b) = c.reference(1) {
                chunks_by_build.entry(b).or_default().push(BuildChunk {
                    delay_sec: c.f64v(3),
                    duration_sec: c.f64v(4),
                    automatic: c.boolean(5),
                });
            }
        }
    }

    for (order, bid) in build_refs.iter().enumerate() {
        let Some(b) = ctx.loaded.msg(*bid) else { continue };
        let Some(drawable_ref) = b.reference(1) else { continue };
        let delivery = match b.string(2).as_deref() {
            Some("build-in") | Some("in") => BuildDelivery::In,
            Some("build-out") | Some("out") => BuildDelivery::Out,
            Some("action") => BuildDelivery::Action,
            _ => BuildDelivery::Other,
        };
        let attrs = b.msg(4);
        let anim = attrs.as_ref().and_then(|a| a.msg(18)); // AnimationAttributesArchive
        let spec = BuildSpec {
            delivery,
            effect: anim.as_ref().and_then(|a| a.string(2)),
            animation_type: anim.as_ref().and_then(|a| a.string(1)),
            duration_sec: anim.as_ref().and_then(|a| a.f64v(3)),
            delay_sec: anim.as_ref().and_then(|a| a.f64v(5)),
            automatic: anim.as_ref().and_then(|a| a.boolean(6)),
            acceleration: attrs
                .as_ref()
                .and_then(|a| a.varint(13))
                .map(|v| match v {
                    1 => BuildAcceleration::EaseIn,
                    2 => BuildAcceleration::EaseOut,
                    3 => BuildAcceleration::EaseBoth,
                    4 => BuildAcceleration::Custom,
                    _ => BuildAcceleration::None,
                }),
            text_delivery: attrs
                .as_ref()
                .and_then(|a| a.varint(20))
                .map(|v| match v {
                    1 => BuildTextDelivery::ByObject,
                    2 => BuildTextDelivery::ByWord,
                    3 => BuildTextDelivery::ByCharacter,
                    _ => BuildTextDelivery::ByLine,
                }),
            chunks: chunks_by_build.get(bid).cloned(),
            motion_blur: attrs.as_ref().and_then(|a| {
                if a.boolean(29).unwrap_or(false) || a.has(39) {
                    Some(MotionBlur { amount: a.f64v(39).unwrap_or(0.0) })
                } else {
                    None
                }
            }),
            order: Some(order as u32),
        };
        for (did, d) in converted.iter_mut() {
            if *did == drawable_ref {
                set_build(d, spec.clone());
                break;
            }
        }
    }
}

fn set_build(d: &mut Drawable, spec: BuildSpec) {
    match d {
        Drawable::Shape { common, .. }
        | Drawable::Textbox { common, .. }
        | Drawable::Image { common, .. }
        | Drawable::Movie { common, .. }
        | Drawable::Group { common, .. }
        | Drawable::ConnectionLine { common, .. }
        | Drawable::Table { common, .. }
        | Drawable::Chart { common, .. } => {
            if common.keynote_build.is_none() {
                common.keynote_build = Some(spec);
            }
        }
        _ => {}
    }
}

/// The slide's master (template_slide, field 17), dereferencing the
/// KN.SlideNodeArchive navigator wrapper (type 4) when present.
fn resolve_template_id(ctx: &Ctx, slide_id: u64) -> Option<u64> {
    let tid = ctx.loaded.msg(slide_id)?.reference(17)?;
    Some(match ctx.loaded.record(tid).map(|r| r.type_id) {
        Some(4) => ctx.loaded.msg(tid).and_then(|n| n.reference(2)).unwrap_or(tid),
        _ => tid,
    })
}

/// Placeholder inheritance (model-design §3.2): a slide placeholder without
/// text or geometry overrides inherits geometry + style from the master's
/// placeholder of the same role and gets `inherited = true`.
fn inherit_placeholders(
    ctx: &mut Ctx,
    slide_id: u64,
    slide: &mut Slide,
    master_names: &HashMap<u64, String>,
) {
    let Some(mid) = resolve_template_id(ctx, slide_id) else { return };
    if let Some(mn) = master_names.get(&mid) {
        slide.master_name = Some(mn.clone());
    }
    // Master placeholders: fields 5 (title), 6 (body), 20 (slide-number),
    // 30 (object) per docs/format/keynote.md.
    let master_placeholders: Vec<(u64, String)> = ctx
        .loaded
        .msg(mid)
        .map(|mm| {
            [5u32, 6, 20, 30]
                .iter()
                .filter_map(|f| mm.reference(*f))
                .filter_map(|pid| placeholder_role(ctx, pid).map(|r| (pid, r)))
                .collect()
        })
        .unwrap_or_default();

    for d in slide.drawables.iter_mut() {
        let (role, has_text) = match d {
            Drawable::Textbox { common, text, .. } => (
                common.placeholder.as_ref().filter(|p| p.inherited.is_none()).map(|p| p.role.clone()),
                !text.paragraphs.is_empty(),
            ),
            Drawable::Shape { common, text, .. } => (
                common.placeholder.as_ref().filter(|p| p.inherited.is_none()).map(|p| p.role.clone()),
                text.as_ref().map(|t| !t.paragraphs.is_empty()).unwrap_or(false),
            ),
            _ => (None, false),
        };
        let Some(role) = role else { continue };
        if has_text {
            continue;
        }
        let Some((mpid, _)) = master_placeholders.iter().find(|(_, r)| *r == role) else {
            continue;
        };
        let Some(master_common) = master_placeholder_common(ctx, *mpid) else { continue };
        let inherited_common = apply_inherited(&master_common);
        match d {
            Drawable::Shape { common, .. } | Drawable::Textbox { common, .. } => {
                *common = inherited_common;
                if let Some(p) = common.placeholder.as_mut() {
                    p.inherited = Some(true);
                }
            }
            _ => {}
        }
    }
}

fn placeholder_role(ctx: &Ctx, pid: u64) -> Option<String> {
    let rec = ctx.loaded.record(pid)?;
    let m = ctx.loaded.msg(pid)?;
    match rec.type_id {
        7 | 12 => {
            let is_kn = rec.name.as_deref().map(|n| n.starts_with("KN.")).unwrap_or(false);
            if is_kn {
                // KN.PlaceholderArchive.Kind (KNArchives.proto:203-209).
                Some(match m.varint(2).unwrap_or(0) {
                    1 => "slide-number".into(),
                    2 => "title".into(),
                    3 => "body".into(),
                    4 => "object".into(),
                    _ => "placeholder".into(),
                })
            } else {
                Some("placeholder".into())
            }
        }
        _ => None,
    }
}

fn master_placeholder_common(ctx: &mut Ctx, pid: u64) -> Option<DrawableCommon> {
    let d = crate::drawables::convert_drawable(ctx, pid);
    match d {
        Drawable::Textbox { common, .. } | Drawable::Shape { common, .. } => Some(common),
        _ => None,
    }
}

/// Bake the master placeholder's geometry/style into the inheriting copy.
fn apply_inherited(master: &DrawableCommon) -> DrawableCommon {
    DrawableCommon {
        position: master.position,
        size: master.size,
        angle_deg: master.angle_deg,
        style: master.style.clone(),
        placeholder: master.placeholder.clone(),
        ..Default::default()
    }
}

/// Slide/master background fill: `KN.SlideStyleArchive.slide_properties(11)
/// .fill(1)`, walking up the `TSS.StyleArchive.parent` chain (theme preset
/// styles) when the style itself sets no fill. Bounded depth.
fn slide_background_fill(ctx: &mut Ctx, style_id: u64, depth: u32) -> Option<Fill> {
    if depth > 16 {
        return None;
    }
    let m = ctx.loaded.msg(style_id).cloned()?;
    if let Some(props) = m.msg(11) {
        if let Some(fill) = props.msg(1).and_then(|f| crate::tsd::fill_of(ctx, &f)) {
            return Some(fill);
        }
    }
    let parent = m.msg(1).and_then(|base| base.reference(3))?;
    slide_background_fill(ctx, parent, depth + 1)
}

// ---------------------------------------------------------------------------
// Master underlay (model-review §3b): which master drawables actually paint
// under a given slide. Ported from the viewer's compositing rules so the
// contract lives in ONE place — the converter — and viewers paint verbatim.
// ---------------------------------------------------------------------------

fn drawable_common(d: &Drawable) -> Option<&DrawableCommon> {
    match d {
        Drawable::Shape { common, .. }
        | Drawable::Textbox { common, .. }
        | Drawable::Image { common, .. }
        | Drawable::Movie { common, .. }
        | Drawable::Group { common, .. }
        | Drawable::ConnectionLine { common, .. }
        | Drawable::Table { common, .. }
        | Drawable::Chart { common, .. } => Some(common),
        _ => None,
    }
}

fn drawable_role(d: &Drawable) -> Option<&str> {
    drawable_common(d)?.placeholder.as_ref().map(|p| p.role.as_str())
}

/// (x, y, w, h) when fully placed.
fn drawable_frame(d: &Drawable) -> Option<(f64, f64, f64, f64)> {
    let c = drawable_common(d)?;
    let p = c.position?;
    let s = c.size?;
    Some((p.x, p.y, s.width, s.height))
}

/// Rounded position+size signature for exact-overlap detection.
fn frame_key(d: &Drawable) -> Option<(i64, i64, i64, i64)> {
    let (x, y, w, h) = drawable_frame(d)?;
    Some((x.round() as i64, y.round() as i64, w.round() as i64, h.round() as i64))
}

/// True when the drawable carries at least one non-whitespace text run.
fn drawable_has_text(d: &Drawable) -> bool {
    let text = match d {
        Drawable::Textbox { text, .. } => Some(text),
        Drawable::Shape { text, .. } => text.as_ref(),
        _ => None,
    };
    let Some(t) = text else { return false };
    t.paragraphs.iter().any(|p| {
        p.items.iter().any(|i| match i {
            ParagraphItem::Plain(s) => !s.trim().is_empty(),
            ParagraphItem::Text { text, .. } => !text.trim().is_empty(),
            _ => false,
        })
    })
}

/// True when `outer` covers at least 60% of `inner`'s area.
fn covers(outer: (f64, f64, f64, f64), inner: (f64, f64, f64, f64)) -> bool {
    let ix = ((outer.0 + outer.2).min(inner.0 + inner.2) - outer.0.max(inner.0)).max(0.0);
    let iy = ((outer.1 + outer.3).min(inner.1 + inner.3) - outer.1.max(inner.1)).max(0.0);
    let area = inner.2 * inner.3;
    area > 0.0 && (ix * iy) / area >= 0.6
}

/// Master furniture that shows under this slide, in master paint order:
/// - placeholder-tagged prompts (title/body/object/slide-number) never paint
///   on slides — Apple shows their prompt text only in the editor;
/// - furniture at exactly a slide drawable's frame is superseded by it;
/// - role-less text prompts (e.g. a "Section Title" shape) are superseded by
///   any slide text drawable covering >=60% of their frame.
fn master_underlay(master: &MasterSlide, slide: &Slide) -> Vec<Drawable> {
    let slide_geoms: std::collections::HashSet<(i64, i64, i64, i64)> =
        slide.drawables.iter().filter_map(frame_key).collect();
    let slide_text_frames: Vec<(f64, f64, f64, f64)> = slide
        .drawables
        .iter()
        .filter(|d| drawable_has_text(d))
        .filter_map(drawable_frame)
        .collect();
    master
        .drawables
        .iter()
        .filter(|d| {
            if drawable_role(d).is_some() {
                return false;
            }
            if let Some(k) = frame_key(d) {
                if slide_geoms.contains(&k) {
                    return false;
                }
            }
            if drawable_has_text(d) {
                if let Some(f) = drawable_frame(d) {
                    if slide_text_frames.iter().any(|sf| covers(*sf, f)) {
                        return false;
                    }
                }
            }
            true
        })
        .cloned()
        .collect()
}
