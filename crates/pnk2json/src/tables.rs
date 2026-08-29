//! TST table conversion: TableModelArchive + DataStore + tiles + data lists →
//! a resolved TableModel (docs/format/tables.md, model-design §2.6). The
//! tile/offset-buffer machinery is fully flattened; only non-empty cells
//! survive, in row-major order. Values are the stored LAST-CALCULATED
//! results; formulas stay opaque (TsceFormulaRef).

use std::collections::HashMap;

use crate::ctx::Ctx;
use crate::model::*;
use crate::pb::Msg;
use crate::styles;

/// One entry of a TST.TableDataList.
#[derive(Debug, Clone, Default)]
struct ListEntry {
    string: Option<String>,
    reference: Option<u64>,
    has_formula: bool,
    format: Option<Msg>,
    custom_format: Option<Msg>,
}

/// A decoded TableDataList (string/style/formula/format tables).
#[derive(Debug, Default)]
struct DataList {
    entries: HashMap<i32, ListEntry>,
}

fn load_data_list(ctx: &Ctx, list_id: Option<u64>) -> DataList {
    let mut out = DataList::default();
    let Some(id) = list_id else { return out };
    let Some(m) = ctx.loaded.msg(id) else { return out };
    for e in m.msgs(3) {
        let key = e.varint(1).unwrap_or(0) as i32;
        out.entries.entry(key).or_insert_with(|| ListEntry {
            string: e.string(3),
            reference: e.reference(4),
            has_formula: e.has(5),
            format: e.msg(6),
            custom_format: e.msg(8),
        });
    }
    // Segmented lists (large tables): segments = 4 → TableDataListSegment
    // { list_type = 1, key_range = 2, entries = 3 }; the key_range location
    // is the key of the segment's first entry.
    for seg_ref in m.references(4) {
        if let Some(seg) = ctx.loaded.msg(seg_ref) {
            let base_key = seg.msg(2).and_then(|r| r.varint(1)).unwrap_or(0) as i32;
            for (i, e) in seg.msgs(3).into_iter().enumerate() {
                let key = base_key + i as i32;
                out.entries.entry(key).or_insert_with(|| ListEntry {
                    string: e.string(3),
                    reference: e.reference(4),
                    has_formula: e.has(5),
                    format: e.msg(6),
                    custom_format: e.msg(8),
                });
            }
        }
    }
    out
}

/// Header buckets → (model index, storage-buffer ordinal) in order
/// (numbers-parser row_storage_map semantics, docs/format/tables.md).
fn strip_map_with_ctx(
    ctx: &Ctx,
    storage: &Msg,
    inline_field: u32,
    ref_field: u32,
) -> Vec<(u32, usize)> {
    // HeaderStorage may be inline (rowHeaders) or referenced (columnHeaders).
    let hs = storage
        .msg(inline_field)
        .or_else(|| storage.reference(ref_field).and_then(|r| ctx.loaded.msg(r).cloned()));
    let Some(hs) = hs else { return Vec::new() };
    let mut out = Vec::new();
    let mut ordinal = 0usize;
    for bref in hs.references(2) {
        if let Some(bucket) = ctx.loaded.msg(bref) {
            for h in bucket.msgs(2) {
                if let Some(idx) = h.varint(1) {
                    out.push((idx as u32, ordinal));
                    ordinal += 1;
                }
            }
        }
    }
    out
}

/// Per-index sizes/hidden flags from header buckets
/// (HeaderStorageBucket.Header { index = 1, size = 2, hidingState = 3 }).
fn header_info(
    ctx: &Ctx,
    storage: Option<&Msg>,
    inline_field: u32,
    ref_field: u32,
) -> Vec<(u32, RowColInfo)> {
    let Some(storage) = storage else { return Vec::new() };
    let hs = storage
        .msg(inline_field)
        .or_else(|| storage.reference(ref_field).and_then(|r| ctx.loaded.msg(r).cloned()));
    let Some(hs) = hs else { return Vec::new() };
    let mut out = Vec::new();
    for bref in hs.references(2) {
        if let Some(bucket) = ctx.loaded.msg(bref) {
            for h in bucket.msgs(2) {
                let Some(idx) = h.varint(1) else { continue };
                out.push((
                    idx as u32,
                    RowColInfo {
                        size_pt: h.f32v(2).map(|v| v as f64),
                        hidden: h.varint(3).map(|v| v != 0),
                    },
                ));
            }
        }
    }
    out.sort_by_key(|(i, _)| *i);
    out
}

