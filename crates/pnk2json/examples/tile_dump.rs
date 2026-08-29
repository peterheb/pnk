use iwadump::Document;
fn main() {
    let arg = std::env::args().nth(1).unwrap();
    let path = std::path::Path::new(&arg);
    let doc = Document::open(path, false).unwrap();
    let loaded = pnk2json::loader::load(&doc.streams, &doc.registry, doc.app);
    let model = loaded.msg(4039).unwrap();
    let store = model.msg(4).unwrap();
    // string_table field 4: check ALL keys including segments
    if let Some(st_id) = store.reference(4) {
        println!("string_table -> {st_id}");
        if let Some(lm) = loaded.msg(st_id) {
            println!("  listType {:?}", lm.varint(1));
            for e in lm.msgs(3).iter().take(10) {
                println!("  inline key {:?} str {:?}", e.varint(1), e.string(3).map(|s| s.chars().take(20).collect::<String>()));
            }
            for seg_ref in lm.references(4) {
                println!("  segment -> {seg_ref}");
                if let Some(seg) = loaded.msg(seg_ref) {
                    let base = seg.varint(2).unwrap_or(0);
                    for (i, e) in seg.msgs(3).into_iter().enumerate().take(20) {
                        println!("    seg key {} str {:?}", base + i as u64,
                            e.string(3).map(|s| s.chars().take(20).collect::<String>()));
                    }
                }
            }
        }
    }
    // field 12 (second string table?)
    if let Some(st2_id) = store.reference(12) {
        println!("field 12 -> {st2_id}");
        if let Some(lm) = loaded.msg(st2_id) {
            println!("  listType {:?}", lm.varint(1));
            for e in lm.msgs(3).iter().take(6) {
                println!("  key {:?} str {:?}", e.varint(1), e.string(3).map(|s| s.chars().take(20).collect::<String>()));
            }
        }
    }
}
