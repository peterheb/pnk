//! Pages conversion: TP.DocumentArchive [10000] → both flavors
//! (docs/format/pages.md, model-design §2.1).
//! - Word-processing: `body_storage` (field 4) holds the flowing text;
//!   `section` (5) is empty.
//! - Page-layout: `section` (5, TP.SectionArchive) holds the canvases;
//!   floating drawables carry each page's content.

use crate::ctx::Ctx;
use crate::model::*;
use crate::pb::{ids, Msg};

pub fn convert_document(ctx: &mut Ctx, root: &Msg) -> PagesDocument {
    let locale = ctx.resolve_locale(root);
    // The text splitter compares run languages against the document locale
    // (headers and text boxes are extracted before the body).
    ctx.meta.locale = locale.clone();

    // Flavor discriminator: TP.SettingsArchive.body (field 1, default true)
    // — "include body text in the document". Layout docs carry body=0 even
    // though body_storage (4) is still present (fixture-verified G1/G2 +
    // corpus: both flavors reference a body storage, so presence is NOT the
    // discriminator; docs/format/pages.md + gotchas #16). Absent settings →
    // default true = word-processing.
    let flavor = match root
        .reference(7)
        .and_then(|sid| ctx.loaded.msg(sid))
        .and_then(|s| s.varint(1))
    {
        Some(0) => PagesFlavor::PageLayout,
        _ => PagesFlavor::WordProcessing,
    };

    let page_size = if root.has(30) || root.has(31) {
        Some(Size {
            width: root.f32v(30).unwrap_or(0.0) as f64,
            height: root.f32v(31).unwrap_or(0.0) as f64,
        })
    } else {
        None
    };

    let page_margins = if root.has(32) || root.has(33) || root.has(34) || root.has(35) {
        Some(PageMargins {
            left: root.f32v(32).map(|v| v as f64),
            right: root.f32v(33).map(|v| v as f64),
            top: root.f32v(34).map(|v| v as f64),
            bottom: root.f32v(35).map(|v| v as f64),
            header: root.f32v(36).map(|v| v as f64),
            footer: root.f32v(37).map(|v| v as f64),
        })
    } else {
        None
    };

    // lays_out_body_vertically (39 [proto: TPArchives.proto]) — vertical
    // (tategaki) body layout. The viewer renders horizontally; warn so the
    // blank/mis-laid render is explained (00V template probe).
    if root.varint(39) == Some(1) {
        ctx.warn(
            WarningCode::UnsupportedFeature,
            "document uses vertical text layout (tategaki); rendered horizontally".to_string(),
        );
    }

    let orientation = root.varint(42).map(|v| {
        if v != 0 {
            PageLayoutOrientation::Landscape
        } else {
            PageLayoutOrientation::Portrait
        }
    });
    let page_scale = root.f32v(38).map(|v| v as f64);

    // Page templates (masters).
    let mut template_ids = root.references(48);
    // PageMasterArchives (10143) carry the headers (f1), footers (f2) and
    // master drawables, and sections reference THEM (fields 23-25). Fresh
    // 26.3 docs (G5) have no field 48 at all; older docs (26a356, a Pages
    // 5-era newsletter) point field 48 at a template container of another
    // type while the masters float free. Either way every master in the
    // graph joins the list, after the field-48 entries, in id order.
    {
        let mut masters: Vec<u64> = ctx
            .loaded
            .records
            .values()
            .filter(|rec| rec.type_id == 10143 && !template_ids.contains(&rec.id))
            .map(|rec| rec.id)
            .collect();
        // records is a hash map: sort for deterministic template order/names.
        masters.sort_unstable();
        template_ids.extend(masters);
    }
    let mut page_templates = Vec::new();
    let mut template_names: std::collections::HashMap<u64, String> =
        std::collections::HashMap::new();
    for (i, tid) in template_ids.iter().enumerate() {
        let (pt, name) = convert_page_template(ctx, *tid, i);
        if let Some(n) = name {
            template_names.insert(*tid, n);
        }
        page_templates.push(pt);
    }

    // Floating drawables: TP.FloatingDrawablesArchive.page_groups →
    // PageGroup { page_index = 1, drawables = 4, background = 2,
    // foreground = 3 }. Paint order follows TP.DrawablesZOrderArchive
    // (field 20 → drawables = 1) when ids are present there.
    let zorder: Vec<u64> = root
        .reference(20)
        .and_then(|z| ctx.loaded.msg(z))
        .map(|z| z.references(1))
        .unwrap_or_default();
    let mut zrank: std::collections::HashMap<u64, usize> = std::collections::HashMap::new();
    for (i, id) in zorder.iter().enumerate() {
        zrank.insert(*id, i);
    }

    let mut floating: Vec<FloatingPage> = Vec::new();
    if let Some(fda_id) = root.reference(3) {
        if let Some(fda) = ctx.loaded.msg(fda_id) {
            for pg in fda.msgs(1) {
                let mut ids_all = pg.references(4);
                ids_all.extend(pg.references(2));
                ids_all.extend(pg.references(3));
                // Deduplicate while keeping first occurrence.
                let mut seen = std::collections::HashSet::new();
                ids_all.retain(|id| seen.insert(*id));
                // Convert and order by z-rank (unknown ids keep list order).
                let mut converted: Vec<(u64, Drawable)> = ids_all
                    .into_iter()
                    .map(|d| (d, crate::drawables::convert_drawable(ctx, d)))
                    .collect();
                converted.sort_by_key(|(id, _)| zrank.get(id).copied().unwrap_or(usize::MAX));
                floating.push(FloatingPage {
                    template_drawables: None,
                    page_index: pg.varint(1).map(|v| v as u32),
                    drawables: converted.into_iter().map(|(_, d)| d).collect(),
                });
            }
        }
    }

    // Body flow: word-processing renders it as `body`; page-layout docs keep
    // a never-rendered body storage — preserved as `hiddenBody` when it has
    // any content (no silent data loss; omitted when empty).
    let mut body = None;
    let mut hidden_body = None;
    let mut footnotes: Vec<Footnote> = Vec::new();
    let mut table_of_contents = None;
    let mut comments = None;
    let mut bookmarks = None;
    let mut changes = None;
    if let Some(bsid) = root.reference(4) {
        if let Some(ex) = crate::text::extract(ctx, bsid) {
            if !ex.toc_entries.is_empty() {
                // A document with several TOC boxes (one per section in
                // 55d37c2b) repeats the same entries; keep one copy each.
                let mut entries: Vec<TocEntry> = Vec::new();
                for e in ex.toc_entries {
                    if !entries.contains(&e) {
                        entries.push(e);
                    }
                }
                table_of_contents = Some(TableOfContents { entries });
            }
            if !ex.comments.is_empty() {
                comments = Some(ex.comments);
            }
            if !ex.bookmarks.is_empty() {
                bookmarks = Some(ex.bookmarks);
            }
            if !ex.changes.is_empty() {
                changes = Some(ex.changes);
            }
            let non_empty = ex.text.paragraphs.iter().any(|p| {
                p.items
                    .iter()
                    .any(|it| !matches!(it, ParagraphItem::Text { text, .. } if text.is_empty()))
            });
            match flavor {
                PagesFlavor::WordProcessing => {
                    body = Some(ex.text);
                    for (para_idx, ftext) in ex.footnotes {
                        footnotes.push(Footnote {
                            anchor_paragraph_index: para_idx,
                            text: ftext,
                        });
                    }
                }
                PagesFlavor::PageLayout => {
                    if non_empty {
                        hidden_body = Some(ex.text);
                    }
                }
            }
        }
    }

    // Sections (TP.SectionArchive). Older docs hang ONE section off
    // DocumentArchive.section (5); modern Pages (26.x) stores sections in the
    // BODY storage's table_section attribute table (StorageArchive field 17
    // [proto: TSWPArchives.proto table_section], offset → SectionArchive —
    // fixture G5: entry at offset 0 → the section carrying the page masters
    // with header/footer text).
    // Entries carry the section's START as a UTF-16 offset into the body
    // buffer; layout (column) styles live in the same offset space
    // (table_layout_style, StorageArchive field 12 [proto: TSWPArchives.proto]
    // → TSWP.ColumnStyleArchive).
    let mut section_ids: Vec<(u64, u64)> = root
        .reference(5)
        .into_iter()
        .map(|sid| (0u64, sid))
        .collect();
    let mut layout_entries: Vec<(u64, u64)> = Vec::new();
    let mut body_text = String::new();
    if let Some(bsid) = root.reference(4) {
        if let Some(bs) = ctx.loaded.msg(bsid) {
            body_text = bs.string(3).unwrap_or_default();
            if section_ids.is_empty() {
                if let Some(table) = bs.msg(17) {
                    let mut entries: Vec<(u64, u64)> = table
                        .msgs(1)
                        .into_iter()
                        .filter_map(|e| Some((e.varint(1).unwrap_or(0), e.reference(2)?)))
                        .collect();
                    entries.sort_by_key(|(off, _)| *off);
                    for (off, sid) in entries {
                        if !section_ids.iter().any(|(_, s)| *s == sid) {
                            section_ids.push((off, sid));
                        }
                    }
                }
            }
            if let Some(table) = bs.msg(12) {
                layout_entries = table
                    .msgs(1)
                    .into_iter()
                    .filter_map(|e| Some((e.varint(1).unwrap_or(0), e.reference(2)?)))
                    .collect();
                layout_entries.sort_by_key(|(off, _)| *off);
            }
        }
    }
    // Gutter fractions resolve against the printable width (page minus the
    // left/right margins; Pages' defaults are 1in).
    let content_width_pt = page_size.as_ref().map(|s| {
        let m = page_margins.as_ref();
        s.width - m.and_then(|m| m.left).unwrap_or(72.0) - m.and_then(|m| m.right).unwrap_or(72.0)
    });
    let mut sections = Vec::new();
    for (i, (off, sec_id)) in section_ids.iter().enumerate() {
        let mut sec = convert_section(ctx, *sec_id, &template_names, i, &mut page_templates);
        if matches!(flavor, PagesFlavor::WordProcessing) {
            // Omit-default: the first section starts at paragraph 0.
            sec.body_paragraph_start = Some(para_index_at(&body_text, *off)).filter(|p| *p > 0);
            // Column layout in effect at the section start: the last
            // table_layout_style entry at or before the section's offset.
            let layout = layout_entries
                .iter()
                .rev()
                .find(|(lo, _)| *lo <= *off)
                .map(|(_, sid)| *sid);
            if let Some(lid) = layout {
                sec.columns = crate::styles::resolve_section_columns(ctx, lid, content_width_pt);
            }
        }
        sections.push(sec);
    }

    // Page-layout: resolve each canvas's template UNDERLAY into
    // FloatingPage.template_drawables (docs/model-review.md §3c) — the
    // section's first/even/odd template drawables plus placeholder drawables
    // not superseded by a same-geometry drawable already on the page.
    if matches!(flavor, PagesFlavor::PageLayout) && !sections.is_empty() {
        let by_name: std::collections::HashMap<String, PageTemplate> = page_templates
            .iter()
            .filter_map(|t| t.name.clone().map(|n| (n, t.clone())))
            .collect();
        for (i, fp) in floating.iter_mut().enumerate() {
            let page = fp.page_index.map(|v| v as usize).unwrap_or(i);
            let sec = &sections[page.min(sections.len() - 1)];
            let name = if page == 0 {
                sec.first_page_template
                    .as_deref()
                    .or(sec.odd_page_template.as_deref())
            } else if (page + 1) % 2 == 0 {
                sec.even_page_template
                    .as_deref()
                    .or(sec.odd_page_template.as_deref())
            } else {
                sec.odd_page_template.as_deref()
            };
            let t = name.and_then(|n| by_name.get(n));
            let mut td: Vec<Drawable> = Vec::new();
            // Page background, resolved: the template's own fill wins, else
            // the section's (Labothek covers: section background_fill
            // #297000 paints the full green page under white title text).
            // Baked as a full-page rect at the bottom of the underlay so the
            // viewer stays a verbatim painter (model-review §3c).
            let bg = t
                .and_then(|t| t.background_fill.clone())
                .or_else(|| sec.background_fill.clone());
            if let (Some(fill), Some(size)) = (bg, page_size.as_ref()) {
                td.push(Drawable::Shape {
                    common: DrawableCommon {
                        position: Some(Point { x: 0.0, y: 0.0 }),
                        size: Some(*size),
                        style: Some(DrawableStyle {
                            fill: Some(fill),
                            ..DrawableStyle::default()
                        }),
                        ..DrawableCommon::default()
                    },
                    geometry: ShapeGeometry {
                        preset: Some("rect".into()),
                        scalar: None,
                        natural_size: None,
                        path: None,
                        callout: None,
                        point: None,
                    },
                    text: None,
                    vertical_alignment: None,
                    text_insets: None,
                    text_fit: None,
                });
            }
            if let Some(t) = t {
                td.extend(t.drawables.iter().cloned());
                for ph in &t.placeholders {
                    if !fp.drawables.iter().any(|d| same_geometry(d, &ph.drawable)) {
                        td.push(ph.drawable.clone());
                    }
                }
            }
            if !td.is_empty() {
                fp.template_drawables = Some(td);
            }
        }
    }

    // Footnote placement: TP.SettingsArchive.footnote_kind (field 30 [proto:
    // TPArchives.proto]) — 0 kFootnoteKindFootnotes (page-bottom, the
    // default → omitted), 1 document endnotes, 2 section endnotes.
    let footnote_placement = root
        .reference(7)
        .and_then(|sid| ctx.loaded.msg(sid))
        .and_then(|s| s.varint(30))
        .and_then(|v| match v {
            1 => Some(FootnotePlacement::DocumentEndnotes),
            2 => Some(FootnotePlacement::SectionEndnotes),
            _ => None,
        });

    PagesDocument {
        footnote_placement,
        kind: "pages".to_string(),
        flavor,
        meta: ctx.meta.clone(),
        warnings: Vec::new(),
        fonts: Vec::new(),
        media: Vec::new(),
        styles: StylePools::default(),
        page_size,
        page_margins,
        orientation,
        page_scale,
        body,
        hidden_body,
        footnotes: if footnotes.is_empty() {
            None
        } else {
            Some(footnotes)
        },
        floating,
        page_templates,
        sections,
        table_of_contents,
        comments,
        bookmarks,
        changes,
    }
    .with_locale(locale)
}

