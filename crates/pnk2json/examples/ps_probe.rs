use iwadump::Document;

fn main() {
    let arg = std::env::args().nth(1).unwrap();
    let path = std::path::Path::new(&arg);
    let doc = Document::open(path, false).unwrap();
    let loaded = pnk2json::loader::load(&doc.streams, &doc.registry, doc.app);
    let root = loaded.msg(1).unwrap();
    let storage = loaded.msg(root.reference(4).unwrap()).unwrap();
    // para style entries (field 5)
    if let Some(t) = storage.msg(5) {
        for e in t.msgs(1).iter().take(5) {
            let idx = e.varint(1);
            let sid = e.reference(2);
            println!("para entry idx={idx:?} sid={sid:?}");
            if let Some(id) = sid {
                if let Some(m) = loaded.msg(id) {
                    // ParagraphStyleArchive { super=1, char_properties=11, para_properties=12 }
                    let cp = m.msg(11);
                    let pp = m.msg(12);
                    println!("  name={:?}", m.msg(1).and_then(|b| b.string(1)));
                    println!(
                        "  char_properties: {:?}",
                        cp.as_ref().map(|c| c
                            .fields
                            .iter()
                            .map(|f| (f.number, iwadump::proto::wire_name(f.wire)))
                            .collect::<Vec<_>>())
                    );
                    if let Some(c) = &cp {
                        // font_size = 3, font_name = 5, font_color = 7
                        println!(
                            "    font_size={:?} font_name={:?} bold={:?} italic={:?}",
                            c.f32v(3),
                            c.string(5),
                            c.boolean(1),
                            c.boolean(2)
                        );
                        if let Some(fc) = c.msg(7) {
                            let mut w = Vec::new();
                            println!(
                                "    font_color={:?}",
                                pnk2json::colors::color_hex(&fc, &mut |r| w.push(r))
                            );
                        }
                    }
                    println!(
                        "  para_properties outline_level={:?}",
                        pp.as_ref().and_then(|p| p.varint(27))
                    );
                }
            }
        }
    }
}
