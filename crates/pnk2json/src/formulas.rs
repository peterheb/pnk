//! TSCE formula text synthesis: a `TSCE.FormulaArchive` AST (a postfix node
//! list, docs/format/calcengine.md) → the text Numbers shows in its formula
//! editor. The walker is a stack machine over `ASTNodeArrayArchive.AST_node`
//! mirroring numbers-parser's `Formula` (formula.py) and `node_to_ref`
//! (model.py:962-1058) [parser: masaccio/numbers-parser@3238795]. Any node
//! kind, function id, or table uuid the walker does not know leaves the whole
//! formula undecoded (`TsceFormulaRef.status = "unparsed"`) — a partial or
//! guessed formula is worse than none.
//!
//! Operator spelling follows the app (and numbers-parser): `×`, `÷`, `≥`,
//! `≤`, `≠`; a date literal prints as `DATE(y,m,d)`; strings double their
//! quotes; relative references are resolved against the owning cell so the
//! text reads as it would in that cell.

use std::collections::HashMap;
use std::rc::Rc;

use crate::ctx::Ctx;
use crate::function_names::function_name;
use crate::pb::Msg;

/// Document-wide reference targets, built once per document.
#[derive(Debug, Default)]
pub struct FormulaNames {
    /// Table base-owner uuid → (sheet name, table name).
    tables: HashMap<u128, (String, String)>,
    /// Haunted-owner uuid (`TableModelArchive` f84) → base-owner uuid, via
    /// the HAUNTED_OWNER (kind 35) `FormulaOwnerDependenciesArchive`
    /// (f1 → f12). Cross-table references name the BASE uuid.
    base_of_haunted: HashMap<u128, u128>,
    /// Table name → occurrences in the document (unique names need no
    /// sheet prefix).
    name_counts: HashMap<String, usize>,
}

impl FormulaNames {
    /// The base-owner uuid a table model's haunted uid maps to (itself when
    /// the document has no HAUNTED_OWNER mapping — older files).
    pub fn base_uid(&self, haunted: u128) -> u128 {
        self.base_of_haunted
            .get(&haunted)
            .copied()
            .unwrap_or(haunted)
    }

    pub fn sheet_of(&self, base: u128) -> Option<&str> {
        self.tables.get(&base).map(|(s, _)| s.as_str())
    }
}

/// Build (or fetch the cached) name map: walks every `TN.SheetArchive` →
/// drawable_infos → `TST.TableInfoArchive` (6000/6007) → model (name f8,
/// haunted owner f84).
pub fn names(ctx: &mut Ctx) -> Rc<FormulaNames> {
    if let Some(n) = &ctx.formula_names {
        return n.clone();
    }
    let mut out = FormulaNames::default();
    for rec in ctx.loaded.records.values() {
        if rec.type_id != 4008 {
            continue;
        }
        let Some(fo) = rec.msg.as_ref() else { continue };
        if fo.varint(3) != Some(35) {
            continue;
        }
        if let (Some(h), Some(b)) = (
            fo.msg(1).as_ref().and_then(uuid_u128),
            fo.msg(12).as_ref().and_then(uuid_u128),
        ) {
            out.base_of_haunted.insert(h, b);
        }
    }
    let sheets: Vec<Msg> = ctx
        .loaded
        .records
        .values()
        .filter(|r| r.name.as_deref() == Some("TN.SheetArchive"))
        .filter_map(|r| r.msg.clone())
        .collect();
    for sheet in sheets {
        let sheet_name = sheet.string(1).unwrap_or_default();
        for info_id in sheet.references(2) {
            let Some(info) = ctx.loaded.records.get(&info_id) else {
                continue;
            };
            if !matches!(info.type_id, 6000 | 6007) {
                continue;
            }
            let Some(model) = info
                .msg
                .as_ref()
                .and_then(|m| m.reference(2))
                .and_then(|mid| ctx.loaded.msg(mid))
            else {
                continue;
            };
            let table_name = model.string(8).unwrap_or_default();
            let entry = (sheet_name.clone(), table_name.clone());
            // Three keys name the same table across format generations:
            // the model's own `table_id` string (f1) — what a 2020-era
            // Numbers writes into cross-table AST nodes as a CFUUID
            // (fixture 16c9478d6d21) — plus the haunted uid (f84) and the
            // base-owner uid it maps to (modern files).
            let mut keyed = false;
            if let Some(u) = model.string(1).as_deref().and_then(uuid_from_string) {
                out.tables.insert(u, entry.clone());
                keyed = true;
            }
            if let Some(haunted) = model
                .msg(84)
                .and_then(|h| h.msg(1))
                .as_ref()
                .and_then(uuid_u128)
            {
                let base = out.base_uid(haunted);
                out.tables.insert(base, entry.clone());
                out.tables.insert(haunted, entry.clone());
                keyed = true;
            }
            if keyed {
                *out.name_counts.entry(table_name).or_insert(0) += 1;
            }
        }
    }
    if std::env::var("PNK_DEBUG_FORMULA").is_ok() {
        eprintln!(
            "formula names: {} tables, {} haunted mappings: {:?}",
            out.tables.len(),
            out.base_of_haunted.len(),
            out.tables
                .iter()
                .map(|(k, v)| format!("{k:032x}={}/{}", v.0, v.1))
                .collect::<Vec<_>>()
        );
    }
    let rc = Rc::new(out);
    ctx.formula_names = Some(rc.clone());
    rc
}

