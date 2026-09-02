//! Agent K: dump TSWP.ListStyleArchive text_indents (12) / indents (13).
use iwadump::proto::Value;
fn floats(m: &pnk2json::pb::Msg, f: u32) -> Vec<f32> {
    m.all(f)
        .into_iter()
        .filter_map(|v| match v {
            Value::Fixed32(b) => Some(f32::from_le_bytes(*b)),
            Value::Bytes(_) => None,
            _ => None,
        })
        .collect()
}
fn main() {
    let arg = std::env::args().nth(1).unwrap();
    let (_d, loaded) = pnk2json::loader::open_document(std::path::Path::new(&arg)).unwrap();
    let mut ids: Vec<u64> = loaded
        .records
        .values()
        .filter(|r| r.name.as_deref() == Some("TSWP.ListStyleArchive"))
        .map(|r| r.id)
        .collect();
    ids.sort();
    for id in ids {
        let m = loaded.msg(id).unwrap();
        println!(
            "list style {id}: text_indents={:?} indents={:?} label_types={:?} strings={:?}",
            floats(m, 12),
            floats(m, 13),
            m.all(11)
                .into_iter()
                .filter_map(|v| match v {
                    Value::Varint(x) => Some(x),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            m.all(16)
                .into_iter()
                .filter_map(|v| match v {
                    Value::Bytes(b) => Some(String::from_utf8_lossy(&b).into_owned()),
                    _ => None,
                })
                .take(3)
                .collect::<Vec<_>>()
        );
    }
}
