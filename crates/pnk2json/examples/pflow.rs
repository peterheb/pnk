//! Linked text boxes: TSWP.FlowInfoArchive { text_storage = 1, textboxes = 2 }
//! with more than one box share one storage across boxes.
use iwadump::Document;
fn main() {
    for path in std::env::args().skip(1) {
        let Ok(doc) = Document::open(std::path::Path::new(&path), false) else { continue };
        let loaded = pnk2json::loader::load(&doc.streams, &doc.registry, doc.app);
        let mut n = 0;
        let mut boxes = 0;
        for rec in loaded.records.values() {
            if rec.name.as_deref() != Some("TSWP.FlowInfoArchive") { continue; }
            let Some(m) = &rec.msg else { continue };
            let k = m.references(2).len();
            if k > 1 { n += 1; boxes += k; }
        }
        if n > 0 { println!("{} flows={} boxes={}", &path[path.rfind('/').map(|i| i + 1).unwrap_or(0)..][..12], n, boxes); }
    }
}
