//! Agent K corpus survey: value-axis number formats + major gridline counts.
use pnk2json::pb::Msg;
use std::collections::BTreeMap;

fn main() {
    let mut fmt: BTreeMap<String, u64> = BTreeMap::new();
    let mut grid: BTreeMap<String, u64> = BTreeMap::new();
    for arg in std::env::args().skip(1) {
        let Ok((_d, loaded)) = pnk2json::loader::open_document(std::path::Path::new(&arg)) else {
            continue;
        };
        for r in loaded.records.values() {
            if r.name.as_deref() != Some("TSCH.ChartDrawableArchive") {
                continue;
            }
            let Some(m) = &r.msg else { continue };
            let Some(ca) = m.msg(10000) else { continue };
            for id in ca.references(14) {
                let Some(ext) = loaded.msg(id).and_then(|x| x.msg(10000)) else {
                    continue;
                };
                let f = ext.msg(42).or_else(|| ext.msg(2));
                let key = match &f {
                    Some(f) => format!(
                        "type={:?} dec={:?} sep={:?} neg={:?} nft={:?}",
                        f.varint(1),
                        f.varint(2),
                        f.varint(5),
                        f.varint(4),
                        ext.varint(3)
                    ),
                    None => "absent".to_string(),
                };
                *fmt.entry(key).or_insert(0) += 1;
                *grid
                    .entry(format!("gridlines={:?}", ext.varint(5)))
                    .or_insert(0) += 1;
            }
        }
    }
    let mut v: Vec<_> = fmt.into_iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    for (k, c) in v.iter().take(25) {
        println!("{c:6}  {k}");
    }
    println!("--");
    let mut g: Vec<_> = grid.into_iter().collect();
    g.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    for (k, c) in g.iter().take(15) {
        println!("{c:6}  {k}");
    }
}
