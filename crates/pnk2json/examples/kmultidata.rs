//! Agent K: multi-data chart shapes — multidataset_index (21) vs grid size.
use iwadump::proto::Value;
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
        let g = ca.msg(7);
        let rows = g.as_ref().map(|g| g.msgs(3).len()).unwrap_or(0);
        let cols = g
            .as_ref()
            .map(|g| g.msgs(3).iter().map(|r| r.msgs(1).len()).max().unwrap_or(0))
            .unwrap_or(0);
        let rn: Vec<String> = g
            .as_ref()
            .map(|g| {
                g.all(1)
                    .into_iter()
                    .filter_map(|v| match v {
                        Value::Bytes(b) => Some(String::from_utf8_lossy(&b).into_owned()),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        println!("{id} type={:?} dir={:?} mdi={:?} grid={}x{} rownames={:?} nseries_styles={} f4entries={}",
            ca.varint(1), ca.varint(5), ca.varint(21), rows, cols, rn,
            ca.references(17).len(),
            g.as_ref().map(|g| g.all(4).len()).unwrap_or(0));
    }
}