/// Paragraph index containing a UTF-16 offset in a storage text buffer:
/// paragraphs are newline-separated, so it is the newline count before the
/// offset (docs/format/text.md §Paragraph model).
/// Paragraph index of a body offset. Paragraphs end at newlines AND at the
/// U+0004 section-break / U+0005 page-break markers (text.rs splits on
/// all three); a section whose offset sits ON its break marker starts with
/// the paragraph after it.
fn para_index_at(text: &str, utf16_off: u64) -> u32 {
    let is_break = |c: char| c == '\n' || c == '\u{4}' || c == '\u{5}';
    let mut acc = 0u64;
    let mut para = 0u32;
    for ch in text.chars() {
        if acc >= utf16_off {
            if ch == '\u{4}' || ch == '\u{5}' {
                para += 1;
            }
            break;
        }
        acc += ch.len_utf16() as u64;
        if is_break(ch) {
            para += 1;
        }
    }
    para
}

/// A page drawable supersedes a template placeholder when it sits at the
/// same position with the same size (±1pt) [inferred heuristic — the format
/// links them via UUIDs we do not model].
fn same_geometry(a: &Drawable, b: &Drawable) -> bool {
    fn common(d: &Drawable) -> Option<&DrawableCommon> {
        match d {
            Drawable::Shape { common, .. } | Drawable::Textbox { common, .. } => Some(common),
            _ => None,
        }
    }
    let (Some(ca), Some(cb)) = (common(a), common(b)) else {
        return false;
    };
    let close = |x: f64, y: f64| (x - y).abs() <= 1.0;
    match (&ca.position, &cb.position, &ca.size, &cb.size) {
        (Some(pa), Some(pb), Some(sa), Some(sb)) => {
            close(pa.x, pb.x)
                && close(pa.y, pb.y)
                && close(sa.width, sb.width)
                && close(sa.height, sb.height)
        }
        _ => false,
    }
}