/// A `TableModelArchive.table_id` string ("BF6BAE7D-1E29-4444-8CBA-…") →
/// the same u128 `uuid_u128` builds from a CFUUID: the 16 bytes read as
/// four little-endian u32 words (w0 = bytes 0-3, …), w3 most significant.
/// [fixture-verified: 16c9478d6d21 — AST words 7dae6bbf/4444291e/
/// 4d58ba8c/974c36f4 name model "BF6BAE7D-1E29-4444-8CBA-584DF4364C97"]
pub fn uuid_from_string(s: &str) -> Option<u128> {
    let hex: String = s.chars().filter(|c| *c != '-').collect();
    if hex.len() != 32 {
        return None;
    }
    let mut bytes = [0u8; 16];
    for (i, b) in bytes.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    let w = |i: usize| {
        u32::from_le_bytes([
            bytes[4 * i],
            bytes[4 * i + 1],
            bytes[4 * i + 2],
            bytes[4 * i + 3],
        ]) as u128
    };
    Some((w(3) << 96) | (w(2) << 64) | (w(1) << 32) | w(0))
}

/// `TSP.CFUUIDArchive` / `TSP.UUID` → u128 (both wire layouts: four u32
/// words f2-f5, or lower/upper u64 f1/f2).
pub fn uuid_u128(m: &Msg) -> Option<u128> {
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

/// The table a formula belongs to: needed to resolve relative references
/// and to decide which cross-table references need a name prefix.
pub struct TableScope<'a> {
    pub names: &'a FormulaNames,
    /// This table's base-owner uuid (references to it carry no prefix).
    pub self_uid: Option<u128>,
    /// This table's sheet name (same-sheet references skip the sheet prefix).
    pub sheet: Option<&'a str>,
    /// Chart-binding scope: the formula belongs to a `TN.ChartMediatorArchive`,
    /// not a cell. Function id 175 — absent from numbers-parser's table and
    /// present as the outermost node of every one of the 4,170 binding
    /// formulas in the 158-file corpus, wrapping one range (or two for a
    /// union) — is the chart-series wrapper; in this scope it prints as its
    /// argument list. Anywhere else it stays unknown. [inferred]
    pub chart: bool,
}

fn sint(m: &Msg, n: u32) -> Option<i64> {
    m.varint(n).map(|v| ((v >> 1) as i64) ^ -((v & 1) as i64))
}

