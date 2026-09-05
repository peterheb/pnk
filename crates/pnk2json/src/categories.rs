//! Numbers category grouping ("Organize by" a column): the group tree, the
//! per-column summary rules, and the app's cached group totals. The base
//! grid stays the ungrouped data; this is the structure Numbers lays over
//! it (group rows with disclosure triangles and summaries). Fixture-verified
//! on 6914f46e51ab (a time sheet grouped by "Projekt": groups "Muster" and
//! blank, duration sums 4h27m / 3h57m, table total 8h24m).
//!
//! Object graph [proto: TSTArchives.proto]:
//! `TableModelArchive.category_owner` (f86) → `CategoryOwnerRefArchive`
//! { group_by = 1: refs } → `GroupByArchive` { group_by_uid 1, group_column
//! 2 [{column_uid 1, grouping_type 2}], group_node_root 3 (inline) /
//! group_node_root_ref 18, aggregator 4 [{column_uid 1, agg_node 2}],
//! column_agg_type 5 [{column_uid 1, level 2, agg_type 3}], is_enabled 6,
//! row_uid_lookup 15 {uuids 1} }. `GroupNodeArchive` { group_uid 1, child 3
//! (inline) / child_ref 10, row_uid 4, group_cell_value 7 (TSCE.
//! CellValueArchive), row_lookup_uids 9 (TSCE.IndexSetArchive: inclusive
//! [range_begin, range_end] entries indexing row_uid_lookup) }.
//! `AggNodeArchive` { accum 2 (AccumulatorArchive: number_count 2,
//! min_value 6, max_value 7, number_total_value 8), child 3 } parallels
//! the group tree. Uids resolve to model indexes through the table's
//! `base_column_row_uids` (f46) → `ColumnRowUIDMapArchive` { sorted_column_
//! uids 1, column_index_for_uid 2, sorted_row_uids 4, row_index_for_uid 5 }.
//! [fixture-verified: index-set ranges are INCLUSIVE — the root covers
//! [0,29] for 30 body rows; lookup order is not model order (the "Muster"
//! row is lookup index 8, model row 1)]

use std::collections::HashMap;

use crate::ctx::Ctx;
use crate::formulas::uuid_u128;
use crate::model::{GridValue, GroupAggregate, GroupTotal, TableGroup, TableGrouping};
use crate::pb::Msg;
use iwadump::proto::Value;

fn varints(m: &Msg, n: u32) -> Vec<u64> {
    let direct: Vec<u64> = m
        .all(n)
        .into_iter()
        .filter_map(|v| match v {
            Value::Varint(x) => Some(*x),
            _ => None,
        })
        .collect();
    if direct.is_empty() {
        m.packed_varints(n)
    } else {
        direct
    }
}

fn uid_index_map(m: &Msg, uid_field: u32, index_field: u32) -> HashMap<u128, u32> {
    let uids: Vec<u128> = m.msgs(uid_field).iter().filter_map(uuid_u128).collect();
    let idx = varints(m, index_field);
    uids.into_iter()
        .zip(idx)
        .map(|(u, i)| (u, i as u32))
        .collect()
}

/// TSCE.CellValueArchive → (value, "date" tag). NIL → None.
fn cell_value(cv: &Msg) -> (GridValue, bool) {
    match cv.varint(1) {
        Some(2) => (
            GridValue::Bool(cv.msg(2).and_then(|b| b.boolean(1)).unwrap_or(false)),
            false,
        ),
        Some(3) => (
            cv.msg(3)
                .and_then(|d| d.f64v(1))
                .map(|s| GridValue::Scalar(crate::colors::iso_from_apple_seconds(s)))
                .unwrap_or(GridValue::None),
            true,
        ),
        Some(4) => (
            cv.msg(4)
                .and_then(|n| n.f64v(1))
                .map(GridValue::Number)
                .unwrap_or(GridValue::None),
            false,
        ),
        Some(5) => (
            cv.msg(5)
                .and_then(|s| s.string(1))
                .map(GridValue::Scalar)
                .unwrap_or(GridValue::None),
            false,
        ),
        _ => (GridValue::None, false),
    }
}

fn number_of(cv: Option<Msg>) -> Option<f64> {
    let cv = cv?;
    if cv.varint(1) != Some(4) {
        return None;
    }
    cv.msg(4).and_then(|n| n.f64v(1))
}

fn accum_totals(column: u32, agg_node: &Msg) -> Option<GroupTotal> {
    let acc = agg_node.msg(2)?;
    let t = GroupTotal {
        column,
        sum: number_of(acc.msg(8)),
        count: acc.varint(2).map(|v| v as u32),
        min: number_of(acc.msg(6)),
        max: number_of(acc.msg(7)),
    };
    (t.sum.is_some() || t.count.is_some()).then_some(t)
}

/// TSCE.CellCoordinateArchive { packedData 1, column 2, row 3 } as a key.
fn coord_key(c: &Msg) -> (u64, u64, u64) {
    (
        c.varint(1).unwrap_or(0),
        c.varint(2).unwrap_or(0),
        c.varint(3).unwrap_or(0),
    )
}

/// One aggregated column: its accumulators keyed by the agg node's
/// formula coordinate. Group nodes name their accumulators through
/// `agg_formula_coords` (f5); child ORDER differs between the two trees
/// (fixture-verified: positional matching swapped the two group sums).
struct Aggregator {
    column: u32,
    by_coord: HashMap<(u64, u64, u64), Msg>,
}

