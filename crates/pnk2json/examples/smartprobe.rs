//! Dump smart-field entry tables for storages whose text matches a needle.
use iwadump::Document;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap();
    let needle = args.next().unwrap();
    let doc = Document::open(std::path::Path::new(&path), false).unwrap();
    let loaded = pnk2json::loader::load(&doc.streams, &doc.registry, doc.app);
    for (id, rec) in &loaded.records {
        let Some(m) = &rec.msg else { continue };
        let Some(text) = m.string(3) else { continue };
        if !text.contains(&needle) {
            continue;
        }
        println!("storage {id}: text={:?}", text);
        for (label, field) in [("char", 8u32), ("smart", 11u32)] {
            let Some(t) = m.msg(field) else { continue };
            for e in t.msgs(1) {
                let off = e.varint(1);
                let oid = e.reference(2);
                let name = oid
                    .and_then(|o| loaded.record(o))
                    .and_then(|r| r.name.clone());
                println!("  {label} off={off:?} obj={oid:?} type={name:?}");
            }
        }
    }
}