fn rows_to_model(info: Vec<(u32, RowColInfo)>, count: u32) -> Option<Vec<RowColInfo>> {
    if info.is_empty() || count == 0 {
        return None;
    }
    let mut out = vec![RowColInfo { size_pt: None, hidden: None }; count as usize];
    for (idx, rc) in info {
        if (idx as usize) < out.len() {
            out[idx as usize] = rc;
        }
    }
    Some(out)
}

pub fn convert_table(ctx: &mut Ctx, model_id: u64) -> TableModel {
    let Some(m) = ctx.loaded.msg(model_id).cloned() else {
        ctx.warn_detail(
            WarningCode::UnresolvedReference,
            format!("table model reference {model_id} points nowhere"),
            model_id.to_string(),
        );
        return empty_table();
    };

    let row_count = m.varint(6).unwrap_or(0) as u32;
    let column_count = m.varint(7).unwrap_or(0) as u32;

    let store = m.msg(4); // base_data_store (inline TST.DataStore)

    // Data lists (docs/format/tables.md §Data lists).
    let string_table = load_data_list(ctx, store.as_ref().and_then(|s| s.reference(4)));
    let style_table = load_data_list(ctx, store.as_ref().and_then(|s| s.reference(5)));
    let formula_table = load_data_list(ctx, store.as_ref().and_then(|s| s.reference(6)));
    let formula_error_table = load_data_list(ctx, store.as_ref().and_then(|s| s.reference(12)));
    let format_table = load_data_list(
        ctx,
        store
            .as_ref()
            .and_then(|s| s.reference(22))
            .or_else(|| store.as_ref().and_then(|s| s.reference(11))),
    );
    let rich_text_table = load_data_list(ctx, store.as_ref().and_then(|s| s.reference(17)));
    let custom_format_table = load_data_list(ctx, store.as_ref().and_then(|s| s.reference(15)));

    // Row/column maps from header buckets.
    let row_map = store
        .as_ref()
        .map(|s| strip_map_with_ctx(ctx, s, 1, 0))
        .unwrap_or_default();
    // Inverse: storage-buffer ordinal → model row.
    let mut ord_to_row: Vec<Option<u32>> = vec![None; row_map.len()];
    for (idx, ord) in &row_map {
        if *ord < ord_to_row.len() {
            ord_to_row[*ord] = Some(*idx);
        }
    }

    let tile_size = store
        .as_ref()
        .and_then(|s| s.msg(3))
        .and_then(|ts| ts.varint(2))
        .filter(|v| *v > 0)
        .unwrap_or(256) as usize;

    let mut cells: Vec<(u32, u32, TableCell, Option<CellFormat>)> = Vec::new();
    let mut saw_pre_bnc = false;
    let mut buffer_ordinal = 0usize;

    if let Some(tiles) = store.as_ref().and_then(|s| s.msg(3)) {
        // TST.TileStorage.Tile { tileid = 1, tile = 2 (TSP.Reference) }
        for tw in tiles.msgs(1) {
            let tileid = tw.varint(1).unwrap_or(0) as usize;
            let Some(tref) = tw.reference(2) else { continue };
            let Some(tile) = ctx.loaded.msg(tref).cloned() else { continue };
            convert_tile(
                ctx,
                &tile,
                tileid,
                tile_size,
                &ord_to_row,
                &string_table,
                &style_table,
                &formula_table,
                &formula_error_table,
                &format_table,
                &custom_format_table,
                &rich_text_table,
                &mut cells,
                &mut saw_pre_bnc,
                &mut buffer_ordinal,
            );
        }
    }
    if saw_pre_bnc {
        ctx.warn(
            WarningCode::TableDegraded,
            format!("table model {model_id} contains pre-BNC tile storage; decode is best-effort"),
        );
    }
    cells.sort_by_key(|(r, c, _, _)| (*r, *c));

    // Merges: merge_region_map (DataStore field 13) → MergeRegionMapArchive
    // { cell_range = 1 } with CellRange { origin = CellID, size = TableSize };
    // packedData: column in the high 16 bits, row in the low 16 bits
    // (docs/format/tables.md §Merges).
    let mut merges = Vec::new();
    if let Some(mrm_id) = store.as_ref().and_then(|s| s.reference(13)) {
        if let Some(mrm) = ctx.loaded.msg(mrm_id) {
            for cr in mrm.msgs(1) {
                let origin = cr.msg(1).and_then(|c| c.get(1).and_then(fixed32));
                let size = cr.msg(2).and_then(|s| s.get(1).and_then(fixed32));
                if let (Some(op), Some(sp)) = (origin, size) {
                    let (ocol, orow) = ((op >> 16) as u32, (op & 0xFFFF) as u32);
                    let (scol, srow) = ((sp >> 16) as u32, (sp & 0xFFFF) as u32);
                    merges.push(TableMerge {
                        anchor_row: orow,
                        anchor_column: ocol,
                        row_span: srow.max(1),
                        column_span: scol.max(1),
                    });
                }
            }
        }
    }
    merges.sort_by_key(|m| (m.anchor_row, m.anchor_column));

    // Table-level style: TST.TableStyleArchive { super = 1,
    // table_properties = 11 } plus the style-network role slots on the model
    // (fields 18-21 cell styles, 24-27 text styles; docs/format/tables.md).
    let style = m.reference(3).and_then(|sid| {
        let props = ctx.loaded.msg(sid).and_then(|sm| sm.msg(11));
        props.map(|p| TableStyle {
            banded_rows: p.boolean(1),
            banded_fill: p.msg(2).and_then(|f| crate::tsd::fill_of(ctx, &f)),
            body_cell_style: m
                .reference(18)
                .and_then(|cid| styles::resolve_cell_style(ctx, cid)),
            header_row_cell_style: m
                .reference(19)
                .and_then(|cid| styles::resolve_cell_style(ctx, cid)),
            header_column_cell_style: m
                .reference(20)
                .and_then(|cid| styles::resolve_cell_style(ctx, cid)),
            footer_row_cell_style: m
                .reference(21)
                .and_then(|cid| styles::resolve_cell_style(ctx, cid)),
        })
    });

    // Dense row-major grid with None holes, plus a deduped formats pool;
    // cells reference formats by index. Malformed formats (negative /
    // absurd decimals) were already warned about and dropped inside
    // decode_cell — the cell is emitted with `formatIndex` absent.
    let mut grid: Vec<Vec<Option<TableCell>>> =
        vec![vec![None; column_count as usize]; row_count as usize];
    let mut formats: Vec<CellFormat> = Vec::new();
    for (row, col, mut cell, format) in cells {
        if let Some(f) = format {
            let idx = match formats.iter().position(|e| *e == f) {
                Some(i) => i,
                None => {
                    formats.push(f);
                    formats.len() - 1
                }
            };
            cell.format_index = Some(idx as u32);
        }
        if (row as usize) < grid.len() && (col as usize) < grid[row as usize].len() {
            grid[row as usize][col as usize] = Some(cell);
        }
    }

    TableModel {
        name: {
            let n = m.string(8);
            match n {
                Some(name) if !name.is_empty() && m.boolean(22).unwrap_or(true) => Some(name),
                _ => None,
            }
        },
        row_count,
        column_count,
        header_row_count: m.varint(9).unwrap_or(0) as u32,
        header_column_count: m.varint(10).unwrap_or(0) as u32,
        footer_row_count: m.varint(11).unwrap_or(0) as u32,
        header_rows_frozen: m.boolean(12),
        header_columns_frozen: m.boolean(13),
        rows: rows_to_model(header_info(ctx, store.as_ref(), 1, 0), row_count),
        columns: rows_to_model(header_info(ctx, store.as_ref(), 0, 2), column_count),
        default_row_height_pt: m.f64v(16),
        default_column_width_pt: m.f64v(17),
        grid,
        formats,
        merges,
        style,
    }
}


