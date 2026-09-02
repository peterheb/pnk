//! Agent-N scratch probe: survey TST table models and merge sources.
//! usage: nprobe <file> [mode]
use iwadump::proto::Value;
use pnk2json::pb::Msg;

fn dump(m: &Msg, prefix: &str, depth: u32) {
    for f in &m.fields {
        match &f.value {
            Value::Bytes(b) => {
                let sub = Msg::parse(b);
                let s = String::from_utf8_lossy(&b[..b.len().min(40)]).to_string();
                let printable = s.chars().all(|c| !c.is_control()) && !s.is_empty();
                println!(
                    "{prefix}f{} len={} {}",
                    f.number,
                    b.len(),
                    if printable {
                        format!("str={s:?}")
                    } else {
                        String::new()
                    }
                );
                if depth > 0 {
                    if let Some(sub) = sub {
                        dump(&sub, &format!("{prefix}  "), depth - 1);
                    }
                }
            }
            v => println!("{prefix}f{} {:?}", f.number, v),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&args[1]);
    let mode = args.get(2).map(|s| s.as_str()).unwrap_or("summary");
    let (_doc, loaded) = pnk2json::loader::open_document(path).unwrap();
    match mode {
        "find" => {
            let want: u32 = args[3].parse().unwrap();
            for r in loaded.records.values() {
                if r.type_id == want {
                    println!("record {} type {}", r.id, r.type_id);
                }
            }
        }
        "types" => {
            let mut counts: std::collections::BTreeMap<u32, usize> = Default::default();
            for r in loaded.records.values() {
                *counts.entry(r.type_id).or_default() += 1;
            }
            for (t, c) in counts {
                println!("type {t} x{c}");
            }
        }
        "tables" => {
            for r in loaded.records.values() {
                if r.type_id != 6001 {
                    continue;
                }
                let Some(m) = &r.msg else { continue };
                println!(
                    "table {} name={:?} rows={:?} cols={:?} f47={:?} f84={:?} store13={:?}",
                    r.id,
                    m.string(8),
                    m.varint(5),
                    m.varint(6),
                    m.msg(47).is_some(),
                    m.msg(84).is_some(),
                    m.msg(4).and_then(|s| s.reference(13)),
                );
            }
        }
        "dump" => {
            for ids in args[3..].iter() {
                let id: u64 = ids.parse().unwrap();
                println!(
                    "== record {id} type={:?}",
                    loaded.record(id).map(|r| r.type_id)
                );
                if let Some(m) = loaded.msg(id) {
                    dump(m, "  ", 9);
                }
            }
        }
        _ => {
            println!("records: {}", loaded.records.len());
        }
    }
}
