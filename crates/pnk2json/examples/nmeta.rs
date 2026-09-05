//! Agent-N probe (round 4): sort rules, control cells, hidden states,
//! comments of every table model. usage: nmeta <file>
use pnk2json::pb::Msg;

fn dump(m: &Msg, prefix: &str, depth: u32) {
    for f in &m.fields {
        match &f.value {
            iwadump::proto::Value::Bytes(b) => {
                let s = String::from_utf8_lossy(&b[..b.len().min(60)]).to_string();
                let printable = s.chars().all(|c| !c.is_control()) && !s.is_empty();
                println!("{prefix}f{} len={} {}", f.number, b.len(), if printable { format!("str={s:?}") } else { String::new() });
                if depth > 0 {
                    if let Some(sub) = Msg::parse(b) {
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
    let (_doc, loaded) = pnk2json::loader::open_document(std::path::Path::new(&args[1])).unwrap();
    for r in loaded.records.values() {
        if r.type_id != 6001 {
            continue;
        }
        let Some(m) = r.msg.as_ref() else { continue };
        println!("=== table {} '{}' {}x{}", r.id, m.string(8).unwrap_or_default(), m.varint(6).unwrap_or(0), m.varint(7).unwrap_or(0));
        if let Some(so) = m.msg(44) {
            println!("sort_order:");
            dump(&so, "  ", 2);
        }
        if let Some(hso) = m.msg(70) {
            println!("hidden_states_owner:");
            dump(&hso, "  ", 4);
        }
        let store = m.msg(4);
        for (field, label) in [(21u32, "control_cell_spec_table"), (19, "commentStorageTable"), (16, "multipleChoiceListFormatTable")] {
            let Some(id) = store.as_ref().and_then(|s| s.reference(field)) else { continue };
            let Some(list) = loaded.msg(id) else { continue };
            println!("{label} (list {id}, type {:?}):", list.varint(1));
            for e in list.msgs(3) {
                println!("  entry key={:?} refcount={:?}", e.varint(1), e.varint(2));
                dump(&e, "    ", 3);
                for rf in [4u32, 9, 10, 12] {
                    if let Some(rid) = e.reference(rf) {
                        if let Some(sub) = loaded.msg(rid) {
                            println!("    -> ref f{rf} = record {rid} (type {:?})", loaded.record(rid).map(|r| r.type_id));
                            dump(sub, "       ", 3);
                        }
                    }
                }
            }
        }
    }
}