impl PagesDocument {
    fn with_locale(mut self, locale: Option<String>) -> PagesDocument {
        self.meta.locale = locale;
        self
    }
}

/// TP.PageTemplateArchive { name = 1, section_template_drawables = 2,
/// placeholder_drawables = 3 (TagDrawablePair { tag = 1, drawable = 2,
/// z_index = 3 }), headers_footers_match_previous_page = 4,
/// hide_headers_footers = 5, background_fill = 6 }.
fn convert_page_template(ctx: &mut Ctx, tid: u64, index: usize) -> (PageTemplate, Option<String>) {
    let Some(m) = ctx.loaded.msg(tid).cloned() else {
        ctx.warn_detail(
            WarningCode::UnresolvedReference,
            format!("page template reference {tid} points nowhere"),
            tid.to_string(),
        );
        return (empty_template(), None);
    };

    // Field 1 is a NAME string on TP.PageTemplateArchive but a header-storage
    // REFERENCE on the 10143 PageMasterArchive layout — a ref misread as a
    // lossy string carries control chars / U+FFFD, so filter those out.
    let name = m
        .string(1)
        .filter(|s| !s.is_empty())
        .filter(|s| !s.chars().any(|c| (c as u32) < 0x20 || c == '\u{FFFD}'));

    // On the PageMasterArchive layout fields 1/2 are header/footer STORAGE
    // refs and field 3 is `master_drawables` (page furniture such as a
    // rotated "DRAFT" watermark box — fixture b31db822, a Pages 5.x doc
    // that 15.3.1 still renders with the watermark on every page)
    // [proto: .scratch/iwork/proto/TPArchives.proto → TP.PageMasterArchive].
    // PageTemplateArchive keeps its drawables in field 2.
    let is_master = ctx.loaded.record(tid).map(|r| r.type_id) == Some(10143);

    let drawables: Vec<Drawable> = m
        .references(if is_master { 3 } else { 2 })
        .into_iter()
        .map(|d| crate::drawables::convert_drawable(ctx, d))
        .collect();

    let mut placeholders = Vec::new();
    for pair in m.msgs(3) {
        let Some(tag) = pair.string(1) else { continue };
        let Some(did) = pair.reference(2) else {
            continue;
        };
        let drawable = crate::drawables::convert_drawable(ctx, did);
        // Record the tag as the placeholder role on the drawable itself.
        let drawable = tag_drawable(drawable, &tag);
        placeholders.push(PagePlaceholder {
            tag,
            drawable,
            z_index: pair.varint(3).map(|v| v as u32),
        });
    }

    // Headers/footers: the 15.3.1 extraction carries no explicit field on
    // PageTemplateArchive (headers/footers belong to the
    // SectionTemplateArchive lineage — docs/format/pages.md field notes), so
    // scan unknown fields (≥ 8) for storage references as a fixture-driven
    // best effort [inferred]. HEADER-kind (1) storages are headers; others
    // are footers.
    // For the 10143 TP.PageMasterArchive layout (fresh 26.3 docs): headers
    // = field 1, footers = field 2 (both direct TSWP.StorageArchive refs,
    // three column storages each — left/center/right [inferred, fixture G5]).
    // For the TP.PageTemplateArchive layout, headers/footers are at f>=8
    // [inferred]. Dispatch on the record's type id.
    let (headers, footers) = if !is_master {
        // Headers/footers via template_headers_footers (PageTemplateArchive)
        template_headers_footers(ctx, &m)
    } else {
        // Direct header/footer storage refs (PageMasterArchive 10143):
        // field 1 = headers (repeated), field 2 = footers (repeated)
        let mut hdr_ids = Vec::new();
        let mut ftr_ids = Vec::new();
        for f in &m.fields {
            if let iwadump::proto::Value::Bytes(b) = &f.value {
                if let Some(inner) = Msg::parse(b) {
                    if let Some(id) = inner.varint(1) {
                        if matches!(
                            ctx.loaded.record(id).map(|r| r.type_id),
                            Some(2001) | Some(2005)
                        ) {
                            if f.number == 1 {
                                hdr_ids.push(id);
                            } else if f.number == 2 {
                                ftr_ids.push(id);
                            }
                        }
                    }
                }
            }
        }
        let to_styled = |ids: Vec<u64>, ctx: &mut Ctx| -> Vec<StyledText> {
            ids.into_iter()
                .filter_map(|sid| crate::text::extract(ctx, sid).map(|e| e.text))
                .collect()
        };
        (to_styled(hdr_ids, ctx), to_styled(ftr_ids, ctx))
    };

    // The resolved name doubles as the section-lookup key (sections refer to
    // templates by name in the model), so unnamed masters get a stable
    // index-based name on the template itself too.
    let resolved_name = name.or_else(|| Some(format!("Template {}", index + 1)));
    (
        PageTemplate {
            name: resolved_name.clone(),
            drawables,
            placeholders,
            background_fill: m.msg(6).and_then(|f| crate::tsd::fill_of(ctx, &f)),
            hide_headers_footers: m.boolean(5),
            headers,
            footers,
            headers_footers_match_previous_page: m.boolean(4).unwrap_or(false),
        },
        resolved_name,
    )
}

