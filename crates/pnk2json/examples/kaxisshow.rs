//! Agent K: axis show flags (style 24/25/27/28, non-style 9/10/11).
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
        let e = |r: Option<u64>| r.and_then(|i| loaded.msg(i)).and_then(|m| m.msg(10000));
        let vs = ca.references(13).first().copied().and_then(|r| e(Some(r)));
        let cs = ca.references(15).first().copied().and_then(|r| e(Some(r)));
        let vn = ca.references(14).first().copied().and_then(|r| e(Some(r)));
        let cn = ca.references(16).first().copied().and_then(|r| e(Some(r)));
        println!("{id} type={:?}", ca.varint(1));
        println!("  STYLE cat: showaxis24={:?} showmajorgrid27={:?} | val: showaxis25={:?} showmajorgrid28={:?}",
            cs.as_ref().and_then(|m| m.varint(24)), cs.as_ref().and_then(|m| m.varint(27)),
            vs.as_ref().and_then(|m| m.varint(25)), vs.as_ref().and_then(|m| m.varint(28)));
        println!("  NONSTYLE cat showlabels9={:?} default10={:?} | val showlabels11={:?} gridlines5={:?}",
            cn.as_ref().and_then(|m| m.varint(9)), cn.as_ref().and_then(|m| m.varint(10)),
            vn.as_ref().and_then(|m| m.varint(11)), vn.as_ref().and_then(|m| m.varint(5)));
    }
}
