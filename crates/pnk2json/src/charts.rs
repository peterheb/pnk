//! TSCH chart conversion: `TSCH.ChartArchive` (carried in the unity extension
//! field 10000 of the chart drawable) → ChartModel with inline grid data
//! (docs/format/charts.md, model-design §2.7). Rendering is deferred to the
//! viewer; only type + data + minimal hints are modeled. TSCE-mediated table
//! bindings stay opaque placeholders.

use crate::ctx::Ctx;
use crate::model::*;
use crate::pb::Msg;

/// Map the ~27-value TSCH.ChartType enum onto the normalized ChartType union;
/// 2D and 3D variants collapse, with `three_d` carrying the distinction.
fn chart_type(v: u64) -> (ChartType, bool) {
    match v {
        1 => (ChartType::Column, false),
        2 => (ChartType::Bar, false),
        3 => (ChartType::Line, false),
        4 => (ChartType::Area, false),
        5 => (ChartType::Pie, false),
        6 => (ChartType::StackedColumn, false),
        7 => (ChartType::StackedBar, false),
        8 => (ChartType::StackedArea, false),
        9 => (ChartType::Scatter, false),
        12 => (ChartType::Column, true),
        13 => (ChartType::Bar, true),
        14 => (ChartType::Line, true),
        15 => (ChartType::Area, true),
        16 => (ChartType::Pie, true),
        17 => (ChartType::StackedColumn, true),
        18 => (ChartType::StackedBar, true),
        19 => (ChartType::StackedArea, true),
        22 => (ChartType::Bubble, false),
        23 => (ChartType::Scatter, false),
        24 => (ChartType::Bubble, false),
        25 => (ChartType::Donut, false),
        26 => (ChartType::Donut, true),
        27 => (ChartType::Radar, false),
        // mixed / two-axis / multi-data variants and anything new: "other"
        _ => (ChartType::Other, (12..=19).contains(&v) || v == 26),
    }
}

pub fn convert_chart(ctx: &mut Ctx, ca: &Msg) -> ChartModel {
    let (ctype, three_d) = ca
        .varint(1)
        .map(chart_type)
        .unwrap_or((ChartType::Other, false));

    let legend_frame = ca.msg(3).and_then(|r| {
        Some(Rect {
            x: r.point(1)?.0,
            y: r.point(1)?.1,
            width: r.size(2)?.0,
            height: r.size(2)?.1,
        })
    });

    let scatter_format = ca.varint(2).map(|v| match v {
        1 => ChartScatterFormat::SeparateX,
        _ => ChartScatterFormat::SharedX,
    });

    // Inline grid (TSCH.ChartGridArchive): row_name = 1, column_name = 2,
    // grid_row = 3 with GridRow.value = repeated GridValue.
    let grid = ca.msg(7);
    let series_direction = ca.varint(5).unwrap_or(0); // by_row = 1, by_column = 2

    let (categories, series) = match &grid {
        Some(g) => extract_grid(g, series_direction),
        None => (Vec::new(), Vec::new()),
    };

    let data_status = if grid.is_some() && !series.is_empty() {
        ChartDataStatus::Inline
    } else if ca.has(8) {
        // Mediator present: Numbers table-bound chart (charts.md §mediator).
        ChartDataStatus::TableBound
    } else if grid.is_some() {
        ChartDataStatus::Inline
    } else {
        ChartDataStatus::Unavailable
    };

    // Numbers table-bound charts: dataBinding placeholder (opaque).
    let data_binding = if data_status == ChartDataStatus::TableBound {
        let id = ca
            .reference(8)
            .map(|mid| {
                // TN.ChartMediatorArchive extends the TSCH mediator and adds
                // the binding formulas (docs/format/charts.md).
                ctx.loaded
                    .msg(mid)
                    .and_then(|mm| mm.msg(3))
                    .and_then(|fs| fs.msg(1))
                    .map(|f| format!("formula:{}", f.varint(1).unwrap_or(0)))
                    .unwrap_or_else(|| format!("mediator:{mid}"))
            })
            .unwrap_or_else(|| "mediator".to_string());
        Some(TsceFormulaRef::unparsed(id))
    } else {
        None
    };

    let series_colors = series_colors(ctx, ca, ctype, series.len());

    // Titles/axes live on the NON-style archives, in the Generated
    // extension at field 10000 (TSCHArchives.GEN.proto): chart_non_style
    // (ref 10) → ChartNonStyleArchive { showtitle = 35, title = 46,
    // showlegend = 34 }; value_axis_nonstyles (14) / category_axis_nonstyles
    // (16) → ChartAxisNonStyleArchive { showtitle 13/14, title 15/16,
    // usermax 17, usermin 18, majorgridlines 5 }. Burndown's "User
    // Stories" chart stores its title here; Apple draws it above the plot.
    let ext = |ctx: &Ctx, id: Option<u64>| -> Option<Msg> {
        id.and_then(|i| ctx.loaded.msg(i)).and_then(|m| m.msg(10000))
    };
    // The Generated ChartNonStyleArchive keeps the chart-level values in
    // its "default" slots: showlegend = 20, showtitle = 21, title = 23
    // (fixture-verified: burndown's "User Stories" sits at 23 with 21 = 1;
    // the 34/35/46 numbers belong to ChartGenericPropertyMapArchive).
    let (mut title, mut legend_visible, mut inner_radius) = (None, None, None);
    if let Some(ns) = ext(ctx, ca.reference(10)) {
        if ns.boolean(21) != Some(false) {
            title = ns.string(23).filter(|t| !t.trim().is_empty());
        }
        legend_visible = ns.boolean(20);
        inner_radius = ns.f32v(27).map(|v| v as f64).filter(|v| *v > 0.0 && *v < 1.0);
    }
    let axis_title = |ctx: &Ctx, ids: Vec<u64>, show: u32, field: u32| -> Option<String> {
        ids.into_iter()
            .filter_map(|id| ext(ctx, Some(id)))
            .find_map(|m| {
                if m.boolean(show) == Some(false) {
                    return None;
                }
                m.string(field).filter(|t| !t.trim().is_empty())
            })
    };
    let value_axis_title = axis_title(ctx, ca.references(14), 14, 16);
    let category_axis_title = axis_title(ctx, ca.references(16), 13, 15);
    let value_ext = ca.references(14).into_iter().find_map(|id| ext(ctx, Some(id)));
    let user_bound = |m: &Msg, f: u32| m.msg(f).and_then(|n| n.f64v(1).or_else(|| n.f32v(1).map(|v| v as f64)));
    let value_axis_max = value_ext.as_ref().and_then(|m| user_bound(m, 17));
    let value_axis_min = value_ext.as_ref().and_then(|m| user_bound(m, 18));
    let value_axis_major_gridlines = value_ext.as_ref().and_then(|m| m.varint(5)).map(|v| v as u32);

    ChartModel {
        r#type: ctype,
        three_d,
        data_status,
        categories,
        series,
        legend_frame,
        legend_visible,
        series_colors,
        data_binding,
        scatter_format,
        title,
        category_axis_title,
        value_axis_title,
        value_axis_min,
        value_axis_max,
        value_axis_major_gridlines,
        inner_radius,
    }
}