fn empty_template() -> PageTemplate {
    PageTemplate {
        name: None,
        drawables: Vec::new(),
        placeholders: Vec::new(),
        background_fill: None,
        hide_headers_footers: None,
        headers: Vec::new(),
        footers: Vec::new(),
        headers_footers_match_previous_page: false,
    }
}

/// Mark a template placeholder drawable with its tag as role.
fn tag_drawable(d: Drawable, tag: &str) -> Drawable {
    match d {
        Drawable::Shape {
            common,
            geometry,
            text,
            vertical_alignment,
            text_insets,
            text_fit,
        } => {
            let mut common = common;
            common.placeholder = Some(PlaceholderInfo {
                role: tag.to_string(),
                inherited: None,
            });
            Drawable::Shape {
                common,
                geometry,
                text,
                vertical_alignment,
                text_insets,
                text_fit,
            }
        }
        Drawable::Textbox {
            common,
            text,
            vertical_alignment,
            text_insets,
            text_fit,
            natural_size,
            flow,
        } => {
            let mut common = common;
            common.placeholder = Some(PlaceholderInfo {
                role: tag.to_string(),
                inherited: None,
            });
            Drawable::Textbox {
                common,
                text,
                vertical_alignment,
                text_insets,
                text_fit,
                natural_size,
                flow,
            }
        }
        other => other,
    }
}

