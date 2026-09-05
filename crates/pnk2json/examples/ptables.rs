//! List the attribute tables each TSWP.StorageArchive carries, and the
//! entries of the annotation tables (bookmark 15, insertion 21, deletion 22,
//! highlight 23) with their target types.
use iwadump::Document;

fn main() {
    let path = std::env::args().nth(1).unwrap();
    let doc = Document::open(std::path::Path::new(&path), false).unwrap();
    let loaded = pnk2json::loader::load(&doc.streams, &doc.registry, doc.app);
    let mut ids: Vec<_> = loaded.records.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let rec = loaded.record(id).unwrap();
        if !matches!(rec.type_id, 2001 | 2005) {
            continue;
        }
        let Some(m) = &rec.msg else { continue };
        let fields: Vec<u32> = {
            let mut v: Vec<u32> = m.fields.iter().map(|f| f.number).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let text = m.string(3).unwrap_or_default();
        let kind = m.varint(1).unwrap_or(3);
        let interesting = fields.iter().any(|f| matches!(f, 15 | 19 | 21 | 22 | 23 | 25 | 18 | 24));
        if !interesting {
            continue;
        }
        println!("storage {id} kind={kind} len={} fields={fields:?}", text.chars().count());
        for field in [15u32, 19, 21, 22, 23, 24, 25] {
            let Some(t) = m.msg(field) else { continue };
            for e in t.msgs(1) {
                let off = e.varint(1);
                let oid = e.reference(2);
                let name = oid.and_then(|o| loaded.record(o)).and_then(|r| r.name.clone());
                let mut extra = String::new();
                if let Some(o) = oid {
                    if let Some(om) = loaded.msg(o) {
                        if field == 23 {
                            if let Some(cs) = om.reference(1).and_then(|c| loaded.msg(c)) {
                                extra = format!(" comment={:?}", cs.string(1).unwrap_or_default());
                            }
                        } else if field == 21 || field == 22 {
                            extra = format!(" kind={:?} session={:?}", om.varint(1), om.reference(2));
                        } else if field == 15 {
                            extra = format!(" name={:?} ranged={:?}", om.string(2), om.varint(3));
                        } else if field == 19 {
                            extra = format!(" lang={:?}", om.string(1));
                        }
                    }
                }
                if field == 19 { println!("    raw: {:?}", e.fields); }
                // ranged tables (25) carry TSP.Range in field 1 instead
                let range = e.msg(1).map(|r| (r.varint(1), r.varint(2)));
                let snippet: String = off
                    .map(|o| text.chars().skip(o as usize).take(30).collect())
                    .unwrap_or_default();
                println!("  f{field} off={off:?} range={range:?} obj={oid:?} {name:?}{extra} text={snippet:?}");
            }
        }
    }
}
