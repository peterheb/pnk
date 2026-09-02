//! Agent K corpus survey: for every incremental-save diff message, report
//! whether it carries diff_read_version (MessageInfo 11) / diff_merge_version
//! (8), and which base fields it changes.
use iwadump::proto::Value;
use iwadump::Document;
use std::collections::BTreeMap;

fn packed(patch: &[(u32, Value)], n: u32) -> Option<Vec<u64>> {
    patch
        .iter()
        .find(|(k, _)| *k == n)
        .and_then(|(_, v)| match v {
            Value::Bytes(b) => {
                let mut out = Vec::new();
                let mut i = 0;
                while i < b.len() {
                    let mut x = 0u64;
                    let mut s = 0;
                    loop {
                        if i >= b.len() {
                            return Some(out);
                        }
                        let c = b[i];
                        i += 1;
                        x |= ((c & 0x7f) as u64) << s;
                        s += 7;
                        if c & 0x80 == 0 {
                            break;
                        }
                    }
                    out.push(x);
                }
                Some(out)
            }
            _ => None,
        })
}

fn main() {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut n_files = 0;
    for arg in std::env::args().skip(1) {
        let Ok(doc) = Document::open(std::path::Path::new(&arg), false) else {
            continue;
        };
        n_files += 1;
        for s in &doc.streams {
            for a in &s.archives {
                if !a.should_merge {
                    continue;
                }
                for m in &a.messages {
                    if m.type_id != 0 {
                        continue;
                    }
                    let rv = packed(&m.patch, 11);
                    let mv = packed(&m.patch, 8);
                    let path = packed(&m.patch, 9).map(|_| ()).is_some();
                    let base_name = a
                        .messages
                        .first()
                        .and_then(|b| doc.registry.name_for(doc.app, b.type_id))
                        .unwrap_or_else(|| "?".into());
                    let key = format!(
                        "rv={:?} mv={:?} haspath={} base={}",
                        rv, mv, path, base_name
                    );
                    *counts.entry(key).or_insert(0) += 1;
                }
            }
        }
    }
    eprintln!("files={n_files}");
    let mut v: Vec<_> = counts.into_iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    for (k, c) in v.iter().take(60) {
        println!("{c:6}  {k}");
    }
}
