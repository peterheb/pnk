//! Dump one record's field walk.
use iwadump::Document;
use iwadump::proto::Value;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap();
    let doc = Document::open(std::path::Path::new(&path), false).unwrap();
    let loaded = pnk2json::loader::load(&doc.streams, &doc.registry, doc.app);
    for ids in args {
        let id: u64 = ids.parse().unwrap();
        let rec = loaded.record(id);
        println!("record {id}: name={:?}", rec.and_then(|r| r.name.clone()));
        if let Some(m) = loaded.msg(id) {
            for f in &m.fields {
                match &f.value {
                    Value::Bytes(b) => println!("  f{} len={} str={:?}", f.number, b.len(), String::from_utf8_lossy(&b[..b.len().min(80)])),
                    v => println!("  f{} {:?}", f.number, v),
                }
            }
        }
    }
}
