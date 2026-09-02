//! Agent K: for the chart archives, print the raw (pre-merge) payload field
//! numbers and any patch messages targeting them.
use iwadump::proto::Value;
use iwadump::Document;

fn main() {
    let arg = std::env::args().nth(1).unwrap();
    let want: Vec<u64> = std::env::args()
        .skip(2)
        .filter_map(|s| s.parse().ok())
        .collect();
    let doc = Document::open(std::path::Path::new(&arg), false).unwrap();
    for s in &doc.streams {
        for a in &s.archives {
            if !want.is_empty() && !want.contains(&a.identifier) {
                continue;
            }
            let name0 = a
                .messages
                .first()
                .and_then(|m| doc.registry.name_for(doc.app, m.type_id));
            if want.is_empty() && !name0.as_deref().is_some_and(|n| n.contains("Chart")) {
                continue;
            }
            println!(
                "== {} archive {} merge={} ({} messages) {:?}",
                s.name,
                a.identifier,
                a.should_merge,
                a.messages.len(),
                name0
            );
            for (i, m) in a.messages.iter().enumerate() {
                let name = doc.registry.name_for(doc.app, m.type_id);
                let pm: Vec<String> = m
                    .patch
                    .iter()
                    .map(|(n, v)| match v {
                        Value::Varint(x) => format!("f{n}={x}"),
                        Value::Bytes(b) => format!("f{n}=bytes{:?}", b),
                        _ => format!("f{n}=?"),
                    })
                    .collect();
                let fields = pnk2json::pb::Msg::parse(&m.payload).map(|mm| {
                    mm.fields
                        .iter()
                        .map(|f| match &f.value {
                            Value::Varint(v) => format!("f{}={}", f.number, v),
                            Value::Bytes(b) => format!("f{}=[{}B]", f.number, b.len()),
                            _ => format!("f{}=~", f.number),
                        })
                        .collect::<Vec<_>>()
                });
                println!(
                    "  msg[{i}] type={} {:?} len={} patch={:?}",
                    m.type_id, name, m.length, pm
                );
                println!("     payload: {:?}", fields);
                if let Some(mm) = pnk2json::pb::Msg::parse(&m.payload) {
                    if let Some(ca) = mm.msg(10000) {
                        println!(
                            "     BASE chart_type={:?} nonstyle_ref={:?}",
                            ca.varint(1),
                            ca.reference(10)
                        );
                    }
                }
                if m.type_id == 0 {
                    println!("     raw: {:02x?}", &m.payload[..m.payload.len().min(64)]);
                }
            }
        }
    }
}