/// 0-based column → A, B, …, Z, AA, ….
pub fn column_name(mut c: i64) -> String {
    let mut s = Vec::new();
    loop {
        s.push(b'A' + (c % 26) as u8);
        c = c / 26 - 1;
        if c < 0 {
            break;
        }
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

fn cell_name(row: i64, col: i64, row_abs: bool, col_abs: bool) -> String {
    format!(
        "{}{}{}{}",
        if col_abs { "$" } else { "" },
        column_name(col),
        if row_abs { "$" } else { "" },
        row + 1
    )
}

/// Civil date from seconds since 2001-01-01T00:00:00 (proleptic Gregorian;
/// Howard Hinnant's days-to-civil).
fn civil_from_apple_seconds(secs: f64) -> (i64, u32, u32) {
    let days = (secs / 86_400.0).floor() as i64 + 11_323; // 2001-01-01 is day 11323 after 1970-01-01
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn number_text(node: &Msg) -> String {
    // decimal128 high word 0x3040000000000000 = exponent 0: an integer, print
    // the low word verbatim (numbers-parser formula.py `number`).
    if node.varint(43) == Some(0x3040_0000_0000_0000) {
        if let Some(low) = node.varint(42) {
            return low.to_string();
        }
    }
    let v = node.f64v(4).unwrap_or(0.0);
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// Table-name prefix for a reference carrying cross-table extra info
/// (node f28 → table_id f1). Empty for this table; `Table::` when the name
/// is unique in the document or on the same sheet; `Sheet::Table::`
/// otherwise. Unknown uuids fail the decode.
fn table_prefix(scope: &TableScope, node: &Msg) -> Result<String, String> {
    let Some(x) = node.msg(28) else {
        return Ok(String::new());
    };
    let uid = x
        .msg(1)
        .as_ref()
        .and_then(uuid_u128)
        .ok_or("cross-table reference without a table uuid")?;
    let base = scope.names.base_uid(uid);
    if Some(base) == scope.self_uid || Some(uid) == scope.self_uid {
        return Ok(String::new());
    }
    let (sheet, table) = scope
        .names
        .tables
        .get(&base)
        .or_else(|| scope.names.tables.get(&uid))
        .ok_or_else(|| format!("reference to unknown table uuid {uid:032x}"))?;
    let unique = scope.names.name_counts.get(table).copied().unwrap_or(0) <= 1;
    if scope.sheet == Some(sheet.as_str()) || unique {
        Ok(format!("{table}::"))
    } else {
        Ok(format!("{sheet}::{table}::"))
    }
}

/// CELL_REFERENCE_NODE (36): AST_column f26 {column sint32, absolute},
/// AST_row f27 {row sint32, absolute}; relative offsets add to the owning
/// cell. Row-only / column-only nodes are whole-row / whole-column refs.
fn cell_reference(scope: &TableScope, node: &Msg, row: u32, col: u32) -> Result<String, String> {
    let prefix = table_prefix(scope, node)?;
    let rowm = node.msg(27);
    let colm = node.msg(26);
    let resolve = |m: &Msg, base: u32| -> Option<(i64, bool)> {
        let v = sint(m, 1)?;
        let abs = m.boolean(2).unwrap_or(false);
        Some((if abs { v } else { base as i64 + v }, abs))
    };
    let r = rowm.as_ref().map(|m| resolve(m, row));
    let c = colm.as_ref().map(|m| resolve(m, col));
    let body = match (r, c) {
        (Some(Some((r, ra))), Some(Some((c, ca)))) => {
            if r < 0 || c < 0 {
                return Ok("#REF!".into());
            }
            cell_name(r, c, ra, ca)
        }
        (Some(Some((r, ra))), None) => {
            if r < 0 {
                return Ok("#REF!".into());
            }
            let d = if ra { "$" } else { "" };
            format!("{d}{}:{d}{}", r + 1, r + 1)
        }
        (None, Some(Some((c, ca)))) => {
            if c < 0 {
                return Ok("#REF!".into());
            }
            format!("{}{}", if ca { "$" } else { "" }, column_name(c))
        }
        _ => return Err("cell reference node without coordinates".into()),
    };
    Ok(prefix + &body)
}

/// COLON_TRACT_NODE (67): AST_colon_tract f40 { relative_column 1,
/// relative_row 2, absolute_column 3, absolute_row 4: [{range_begin,
/// range_end?}] } with AST_sticky_bits f33 { begin_row_abs 1,
/// begin_col_abs 2, end_row_abs 3, end_col_abs 4 }. A begin of 0x7FFFFFFF
/// (rows) / 0x7FFF (columns) with no relative entry means "unbounded" —
/// a whole-column / whole-row range. [parser: numbers-parser model.py
/// node_to_ref resolve_range / resolve_range_end]
fn colon_tract(scope: &TableScope, node: &Msg, row: u32, col: u32) -> Result<String, String> {
    let prefix = table_prefix(scope, node)?;
    let tract = node.msg(40).ok_or("colon tract node without a tract")?;
    let sticky = node.msg(33).ok_or("colon tract node without sticky bits")?;
    let bits = |n| sticky.boolean(n).unwrap_or(false);
    let (br_abs, bc_abs, er_abs, ec_abs) = (bits(1), bits(2), bits(3), bits(4));
    let rel_col = tract.msgs(1);
    let rel_row = tract.msgs(2);
    let abs_col = tract.msgs(3);
    let abs_row = tract.msgs(4);
    // (begin, end) of one axis, None = unbounded on that axis.
    let axis = |begin_abs: bool,
                end_abs: bool,
                abs: &[Msg],
                rel: &[Msg],
                base: u32,
                max: i64|
     -> Result<Option<(i64, i64)>, String> {
        let abs_begin = abs.first().and_then(|m| m.varint(1)).map(|v| v as i64);
        let abs_end = abs
            .first()
            .and_then(|m| m.varint(2).or_else(|| m.varint(1)))
            .map(|v| v as i64);
        let rel_begin = rel.first().and_then(|m| sint(m, 1));
        let rel_end = rel.first().and_then(|m| sint(m, 2).or_else(|| sint(m, 1)));
        let begin = if begin_abs {
            abs_begin.ok_or("absolute range without an absolute entry")?
        } else if rel.is_empty() && abs_begin == Some(max) {
            max
        } else {
            base as i64 + rel_begin.ok_or("relative range without a relative entry")?
        };
        let end = if end_abs {
            abs_end.ok_or("absolute range without an absolute entry")?
        } else if rel.is_empty() && abs_end == Some(max) {
            max
        } else {
            base as i64 + rel_end.ok_or("relative range without a relative entry")?
        };
        if begin == max || end == max {
            return Ok(None);
        }
        Ok(Some((begin, end)))
    };
    let rows = axis(br_abs, er_abs, &abs_row, &rel_row, row, 0x7FFF_FFFF)?;
    let cols = axis(bc_abs, ec_abs, &abs_col, &rel_col, col, 0x7FFF)?;
    let dollar = |b: bool| if b { "$" } else { "" };
    let body = match (rows, cols) {
        (Some((r1, r2)), Some((c1, c2))) => {
            if r1 < 0 || r2 < 0 || c1 < 0 || c2 < 0 {
                return Ok("#REF!".into());
            }
            format!(
                "{}:{}",
                cell_name(r1, c1, br_abs, bc_abs),
                cell_name(r2, c2, er_abs, ec_abs)
            )
        }
        (Some((r1, r2)), None) => {
            if r1 < 0 || r2 < 0 {
                return Ok("#REF!".into());
            }
            format!("{}{}:{}{}", dollar(br_abs), r1 + 1, dollar(er_abs), r2 + 1)
        }
        (None, Some((c1, c2))) => {
            if c1 < 0 || c2 < 0 {
                return Ok("#REF!".into());
            }
            format!(
                "{}{}:{}{}",
                dollar(bc_abs),
                column_name(c1),
                dollar(ec_abs),
                column_name(c2)
            )
        }
        (None, None) => return Err("colon tract unbounded on both axes".into()),
    };
    Ok(prefix + &body)
}

/// Join two references into a range, hoisting a shared `Table::` prefix
/// (numbers-parser formula.py `range`).
fn join_range(a: String, b: String) -> String {
    if let Some(p) = a.rfind("::") {
        let prefix = &a[..p + 2];
        if let Some(rest) = b.strip_prefix(prefix) {
            return format!("{a}:{rest}");
        }
    }
    format!("{a}:{b}")
}

/// Decode one `TSCE.FormulaArchive` (the FORMULA data-list entry's field 5)
/// for the cell at (row, col). `Err` names the first unsupported thing.
pub fn decode(scope: &TableScope, formula: &Msg, row: u32, col: u32) -> Result<String, String> {
    let array = formula.msg(1).ok_or("formula without an AST node array")?;
    let nodes = array.msgs(1);
    if nodes.is_empty() {
        return Err("formula with no AST nodes".into());
    }
    let mut stack: Vec<String> = Vec::new();
    fn pop2(stack: &mut Vec<String>) -> Result<(String, String), String> {
        let b = stack.pop().ok_or("operator with an empty stack")?;
        let a = stack.pop().ok_or("operator with an empty stack")?;
        Ok((a, b))
    }
    fn popn(stack: &mut Vec<String>, n: usize) -> Result<Vec<String>, String> {
        if stack.len() < n {
            return Err(format!(
                "{n} arguments requested, {} on the stack",
                stack.len()
            ));
        }
        Ok(stack.split_off(stack.len() - n))
    }
    for node in &nodes {
        let kind = node.varint(1).unwrap_or(0);
        let binary = |stack: &mut Vec<String>, op: &str| -> Result<(), String> {
            let (a, b) = pop2(stack)?;
            stack.push(format!("{a}{op}{b}"));
            Ok(())
        };
        match kind {
            1 => binary(&mut stack, "+")?,
            2 => binary(&mut stack, "-")?,
            3 => binary(&mut stack, "×")?,
            4 => binary(&mut stack, "÷")?,
            5 => binary(&mut stack, "^")?,
            6 => binary(&mut stack, "&")?,
            7 => binary(&mut stack, ">")?,
            8 => binary(&mut stack, "≥")?,
            9 => binary(&mut stack, "<")?,
            10 => binary(&mut stack, "≤")?,
            11 => binary(&mut stack, "=")?,
            12 => binary(&mut stack, "≠")?,
            13 => {
                let a = stack.pop().ok_or("negation with an empty stack")?;
                stack.push(format!("-{a}"));
            }
            14 => {} // unary plus: not printed
            15 => {
                let a = stack.pop().ok_or("percent with an empty stack")?;
                stack.push(format!("{a}%"));
            }
            16 if scope.chart && node.varint(2) == Some(175) => {
                let nargs = node.varint(3).unwrap_or(0) as usize;
                let args = popn(&mut stack, nargs)?;
                stack.push(args.join(","));
            }
            16 | 31 => {
                let (name, nargs) = if kind == 16 {
                    let idx = node.varint(2).unwrap_or(0) as u32;
                    (
                        function_name(idx)
                            .ok_or_else(|| format!("unknown function id {idx}"))?
                            .to_string(),
                        node.varint(3).unwrap_or(0) as usize,
                    )
                } else {
                    (
                        node.string(17)
                            .ok_or("unknown-function node without a name")?,
                        node.varint(18).unwrap_or(0) as usize,
                    )
                };
                let args = popn(&mut stack, nargs)?;
                stack.push(format!("{name}({})", args.join(",")));
            }
            17 => stack.push(number_text(node)),
            18 => stack.push(
                if node.boolean(5).unwrap_or(false) {
                    "TRUE"
                } else {
                    "FALSE"
                }
                .into(),
            ),
            23 => stack.push(
                if node.boolean(10).unwrap_or(false) {
                    "TRUE"
                } else {
                    "FALSE"
                }
                .into(),
            ),
            19 => {
                let s = node.string(6).unwrap_or_default().replace('"', "\"\"");
                stack.push(format!("\"{s}\""));
            }
            20 => {
                let secs = node.f64v(7).ok_or("date node without a value")?;
                let (y, m, d) = civil_from_apple_seconds(secs);
                stack.push(format!("DATE({y},{m},{d})"));
            }
            22 => stack.push(String::new()),
            24 => {
                let ncol = node.varint(11).unwrap_or(0) as usize;
                let nrow = node.varint(12).unwrap_or(1) as usize;
                let mut rows = Vec::with_capacity(nrow);
                for _ in 0..nrow {
                    rows.push(popn(&mut stack, ncol)?.join(","));
                }
                rows.reverse();
                stack.push(format!("{{{}}}", rows.join(";")));
            }
            25 => {
                let n = node.varint(13).unwrap_or(0) as usize;
                let args = popn(&mut stack, n)?;
                stack.push(format!("({})", args.join(",")));
            }
            29 | 45 => {
                let (a, b) = pop2(&mut stack)?;
                stack.push(join_range(a, b));
            }
            30 | 46 => stack.push("#REF!".into()),
            32..=35 => {} // whitespace / thunk brackets: not printed
            36 => stack.push(cell_reference(scope, node, row, col)?),
            67 => stack.push(colon_tract(scope, node, row, col)?),
            other => return Err(format!("unsupported AST node type {other}")),
        }
    }
    if stack.len() != 1 {
        return Err(format!("{} values left on the stack", stack.len()));
    }
    Ok(stack.pop().unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_names() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(701), "ZZ");
        assert_eq!(column_name(702), "AAA");
    }

    #[test]
    fn apple_epoch_dates() {
        assert_eq!(civil_from_apple_seconds(0.0), (2001, 1, 1));
        assert_eq!(civil_from_apple_seconds(86_400.0 * 59.0), (2001, 3, 1));
        assert_eq!(civil_from_apple_seconds(-86_400.0), (2000, 12, 31));
    }

    #[test]
    fn table_id_string_matches_cfuuid_words() {
        let from_words = ((0x974c36f4u128) << 96)
            | ((0x4d58ba8cu128) << 64)
            | ((0x4444291eu128) << 32)
            | 0x7dae6bbfu128;
        assert_eq!(
            uuid_from_string("BF6BAE7D-1E29-4444-8CBA-584DF4364C97"),
            Some(from_words)
        );
    }

    #[test]
    fn range_prefix_hoisting() {
        assert_eq!(join_range("A1".into(), "B2".into()), "A1:B2");
        assert_eq!(join_range("T::A1".into(), "T::B2".into()), "T::A1:B2");
        assert_eq!(
            join_range("S::T::A1".into(), "S::T::B2".into()),
            "S::T::A1:B2"
        );
    }
}
