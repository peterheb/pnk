//! Agent K: chart paragraph_styles (ChartArchive 20) — font sizes for chart text.
fn main() {
    let arg = std::env::args().nth(1).unwrap();
    let only: Option<u64> = std::env::args().nth(2).and_then(|s| s.parse().ok());
    let (_d, loaded) = pnk2json::loader::open_document(std::path::Path::new(&arg)).unwrap();
    let mut ids: Vec<u64> = loaded
        .records
        .values()
        .filter(|r| r.name.as_deref() == Some("TSCH.ChartDrawableArchive"))
        .map(|r| r.id)
        .collect();
    ids.sort();
    for id in ids {
        if only.is_some_and(|o| o != id) {
            continue;
        }
        let Some(ca) = loaded.msg(id).and_then(|m| m.msg(10000)) else {
            continue;
        };
        println!("chart {id} type={:?}", ca.varint(1));
        for (i, p) in ca.references(20).into_iter().enumerate() {
            let name = loaded.record(p).and_then(|r| r.name.clone());
            let m = loaded.msg(p);
            // TSWP.ParagraphStyleArchive: char props in field 11 (TSWP.CharacterStylePropertiesArchive)
            let cp = m.and_then(|m| m.msg(11));
            let size = cp.as_ref().and_then(|c| c.f32v(20).or_else(|| c.f32v(21)));
            let mut fields = Vec::new();
            if let Some(c) = &cp {
                for f in &c.fields {
                    fields.push(format!("{}={:?}", f.number, f.value));
                }
            }
            println!("  para[{i}] ref({p}) {:?} size={:?}", name, size);
            println!("     charprops: {}", fields.join(" "));
        }
        // chart style dataset-name paragraph style index
        if let Some(cs) = ca
            .reference(9)
            .and_then(|r| loaded.msg(r))
            .and_then(|m| m.msg(10000))
        {
            println!(
                "  chart_style: datasetnameparaidx={:?} titleparaidx={:?} summarylabelparaidx={:?}",
                cs.varint(21),
                cs.varint(20),
                cs.varint(30)
            );
        }
    }
}
