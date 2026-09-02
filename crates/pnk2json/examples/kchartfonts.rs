//! Agent K: resolve chart text sizes through the paragraph-style INDEX slots.
use pnk2json::pb::Msg;

fn size_of(loaded: &pnk2json::loader::Loaded, paras: &[u64], idx: Option<u64>) -> Option<f32> {
    let i = idx? as usize;
    let p = *paras.get(i)?;
    let m = loaded.msg(p)?;
    m.msg(11)?.f32v(3)
}

fn ext(loaded: &pnk2json::loader::Loaded, id: Option<u64>) -> Option<Msg> {
    id.and_then(|i| loaded.msg(i)).and_then(|m| m.msg(10000))
}

fn main() {
    let arg = std::env::args().nth(1).unwrap();
    let (_d, loaded) = pnk2json::loader::open_document(std::path::Path::new(&arg)).unwrap();
    let mut ids: Vec<u64> = loaded
        .records
        .values()
        .filter(|r| r.name.as_deref() == Some("TSCH.ChartDrawableArchive"))
        .map(|r| r.id)
        .collect();
    ids.sort();
    for id in ids {
        let Some(ca) = loaded.msg(id).and_then(|m| m.msg(10000)) else {
            continue;
        };
        let paras = ca.references(20);
        let cs = ext(&loaded, ca.reference(9));
        let ls = ext(&loaded, ca.reference(11));
        let vax = ca
            .references(13)
            .first()
            .and_then(|r| ext(&loaded, Some(*r)));
        let cax = ca
            .references(15)
            .first()
            .and_then(|r| ext(&loaded, Some(*r)));
        let ss = ca
            .msg(18)
            .and_then(|s| s.msgs(2).first().and_then(|e| e.reference(2)))
            .or_else(|| ca.references(17).first().copied())
            .and_then(|r| ext(&loaded, Some(r)));
        println!("chart {id} type={:?} paras={}", ca.varint(1), paras.len());
        println!(
            "   title(cs20)={:?} datasetname(cs21)={:?} summary(cs30)={:?}",
            size_of(&loaded, &paras, cs.as_ref().and_then(|m| m.varint(20))),
            size_of(&loaded, &paras, cs.as_ref().and_then(|m| m.varint(21))),
            size_of(&loaded, &paras, cs.as_ref().and_then(|m| m.varint(30)))
        );
        println!(
            "   legend(ls2)={:?} valueaxis(vax8)={:?} catetgoryaxis(cax6)={:?} defaultaxis(7)={:?}",
            size_of(&loaded, &paras, ls.as_ref().and_then(|m| m.varint(2))),
            size_of(&loaded, &paras, vax.as_ref().and_then(|m| m.varint(8))),
            size_of(&loaded, &paras, cax.as_ref().and_then(|m| m.varint(6))),
            size_of(&loaded, &paras, cax.as_ref().and_then(|m| m.varint(7)))
        );
        for (name, f) in [
            ("pie23", 23u32),
            ("pieoutside29", 29),
            ("donut152", 152),
            ("donutoutside153", 153),
            ("default20", 20),
            ("defaultoutside27", 27),
            ("line21", 21),
        ] {
            if let Some(v) = ss.as_ref().and_then(|m| m.varint(f)) {
                println!(
                    "   series {name} idx={v} size={:?}",
                    size_of(&loaded, &paras, Some(v))
                );
            }
        }
    }
}
