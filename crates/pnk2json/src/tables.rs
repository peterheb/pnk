//! TST table conversion: TableModelArchive + DataStore + tiles + data lists →
//! a resolved TableModel (docs/format/tables.md, model-design §2.6). The
//! tile/offset-buffer machinery is fully flattened; only non-empty cells
//! survive, in row-major order. Values are the stored LAST-CALCULATED
//! results; formulas stay opaque (TsceFormulaRef).

use std::collections::HashMap;

use crate::ctx::{Ctx, StylePool};
use crate::model::{CellFormatKind, CellTypeTag, GridCell, GridPlain, GridValue};
use crate::model::*;
use crate::pb::Msg;
use crate::styles;

/// One entry of a TST.TableDataList.
#[derive(Debug, Clone, Default)]
struct ListEntry {
    string: Option<String>,
    reference: Option<u64>,
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
            reference: e.reference(4).or_else(|| e.reference(9)),
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
                    reference: e.reference(4).or_else(|| e.reference(9)),
                    format: e.msg(6),
                    custom_format: e.msg(8),
                });
            }
        }
    }
    out
}

/// HeaderStorage field 2 entries: buckets may be TSP.References (v5/BNC)
/// or INLINE HeaderStorageBucket messages (v4/pre-BNC, fixture-verified
/// lafs_playlist). Yields each bucket's Msg.
fn header_buckets(ctx: &Ctx, hs: &Msg) -> Vec<Msg> {
    hs.fields
        .iter()
        .filter(|f| f.number == 2)
        .filter_map(|f| match &f.value {
            iwadump::proto::Value::Bytes(b) => {
                // Could be a TSP.Reference { 1: id } or an inline bucket.
                // A bucket has fields 1..6 (hash/size/hiding/ncells/refs);
                // a reference has only field 1 with a plausible id.
                match Msg::parse(b) {
                    Some(inner) => {
                        let n_fields = inner.fields.len();
                        // Reference: exactly one varint field
                        if n_fields == 1 {
                            if let Some(id) = inner.varint(1) {
                                return ctx.loaded.msg(id).cloned();
                            }
                        }
                        // Inline bucket: multiple fields or non-reference f1
                        Some(inner)
                    }
                    None => None,
                }
            }
            _ => None,
        })
        .collect()
}

