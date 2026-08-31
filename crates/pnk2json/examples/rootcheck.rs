//! Which corpus files lack a decodable root object 1?
use iwadump::Document;
fn main() {
    for arg in std::env::args().skip(1) {
        let Ok(doc) = Document::open(std::path::Path::new(&arg), false) else {
            continue;
        };
        let loaded = pnk2json::loader::load(&doc.streams, &doc.registry, doc.app);
        match loaded.record(1) {
            None => println!("NO-RECORD-1 {arg}"),
            Some(r) if r.msg.is_none() => println!("UNDECODABLE-1 {arg}"),
            Some(r) => {
                if r.name.is_none() {
                    println!("UNNAMED-1 type={} {arg}", r.type_id);
                }
            }
        }
    }
}