fn fixed32(v: &iwadump::proto::Value) -> Option<u32> {
    match v {
        iwadump::proto::Value::Fixed32(b) => Some(u32::from_le_bytes(*b)),
        _ => None,
    }
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
        merges: Vec::new(),
        style: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn convert_tile(
    ctx: &mut Ctx,
    tile: &Msg,
    tileid: usize,
    tile_size: usize,
    ord_to_row: &[Option<u32>],
    string_table: &DataList,
    style_table: &DataList,
    formula_table: &DataList,
    formula_error_table: &DataList,
    format_table: &DataList,
    custom_format_table: &DataList,
    rich_text_table: &DataList,
    out: &mut Vec<(u32, u32, TableCell, Option<CellFormat>)>,
    saw_pre_bnc: &mut bool,
    buffer_ordinal: &mut usize,
) {
    for ri in tile.msgs(5) {
        // TST.TileRowInfo: modern BNC fields are cell_storage_buffer = 6 /
        // cell_offsets = 7; pre-BNC tiles (storage_version 4, seen in
        // Numbers 11.x-era documents) keep the data in the *_pre_bnc fields
        // 3/4 with the same offset mechanics but a different cell layout.
        let buffer = ri
            .bytes(6)
            .or_else(|| ri.bytes(3))
            .map(|b| b.to_vec());
        let Some(buffer) = buffer else {
            *saw_pre_bnc = true;
            continue;
        };
        if buffer.is_empty() {
            continue;
        }
        let is_pre_bnc = ri.bytes(6).is_none() && ri.bytes(3).is_some();

        // The k-th rowInfo across all tiles is storage-buffer ordinal k
        // (docs/format/tables.md §Tiles); fall back to tile arithmetic.
        let model_row: u32 = match ord_to_row.get(*buffer_ordinal).copied().flatten() {
            Some(r) => r,
            None => {
                let tri = ri.varint(1).unwrap_or(0) as usize;
                (tileid * tile_size + tri) as u32
            }
        };
        *buffer_ordinal += 1;

        let offsets_raw = ri
            .bytes(7)
            .or_else(|| ri.bytes(4))
            .map(|b| b.to_vec())
            .unwrap_or_default();
        let wide = ri.boolean(8).unwrap_or(false);
        if is_pre_bnc {
            *saw_pre_bnc = true;
        }
        // Packed little-endian signed 16-bit offsets; wide offsets are
        // quarter-offsets (multiply by 4). Negative = absent cell.
        let mut offsets: Vec<i32> = offsets_raw
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]) as i32)
            .collect();
        if wide {
            for o in offsets.iter_mut() {
                *o *= 4;
            }
        }

        for (slot, &off) in offsets.iter().enumerate() {
            if off < 0 {
                continue;
            }
            let off = off as usize;
            if off >= buffer.len() {
                continue;
            }
            // Cell span runs to the next positive offset (or buffer end).
            let mut end = buffer.len();
            for &later in offsets.iter().skip(slot + 1) {
                if later > off as i32 {
                    end = (later as usize).min(buffer.len());
                    break;
                }
            }
            let cell_buf = &buffer[off..end];
            if cell_buf.len() < 12 {
                continue;
            }
            let decoded = if cell_buf[0] == 4 {
                decode_cell_v4(
                    ctx,
                    model_row,
                    slot as u32,
                    cell_buf,
                    string_table,
                    style_table,
                    format_table,
                )
            } else {
                decode_cell(
                    ctx,
                    model_row,
                    slot as u32,
                    cell_buf,
                    string_table,
                    style_table,
                    formula_table,
                    formula_error_table,
                    format_table,
                    custom_format_table,
                    rich_text_table,
                )
            };
            if let Some(t) = decoded {
                out.push(t);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_cell(
    ctx: &mut Ctx,
    row: u32,
    col: u32,
    buf: &[u8],
    string_table: &DataList,
    style_table: &DataList,
    formula_table: &DataList,
    formula_error_table: &DataList,
    format_table: &DataList,
    custom_format_table: &DataList,
    rich_text_table: &DataList,
) -> Option<(u32, u32, TableCell, Option<CellFormat>)> {
    if buf[0] != 5 {
        ctx.warn_detail(
            WarningCode::TableDegraded,
            format!("cell storage version {} unsupported (expected 5)", buf[0]),
            format!("r{row}c{col}"),
        );
        return None;
    }
    let flags = i32::from_le_bytes([buf[8], buf[9], buf[10], buf[11]]);
    let cell_type = buf[1];

    let mut off = 12usize;
    macro_rules! take_n {
        ($n:expr) => {{
            if off + $n <= buf.len() {
                let s = buf[off..off + $n].to_vec();
                off += $n;
                Some(s)
            } else {
                None
            }
        }};
    }
    macro_rules! take_i32 {
        () => {
            take_n!(4).map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
    }
    macro_rules! take_f64 {
        () => {
            take_n!(8).map(|b| f64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]))
        };
    }

    // Payload fields follow the storage-flag order (docs/format/tables.md
    // §Tiles and cell storage; mirrors numbers-parser Cell._from_storage).
    let d128 = if flags & 0x1 != 0 { take_n!(16).map(|b| unpack_decimal128(&b)) } else { None };
    let double = if flags & 0x2 != 0 { take_f64!() } else { None };
    let seconds = if flags & 0x4 != 0 { take_f64!() } else { None };
    let string_id = if flags & 0x8 != 0 { take_i32!() } else { None };
    let rich_id = if flags & 0x10 != 0 { take_i32!() } else { None };
    let cell_style_id = if flags & 0x20 != 0 { take_i32!() } else { None };
    let text_style_id = if flags & 0x40 != 0 { take_i32!() } else { None };
    let _cond_style = if flags & 0x80 != 0 { take_i32!() } else { None };
    let _cond_rule = if flags & 0x100 != 0 { take_i32!() } else { None };
    let formula_id = if flags & 0x200 != 0 { take_i32!() } else { None };
    let _control = if flags & 0x400 != 0 { take_i32!() } else { None };
    let formula_error_id = if flags & 0x800 != 0 { take_i32!() } else { None };
    let _suggestion = if flags & 0x1000 != 0 { take_i32!() } else { None };
    let num_format_id = if flags & 0x2000 != 0 { take_i32!() } else { None };
    let currency_format_id = if flags & 0x4000 != 0 { take_i32!() } else { None };
    let date_format_id = if flags & 0x8000 != 0 { take_i32!() } else { None };
    let duration_format_id = if flags & 0x10000 != 0 { take_i32!() } else { None };
    let text_format_id = if flags & 0x20000 != 0 { take_i32!() } else { None };
    let _bool_format = if flags & 0x40000 != 0 { take_i32!() } else { None };

    // Value by cell type (TST.CellType byte 1; 10 = currency per
    // numbers-parser CURRENCY_CELL_TYPE).
    let value = match cell_type {
        2 | 10 => {
            let v = d128.or(double)?;
            if cell_type == 10 {
                let code = currency_format_id
                    .and_then(|id| format_table.entries.get(&id))
                    .and_then(|e| e.format.as_ref())
                    .and_then(|f| f.string(3));
                CellValue::Currency { value: v, currency_code: code }
            } else {
                CellValue::Number { value: v }
            }
        }
        3 => {
            let sid = string_id?;
            CellValue::Text {
                value: string_table
                    .entries
                    .get(&sid)
                    .and_then(|e| e.string.clone())
                    .unwrap_or_default(),
            }
        }
        5 => {
            let sec = seconds?;
            CellValue::Date { value: crate::colors::iso_from_apple_seconds(sec) }
        }
        6 => CellValue::Bool { value: double.unwrap_or(0.0) > 0.0 },
        7 => CellValue::Duration { value: double? },
        8 => CellValue::Error {
            value: formula_error_id
                .and_then(|id| formula_error_table.entries.get(&id))
                .and_then(|e| e.string.clone())
                .unwrap_or_else(|| "formula error".to_string()),
        },
        9 => {
            let rid = rich_id?;
            let storage_id = rich_text_table
                .entries
                .get(&rid)
                .and_then(|e| e.reference)
                .and_then(|ref_id| ctx.loaded.msg(ref_id))
                .and_then(|p| p.reference(1)); // RichTextPayloadArchive.storage
            match storage_id.and_then(|sid| crate::text::extract(ctx, sid)) {
                Some(ex) => CellValue::Richtext { text: ex.text },
                None => CellValue::Text { value: String::new() },
            }
        }
        0 | 1 => return None, // generic / span cell: no stored value
        4 => match (d128.or(double), seconds, string_id) {
            (Some(v), _, _) => CellValue::Number { value: v },
            (_, Some(sec), _) => {
                CellValue::Date { value: crate::colors::iso_from_apple_seconds(sec) }
            }
            (_, _, Some(sid)) => CellValue::Text {
                value: string_table
                    .entries
                    .get(&sid)
                    .and_then(|e| e.string.clone())
                    .unwrap_or_default(),
            },
            _ => return None,
        },
        _ => {
            ctx.warn_detail(
                WarningCode::TableDegraded,
                format!("unrecognized cell type {cell_type}; cell skipped"),
                format!("r{row}c{col}"),
            );
            return None;
        }
    };

    // Style: per-cell cell_style_id → STYLE list entry.reference →
    // TST.CellStyleArchive; the text style id resolves on top (CellStyle.text).
    let mut style = cell_style_id.and_then(|id| {
        style_table
            .entries
            .get(&id)
            .and_then(|e| e.reference)
            .and_then(|cid| styles::resolve_cell_style(ctx, cid))
    });
    if let Some(s) = style.as_mut() {
        if let Some(tid) = text_style_id {
            if let Some(e) = style_table.entries.get(&tid) {
                if let Some(tref) = e.reference {
                    s.text = Some(styles::resolve_char_style(ctx, tref));
                }
            }
        }
    }

    // Formula placeholder (TSCE stays opaque; model-design §2.8).
    let formula = formula_id.map(|id| TsceFormulaRef::unparsed(id.to_string()));

    // Format hint: per-type format id wins; custom formats degrade to "custom"
    // + raw string (docs/model-design.md §2.6). Malformed formats (negative /
    // absurd decimals, e.g. a -1 int32 sentinel) are dropped with a warning —
    // the cell is emitted unformatted rather than clamped.
    let (format, malformed_format) = pick_format(
        num_format_id,
        currency_format_id,
        date_format_id,
        duration_format_id,
        text_format_id,
        format_table,
        custom_format_table,
    );
    if malformed_format {
        ctx.warn_detail(
            WarningCode::TableDegraded,
            format!(
                "malformed number format (decimals out of range) dropped; cell r{row}c{col} emitted unformatted"
            ),
            format!("r{row}c{col}"),
        );
    }

    Some((row, col, TableCell { value, style, format_index: None, formula }, format))
}

/// decimal128 (binary integer decimal) → f64, per numbers-parser
/// `_unpack_decimal128` (cell.py:1569-1583).
fn unpack_decimal128(b: &[u8]) -> f64 {
    const BIAS: i64 = 0x1820;
    if b.len() < 16 {
        return 0.0;
    }
    let exp = ((((b[15] & 0x7F) as i64) << 7) | ((b[14] >> 1) as i64)) - BIAS;
    let mut mantissa: u128 = (b[14] & 1) as u128;
    for i in (0..=13).rev() {
        mantissa = mantissa * 256 + b[i] as u128;
    }
    let mut v = mantissa as f64;
    if b[15] & 0x80 != 0 {
        v = -v;
    }
    v * 10f64.powi(exp.clamp(-400, 400) as i32)
}

fn pick_format(
    num: Option<i32>,
    currency: Option<i32>,
    date: Option<i32>,
    duration: Option<i32>,
    text: Option<i32>,
    format_table: &DataList,
    custom_format_table: &DataList,
) -> (Option<CellFormat>, bool) {
    let found = [
        (num, CellFormatKind::Number),
        (currency, CellFormatKind::Currency),
        (date, CellFormatKind::Date),
        (duration, CellFormatKind::Duration),
        (text, CellFormatKind::Text),
    ]
    .into_iter()
    .find_map(|(id, kind)| {
        id.and_then(|id| format_table.entries.get(&id))
            .and_then(|e| e.format.clone())
            .map(|f| (f, kind))
    });
    if let Some((f, kind)) = found {
        // Decimal places are a count: negative or absurd values (e.g. a -1
        // int32 sentinel) are malformed — drop the format, never clamp.
        let malformed = f.varint(2).is_some_and(|v| v > 20);
        if malformed {
            return (None, true);
        }
        let decimals = f.varint(2).map(|v| v as u32);
        return (
            Some(CellFormat {
                kind,
                decimals,
                currency_code: f.string(3),
                format_string: f.string(18),
            }),
            false,
        );
    }
    let custom = custom_format_table
        .entries
        .values()
        .find_map(|e| e.custom_format.clone())
        .map(|cf| CellFormat {
            kind: CellFormatKind::Custom,
            decimals: None,
            currency_code: None,
            format_string: cf.string(3).or_else(|| cf.string(18)),
        });
    (custom, false)
}

/// Pre-BNC (storage_version 4) cell block decoder — fixture-verified against
/// Numbers 11.x-era documents (docs/format/gotchas.md #13). Layout differs
/// from BNC v5: the block reads as 32-bit slots
/// `[version|type][?][flags][cell style id][text style id]…` with the value
/// at fixed positions: string keys for text cells at slot 6, f64 seconds
/// (durations/dates/numbers) in the 8 bytes before the trailing slot.
#[allow(clippy::too_many_arguments)]
fn decode_cell_v4(
    ctx: &mut Ctx,
    row: u32,
    col: u32,
    buf: &[u8],
    string_table: &DataList,
    style_table: &DataList,
    format_table: &DataList,
) -> Option<(u32, u32, TableCell, Option<CellFormat>)> {
    if buf.len() < 28 {
        return None;
    }
    let cell_type = buf[1];
    let u32s: Vec<u32> = buf
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    // Style ids (fixture-verified positions: u32[3] varies per cell type =
    // cell style; u32[4] varies per column = text style).
    let cell_style_id = u32s.get(3).copied().map(|v| v as i32);
    let text_style_id = u32s.get(4).copied().map(|v| v as i32);

    // Value at fixed slots: text key = slot 6; numeric value = f64 at
    // [len-12..len-4] (the 8 bytes before the trailing slot).
    let f64_at = |i: usize| -> Option<f64> {
        let b = buf.get(i * 8..i * 8 + 8)?;
        Some(f64::from_le_bytes(b.try_into().unwrap()))
    };

    let value = match cell_type {
        3 => {
            let sid = u32s.get(6).map(|v| *v as i32)?;
            match string_table.entries.get(&sid).and_then(|e| e.string.clone()) {
                Some(s) => CellValue::Text { value: s },
                None => {
                    ctx.warn_detail(
                        WarningCode::TableDegraded,
                        format!("v4 string key {sid} not resolvable in the string table; cell r{row}c{col} dropped"),
                        format!("r{row}c{col}"),
                    );
                    return None;
                }
            }
        }
        7 => CellValue::Duration { value: f64_at(3)? },
        5 => CellValue::Date { value: crate::colors::iso_from_apple_seconds(f64_at(3)?) },
        2 => CellValue::Number { value: f64_at(3)? },
        9 => {
            // Rich-text slot unverified for v4; resolve via the string table
            // fallback, else drop with a warning.
            let sid = u32s.get(6).map(|v| *v as i32)?;
            match string_table.entries.get(&sid).and_then(|e| e.string.clone()) {
                Some(s) => CellValue::Text { value: s },
                None => {
                    ctx.warn_detail(
                        WarningCode::TableDegraded,
                        format!("v4 rich-text cell r{row}c{col} not resolvable; dropped"),
                        format!("r{row}c{col}"),
                    );
                    return None;
                }
            }
        }
        0 | 1 => return None,
        _ => {
            ctx.warn_detail(
                WarningCode::TableDegraded,
                format!("unrecognized v4 cell type {cell_type}; cell skipped"),
                format!("r{row}c{col}"),
            );
            return None;
        }
    };

    // Style via the shared STYLE table; per-cell text style on top.
    let mut style = cell_style_id.and_then(|id| {
        style_table
            .entries
            .get(&id)
            .and_then(|e| e.reference)
            .and_then(|cid| styles::resolve_cell_style(ctx, cid))
    });
    if let Some(s) = style.as_mut() {
        if let Some(tid) = text_style_id {
            if let Some(e) = style_table.entries.get(&tid) {
                if let Some(tref) = e.reference {
                    s.text = Some(styles::resolve_char_style(ctx, tref));
                }
            }
        }
    }

    // Format id (fixture-verified: duration/date cells carry it at slot 5;
    // the FORMAT list holds those keys).
    let format = match cell_type {
        2 | 5 | 7 => u32s.get(5).map(|v| *v as i32).and_then(|id| {
            format_table.entries.get(&id).and_then(|e| {
                e.format.clone().map(|f| CellFormat {
                    kind: match f.varint(1).unwrap_or(0) {
                        257 => CellFormatKind::Currency,
                        258 => CellFormatKind::Percent,
                        261 => CellFormatKind::Date,
                        268 => CellFormatKind::Duration,
                        260 => CellFormatKind::Text,
                        _ => CellFormatKind::Number,
                    },
                    decimals: f.varint(2).filter(|v| *v <= 20).map(|v| v as u32),
                    currency_code: f.string(3),
                    format_string: f.string(18),
                })
            })
        }),
        _ => None,
    };

    Some((row, col, TableCell { value, style, format_index: None, formula: None }, format))
}
