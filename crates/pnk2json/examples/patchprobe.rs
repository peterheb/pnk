//! Inspect pathless (whole-message) incremental patches: dump base vs patch
//! field numbers to verify merge semantics before implementing them.
use iwadump::proto::Value;
use iwadump::Document;

fn field_nums(payload: &[u8]) -> Option<Vec<(u32, &'static str)>> {
    pnk2json::pb::Msg::parse(payload).map(|m| {
        m.fields
            .iter()
            .map(|f| (f.number, iwadump::proto::wire_name(f.wire)))
            .collect()
    })
}

fn main() {
    let mut shown = 0;
    for arg in std::env::args().skip(1) {
        let Ok(doc) = Document::open(std::path::Path::new(&arg), false) else {
            continue;
        };
        for s in &doc.streams {
            for a in &s.archives {
                if !a.should_merge {
                    continue;
                }
                let has_pathless = a.messages.iter().any(|m| {
                    m.type_id == 0 && !m.payload.is_empty() && !m.patch.iter().any(|(n, _)| *n == 9)
                });
                if !has_pathless {
                    continue;
                }
                println!(
                    "== {} archive {} ({} messages)",
                    arg,
                    a.identifier,
                    a.messages.len()
                );
                for (i, m) in a.messages.iter().enumerate() {
                    let name = doc.registry.name_for(doc.app, m.type_id);
                    let patch_meta: Vec<String> = m
                        .patch
                        .iter()
                        .map(|(n, v)| match v {
                            Value::Varint(x) => format!("f{n}={x}"),
                            Value::Bytes(b) => format!(
                                "f{n}=bytes[{}]{:?}",
                                b.len(),
                                pnk2json::pb::Msg::parse(b).map(|mm| mm.packed_varints(1))
                            ),
                            _ => format!("f{n}=?"),
                        })
                        .collect();
                    println!(
                        "  msg[{i}] type={} name={:?} len={} patch_meta={:?}",
                        m.type_id, name, m.length, patch_meta
                    );
                    println!("    fields: {:?}", field_nums(&m.payload));
                }
                shown += 1;
                if shown >= 3 {
                    return;
                }
            }
        }
    }
}