/// Best-effort header/footer storages for a page template.
fn template_headers_footers(ctx: &mut Ctx, m: &Msg) -> (Vec<StyledText>, Vec<StyledText>) {
    let mut header_ids: Vec<u64> = Vec::new();
    let mut other_ids: Vec<u64> = Vec::new();
    for f in &m.fields {
        if f.number < 8 {
            continue;
        }
        let b = match &f.value {
            iwadump::proto::Value::Bytes(b) => b,
            _ => continue,
        };
        let Some(inner) = Msg::parse(b) else { continue };
        let Some(id) = inner.varint(1) else { continue };
        let Some(rec) = ctx.loaded.record(id) else {
            continue;
        };
        if !matches!(rec.type_id, ids::STORAGE | ids::STORAGE_ALT) {
            continue;
        }
        let kind = ctx.loaded.msg(id).and_then(|sm| sm.varint(1)).unwrap_or(3);
        if kind == 1 {
            header_ids.push(id);
        } else {
            other_ids.push(id);
        }
    }
    let to_styled = |ids: Vec<u64>, ctx: &mut Ctx| -> Vec<StyledText> {
        ids.into_iter()
            .filter_map(|sid| crate::text::extract(ctx, sid).map(|e| e.text))
            .collect()
    };
    (to_styled(header_ids, ctx), to_styled(other_ids, ctx))
}