/// Header buckets → (model index, storage-buffer ordinal) in order
/// (numbers-parser row_storage_map semantics, docs/format/tables.md).
fn strip_map_with_ctx(
    ctx: &Ctx,
    storage: &Msg,
    inline_field: u32,
    ref_field: u32,
) -> Vec<(u32, usize)> {
    let hs = storage
        .msg(inline_field)
        .or_else(|| storage.reference(ref_field).and_then(|r| ctx.loaded.msg(r).cloned()));
    let Some(hs) = hs else { return Vec::new() };
    let mut out = Vec::new();
    let mut ordinal = 0usize;
    for bucket in header_buckets(ctx, &hs) {
        let is_v4_header = bucket
            .get(2)
            .map(|v| matches!(v, iwadump::proto::Value::Fixed32(_)))
            .unwrap_or(false);
        if is_v4_header {
            if let Some(idx) = bucket.varint(1) {
                out.push((idx as u32, ordinal));
                ordinal += 1;
            }
        } else {
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

/// Per-index row/column DEFAULT styles from header buckets
/// (HeaderStorageBucket.Header cell_style = 5 / text_style = 6, proto
/// TSTArchives.proto:276-287). Apple's templates style entire rows this
/// way — 07_Calendar's 48pt orange "September" row carries no per-cell
/// style at all; the look hangs off the ROW header. Applied to cells
/// without an explicit style (cell > row > column precedence).
fn header_styles(
    ctx: &mut Ctx,
    storage: Option<&Msg>,
    inline_field: u32,
    ref_field: u32,
) -> HashMap<u32, TableCellStyle> {
    let Some(storage) = storage else { return HashMap::new() };
    let hs = storage
        .msg(inline_field)
        .or_else(|| storage.reference(ref_field).and_then(|r| ctx.loaded.msg(r).cloned()));
    let Some(hs) = hs else { return HashMap::new() };
    let mut refs: Vec<(u32, Option<u64>, Option<u64>)> = Vec::new();
    for bucket in header_buckets(ctx, &hs) {
        let is_v4_header = bucket
            .get(2)
            .map(|v| matches!(v, iwadump::proto::Value::Fixed32(_)))
            .unwrap_or(false);
        if is_v4_header {
            if let Some(idx) = bucket.varint(1) {
                let (c, t) = (bucket.reference(5), bucket.reference(6));
                if c.is_some() || t.is_some() {
                    refs.push((idx as u32, c, t));
                }
            }
        } else {
            for h in bucket.msgs(2) {
                if let Some(idx) = h.varint(1) {
                    let (c, t) = (h.reference(5), h.reference(6));
                    if c.is_some() || t.is_some() {
                        refs.push((idx as u32, c, t));
                    }
                }
            }
        }
    }
    let mut out = HashMap::new();
    for (idx, c, t) in refs {
        let mut s = c
            .and_then(|cid| styles::resolve_cell_style(ctx, cid))
            .unwrap_or_default();
        if let Some(tid) = t {
            let text = crate::ctx::strip_char_defaults(styles::resolve_char_style(ctx, tid));
            let para = crate::ctx::strip_para_defaults(styles::resolve_para_style(ctx, tid));
            if text != CharStyle::default() && s.text.is_none() {
                s.text = Some(text);
            }
            if para != ParaStyle::default() && s.paragraph.is_none() {
                s.paragraph = Some(para);
            }
        }
        let s = crate::ctx::strip_cell_defaults(s);
        if s != TableCellStyle::default() {
            out.insert(idx, s);
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
    for bucket in header_buckets(ctx, &hs) {
        // v4/pre-BNC: the bucket IS the Header (f1=index, f2=fixed32 size,
        // f3=hiding) — no nested f2 headers. v5/BNC: the bucket contains
        // Headers at f2. Distinguish by f2's wire type: Fixed32 = v4 header
        // field, LEN = v5 bucket nesting.
        let is_v4_header = bucket
            .get(2)
            .map(|v| matches!(v, iwadump::proto::Value::Fixed32(_)))
            .unwrap_or(false);
        if is_v4_header {
            let Some(idx) = bucket.varint(1) else { continue };
            out.push((
                idx as u32,
                RowColInfo {
                    size_pt: bucket.f32v(2).map(|v| v as f64),
                    hidden: bucket.varint(3).map(|v| v != 0),
                },
            ));
        } else {
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

/// u128 from a TSP.UUID ({lower=1, upper=2}) or TSP.CFUUIDArchive
/// ({uuid_w0..w3 = fields 2..5}); both normalize to the same 128-bit value
/// (parser: masaccio/numbers-parser@3238795 numbers_uuid.py).
fn uuid_u128(m: &Msg) -> Option<u128> {
    if let (Some(w2), Some(w3)) = (m.varint(4), m.varint(5)) {
        let w0 = m.varint(2)?;
        let w1 = m.varint(3)?;
        return Some(
            ((w3 as u128) << 96) | ((w2 as u128) << 64) | ((w1 as u128) << 32) | w0 as u128,
        );
    }
    let lower = m.varint(1)?;
    let upper = m.varint(2)?;
    Some(((upper as u128) << 64) | lower as u128)
}

/// Merge rects from TSCE.FormulaOwnerDependenciesArchive (type 4008)
/// records: the HAUNTED_OWNER (kind 35) archive whose formula_owner_uid
/// (f1) equals the table's haunted uid gives the table's base_owner_uid
/// (f12); MERGE_OWNER (kind 5) archives carry the rects in
/// range_dependencies (f5) back_dependency (f2) internal_range_reference
/// (f4) { owner_id = 1, range = 2 {tl_col=1, tl_row=2, br_col=3, br_row=4}},
/// with owner_id resolving to a uuid via the calc engine's (type 4000)
/// dependency_tracker (f2) owner_id_map (f3) map_entry (f1)
/// { internal_owner_id = 1, owner_id = 2 (CFUUID) }.
/// [parser: masaccio/numbers-parser@3238795 model.py:746-771,877-908]
fn dependency_merges(ctx: &Ctx, haunted: u128, rows: u32, cols: u32) -> Vec<TableMerge> {
    let mut base: Option<u128> = None;
    for rec in ctx.loaded.records.values() {
        if rec.type_id != 4008 {
            continue;
        }
        let Some(fo) = rec.msg.as_ref() else { continue };
        if fo.varint(3) != Some(35) {
            continue; // not a HAUNTED_OWNER mapping entry
        }
        if fo.msg(1).as_ref().and_then(uuid_u128) == Some(haunted) {
            base = fo.msg(12).as_ref().and_then(uuid_u128);
            break;
        }
    }
    let Some(base) = base else { return Vec::new() };

    // internal owner id -> uuid (calc engine owner-id map)
    let mut owner_uuid: HashMap<u64, u128> = HashMap::new();
    for rec in ctx.loaded.records.values() {
        if rec.type_id != 4000 {
            continue;
        }
        let Some(ce) = rec.msg.as_ref() else { continue };
        if let Some(map) = ce.msg(2).and_then(|dt| dt.msg(3)) {
            for e in map.msgs(1) {
                if let (Some(id), Some(uuid)) =
                    (e.varint(1), e.msg(2).as_ref().and_then(uuid_u128))
                {
                    owner_uuid.insert(id, uuid);
                }
            }
        }
    }

    let mut merges = Vec::new();
    for rec in ctx.loaded.records.values() {
        if rec.type_id != 4008 {
            continue;
        }
        let Some(fo) = rec.msg.as_ref() else { continue };
        if fo.varint(3) != Some(5) {
            continue; // not a MERGE_OWNER
        }
        let Some(deps) = fo.msg(5) else { continue };
        for bd in deps.msgs(2) {
            let Some(irr) = bd.msg(4) else { continue };
            let owned_by = irr.varint(1).and_then(|id| owner_uuid.get(&id).copied());
            if owned_by != Some(base) {
                continue;
            }
            let Some(range) = irr.msg(2) else { continue };
            let tlc = range.varint(1).unwrap_or(0);
            let tlr = range.varint(2).unwrap_or(0);
            let brc = range.varint(3).unwrap_or(tlc);
            let brr = range.varint(4).unwrap_or(tlr);
            push_merge(&mut merges, tlr, tlc, brr, brc, rows, cols);
        }
    }
    merges
}

/// Overlay explicit edge strokes onto a cell style's borders.
fn merge_borders(s: &mut TableCellStyle, b: CellBorders) {
    let dst = s.borders.get_or_insert(CellBorders {
        top: None,
        right: None,
        bottom: None,
        left: None,
    });
    if b.top.is_some() {
        dst.top = b.top;
    }
    if b.right.is_some() {
        dst.right = b.right;
    }
    if b.bottom.is_some() {
        dst.bottom = b.bottom;
    }
    if b.left.is_some() {
        dst.left = b.left;
    }
}

/// A table stroke, or None when it draws nothing: StrokePatternArchive
/// type 2 = TSDEmptyPattern ("no line" — distinct from solid), or a zero
/// width. [proto TSDArchives.proto StrokePatternArchive]
fn table_stroke(ctx: &mut Ctx, m: &Msg) -> Option<Stroke> {
    if m.msg(6).and_then(|p| p.varint(1)) == Some(2) {
        return None;
    }
    let s = crate::tsd::stroke_of(ctx, m)?;
    if s.width_pt <= 0.0 {
        return None;
    }
    Some(s)
}

/// Merge default grid strokes into a section style's borders (only where
/// the section style doesn't already carry that edge).
fn attach_borders(
    section: &mut Option<TableCellStyle>,
    top: Option<Stroke>,
    right: Option<Stroke>,
    bottom: Option<Stroke>,
    left: Option<Stroke>,
) {
    if top.is_none() && right.is_none() && bottom.is_none() && left.is_none() {
        return;
    }
    let s = section.get_or_insert_with(TableCellStyle::default);
    let b = s.borders.get_or_insert(CellBorders {
        top: None,
        right: None,
        bottom: None,
        left: None,
    });
    if b.top.is_none() {
        b.top = top;
    }
    if b.right.is_none() {
        b.right = right;
    }
    if b.bottom.is_none() {
        b.bottom = bottom;
    }
    if b.left.is_none() {
        b.left = left;
    }
}

/// Per-cell edge stroke overrides from the StrokeSidecarArchive (model
/// f49): left/right COLUMN layers (f4/f5, row_column_index = column, runs
/// range over rows) and top/bottom ROW layers (f6/f7, index = row, runs
/// over columns). Each run { origin = 1, length = 2, stroke = 3 } paints
/// one edge of a contiguous cell range. [proto TSTArchives.proto
/// StrokeSidecarArchive/StrokeLayerArchive; fixture-verified: 02_Invoice
/// black rules above Tax/Total]
fn sidecar_borders(ctx: &mut Ctx, m: &Msg) -> HashMap<(u32, u32), CellBorders> {
    let mut out: HashMap<(u32, u32), CellBorders> = HashMap::new();
    let Some(sc) = m.reference(49).and_then(|id| ctx.loaded.msg(id).cloned()) else {
        return out;
    };
    for (field, edge) in [(4u32, 0u8), (5, 1), (6, 2), (7, 3)] {
        let layer_ids: Vec<u64> = sc.references(field);
        for lid in layer_ids {
            let Some(layer) = ctx.loaded.msg(lid).cloned() else { continue };
            let idx = layer.varint(1).unwrap_or(0) as u32;
            for run in layer.msgs(2) {
                let Some(sm) = run.msg(3) else { continue };
                // An EmptyPattern / zero-width run ERASES the default grid
                // stroke on that edge — encoded as a 0-width stroke (the
                // viewer renders 0px = no line, overriding the section
                // default).
                let stroke = table_stroke(ctx, &sm).unwrap_or(Stroke {
                    color: "#00000000".to_string(),
                    width_pt: 0.0,
                    cap: StrokeCap::Butt,
                    join: StrokeJoin::Miter,
                    miter_limit: None,
                    dash: None,
                    dash_phase: None,
                });
                let origin = run.varint(1).unwrap_or(0) as u32;
                let length = run.varint(2).unwrap_or(1).max(1) as u32;
                for i in origin..origin + length {
                    // column layers: idx = column, i = row; row layers:
                    // idx = row, i = column
                    let key = if field <= 5 { (i, idx) } else { (idx, i) };
                    let b = out.entry(key).or_insert(CellBorders {
                        top: None,
                        right: None,
                        bottom: None,
                        left: None,
                    });
                    let slot = match edge {
                        0 => &mut b.left,
                        1 => &mut b.right,
                        2 => &mut b.top,
                        _ => &mut b.bottom,
                    };
                    *slot = Some(stroke.clone());
                }
            }
        }
    }
    out
}

/// Clamp a decoded merge range to the table bounds and push it; degenerate
/// 1x1 "merges" are no-ops and dropped.
fn push_merge(
    merges: &mut Vec<TableMerge>,
    r0: u64,
    c0: u64,
    r1: u64,
    c1: u64,
    rows: u32,
    cols: u32,
) {
    if rows == 0 || cols == 0 || r0 >= rows as u64 || c0 >= cols as u64 {
        return;
    }
    let r1 = r1.min(rows as u64 - 1);
    let c1 = c1.min(cols as u64 - 1);
    let row_span = (r1 - r0 + 1) as u32;
    let column_span = (c1 - c0 + 1) as u32;
    if row_span == 1 && column_span == 1 {
        return;
    }
    merges.push(TableMerge {
        anchor_row: r0 as u32,
        anchor_column: c0 as u32,
        row_span,
        column_span,
    });
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
    // RATIFIED (docs/model-review.md §1 Leak B): truncate after the last
    // non-default entry; an all-default array is omitted entirely. Readers
    // treat missing positions as default (matters on 28k-row sheets).
    let default = RowColInfo { size_pt: None, hidden: None };
    let is_default =
        |rc: &RowColInfo| *rc == default || (rc.size_pt == Some(0.0) && rc.hidden != Some(true));
    let keep = out.iter().rposition(|rc| !is_default(rc)).map(|i| i + 1).unwrap_or(0);
    if keep == 0 {
        return None;
    }
    out.truncate(keep);
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

    // Dimensions come straight from document varints and size dense
    // allocations below — validate before trusting. Numbers itself caps
    // tables at 1,000,000 rows x 1,000 columns; the cell budget bounds the
    // dense grid a crafted 65,536 x 65,536 claim would otherwise allocate.
    const MAX_ROWS: u32 = 1_000_000;
    const MAX_COLUMNS: u32 = 10_000;
    const MAX_CELLS: u64 = 8_000_000;
    let mut row_count = m.varint(6).unwrap_or(0) as u32;
    let mut column_count = m.varint(7).unwrap_or(0) as u32;
    if row_count > MAX_ROWS || column_count > MAX_COLUMNS {
        ctx.warn_detail(
            WarningCode::TableDegraded,
            format!(
                "table declares {row_count} x {column_count} cells, past app limits; clamping"
            ),
            model_id.to_string(),
        );
        row_count = row_count.min(MAX_ROWS);
        column_count = column_count.min(MAX_COLUMNS);
    }
    if row_count as u64 * column_count as u64 > MAX_CELLS {
        let clamped_rows = (MAX_CELLS / column_count.max(1) as u64) as u32;
        ctx.warn_detail(
            WarningCode::TableDegraded,
            format!(
                "table declares {row_count} x {column_count} cells, past the {MAX_CELLS}-cell budget; keeping the first {clamped_rows} rows"
            ),
            model_id.to_string(),
        );
        row_count = clamped_rows;
    }
    let (row_count, column_count) = (row_count, column_count);

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
    let mut cell_pool: StylePool<TableCellStyle> = StylePool::default();
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
                &mut cell_pool,
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

    // Merges, in numbers-parser's priority order (model.py merge_cells):
    // (1) merge_owner (model f47) formula-store ranges — modern files write
    //     COLON_TRACT_NODE (67) ASTs with absolute row/col tracts; pre-BNC
    //     files write RPN CELL_REFERENCE_NODE (36) pairs + COLON_NODE (29)
    //     — both handled here; (2) the legacy packed-int merge_region_map
    //     (DataStore f13). Stored ranges can be stale-wide (lafs_playlist:
    //     A1:G1 on a 4-column table) — spans clamp to the table bounds.
    //     [parser: masaccio/numbers-parser@3238795 model.py:848-876 +
    //     proto TSCEArchives.proto ASTNodeArchive; RPN pair form inferred,
    //     fixture-verified on lafs 6d101a3e]
    let mut merges = Vec::new();
    let mut merge_candidates = 0usize;
    if let Some(fs) = m.msg(47).and_then(|mo| mo.msg(2)) {
        for pair in fs.msgs(3) {
            let Some(nodes) = pair.msg(2).and_then(|f| f.msg(1)).map(|arr| arr.msgs(1)) else {
                continue;
            };
            merge_candidates += 1;
            // modern: colon tract node carries the whole range
            if let Some(tract) = nodes
                .iter()
                .find(|n| n.varint(1) == Some(67))
                .and_then(|n| n.msg(40))
            {
                let row = tract.msgs(4).into_iter().next();
                let col = tract.msgs(3).into_iter().next();
                if let (Some(row), Some(col)) = (row, col) {
                    if let (Some(r0), Some(c0)) = (row.varint(1), col.varint(1)) {
                        let r1 = row.varint(2).unwrap_or(r0).max(r0);
                        let c1 = col.varint(2).unwrap_or(c0).max(c0);
                        push_merge(&mut merges, r0, c0, r1, c1, row_count, column_count);
                    }
                }
                continue;
            }
            // pre-BNC: two cell refs (AST_column f26 / AST_row f27) + colon
            let refs: Vec<(u64, u64)> = nodes
                .iter()
                .filter(|n| n.varint(1) == Some(36))
                .map(|n| {
                    (
                        n.msg(27).and_then(|r| r.varint(1)).unwrap_or(0),
                        n.msg(26).and_then(|c| c.varint(1)).unwrap_or(0),
                    )
                })
                .collect();
            if refs.len() == 2 && nodes.iter().any(|n| n.varint(1) == Some(29)) {
                push_merge(
                    &mut merges,
                    refs[0].0.min(refs[1].0),
                    refs[0].1.min(refs[1].1),
                    refs[0].0.max(refs[1].0),
                    refs[0].1.max(refs[1].1),
                    row_count,
                    column_count,
                );
            }
        }
    }
    // Legacy fallback: merge_region_map (DataStore field 13) →
    // MergeRegionMapArchive { cell_range = 1 } with CellRange { origin =
    // CellID, size = TableSize }; packedData: column in the high 16 bits,
    // row in the low 16 bits (docs/format/tables.md §Merges).
    if let Some(mrm_id) =
        store.as_ref().and_then(|s| s.reference(13)).filter(|_| merges.is_empty())
    {
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
    // Last resort: TSCE dependency-archive merges (numbers-parser merge
    // path 2, model.py:877-908). The table's haunted_owner uid (model f84)
    // maps through a HAUNTED_OWNER (kind 35) FormulaOwnerDependenciesArchive
    // to the table's base owner uid; MERGE_OWNER (kind 5) dependency
    // archives then list merge rects as internal range references whose
    // owner ids resolve via the calc engine's owner-id map.
    if merges.is_empty() {
        if let Some(haunted) = m.msg(84).and_then(|h| h.msg(1)).as_ref().and_then(uuid_u128) {
            merges = dependency_merges(ctx, haunted, row_count, column_count);
        }
    }
    merges.sort_by_key(|m| (m.anchor_row, m.anchor_column));

    // Table-level style: TST.TableStyleArchive { super = 1,
    // table_properties = 11 } plus the style-network role slots on the model
    // (fields 18-21 cell styles, 24-27 text styles; docs/format/tables.md).
    // Outer-frame strokes are captured out of the closure for per-cell
    // baking below.
    let mut frame_h: Option<Stroke> = None;
    let mut frame_v: Option<Stroke> = None;
    let mut frame_hdr_h: Option<Stroke> = None;
    let mut frame_hdr_v: Option<Stroke> = None;
    let style = m.reference(3).and_then(|sid| {
        // TableStyleArchive properties INHERIT along the TSS parent chain
        // (02_Invoice's dashed body horizontal stroke lives only on the
        // preset parent); first-wins per field like every other style walk.
        let props = styles::chain(ctx, sid, 11);
        if props.is_empty() {
            return None;
        }
        let first = |f: u32| props.iter().find(|p| p.has(f)).cloned();
        let flag = |f: u32, default: bool| {
            props
                .iter()
                .find_map(|p| p.boolean(f))
                .unwrap_or(default)
        };
        // Default gridline strokes (TableStylePropertiesArchive modern
        // stroke slots f46-61 + visibility flags f33/f34/f35/f36/f37)
        // become section-style borders: a HORIZONTAL stroke is every
        // cell's bottom edge, a VERTICAL one its right edge, and the
        // separators paint the header/footer boundary. Strokes with an
        // empty pattern (TSDEmptyPattern) or zero width draw nothing.
        let h_vis = flag(34, true);
        let v_vis = flag(33, true);
        let mut body = section_style(ctx, &m, 18, 24);
        let mut hrow = section_style(ctx, &m, 19, 25);
        let mut hcol = section_style(ctx, &m, 20, 26);
        let mut foot = section_style(ctx, &m, 21, 27);
        let stroke_at = |ctx: &mut Ctx, f: u32| {
            first(f).and_then(|p| p.msg(f)).and_then(|sm| table_stroke(ctx, &sm))
        };
        let body_h = if h_vis { stroke_at(ctx, 60) } else { None };
        let body_v = if v_vis { stroke_at(ctx, 61) } else { None };
        attach_borders(&mut body, None, body_v.clone(), body_h.clone(), None);
        let hr_sep = if flag(35, true) { stroke_at(ctx, 46) } else { None };
        attach_borders(
            &mut hrow,
            None,
            stroke_at(ctx, 49),
            hr_sep.or_else(|| stroke_at(ctx, 48)),
            None,
        );
        let hc_sep = if flag(36, true) { stroke_at(ctx, 51) } else { None };
        attach_borders(
            &mut hcol,
            None,
            hc_sep.or_else(|| stroke_at(ctx, 53)),
            stroke_at(ctx, 52),
            None,
        );
        let foot_sep = if flag(37, true) { stroke_at(ctx, 54) } else { None };
        attach_borders(&mut foot, foot_sep, stroke_at(ctx, 57), stroke_at(ctx, 56), None);
        // Outer frame: the section gridline defaults above are half-open
        // (each cell owns right+bottom) so boundary 0 — the table's top and
        // left OUTER edge — has no owner and never painted. Apple draws the
        // frame with its own strokes: table_body_*_border_stroke (f58/f59,
        // gated by table_border_visible f38), and the header region's outer
        // edge with header_row/column_border_stroke (f47/f50, gated f39)
        // [proto TSTStylePropertyArchiving.proto:70-101]. Captured here,
        // baked onto boundary cells below.
        if flag(38, true) {
            frame_h = stroke_at(ctx, 58);
            frame_v = stroke_at(ctx, 59);
        }
        if flag(39, true) {
            frame_hdr_h = stroke_at(ctx, 47);
            frame_hdr_v = stroke_at(ctx, 50);
        }
        Some(TableStyle {
            banded_rows: props.iter().find_map(|p| p.boolean(1)),
            banded_fill: first(2)
                .and_then(|p| p.msg(2))
                .and_then(|f| crate::tsd::fill_of(ctx, &f)),
            body_cell_style: body,
            header_row_cell_style: hrow,
            header_column_cell_style: hcol,
            footer_row_cell_style: foot,
        })
    });

    // Dense row-major grid with None holes, plus a deduped formats pool;
    // cells reference formats by index. Malformed formats (negative /
    // absurd decimals) were already warned about and dropped inside
    // decode_cell — the cell is emitted with `formatIndex` absent.
    let mut grid: Vec<Vec<Option<GridCell>>> =
        vec![vec![None; column_count as usize]; row_count as usize];
    // Row/column DEFAULT styles (header buckets) fill in for cells with no
    // per-cell style; row beats column.
    let row_styles = header_styles(ctx, store.as_ref(), 1, 0);
    let col_styles = header_styles(ctx, store.as_ref(), 0, 2);
    // Per-cell edge stroke overrides (stroke sidecar) merge into the
    // pooled cell styles; leftovers on empty positions synthesize a
    // value-less styled cell after the loop.
    let mut edge_overrides = sidecar_borders(ctx, &m);
    // Bake the outer frame onto boundary cells (weakest layer: only sides
    // the sidecar left unset — its explicit strokes AND erases win). The
    // header region's outer edge prefers the header border strokes; the
    // bottom/right outer edge also gets the frame so it draws Apple's
    // border stroke rather than the interior gridline.
    if frame_h.is_some() || frame_v.is_some() || frame_hdr_h.is_some() || frame_hdr_v.is_some() {
        let hdr_rows = m.varint(9).unwrap_or(0) as u32;
        let hdr_cols = m.varint(10).unwrap_or(0) as u32;
        let empty = || CellBorders { top: None, right: None, bottom: None, left: None };
        for c in 0..column_count {
            let top = if hdr_rows > 0 { frame_hdr_h.as_ref().or(frame_h.as_ref()) } else { frame_h.as_ref() };
            if let Some(s) = top {
                let e = edge_overrides.entry((0, c)).or_insert_with(empty);
                if e.top.is_none() {
                    e.top = Some(s.clone());
                }
            }
            if let (Some(s), true) = (frame_h.as_ref(), row_count > 0) {
                let e = edge_overrides.entry((row_count - 1, c)).or_insert_with(empty);
                if e.bottom.is_none() {
                    e.bottom = Some(s.clone());
                }
            }
        }
        for r in 0..row_count {
            let left = if hdr_cols > 0 { frame_hdr_v.as_ref().or(frame_v.as_ref()) } else { frame_v.as_ref() };
            if let Some(s) = left {
                let e = edge_overrides.entry((r, 0)).or_insert_with(empty);
                if e.left.is_none() {
                    e.left = Some(s.clone());
                }
            }
            if let (Some(s), true) = (frame_v.as_ref(), column_count > 0) {
                let e = edge_overrides.entry((r, column_count - 1)).or_insert_with(empty);
                if e.right.is_none() {
                    e.right = Some(s.clone());
                }
            }
        }
    }
    let mut formats: Vec<CellFormat> = Vec::new();
    // Dedupe index keyed by serialized format: a linear scan of the pool per
    // formatted cell goes quadratic when a document carries many distinct
    // formats. Pool order stays first-appearance, so output is unchanged.
    let mut format_index: HashMap<String, u32> = HashMap::new();
    for (row, col, mut cell, format) in cells {
        if cell.cell_style_index.is_none() {
            if let Some(s) = row_styles.get(&row).or_else(|| col_styles.get(&col)) {
                cell.cell_style_index = cell_pool.intern(s.clone());
            }
        }
        if let Some(b) = edge_overrides.remove(&(row, col)) {
            let mut s = cell
                .cell_style_index
                .and_then(|i| cell_pool.items.get(i as usize).cloned())
                .unwrap_or_default();
            merge_borders(&mut s, b);
            cell.cell_style_index = cell_pool.intern(s);
        }
        if let Some(f) = format {
            let key = serde_json::to_string(&f).unwrap_or_default();
            let idx = match format_index.get(&key) {
                Some(i) => *i,
                None => {
                    let i = formats.len() as u32;
                    formats.push(f);
                    format_index.insert(key, i);
                    i
                }
            };
            cell.fmt = Some(idx);
        }
        let slot = match (&cell.v, cell.r#type, cell.fmt, cell.cell_style_index, &cell.formula) {
            // Plain unformatted scalar: bare value, no object wrapper.
            (GridValue::Scalar(s), None, None, None, None) => {
                GridCell::Plain(GridPlain::Text(s.clone()))
            }
            (GridValue::Number(n), None, None, None, None) => {
                GridCell::Plain(GridPlain::Number(number_from_f64(*n)))
            }
            (GridValue::Bool(b), None, None, None, None) => GridCell::Plain(GridPlain::Bool(*b)),
            _ => GridCell::Cell(cell),
        };
        if (row as usize) < grid.len() && (col as usize) < grid[row as usize].len() {
            grid[row as usize][col as usize] = Some(slot);
        }
    }
    // Sidecar strokes on positions with no stored cell (02_Invoice's empty
    // spacer row still carries its heavy rule): synthesize a value-less
    // styled cell so the edge renders. Drained in (row, column) order —
    // HashMap iteration order would make the interned style-pool indices,
    // and therefore the JSON bytes, vary run to run.
    let mut leftover_overrides: Vec<((u32, u32), CellBorders)> = edge_overrides.drain().collect();
    leftover_overrides.sort_unstable_by_key(|(pos, _)| *pos);
    for ((row, col), b) in leftover_overrides {
        if (row as usize) < grid.len()
            && (col as usize) < grid[row as usize].len()
            && grid[row as usize][col as usize].is_none()
        {
            let mut s = TableCellStyle::default();
            merge_borders(&mut s, b);
            grid[row as usize][col as usize] = Some(GridCell::Cell(TableCell {
                v: GridValue::None,
                r#type: None,
                cur: None,
                fmt: None,
                cell_style_index: cell_pool.intern(s),
                formula: None,
            }));
        }
    }

    // All three numbers-parser merge sources are decoded above. Only warn
    // when the merge owner's formula store HELD candidate ranges that we
    // failed to decode — a bare merge owner with an empty store (written
    // for merge-free tables too) plus sparse nulls is not evidence of
    // missing merges (gotcha #18).
    if merge_candidates > 0 && merges.is_empty() {
        let null_cells: u32 = grid
            .iter()
            .flat_map(|row| row.iter())
            .filter(|slot| slot.is_none())
            .count() as u32;
        if null_cells > 0 {
            ctx.warn_detail(
                WarningCode::TableDegraded,
                format!(
                    "merge owner present but no ranges decoded (formula-store + region-map paths empty; TSCE dependency-archive merges not yet read); {null_cells} covered cells emit as null without span info"
                ),
                format!("table '{}'", m.string(8).unwrap_or_default()),
            );
        }
    }

    TableModel {
        name: {
            // table_name_enabled (f22) defaults OFF: Apple's own templates
            // (02_Invoice "Details") and older docs (lafs "Table 1") omit the
            // field and render no table name; table_name_height (f33) is 0
            // there too. Only an explicit true shows the caption.
            let n = m.string(8);
            match n {
                Some(name) if !name.is_empty() && m.boolean(22) == Some(true) => Some(name),
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
        cell_styles: std::mem::take(&mut cell_pool.items),
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
        cell_styles: Vec::new(),
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
    cell_pool: &mut StylePool<TableCellStyle>,
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
            let decoded = if cell_buf[0] == 3 {
                decode_cell_v3(
                    ctx,
                    model_row,
                    slot as u32,
                    cell_buf,
                    string_table,
                    style_table,
                    format_table,
                    rich_text_table,
                    cell_pool,
                )
            } else if cell_buf[0] == 4 {
                decode_cell_v4(
                    ctx,
                    model_row,
                    slot as u32,
                    cell_buf,
                    string_table,
                    style_table,
                    format_table,
                    rich_text_table,
                    formula_table,
                    cell_pool,
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
                    cell_pool,
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
    _formula_table: &DataList,
    formula_error_table: &DataList,
    format_table: &DataList,
    custom_format_table: &DataList,
    rich_text_table: &DataList,
    cell_pool: &mut StylePool<TableCellStyle>,
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
    let _ = off; // the final take's advance is intentionally unread

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
        // Generic/span cell: no stored value, but style/formula/format
        // metadata may still paint it (a fill, a rule) — resolve those
        // below and keep the cell when any survive; a fully bare one is
        // dropped at the end (FINDINGS.md M-6).
        0 | 1 => CellValue::Empty,
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
    // TST.CellStyleArchive; the text style id resolves on top; pooled
    // per table (TableCell.cellStyleIndex).
    let mut style = cell_style_id.and_then(|id| {
        style_table
            .entries
            .get(&id)
            .and_then(|e| e.reference)
            .and_then(|cid| styles::resolve_cell_style(ctx, cid))
    });
    if let Some(tref) = text_style_id
        .and_then(|tid| style_table.entries.get(&tid))
        .and_then(|e| e.reference)
    {
        // The per-cell TEXT style carries char props AND paragraph props —
        // alignment lives in the latter (lafs title centers via the text
        // style). A text style with no cell style still counts.
        let text = crate::ctx::strip_char_defaults(styles::resolve_char_style(ctx, tref));
        let para = crate::ctx::strip_para_defaults(styles::resolve_para_style(ctx, tref));
        if text != CharStyle::default() || para != ParaStyle::default() {
            let s = style.get_or_insert_with(TableCellStyle::default);
            if text != CharStyle::default() {
                s.text = Some(text);
            }
            if para != ParaStyle::default() {
                s.paragraph = Some(para);
            }
        }
    }
    let cell_style_index = style.and_then(|s| cell_pool.intern(crate::ctx::strip_cell_defaults(s)));

    // Formula placeholder (TSCE stays opaque; model-design §2.8).
    let formula = formula_id.map(|id| TsceFormulaRef::unparsed(id.to_string()));

    // Format hint: per-type format id wins; custom formats degrade to "custom"
    // + raw string (docs/model-design.md §2.6). Malformed formats (negative /
    // absurd decimals, e.g. a -1 int32 sentinel) are dropped with a warning —
    // the cell is emitted unformatted rather than clamped.
    let (format, malformed_format) = pick_format(
        ctx,
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
    let (v, type_tag, cur) = value_into_parts(value);
    // A valueless cell that resolved no style, formula, or format carries
    // nothing renderable — only now is dropping it safe (FINDINGS.md M-6).
    if matches!(v, GridValue::None)
        && type_tag.is_none()
        && cell_style_index.is_none()
        && formula.is_none()
        && format.is_none()
    {
        return None;
    }
    Some((
        row,
        col,
        TableCell { v, r#type: type_tag, cur, fmt: None, cell_style_index, formula },
        format,
    ))
}

/// Map the internal decoded value onto the GridValue + type-tag contract:
/// plain text/number/bool stay untagged; ISO dates, durations (seconds),
/// currency (with code), rich text and formula errors carry short tags.
fn value_into_parts(value: CellValue) -> (GridValue, Option<CellTypeTag>, Option<String>) {
    match value {
        CellValue::Empty => (GridValue::None, None, None),
        CellValue::Number { value } => (GridValue::Number(value), None, None),
        CellValue::Text { value } => (GridValue::Scalar(value), None, None),
        CellValue::Bool { value } => (GridValue::Bool(value), None, None),
        CellValue::Date { value } => (GridValue::Scalar(value), Some(CellTypeTag::Date), None),
        CellValue::Duration { value } => (GridValue::Number(value), Some(CellTypeTag::Duration), None),
        CellValue::Currency { value, currency_code } => {
            (GridValue::Number(value), Some(CellTypeTag::Currency), currency_code)
        }
        CellValue::Richtext { text } => (GridValue::Richtext(text), Some(CellTypeTag::Richtext), None),
        CellValue::Error { value } => (GridValue::Scalar(value), Some(CellTypeTag::Error), None),
    }
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

/// One section's default look: the cell style (TableModelArchive fields
/// 18-21) merged with the parallel section TEXT style (fields 24-27:
/// body/header-row/header-column/footer-row — proto TSTArchives.proto
/// TableModelArchive). Apple's stock templates put the entire header look
/// (bold, white font, alignment) in the section text style and leave the
/// per-cell style pool empty (fixture-verified: 02_Invoice has zero
/// cellStyles; its blue header text lives at field 25).
fn section_style(
    ctx: &mut Ctx,
    m: &Msg,
    cell_field: u32,
    text_field: u32,
) -> Option<TableCellStyle> {
    let mut s = m
        .reference(cell_field)
        .and_then(|cid| styles::resolve_cell_style(ctx, cid));
    if let Some(tid) = m.reference(text_field) {
        let text = crate::ctx::strip_char_defaults(styles::resolve_char_style(ctx, tid));
        let para = crate::ctx::strip_para_defaults(styles::resolve_para_style(ctx, tid));
        let text = (text != CharStyle::default()).then_some(text);
        let para = (para != ParaStyle::default()).then_some(para);
        if text.is_some() || para.is_some() {
            let st = s.get_or_insert_with(TableCellStyle::default);
            if st.text.is_none() {
                st.text = text;
            }
            if st.paragraph.is_none() {
                st.paragraph = para;
            }
        }
    }
    s
}

/// Pattern for a custom format: the FormatStruct's inline
/// CustomFormatArchive (field 42) or its custom_uid (field 41) resolved
/// through the document-level TSK.CustomFormatListArchive (registry type
/// 222), where uuids (f1) pair with custom_formats (f2) by index; the
/// pattern is default_format.custom_format_string (f3 → f18). [proto:
/// TSKArchives.proto CustomFormatArchive/CustomFormatListArchive;
/// fixture-verified: 07_Calendar "Day Only" → "d"]
fn custom_pattern(ctx: &Ctx, f: &Msg) -> Option<String> {
    if let Some(s) = f
        .msg(42)
        .and_then(|cf| cf.msg(3))
        .and_then(|df| df.string(18))
        .filter(|s| !s.is_empty())
    {
        return Some(s);
    }
    let uid = f.msg(41)?;
    let key = (uid.varint(1)?, uid.varint(2)?);
    for rec in ctx.loaded.records.values() {
        if rec.type_id != 222 {
            continue;
        }
        let Some(list) = rec.msg.as_ref() else { continue };
        let uuids = list.msgs(1);
        let formats = list.msgs(2);
        for (i, u) in uuids.iter().enumerate() {
            if (u.varint(1), u.varint(2)) == (Some(key.0), Some(key.1)) {
                if let Some(s) = formats
                    .get(i)
                    .and_then(|cf| cf.msg(3))
                    .and_then(|df| df.string(18))
                    .filter(|s| !s.is_empty())
                {
                    return Some(s);
                }
            }
        }
    }
    None
}

fn pick_format(
    ctx: &Ctx,
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
    .find_map(|(id, slot_kind)| {
        id.and_then(|id| format_table.entries.get(&id))
            .and_then(|e| e.format.clone())
            .map(|f| (f, slot_kind))
    });
    if let Some((f, slot_kind)) = found {
        // decimal_places > 20 is the app's AUTO sentinel (253 = -3 as i8 /
        // 4294967293 = -3 as u32, fixture-verified on G5: decimal/currency/
        // percent/scientific all carry it) — legit "auto decimals": emit the
        // format with `decimals` absent, never degrade.
        let decimals = f
            .varint(2)
            .filter(|v| *v <= 20)
            .map(|v| v as u32);
        // Kind from the format's OWN format_type (TSK.FormatStructArchive f1,
        // numbers-parser FormatType) — the referencing slot only says WHICH
        // per-type table the id lives in, not the true kind (a "number
        // format" id can resolve to base-16/fraction/scientific archives).
        let ft = f.varint(1);
        let kind = match ft {
            Some(257) => CellFormatKind::Currency,
            Some(258) => CellFormatKind::Percent,
            Some(261) => CellFormatKind::Date,
            Some(268) => CellFormatKind::Duration,
            Some(260) => CellFormatKind::Text,
            Some(270..=274) => CellFormatKind::Custom,
            _ => slot_kind,
        };
        // Blessed convention (Main/ModelAgent): kind stays closed; the
        // display semantic lives in formatString. Base-N (hex/binary/octal)
        // surfaces as "base-<n>"; scientific surfaces via the stored
        // scientific_pattern (f44) when customized — auto patterns are not
        // persisted, so formatString is legitimately absent there.
        let format_string = f.string(18).or_else(|| match (ft, f.varint(8)) {
            (Some(269), Some(base)) => Some(format!("base-{base}")),
            _ => None,
        }).or_else(|| match (ft, f.string(44)) {
            (Some(259), Some(pattern)) if !pattern.is_empty() => Some(pattern),
            _ => None,
        }).or_else(|| match ft {
            // stock date/time formats persist their ICU-ish pattern in
            // date_time_format (f14), e.g. "d. MMMM yyyy" / "d"
            Some(261) => f.string(14).filter(|s| !s.is_empty()),
            _ => None,
        }).or_else(|| match ft {
            // display-semantic markers (kind stays closed per the blessed
            // convention): auto scientific has no persisted pattern;
            // fractions carry an accuracy code in f11
            Some(259) => Some("scientific".to_string()),
            // fraction accuracy (f11): a small value is an exact
            // denominator (2/4/8/10/16/100); 0xFFFFFFFD..FF are the
            // up-to-N-digit sentinels (parser: numbers-parser
            // FractionAccuracy)
            Some(262) => Some(match f.varint(11) {
                Some(acc) if acc >= 2 && acc <= 100 => format!("fraction-{acc}"),
                _ => "fraction".to_string(),
            }),
            // durations: style (f7: 0 compact "28:40" / 1 short "28m 40s" /
            // 2 long), unit range largest/smallest (f15/f16: 1 week, 2 day,
            // 4 hour, 8 minute, 16 second, 32 ms), automatic units (f40) —
            // packed for the viewer's renderer [parser: numbers-parser
            // cell.py _duration_format/_auto_units]
            Some(268) => Some(format!(
                "duration-{}-{}-{}-{}",
                f.varint(7).unwrap_or(0),
                f.varint(15).unwrap_or(4),
                f.varint(16).unwrap_or(16),
                u8::from(f.boolean(40).unwrap_or(false)),
            )),
            _ => None,
        }).or_else(|| match ft {
            // custom formats (270-274): pattern lives behind the inline
            // CustomFormatArchive or the document custom-format list
            Some(270..=274) => custom_pattern(ctx, &f),
            _ => None,
        });
        return (
            Some(CellFormat {
                kind,
                decimals,
                currency_code: f.string(3),
                // show_thousands_separator (f5), raw presence: absent means
                // the KIND's default (currency groups, number does not)
                grouping: f.boolean(5),
                format_string,
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
            grouping: None,
            // CustomFormatArchive: the pattern is default_format (f3)
            // .custom_format_string (f18) — f3 is a message, not a string
            format_string: cf.msg(3).and_then(|df| df.string(18)),
        });
    (custom, false)
}

/// iWork '13-era (storage_version 3) cell block decoder — fixture-verified
/// against two 2014-saved Pages docs (LED price list 6478639…, HEE appendix
/// b31db82…; corpus census class 1). Layout [inferred, all cells in both docs
/// consistent]:
/// `[03][00][cell_type][?]` + flags u64 LE at 4..12 + payload at 12,
/// fields in fixed order, each present iff its flag bit is set:
///   bit 1 cell style key · bit 7 text style key · bit 2 format key ·
///   bit 4 string key · bit 5 f64 value · bit 9 rich-text key ·
///   bit 48 trailing u32 (repeats the format key on number cells; skipped).
/// High bits 20/22/23/27 vary without consuming payload (metadata).
/// cell_type at byte 2 matches the v5 enum: 0 empty (style-only), 2 number,
/// 3 text, 9 rich text. Keys index the same DataLists as v5.
#[allow(clippy::too_many_arguments)]
fn decode_cell_v3(
    ctx: &mut Ctx,
    row: u32,
    col: u32,
    buf: &[u8],
    string_table: &DataList,
    style_table: &DataList,
    format_table: &DataList,
    rich_text_table: &DataList,
    cell_pool: &mut StylePool<TableCellStyle>,
) -> Option<(u32, u32, TableCell, Option<CellFormat>)> {
    if buf.len() < 12 {
        return None;
    }
    let cell_type = buf[2];
    let flags = u64::from_le_bytes(buf[4..12].try_into().unwrap());

    // Unknown low bits would shift every later field — refuse to guess.
    const KNOWN_CONSUMING: u64 = (1 << 1) | (1 << 2) | (1 << 4) | (1 << 5) | (1 << 7) | (1 << 9);
    if flags & 0xffff & !KNOWN_CONSUMING != 0 {
        ctx.warn_detail(
            WarningCode::TableDegraded,
            format!(
                "v3 cell with unknown storage flags {:#x}; cell r{row}c{col} dropped",
                flags & 0xffff
            ),
            format!("r{row}c{col}"),
        );
        return None;
    }

    let mut off = 12usize;
    macro_rules! take_u32_if {
        ($bit:expr) => {
            if flags & (1 << $bit) != 0 {
                if let Some(b) = buf.get(off..off + 4) {
                    let v = i32::from_le_bytes(b.try_into().unwrap());
                    off += 4;
                    Some(v)
                } else {
                    None
                }
            } else {
                None
            }
        };
    }
    let cell_style_id = take_u32_if!(1);
    let text_style_id = take_u32_if!(7);
    let format_id = take_u32_if!(2);
    let string_id = take_u32_if!(4);
    let double = if flags & (1 << 5) != 0 {
        buf.get(off..off + 8).map(|b| {
            off += 8;
            f64::from_le_bytes(b.try_into().unwrap())
        })
    } else {
        None
    };
    let rich_id = take_u32_if!(9);
    let _ = off;

    let value = match cell_type {
        0 | 1 => CellValue::Empty, // style-only cell (image/fill cells in the LED doc)
        2 => CellValue::Number { value: double? },
        3 => {
            let sid = string_id?;
            match string_table.entries.get(&sid).and_then(|e| e.string.clone()) {
                Some(s) => CellValue::Text { value: s },
                None => {
                    ctx.warn_detail(
                        WarningCode::TableDegraded,
                        format!("v3 string key {sid} not resolvable in the string table; cell r{row}c{col} dropped"),
                        format!("r{row}c{col}"),
                    );
                    return None;
                }
            }
        }
        9 => {
            // RichTextPayloadArchive { storage = 1 } via the rich-text list.
            let storage_id = rich_id
                .and_then(|id| rich_text_table.entries.get(&id))
                .and_then(|e| e.reference)
                .and_then(|rtp| ctx.loaded.msg(rtp))
                .and_then(|p| p.reference(1));
            match storage_id.and_then(|sid| crate::text::extract(ctx, sid)) {
                Some(ex) => CellValue::Richtext { text: ex.text },
                None => {
                    ctx.warn_detail(
                        WarningCode::TableDegraded,
                        format!("v3 rich-text key {rich_id:?} not decodable; cell r{row}c{col} dropped"),
                        format!("r{row}c{col}"),
                    );
                    return None;
                }
            }
        }
        _ => {
            ctx.warn_detail(
                WarningCode::TableDegraded,
                format!("unrecognized v3 cell type {cell_type}; cell skipped"),
                format!("r{row}c{col}"),
            );
            return None;
        }
    };

    // Styles resolve through the shared STYLE list exactly like v5.
    let mut style = cell_style_id.and_then(|id| {
        style_table
            .entries
            .get(&id)
            .and_then(|e| e.reference)
            .and_then(|cid| styles::resolve_cell_style(ctx, cid))
    });
    if let Some(tref) = text_style_id
        .and_then(|tid| style_table.entries.get(&tid))
        .and_then(|e| e.reference)
    {
        let text = crate::ctx::strip_char_defaults(styles::resolve_char_style(ctx, tref));
        let para = crate::ctx::strip_para_defaults(styles::resolve_para_style(ctx, tref));
        if text != CharStyle::default() || para != ParaStyle::default() {
            let s = style.get_or_insert_with(TableCellStyle::default);
            if text != CharStyle::default() {
                s.text = Some(text);
            }
            if para != ParaStyle::default() {
                s.paragraph = Some(para);
            }
        }
    }
    let cell_style_index = style.and_then(|s| cell_pool.intern(crate::ctx::strip_cell_defaults(s)));
    if matches!(value, CellValue::Empty) && cell_style_index.is_none() {
        return None;
    }

    // The single v3 format key routes through the same FORMAT list as v5.
    let format = format_id.and_then(|id| {
        format_table.entries.get(&id).and_then(|e| {
            e.format.as_ref().map(|f| CellFormat {
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
                grouping: f.boolean(5),
                format_string: f.string(18).or_else(|| f.string(14).filter(|s| !s.is_empty())),
            })
        })
    });

    let (v, type_tag, cur) = value_into_parts(value);
    Some((
        row,
        col,
        TableCell { v, r#type: type_tag, cur, fmt: None, cell_style_index, formula: None },
        format,
    ))
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
    rich_text_table: &DataList,
    formula_table: &DataList,
    cell_pool: &mut StylePool<TableCellStyle>,
) -> Option<(u32, u32, TableCell, Option<CellFormat>)> {
    if std::env::var("PNK_DEBUG").is_ok() {
        eprintln!("v4 r{row}c{col} len={} type={}", buf.len(), buf.get(1).copied().unwrap_or(255));
    }
    if buf.len() < 16 {
        return None;
    }
    let cell_type = buf[1];
    let u32s: Vec<u32> = buf
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();

    // Slot 1 is a v3-style presence bitfield for the LEADING fields, laid out
    // from slot 3 in fixed order: cell style (bit 1), text style (bit 7),
    // format (bit 2), two unknown ids (bits 10/11), formula key (bit 3),
    // string key (bit 4). Fixture-verified across every shape in the corpus:
    // 0x14 {2,4} = [fmt, str] (budget County Tax Rates headers, previously
    // dropped), 0x96 {1,2,4,7} = [cs, ts, fmt, str] (= the old fixed slots),
    // 0xc9e adds b10/b11 before the formula key (match-score doc), 0x8c
    // {2,3,7} = [ts, fmt, formula] (type-8 cells).
    let flags1 = u32s.get(1).copied().unwrap_or(0);
    let mut lead_idx = 3usize;
    macro_rules! lead {
        ($bit:expr) => {
            if flags1 & (1 << $bit) != 0 {
                let v = u32s.get(lead_idx).map(|v| *v as i32);
                lead_idx += 1;
                v
            } else {
                None
            }
        };
    }
    let cell_style_id = lead!(1);
    let text_style_id = lead!(7);
    let fmt_lead = lead!(2);
    let _f10 = lead!(10);
    let _f11 = lead!(11);
    let formula_key = lead!(3);
    let string_key = lead!(4);
    let _ = lead_idx;

    // Value position: text key = slot 6; the numeric f64 sits right before a
    // TRAILING block of format keys — one u32 per set bit among flag bits
    // 16-23 of slot 2 (the v4 analogue of v5's per-kind trailing format ids;
    // lafs durations carry bit 18 -> one key, the budget doc's 0x90013
    // records bits 16+19 -> two, the match-score doc's 0x910451 bits
    // 16+20+23 -> three). Corpus-validated: 294k v4 numeric cells across all
    // 158 .numbers fixtures place a sane f64 here with zero violations
    // (the old fixed slot-3 read produced denormal garbage on multi-key
    // records). The format id = first trailing key found in the FORMAT list.
    let flags2 = u32s.get(2).copied().unwrap_or(0);
    let ntrail = (flags2 & 0x00FF_0000).count_ones() as usize;
    let val_off = buf.len().checked_sub(8 + 4 * ntrail);
    let f64_value = val_off
        .and_then(|o| buf.get(o..o + 8))
        .map(|b| f64::from_le_bytes(b.try_into().unwrap()));
    let trailing_keys: Vec<i32> = (0..ntrail)
        .filter_map(|i| {
            let o = val_off? + 8 + 4 * i;
            Some(i32::from_le_bytes(buf.get(o..o + 4)?.try_into().ok()?))
        })
        .collect();
    // The leading bit-2 key is the cell's display format (County Tax Rates
    // currency); the trailing keys are per-kind fallbacks like v5's — probe
    // the leading key first.
    let fmt_key = fmt_lead
        .filter(|k| format_table.entries.contains_key(k))
        .or_else(|| {
            trailing_keys.iter().copied().find(|k| format_table.entries.contains_key(k))
        })
        .or(fmt_lead)
        .or_else(|| trailing_keys.first().copied());

    // v4 formula cells cache no computed value in the record — the key in the
    // FORMULA list is all there is (budget doc e138671a: type-8 cells; text
    // doc 021084ac: type-3 cells whose slot-6 key lives in the formula list,
    // not the string list). Emit an empty cell carrying the formula ref.
    let mut formula: Option<TsceFormulaRef> = None;

    let value = match cell_type {
        3 => {
            // string key from the leading bitfield (bit 4); older fixed-slot
            // fallback (slot 6) kept as a safety net.
            let sid = string_key.or_else(|| u32s.get(6).map(|v| *v as i32))?;
            match string_table.entries.get(&sid).and_then(|e| e.string.clone()) {
                Some(s) => CellValue::Text { value: s },
                None if formula_key.is_some_and(|k| formula_table.entries.contains_key(&k)) => {
                    // Formula-driven text cell; the result is not cached in
                    // pre-BNC storage, so the value stays empty.
                    formula = formula_key.map(|k| TsceFormulaRef::unparsed(k.to_string()));
                    CellValue::Empty
                }
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
        8 => {
            // Formula/error cell: no cached result in pre-BNC storage; keep
            // the formula reference so the cell exists (styled, empty).
            match formula_key {
                Some(k) => {
                    formula = Some(TsceFormulaRef::unparsed(k.to_string()));
                    CellValue::Empty
                }
                None => {
                    ctx.warn_detail(
                        WarningCode::TableDegraded,
                        format!("v4 formula cell without a formula key; cell r{row}c{col} dropped"),
                        format!("r{row}c{col}"),
                    );
                    return None;
                }
            }
        }
        0 | 1 if cell_style_id.is_some() || text_style_id.is_some() => {
            // Style-only cell (colored empty cells: County Tax Rates bands).
            CellValue::Empty
        }
        7 => CellValue::Duration { value: f64_value? },
        5 => CellValue::Date { value: crate::colors::iso_from_apple_seconds(f64_value?) },
        2 => CellValue::Number { value: f64_value? },
        6 => CellValue::Bool { value: f64_value? > 0.0 },
        9 => {
            // v4 rich-text cells: the rich-text table key sits in the
            // TRAILING u32 slot — 24-byte blocks carry it at slot 5 (IVS
            // doc bc5e6bd1), 28-byte blocks at slot 6 (bd3a64fb, where
            // slot 5 is constant 1). Probe trailing first, then 5/6.
            let rid = [u32s.last().copied(), u32s.get(5).copied(), u32s.get(6).copied()]
                .into_iter()
                .flatten()
                .map(|v| v as i32)
                .find(|id| rich_text_table.entries.contains_key(id));
            let rtp_id = rid.and_then(|id| rich_text_table.entries.get(&id).and_then(|e| e.reference));
            match rtp_id {
                Some(rtp_id) => {
                    // RichTextPayloadArchive { storage = 1, range = 2, cellid = 3 }
                    let storage_id = ctx
                        .loaded
                        .msg(rtp_id)
                        .and_then(|p| p.reference(1));
                    match storage_id.and_then(|sid| crate::text::extract(ctx, sid)) {
                        Some(ex) => CellValue::Richtext { text: ex.text },
                        None => {
                            ctx.warn_detail(
                                WarningCode::TableDegraded,
                                format!("v4 rich-text payload {rtp_id} not decodable; cell r{row}c{col} dropped"),
                                format!("r{row}c{col}"),
                            );
                            return None;
                        }
                    }
                }
                None => {
                    ctx.warn_detail(
                        WarningCode::TableDegraded,
                        format!("v4 rich-text key {rid:?} not in the rich-text table; cell r{row}c{col} dropped"),
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
    if let Some(tref) = text_style_id
        .and_then(|tid| style_table.entries.get(&tid))
        .and_then(|e| e.reference)
    {
        // The per-cell TEXT style carries char props AND paragraph props —
        // alignment lives in the latter (lafs title centers via the text
        // style). A text style with no cell style still counts.
        let text = crate::ctx::strip_char_defaults(styles::resolve_char_style(ctx, tref));
        let para = crate::ctx::strip_para_defaults(styles::resolve_para_style(ctx, tref));
        if text != CharStyle::default() || para != ParaStyle::default() {
            let s = style.get_or_insert_with(TableCellStyle::default);
            if text != CharStyle::default() {
                s.text = Some(text);
            }
            if para != ParaStyle::default() {
                s.paragraph = Some(para);
            }
        }
    }
    let cell_style_index = style.and_then(|s| cell_pool.intern(crate::ctx::strip_cell_defaults(s)));
    if matches!(value, CellValue::Empty) && cell_style_index.is_none() && formula.is_none() {
        return None;
    }

    // Format id: first trailing key found in the FORMAT list, else the
    // leading bit-2 field (equals the old slot-5 read on lafs).
    let format = match cell_type {
        2 | 5 | 7 => fmt_key.and_then(|id| {
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
                    // show_thousands_separator (f5), raw presence: absent
                    // means the KIND's default (currency groups)
                    grouping: f.boolean(5),
                    format_string: f
                        .string(18)
                        .or_else(|| f.string(14).filter(|s| !s.is_empty()))
                        .or_else(|| match f.varint(1) {
                            // duration spec (same packing as the BNC path)
                            Some(268) => Some(format!(
                                "duration-{}-{}-{}-{}",
                                f.varint(7).unwrap_or(0),
                                f.varint(15).unwrap_or(4),
                                f.varint(16).unwrap_or(16),
                                u8::from(f.boolean(40).unwrap_or(false)),
                            )),
                            _ => None,
                        }),
                })
            })
        }),
        _ => None,
    };

    let (v, type_tag, cur) = value_into_parts(value);
    Some((
        row,
        col,
        TableCell { v, r#type: type_tag, cur, fmt: None, cell_style_index, formula },
        format,
    ))
}

/// f64 -> JSON number preserving integral-ness: 7434.0 serializes as 7434
/// (smaller envelopes, same value); fractional values stay floats.
fn number_from_f64(n: f64) -> serde_json::Number {
    if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
        serde_json::Number::from(n as i64)
    } else {
        serde_json::Number::from_f64(n).unwrap_or_else(|| serde_json::Number::from(0))
    }
}

/// Strip documented defaults from a resolved TableCellStyle.
pub use crate::ctx::strip_char_defaults;
