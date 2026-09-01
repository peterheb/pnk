//! Find storages whose text contains a needle and walk their referrers.
//! usage: refs_probe <file> <needle> [depth]
use iwadump::proto::Value;
use pnk2json::pb::Msg;
use std::collections::HashMap;

fn refs_of(m: &Msg, out: &mut Vec<u64>, depth: u32) {
    for f in &m.fields {
        match &f.value {
            Value::Varint(_) => {}
            Value::Bytes(b) => {
                if let Some(sub) = Msg::parse(b) {
                    // a {1: varint} wrapper is a TSP.Reference
                    if sub.fields.len() == 1 && sub.fields[0].number == 1 {
                        if let Value::Varint(v) = sub.fields[0].value {
                            out.push(v);
                            continue;
                        }
                    }
                    if depth > 0 {
                        refs_of(&sub, out, depth - 1);
                    }
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&args[1]);
    let needle = &args[2];
    let depth: u32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3);
    let (_doc, loaded) = pnk2json::loader::open_document(path).unwrap();
    let mut referrers: HashMap<u64, Vec<(u64, u32, Option<String>)>> = HashMap::new();
    for r in loaded.records.values() {
        if let Some(m) = &r.msg {
            let mut v = Vec::new();
            refs_of(m, &mut v, 4);
            for t in v {
                referrers.entry(t).or_default().push((r.id, r.type_id, r.name.clone()));
            }
        }
    }
    let mut hits: Vec<u64> = Vec::new();
    for r in loaded.records.values() {
        if r.type_id == 2001 {
            if let Some(m) = &r.msg {
                if m.string(3).map(|s| s.contains(needle.as_str())).unwrap_or(false) {
                    hits.push(r.id);
                }
            }
        }
    }
    hits.sort();
    for h in hits {
        println!("storage {h}");
        let mut frontier = vec![(h, 0u32)];
        while let Some((id, d)) = frontier.pop() {
            if d >= depth { continue; }
            for (rid, tid, name) in referrers.get(&id).cloned().unwrap_or_default() {
                println!("{}<- {} type={} {}", "  ".repeat(d as usize + 1), rid, tid, name.unwrap_or_default());
                frontier.push((rid, d + 1));
            }
        }
    }
}