/// TP.SectionArchive active fields (docs/format/pages.md field notes:
/// templates 23-25, numbering 20-22, inherit 17, name 26, background 30).
fn convert_section(
    ctx: &mut Ctx,
    sec_id: u64,
    template_names: &std::collections::HashMap<u64, String>,
    _index: usize,
    page_templates: &mut [PageTemplate],
) -> PagesSection {
    let Some(m) = ctx.loaded.msg(sec_id).cloned() else {
        ctx.warn_detail(
            WarningCode::UnresolvedReference,
            format!("section reference {sec_id} points nowhere"),
            sec_id.to_string(),
        );
        return PagesSection::default();
    };
    let name_of =
        |r: Option<u64>| -> Option<String> { r.and_then(|id| template_names.get(&id).cloned()) };
    // section_template_first_page_hides_header_footer (28 [proto:
    // TPArchives.proto SectionArchive]) — G5: Apple's export shows no
    // header/footer on page 1. Surface it as hide_headers_footers on the
    // FIRST-page template so the model needs no new field.
    if m.boolean(28) == Some(true) {
        if let Some(first_name) = name_of(m.reference(23)) {
            for t in page_templates.iter_mut() {
                if t.name.as_deref() == Some(first_name.as_str()) {
                    t.hide_headers_footers = Some(true);
                }
            }
        }
    }
    let page_numbering = if m.has(20) || m.has(21) || m.has(22) {
        Some(PageNumbering {
            restart: m.varint(20).map(|v| v != 0),
            start_at: m.int(22).map(|v| v as f64),
            first_page_number_kind: m.varint(21).map(|v| match v {
                1 => FirstPageNumberKind::RestartAt,
                2 => FirstPageNumberKind::FromPrevious,
                _ => FirstPageNumberKind::Continue,
            }),
        })
    } else {
        None
    };
    // section_template_first_page_different (18) and
    // section_template_even_odd_pages_different (19) [proto]: when a flag
    // is explicitly false the section still references a first/even master
    // (26a356dc keeps a template-era header "6 JANUARY 2026 · CURABITUR
    // LEO" on its unused first-page master) but Pages lays every page out
    // with the odd master. Resolve the flags here so the viewer never sees
    // the unused master.
    let odd = name_of(m.reference(25));
    let first_page_template = if m.boolean(18) == Some(false) {
        None
    } else {
        name_of(m.reference(23))
    };
    let even_page_template = if m.boolean(19) == Some(false) {
        odd.clone()
    } else {
        name_of(m.reference(24))
    };
    PagesSection {
        columns: None,
        name: m.string(26),
        first_page_template,
        even_page_template,
        odd_page_template: odd,
        page_numbering,
        inherit_previous_header_footer: m.boolean(17),
        background_fill: m.msg(30).and_then(|f| crate::tsd::fill_of(ctx, &f)),
        body_paragraph_start: None,
    }
}