fn flatten_agg(node: &Msg, out: &mut HashMap<(u64, u64, u64), Msg>) {
    if let Some(c) = node.msg(1) {
        out.insert(coord_key(&c), node.clone());
    }
    for kid in node.msgs(3) {
        flatten_agg(&kid, out);
    }
}

struct Walk<'a> {
    ctx: &'a Ctx,
    lookup: &'a [u128],
    row_index: &'a HashMap<u128, u32>,
    aggregators: &'a [Aggregator],
}

impl Walk<'_> {
    fn children(&self, node: &Msg) -> Vec<Msg> {
        let mut kids = node.msgs(3);
        for r in node.references(10) {
            if let Some(m) = self.ctx.loaded.msg(r) {
                kids.push(m.clone());
            }
        }
        kids
    }

    fn rows(&self, node: &Msg) -> Vec<u32> {
        let mut out = Vec::new();
        if let Some(set) = node.msg(9) {
            for e in set.msgs(1) {
                let Some(b) = e.varint(1) else { continue };
                let end = e.varint(2).unwrap_or(b);
                for i in b..=end {
                    if let Some(r) = self
                        .lookup
                        .get(i as usize)
                        .and_then(|u| self.row_index.get(u))
                    {
                        out.push(*r);
                    }
                }
            }
        } else {
            for u in node.msgs(4) {
                if let Some(r) = uuid_u128(&u).and_then(|u| self.row_index.get(&u)) {
                    out.push(*r);
                }
            }
        }
        out.sort_unstable();
        out
    }

    fn totals(&self, node: &Msg) -> Option<Vec<GroupTotal>> {
        let coords: Vec<(u64, u64, u64)> = node.msgs(5).iter().map(coord_key).collect();
        let t: Vec<GroupTotal> = self
            .aggregators
            .iter()
            .filter_map(|a| {
                coords
                    .iter()
                    .find_map(|k| a.by_coord.get(k))
                    .and_then(|an| accum_totals(a.column, an))
            })
            .collect();
        (!t.is_empty()).then_some(t)
    }

    fn group(&self, node: &Msg) -> TableGroup {
        let (value, is_date) = node
            .msg(7)
            .map(|cv| cell_value(&cv))
            .unwrap_or((GridValue::None, false));
        let children: Vec<TableGroup> = self.children(node).iter().map(|k| self.group(k)).collect();
        TableGroup {
            value,
            date: is_date.then_some(true),
            rows: if children.is_empty() {
                let r = self.rows(node);
                (!r.is_empty()).then_some(r)
            } else {
                None
            },
            children: (!children.is_empty()).then_some(children),
            totals: self.totals(node),
        }
    }
}

/// The enabled grouping of a table model, or None when the table is not
/// organized by a column.
pub fn extract(ctx: &Ctx, m: &Msg) -> Option<TableGrouping> {
    let owner = ctx.loaded.msg(m.reference(86)?)?;
    let group_by = owner
        .references(1)
        .into_iter()
        .filter_map(|r| ctx.loaded.msg(r).cloned())
        .find(|g| g.boolean(6) == Some(true))?;
    let uidmap = ctx.loaded.msg(m.reference(46)?)?;
    let col_index = uid_index_map(uidmap, 1, 2);
    let row_index = uid_index_map(uidmap, 4, 5);
    let columns: Vec<u32> = group_by
        .msgs(2)
        .iter()
        .filter_map(|gc| gc.msg(1).as_ref().and_then(uuid_u128))
        .filter_map(|u| col_index.get(&u).copied())
        .collect();
    if columns.is_empty() {
        return None;
    }
    let lookup: Vec<u128> = group_by
        .msg(15)
        .map(|l| l.msgs(1).iter().filter_map(uuid_u128).collect())
        .unwrap_or_default();
    let aggregates: Vec<GroupAggregate> = group_by
        .msgs(5)
        .iter()
        .filter_map(|a| {
            let col = a.msg(1).as_ref().and_then(uuid_u128).and_then(|u| col_index.get(&u).copied())?;
            Some(GroupAggregate {
                column: col,
                rule: a.varint(3).unwrap_or(0) as u32,
                level: a.varint(2).map(|v| v as u32).filter(|v| *v != 1),
            })
        })
        .collect();
    let aggregators: Vec<Aggregator> = group_by
        .msgs(4)
        .iter()
        .filter_map(|a| {
            let column = a.msg(1).as_ref().and_then(uuid_u128).and_then(|u| col_index.get(&u).copied())?;
            let mut by_coord = HashMap::new();
            if let Some(root) = a.msg(2) {
                flatten_agg(&root, &mut by_coord);
            }
            Some(Aggregator { column, by_coord })
        })
        .collect();
    let root = group_by
        .msg(3)
        .or_else(|| group_by.reference(18).and_then(|r| ctx.loaded.msg(r).cloned()))?;
    let walk = Walk {
        ctx,
        lookup: &lookup,
        row_index: &row_index,
        aggregators: &aggregators,
    };
    let root_group = walk.group(&root);
    Some(TableGrouping {
        columns,
        aggregates: (!aggregates.is_empty()).then_some(aggregates),
        groups: root_group.children.unwrap_or_default(),
        totals: root_group.totals,
    })
}
