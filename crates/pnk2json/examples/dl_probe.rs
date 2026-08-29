use iwadump::{Document, Layer};
use std::collections::HashMap;

fn varint(f: &iwadump::proto::Field) -> Option<u64> {
    match &f.value { iwadump::proto::Value::Varint(v) => Some(*v), _ => None }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let doc = Document::open(std::path::Path::new(path), false).unwrap();

    // Build key -> string map for EVERY TableDataList in the doc (inline +
    // segmented entries).
    let mut lists: HashMap<u64, (u64, HashMap<u64, String>)> = HashMap::new();
    for stream in &doc.streams {
        for a in &stream.archives {
            let Some(m) = a.messages.first() else { continue };
            let Ok(fields) = iwadump::proto::parse_fields(&m.payload, Layer::Message) else { continue };
            let ltype = fields.iter().find(|f| f.number == 1).and_then(varint).unwrap_or(0);
            let mut map = HashMap::new();
            for e in fields.iter().filter(|f| f.number == 3) {
                let b = match &e.value { iwadump::proto::Value::Bytes(b) => b.clone(), _ => continue };
                let Ok(ef) = iwadump::proto::parse_fields(&b, Layer::Message) else { continue };
                let key = ef.iter().find(|f| f.number == 1).and_then(varint).unwrap_or(0);
                let s = ef.iter().find(|f| f.number == 3).and_then(|f| match &f.value {
                    iwadump::proto::Value::Bytes(b) => Some(String::from_utf8_lossy(b).into_owned()),
                    _ => None,
                });
                if let Some(s) = s { map.insert(key, s); }
            }
            lists.insert(a.identifier, (ltype, map));
        }
    }

    for (id, (t, m)) in &lists {
        if !m.is_empty() {
            let mut keys: Vec<u64> = m.keys().copied().collect();
            keys.sort();
            println!("list {id} type {t}: {} entries, keys {:?}..{:?}", m.len(), keys.first(), keys.last());
        }
    }

    if args.len() < 3 { return; }
    let tile_id: u64 = args[2].parse().unwrap();
    let (_, mv) = doc.find_message(tile_id).unwrap();
    let fields = iwadump::proto::parse_fields(&mv.payload, Layer::Message).unwrap();
    for f in &fields {
        if f.number != 5 { continue; }
        let ri_bytes = match &f.value { iwadump::proto::Value::Bytes(b) => b.clone(), _ => continue };
        let ri = iwadump::proto::parse_fields(&ri_bytes, Layer::Message).unwrap();
        let idx = ri.iter().find(|x| x.number == 1).and_then(varint).unwrap_or(99);
        let buf = ri.iter().find(|x| x.number == 3).and_then(|f| match &f.value { iwadump::proto::Value::Bytes(b) => Some(b.clone()), _ => None });
        let off = ri.iter().find(|x| x.number == 4).and_then(|f| match &f.value { iwadump::proto::Value::Bytes(b) => Some(b.clone()), _ => None });
        let (Some(buf), Some(off)) = (buf, off) else { continue };
        let raw: Vec<i32> = off.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as i32).collect();
        let mut line = format!("row {idx}:");
        for (slot, &o) in raw.iter().enumerate().take(8) {
            if o < 0 { continue; }
            let o = o as usize;
            if o >= buf.len() { continue; }
            let mut end = buf.len();
            for &later in raw.iter().skip(slot + 1) {
                if later > o as i32 { end = (later as usize).min(buf.len()); break; }
            }
            let u32s: Vec<u32> = buf[o..end].chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
            line.push_str(&format!(" [col{slot} {:?}]", u32s));
            // resolve every u32 against all lists
            let mut resol = String::new();
            for (i, u) in u32s.iter().enumerate() {
                for (id, (t, m)) in &lists {
                    if let Some(s) = m.get(&(*u as u64)) {
                        resol.push_str(&format!(" u{i}={u}(list{id} t{t}: {:?})", truncate(s)));
                    }
                }
            }
            if !resol.is_empty() { line.push_str(&format!("   ->{resol}")); }
        }
        println!("{line}");
    }
}

fn truncate(s: &str) -> String {
    if s.len() > 28 { format!("{}…", &s[..26]) } else { s.to_string() }
}