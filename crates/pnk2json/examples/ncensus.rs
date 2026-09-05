//! Agent-N census (round 4): what spreadsheet metadata the corpus carries
//! that the JSON model does not. usage: ncensus <file>...
//! Prints one line per table model: hidden rows/cols (header buckets and
//! model counters), sort order, hidden-state/filter sets, comments,
//! control cells, conditional styles, multiple-choice lists, summary model.
use pnk2json::pb::Msg;

fn header_hidden(loaded: &pnk2json::loader::Loaded, hs: Option<Msg>) -> (usize, usize) {
    let Some(hs) = hs else { return (0, 0) };
    let (mut n, mut hidden) = (0, 0);
    for f in &hs.fields {
        if f.number != 2 {
            continue;
        }
        let iwadump::proto::Value::Bytes(b) = &f.value else {
            continue;
        };
        let Some(inner) = Msg::parse(b) else { continue };
        let bucket = if inner.fields.len() == 1 {
            inner.varint(1).and_then(|id| loaded.msg(id).cloned())
        } else {
            Some(inner)
        };
        let Some(bucket) = bucket else { continue };
        for h in bucket.msgs(2) {
            n += 1;
            if h.varint(3).unwrap_or(0) != 0 {
                hidden += 1;
            }
        }
    }
    (n, hidden)
}

fn list_len(loaded: &pnk2json::loader::Loaded, id: Option<u64>) -> usize {
    id.and_then(|i| loaded.msg(i))
        .map(|m| m.msgs(3).len())
        .unwrap_or(0)
}

fn main() {
    for path in std::env::args().skip(1) {
        let p = std::path::Path::new(&path);
        let Ok((_doc, loaded)) = pnk2json::loader::open_document(p) else {
            println!("{path}\tOPEN-FAIL");
            continue;
        };
        let short = p.file_name().unwrap().to_string_lossy()[..12].to_string();
        let sheets: Vec<&pnk2json::loader::Record> = loaded
            .records
            .values()
            .filter(|r| r.name.as_deref() == Some("TN.SheetArchive"))
            .collect();
        let hidden_sheets = sheets
            .iter()
            .filter(|r| r.msg.as_ref().and_then(|m| m.boolean(25)) == Some(true))
            .count();
        println!(
            "{short}\tDOC\tsheets={}\thidden_sheets={hidden_sheets}",
            sheets.len()
        );
        // TableInfo -> summary model, per model id
        let mut summary: std::collections::HashMap<u64, u64> = Default::default();
        for r in loaded.records.values() {
            if !matches!(r.type_id, 6000 | 6007) {
                continue;
            }
            let Some(m) = r.msg.as_ref() else { continue };
            if let (Some(mid), Some(sm)) = (m.reference(2), m.reference(4)) {
                summary.insert(mid, sm);
            }
        }
        for r in loaded.records.values() {
            if r.type_id != 6001 {
                continue;
            }
            let Some(m) = r.msg.as_ref() else { continue };
            let store = m.msg(4);
            let (rows, rows_hidden) = header_hidden(&loaded, store.as_ref().and_then(|s| s.msg(1)));
            let (cols, cols_hidden) = header_hidden(
                &loaded,
                store
                    .as_ref()
                    .and_then(|s| s.reference(2))
                    .and_then(|id| loaded.msg(id).cloned()),
            );
            let sort = m.msg(44).map(|s| s.msgs(2).len()).unwrap_or(0);
            let mut user_hidden = 0;
            let mut filtered = 0;
            let mut filter_rules = 0;
            let mut filter_enabled = false;
            if let Some(hso) = m.msg(70) {
                for hs in hso.msgs(2) {
                    for ext in [hs.msg(2), hs.msg(3)].into_iter().flatten() {
                        for st in ext.msgs(2) {
                            if st.boolean(2) == Some(true) {
                                user_hidden += 1;
                            }
                            if st.boolean(3) == Some(true) {
                                filtered += 1;
                            }
                        }
                        if let Some(fs) = ext.reference(8).and_then(|id| loaded.msg(id)) {
                            filter_rules += fs.msgs(7).len() + fs.msgs(3).len();
                            filter_enabled |= fs.boolean(2) != Some(false)
                                && !(fs.msgs(7).is_empty() && fs.msgs(3).is_empty());
                        }
                    }
                }
            }
            let comments = list_len(&loaded, store.as_ref().and_then(|s| s.reference(19)));
            let controls = list_len(&loaded, store.as_ref().and_then(|s| s.reference(21)));
            let conds = list_len(&loaded, store.as_ref().and_then(|s| s.reference(18)));
            let choices = list_len(&loaded, store.as_ref().and_then(|s| s.reference(16)));
            let sm_width = summary
                .get(&r.id)
                .and_then(|id| loaded.msg(*id))
                .and_then(|sm| sm.f64v(10));
            let cat = m.reference(86).is_some();
            let interesting = rows_hidden
                + cols_hidden
                + sort
                + user_hidden
                + filtered
                + filter_rules
                + comments
                + controls
                + conds
                + choices
                > 0
                || m.varint(14).unwrap_or(0)
                    + m.varint(15).unwrap_or(0)
                    + m.varint(40).unwrap_or(0)
                    + m.varint(41).unwrap_or(0)
                    + m.varint(42).unwrap_or(0)
                    > 0
                || sm_width.is_some();
            if !interesting {
                continue;
            }
            println!(
                "{short}\tTABLE\t{}\trows={rows}/{}\tcols={cols}/{}\thid_r={rows_hidden}\thid_c={cols_hidden}\tf14={:?}\tf15={:?}\tf40={:?}\tf41={:?}\tf42={:?}\tsort={sort}\tuser_hidden={user_hidden}\tfiltered={filtered}\tfilter_rules={filter_rules}\tfilter_on={filter_enabled}\tcomments={comments}\tcontrols={controls}\tconds={conds}\tchoices={choices}\tcat={cat}\tsummary_w={sm_width:?}",
                m.string(8).unwrap_or_default(),
                m.varint(6).unwrap_or(0),
                m.varint(7).unwrap_or(0),
                m.varint(14), m.varint(15), m.varint(40), m.varint(41), m.varint(42),
            );
        }
    }
}