/// Per-series display colors from the TSCH series style archives.
///
/// `ChartArchive.series_private_styles = 18` (TSP.SparseReferenceArray keyed
/// by series index) overrides `series_theme_styles = 17` (repeated reference,
/// cycled). Each resolves to a `TSCH.ChartSeriesStyleArchive` whose properties
/// live in the Generated extension at field 10000
/// (`TSCH.Generated.ChartSeriesStyleArchive`, TSCHArchives.GEN.proto) — the
/// color slot is per chart type: line takes `linestroke = 48`, column
/// `columnfill = 13`, bar `barfill = 12`, area `areafill = 11`, pie/donut
/// `piefill = 17`, scatter `scattersymbolfill = 59` / `scatterstroke = 53`,
/// bubble `bubblesymbolfill = 55`, radar `radarareastroke = 172` /
/// `radarareafill = 165`, fallback `defaultfill = 14`. Absent locally, the
/// TSS parent chain (super field 1 → parent ref 3) supplies it.
/// Fixture-verified: 01_Running_Log pace line = #ff9e41 orange from the
/// private style's linestroke. [proto + inferred]
fn series_colors(ctx: &mut Ctx, ca: &Msg, ctype: ChartType, n: usize) -> Option<Vec<HexColor>> {
    if n == 0 {
        return None;
    }
    // (generated-ext field, is_stroke) in priority order, per chart type.
    let slots: &[(u32, bool)] = match ctype {
        ChartType::Line => &[(48, true), (14, false)],
        ChartType::Column | ChartType::StackedColumn => &[(13, false), (14, false)],
        ChartType::Bar | ChartType::StackedBar => &[(12, false), (14, false)],
        ChartType::Area | ChartType::StackedArea => &[(11, false), (14, false)],
        ChartType::Pie | ChartType::Donut => &[(17, false), (14, false)],
        ChartType::Scatter => &[(59, false), (53, true), (48, true), (14, false)],
        ChartType::Bubble => &[(55, false), (14, false)],
        ChartType::Radar => &[(172, true), (165, false), (14, false)],
        ChartType::Other => &[(14, false)],
    };

    let theme: Vec<u64> = ca.references(17);
    // Sparse array: entries { index = 1, reference = 2 }.
    let mut private: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    if let Some(sparse) = ca.msg(18) {
        for e in sparse.msgs(2) {
            if let (Some(idx), Some(r)) = (e.varint(1), e.reference(2)) {
                private.insert(idx, r);
            }
        }
    }

    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let style_id = private
            .get(&(i as u64))
            .copied()
            .or_else(|| (!theme.is_empty()).then(|| theme[i % theme.len()]))?;
        out.push(series_style_color(ctx, style_id, slots)?);
    }
    Some(out)
}

