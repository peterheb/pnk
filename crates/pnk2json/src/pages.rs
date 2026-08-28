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
    // Locale via the TSA super chain (super = 15 → TSA.DocumentArchive
    // super = 1 → TSK.DocumentArchive.locale_identifier = 4).
    let locale = root
        .reference(15)
        .and_then(|tsa| ctx.loaded.msg(tsa))
        .and_then(|tsa| tsa.reference(1))
        .and_then(|tsk| ctx.loaded.msg(tsk))
        .and_then(|tsk| tsk.string(4));

    let flavor = if root.has(4) {
        PagesFlavor::WordProcessing
    } else {
        PagesFlavor::PageLayout
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

    let orientation = root.varint(42).map(|v| {
        if v != 0 {
            PageLayoutOrientation::Landscape
        } else {
            PageLayoutOrientation::Portrait
        }
    });
    let page_scale = root.f32v(38).map(|v| v as f64);

    // Page templates (masters).
    let template_ids = root.references(48);
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
                    page_index: pg.varint(1).map(|v| v as u32),
                    drawables: converted.into_iter().map(|(_, d)| d).collect(),
                });
            }
        }
    }

    // Word-processing body + footnotes.
    let mut body = None;
    let mut footnotes: Vec<Footnote> = Vec::new();
    if flavor == PagesFlavor::WordProcessing {
        if let Some(bsid) = root.reference(4) {
            if let Some(ex) = crate::text::extract(ctx, bsid) {
                body = Some(ex.text);
                for (para_idx, ftext) in ex.footnotes {
                    footnotes.push(Footnote { anchor_paragraph_index: para_idx, text: ftext });
                }
            }
        }
    }

    // Sections (TP.SectionArchive [10011]).
    let mut sections = Vec::new();
    if let Some(sec_id) = root.reference(5) {
        sections.push(convert_section(ctx, sec_id, &template_names));
    }

    PagesDocument {
        kind: "pages".to_string(),
        flavor,
        meta: ctx.meta.clone(),
        warnings: Vec::new(),
        fonts: Vec::new(),
        media: Vec::new(),
        page_size,
        page_margins,
        orientation,
        page_scale,
        body,
        footnotes: if footnotes.is_empty() { None } else { Some(footnotes) },
        floating,
        page_templates,
        sections,
        table_of_contents: None,
    }
    .with_locale(locale)
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

    let name = m.string(1).filter(|s| !s.is_empty());

    let drawables: Vec<Drawable> = m
        .references(2)
        .into_iter()
        .map(|d| crate::drawables::convert_drawable(ctx, d))
        .collect();

    let mut placeholders = Vec::new();
    for pair in m.msgs(3) {
        let Some(tag) = pair.string(1) else { continue };
        let Some(did) = pair.reference(2) else { continue };
        let drawable = crate::drawables::convert_drawable(ctx, did);
        // Record the tag as the placeholder role on the drawable itself.
        let drawable = tag_drawable(drawable, &tag);
        placeholders.push(PagePlaceholder { tag, drawable, z_index: pair.varint(3).map(|v| v as u32) });
    }

    // Headers/footers: the 15.3.1 extraction carries no explicit field on
    // PageTemplateArchive (headers/footers belong to the
    // SectionTemplateArchive lineage — docs/format/pages.md field notes), so
    // scan unknown fields (≥ 8) for storage references as a fixture-driven
    // best effort [inferred]. HEADER-kind (1) storages are headers; others
    // are footers.
    let (headers, footers) = template_headers_footers(ctx, &m);

    (
        PageTemplate {
            name: name.clone(),
            drawables,
            placeholders,
            background_fill: m.msg(6).and_then(|f| crate::tsd::fill_of(ctx, &f)),
            hide_headers_footers: m.boolean(5),
            headers,
            footers,
            headers_footers_match_previous_page: m.boolean(4).unwrap_or(false),
        },
        name.or_else(|| Some(format!("Template {}", index + 1))),
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
        Drawable::Shape { common, geometry, text, vertical_alignment, text_insets } => {
            let mut common = common;
            common.placeholder =
                Some(PlaceholderInfo { role: tag.to_string(), inherited: None });
            Drawable::Shape { common, geometry, text, vertical_alignment, text_insets }
        }
        Drawable::Textbox { common, text, vertical_alignment, text_insets } => {
            let mut common = common;
            common.placeholder =
                Some(PlaceholderInfo { role: tag.to_string(), inherited: None });
            Drawable::Textbox { common, text, vertical_alignment, text_insets }
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
        let Some(rec) = ctx.loaded.record(id) else { continue };
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
) -> PagesSection {
    let Some(m) = ctx.loaded.msg(sec_id).cloned() else {
        ctx.warn_detail(
            WarningCode::UnresolvedReference,
            format!("section reference {sec_id} points nowhere"),
            sec_id.to_string(),
        );
        return PagesSection::default();
    };
    let name_of = |r: Option<u64>| -> Option<String> {
        r.and_then(|id| template_names.get(&id).cloned())
    };
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
    PagesSection {
        name: m.string(26),
        first_page_template: name_of(m.reference(23)),
        even_page_template: name_of(m.reference(24)),
        odd_page_template: name_of(m.reference(25)),
        page_numbering,
        inherit_previous_header_footer: m.boolean(17),
        background_fill: m.msg(30).and_then(|f| crate::tsd::fill_of(ctx, &f)),
        body_paragraph_start: None,
    }
}
