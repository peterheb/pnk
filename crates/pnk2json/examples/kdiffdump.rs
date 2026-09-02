//! Agent K: dump base message version + diff payload for should_merge archives
//! whose base type name matches a needle.
use iwadump::proto::Value;
use iwadump::Document;

fn dump(m: &pnk2json::pb::Msg, ind: &str) {
    for f in &m.fields {
        match &f.value {
            Value::Varint(v) => println!("{ind}f{}={v}", f.number),
            Value::Bytes(b) => {
                if let Some(s) = pnk2json::pb::Msg::parse(b) {
                    println!("{ind}f{}=msg[{}B]", f.number, b.len());
                    if ind.len() < 8 {
                        dump(&s, &format!("{ind}  "));
                    }
                } else {
                    println!(
                        "{ind}f{}=bytes {:?}",
                        f.number,
                        String::from_utf8_lossy(&b[..b.len().min(40)])
                    );
                }
            }
            Value::Fixed32(v) => println!("{ind}f{}=f32 {}", f.number, f32::from_le_bytes(*v)),
            Value::Fixed64(v) => println!("{ind}f{}=f64 {}", f.number, f64::from_le_bytes(*v)),
            Value::Group(_) => {}
        }
    }
}

fn main() {
    let arg = std::env::args().nth(1).unwrap();
    let needle = std::env::args().nth(2).unwrap();
    let limit: usize = std::env::args()
        .nth(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(2);
    let doc = Document::open(std::path::Path::new(&arg), false).unwrap();
    let mut shown = 0;
    for s in &doc.streams {
        for a in &s.archives {
            if !a.should_merge {
                continue;
            }
            let bn = a
                .messages
                .first()
                .and_then(|b| doc.registry.name_for(doc.app, b.type_id))
                .unwrap_or_default();
            if !bn.contains(&needle) {
                continue;
            }
            if shown >= limit {
                return;
            }
            shown += 1;
            println!("== archive {} base={bn}", a.identifier);
            for (i, m) in a.messages.iter().enumerate() {
                let pm: Vec<String> = m.patch.iter().map(|(n, v)| format!("f{n}={v:?}")).collect();
                println!("  msg[{i}] type={} version_meta={:?}", m.type_id, pm);
                if let Some(mm) = pnk2json::pb::Msg::parse(&m.payload) {
                    dump(&mm, "     ");
                }
            }
        }
    }
}