/// Walk a series style's TSS parent chain looking for the first of `slots`
/// carried in the Generated ext (10000); stroke slots read
/// `TSD.StrokeArchive.color = 1`, fill slots go through `tsd::fill_of` (solid
/// color, or a gradient's first stop).
fn series_style_color(ctx: &mut Ctx, style_id: u64, slots: &[(u32, bool)]) -> Option<HexColor> {
    let mut exts = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut cur = Some(style_id);
    while let Some(sid) = cur {
        if !seen.insert(sid) {
            break;
        }
        let Some(m) = ctx.loaded.msg(sid) else { break };
        if let Some(ext) = m.msg(10000) {
            exts.push(ext);
        }
        cur = m.msg(1).and_then(|sup| sup.reference(3));
    }
    for &(field, is_stroke) in slots {
        for ext in &exts {
            let Some(payload) = ext.msg(field) else {
                continue;
            };
            if is_stroke {
                if let Some(c) = payload.msg(1) {
                    let mut warns = Vec::new();
                    if let Some(hex) = crate::colors::color_hex(&c, &mut |r| warns.push(r)) {
                        for w in warns {
                            ctx.warn(WarningCode::ColorDegraded, w);
                        }
                        return Some(hex);
                    }
                }
            } else if let Some(fill) = crate::tsd::fill_of(ctx, &payload) {
                match fill {
                    Fill::Solid { color } => return Some(color),
                    Fill::Gradient { gradient } => {
                        if let Some(stop) = gradient.stops.first() {
                            return Some(stop.color.clone());
                        }
                    }
                    Fill::Image { tint, .. } => {
                        if let Some(t) = tint {
                            return Some(t);
                        }
                    }
                }
            }
        }
    }
    None
}

/// Grid → (categories, series). Role assignment follows `series_direction`
/// (by_row = 1 / by_column = 2; docs/format/charts.md §inline grid):
/// - by_row: rows are series, row_name = series labels, column_name =
///   category labels.
/// - by_column: columns are series, column_name = series labels, row_name =
///   category labels.
fn extract_grid(g: &Msg, series_direction: u64) -> (Vec<String>, Vec<ChartSeries>) {
    let row_names: Vec<String> = strings(g, 1);
    let col_names: Vec<String> = strings(g, 2);
    let rows: Vec<Vec<GridVal>> = g
        .msgs(3)
        .into_iter()
        .map(|r| {
            r.msgs(1)
                .into_iter()
                .map(|v| {
                    if let Some(n) = v.f64v(1) {
                        GridVal::Num(n)
                    } else if let Some(d) = v.f64v(2).or_else(|| v.f64v(4)) {
                        GridVal::Date(crate::colors::iso_from_apple_seconds(d))
                    } else if let Some(d) = v.f64v(3) {
                        GridVal::Num(d)
                    } else {
                        GridVal::Hole
                    }
                })
                .collect()
        })
        .collect();

    let to_series = |values: Vec<GridVal>| -> Vec<Option<ChartValue>> {
        values
            .into_iter()
            .map(|v| match v {
                GridVal::Num(n) => Some(ChartValue::Number(n)),
                GridVal::Date(d) => Some(ChartValue::Date(d)),
                GridVal::Hole => None,
            })
            .collect()
    };

    match series_direction {
        1 => {
            // by_row: each grid row is a series.
            let categories = col_names;
            let series = rows
                .iter()
                .enumerate()
                .map(|(i, vals)| ChartSeries {
                    name: row_names.get(i).cloned(),
                    values: to_series(vals.clone()),
                })
                .collect();
            (categories, series)
        }
        _ => {
            // by_column (default when direction unknown): each grid column is
            // a series; row_name holds the category labels.
            let categories = row_names;
            let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
            let mut series = Vec::new();
            for c in 0..ncols {
                let values: Vec<GridVal> = rows
                    .iter()
                    .map(|r| r.get(c).cloned().unwrap_or(GridVal::Hole))
                    .collect();
                series.push(ChartSeries {
                    name: col_names.get(c).cloned(),
                    values: to_series(values),
                });
            }
            (categories, series)
        }
    }
}

fn strings(m: &Msg, field: u32) -> Vec<String> {
    m.all(field)
        .into_iter()
        .filter_map(|v| match v {
            iwadump::proto::Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
            _ => None,
        })
        .collect()
}

#[derive(Debug, Clone)]
enum GridVal {
    Num(f64),
    Date(String),
    Hole,
}
