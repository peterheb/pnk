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
        _ => (ChartType::Other, v >= 12 && v <= 19 || v == 26),
    }
}

pub fn convert_chart(ctx: &mut Ctx, ca: &Msg) -> ChartModel {
    let (ctype, three_d) = ca.varint(1).map(chart_type).unwrap_or((ChartType::Other, false));

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

    ChartModel {
        r#type: ctype,
        three_d,
        data_status,
        categories,
        series,
        legend_frame,
        legend_visible: None,
        series_colors: None,
        data_binding,
        scatter_format,
    }
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
                let values: Vec<GridVal> =
                    rows.iter().map(|r| r.get(c).cloned().unwrap_or(GridVal::Hole)).collect();
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
