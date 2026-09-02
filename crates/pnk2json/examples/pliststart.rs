//! Dump TSWP.StorageArchive's per-paragraph list tables for the BIGGEST text
//! storage in a document, next to the text each entry sits on:
//! table_para_data (6) = list level, table_para_starts (14), table_list_style
//! (7) = list-style ranges. Answers "does Apple restart numbering at this
//! paragraph, or continue?" for a fixture.
//! usage: pliststart <file>
use iwadump::proto::Value;
use pnk2json::pb::Msg;

fn entries(m: Option<Msg>) -> Vec<(u64, Vec<(u32, u64)>)> {
    let mut out = Vec::new();
    let Some(m) = m else { return out };
    for e in m.msgs(1) {
        let idx = e.varint(1).unwrap_or(0);
        let rest: Vec<(u32, u64)> = e
            .fields
            .iter()
            .filter(|f| f.number != 1)
            .filter_map(|f| match f.value {
                Value::Varint(v) => Some((f.number, v)),
                _ => None,
            })
            .collect();
        out.push((idx, rest));
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&args[1]);
    let (_doc, loaded) = pnk2json::loader::open_document(path).unwrap();
    let mut best: Option<(u64, String, Msg)> = None;
    for (id, r) in loaded.records.iter() {
        let Some(m) = &r.msg else { continue };
        let txt: String = m
            .all(3)
            .iter()
            .filter_map(|v| match v {
                Value::Bytes(b) => String::from_utf8(b.to_vec()).ok(),
                _ => None,
            })
            .collect();
        if txt.is_empty() {
            continue;
        }
        if best.as_ref().is_none_or(|(_, t, _)| t.len() < txt.len()) {
            best = Some((*id, txt, m.clone()));
        }
    }
    let Some((id, txt, m)) = best else { return };
    let u16s: Vec<u16> = txt.encode_utf16().collect();
    let at = |i: u64| -> String {
        let s = i as usize;
        let e = (s + 46).min(u16s.len());
        if s >= u16s.len() {
            return String::new();
        }
        String::from_utf16_lossy(&u16s[s..e]).replace('\n', "\\n")
    };
    println!("== storage {id}, {} utf16 units", u16s.len());
    for (name, f) in [("para_data(6)", 6u32), ("para_starts(14)", 14)] {
        let es = entries(m.msg(f));
        println!("  {name}: {} entries", es.len());
        for (idx, rest) in es.iter().take(80) {
            println!("     @{idx:6} {rest:?}  {:?}", at(*idx));
        }
    }
}
