//! Numbers conversion: TN.DocumentArchive [1] → sheets → drawables/tables
//! (docs/format/numbers.md, model-design §2.1). A sheet IS the canvas:
//! `TN.SheetArchive.drawable_infos` holds every table/image/chart/shape
//! directly — document → sheets → drawables, one level.

use crate::ctx::Ctx;
use crate::model::*;
use crate::pb::{ids, Msg};

pub fn convert_document(ctx: &mut Ctx, root: &Msg) -> NumbersDocument {
    let locale = ctx.resolve_locale(root);

    let page_size = root.size(12).map(|(w, h)| Size { width: w, height: h });

    let mut sheets = Vec::new();
    let mut forms: Vec<NumbersForm> = Vec::new();
    for sid in root.references(1) {
        let rec = ctx.loaded.record(sid);
        let type_id = rec.map(|r| r.type_id).unwrap_or(0);
        match type_id {
            ids::TN_FORM_BASED_SHEET => {
                // TN.FormBasedSheetArchive { super = 1 (SheetArchive),
                // table_id = 2 (CFUUID) } — recorded, not rendered.
                let form = ctx.loaded.msg(sid).and_then(|m| {
                    m.reference(1).and_then(|super_id| {
                        ctx.loaded.msg(super_id).and_then(|s| {
                            Some(NumbersForm {
                                sheet_name: s.string(1),
                                bound_table_name: m
                                    .msg(2)
                                    .and_then(|u| u.string(1)),
                            })
                        })
                    })
                });
                if let Some(f) = form {
                    forms.push(f);
                }
            }
            ids::TN_SHEET => sheets.push(convert_sheet(ctx, sid)),
            _ => {
                ctx.warn_detail(
                    WarningCode::UnsupportedFeature,
                    format!(
                        "unmodeled sheet-like object (type id {type_id}) on document root"
                    ),
                    format!("0x{type_id:x}"),
                );
            }
        }
    }

    NumbersDocument {
        kind: "numbers".to_string(),
        meta: ctx.meta.clone(),
        warnings: Vec::new(),
        fonts: Vec::new(),
        media: Vec::new(),
        styles: StylePools::default(),
        sheets,
        page_size,
        forms: if forms.is_empty() { None } else { Some(forms) },
    }
    .with_locale(locale)
}

impl NumbersDocument {
    fn with_locale(mut self, locale: Option<String>) -> NumbersDocument {
        self.meta.locale = locale;
        self
    }
}

fn convert_sheet(ctx: &mut Ctx, sid: u64) -> Sheet {
    let Some(m) = ctx.loaded.msg(sid).cloned() else {
        ctx.warn_detail(
            WarningCode::UnresolvedReference,
            format!("sheet reference {sid} points nowhere"),
            sid.to_string(),
        );
        return Sheet {
            name: String::new(),
            hidden: None,
            drawables: Vec::new(),
            headers: None,
            footers: None,
            uses_single_header_footer: None,
            style: None,
            print: None,
            layout_direction_rtl: None,
        };
    };

    let drawables: Vec<Drawable> = m
        .references(2) // drawable_infos, in order (paint order per numbers.md)
        .into_iter()
        .map(|d| crate::drawables::convert_drawable(ctx, d))
        .collect();

    // Headers/footers: fields 18/19 (repeated storage refs); fields 15/16 are
    // the deprecated single-storage forms.
    let headers = storages_to_styled(ctx, &m, 18, 15);
    let footers = storages_to_styled(ctx, &m, 19, 16);

    // Print setup (fields 3-14, docs/format/numbers.md tree).
    let print = {
        let any = m.has(3)
            || m.has(5)
            || m.has(7)
            || m.has(8)
            || m.has(10)
            || m.has(11)
            || m.has(12)
            || m.has(13)
            || m.has(14);
        if any {
            Some(SheetPrintSetup {
                orientation: m.varint(3).map(|v| {
                    if v != 0 {
                        PageLayoutOrientation::Landscape
                    } else {
                        PageLayoutOrientation::Portrait
                    }
                }),
                show_page_numbers: m.boolean(5),
                content_scale: m.f32v(7).map(|v| v as f64),
                page_order: m.varint(8).map(|v| match v {
                    1 => PageOrder::OverThenDown,
                    _ => PageOrder::DownThenOver,
                }),
                margins: m.msg(10).and_then(|e| {
                    Some(EdgeInsets {
                        top: e.f32v(1)? as f64,
                        left: e.f32v(2)? as f64,
                        bottom: e.f32v(3)? as f64,
                        right: e.f32v(4)? as f64,
                    })
                }),
                start_page_number: m.int(12).map(|v| v as f64),
                use_custom_start_page_number: m.boolean(11),
                page_header_inset: m.f32v(13).map(|v| v as f64),
                page_footer_inset: m.f32v(14).map(|v| v as f64),
            })
        } else {
            None
        }
    };

    // Sheet style: TN.SheetStyleArchive (tab color / canvas fill) — field 22.
    // The message shape is not in the local protos; decode best-effort:
    // treat a TSP.Color fill at field 1 and a color at field 2.
    let style = m.reference(22).and_then(|stid| {
        ctx.loaded.msg(stid).map(|st| SheetStyle {
            tab_color: st.msg(1).and_then(|c| {
                let mut w = Vec::new();
                crate::colors::color_hex(&c, &mut |r| w.push(r))
            }),
            fill: st.msg(2).and_then(|c| {
                let mut w = Vec::new();
                crate::colors::color_hex(&c, &mut |r| w.push(r))
            }),
        })
    });

    Sheet {
        name: m.string(1).unwrap_or_default(),
        hidden: m.boolean(25),
        drawables,
        headers,
        footers,
        uses_single_header_footer: m.boolean(20),
        style,
        print,
        layout_direction_rtl: m.int(21).map(|v| v == 1),
    }
}

/// Repeated storage references (field) → StyledText list; falls back to the
/// deprecated single storage (fallback_field) when the list form is absent.
fn storages_to_styled(ctx: &mut Ctx, m: &Msg, field: u32, fallback_field: u32) -> Option<Vec<StyledText>> {
    let ids = m.references(field);
    if !ids.is_empty() {
        let out: Vec<StyledText> = ids
            .into_iter()
            .filter_map(|sid| crate::text::extract(ctx, sid).map(|e| e.text))
            .collect();
        return if out.is_empty() { None } else { Some(out) };
    }
    let fid = m.reference(fallback_field)?;
    let out = crate::text::extract(ctx, fid).map(|e| vec![e.text]);
    out
}
