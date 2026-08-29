use iwadump::{Document, Layer};

fn varint(f: &iwadump::proto::Field) -> Option<u64> {
    match &f.value { iwadump::proto::Value::Varint(v) => Some(*v), _ => None }
}
fn bytes(f: &iwadump::proto::Field) -> Option<Vec<u8>> {
    match &f.value { iwadump::proto::Value::Bytes(b) => Some(b.clone()), _ => None }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let id: u64 = args[2].parse().unwrap();
    let doc = Document::open(std::path::Path::new(path), false).unwrap();
    let (_, mv) = doc.find_message(id).unwrap();
    let fields = iwadump::proto::parse_fields(&mv.payload, iwadump::Layer::Message).unwrap();
    for f in &fields {
        if f.number == 5 {
            let ri_bytes = bytes(f).unwrap();
            let ri = iwadump::proto::parse_fields(&ri_bytes, Layer::Message).unwrap();
            let idx = ri.iter().find(|x| x.number == 1).and_then(varint).unwrap_or(99);
            let count = ri.iter().find(|x| x.number == 2).and_then(varint).unwrap_or(0);
            let wide = ri.iter().find(|x| x.number == 8).and_then(varint);
            let buf = ri.iter().find(|x| x.number == 3).and_then(bytes);
            let off = ri.iter().find(|x| x.number == 4).and_then(bytes);
            let buf6 = ri.iter().find(|x| x.number == 6).and_then(bytes);
            let off7 = ri.iter().find(|x| x.number == 7).and_then(bytes);
            let buffer = buf.or(buf6).unwrap_or_default();
            let offsets = off.or(off7).unwrap_or_default();
            println!("== row {idx} count {count} wide {wide:?} buflen {} offlen {}", buffer.len(), offsets.len());
            let raw: Vec<i32> = offsets.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as i32).collect();
            let scaled: Vec<i32> = if wide.unwrap_or(0) != 0 { raw.iter().map(|v| v * 4).collect() } else { raw.clone() };
            // Print cell blocks per the offsets (span to next positive offset).
            for (slot, &o) in scaled.iter().enumerate().take(8) {
                if o < 0 { continue; }
                let o = o as usize;
                if o >= buffer.len() { continue; }
                let mut end = buffer.len();
                for &later in scaled.iter().skip(slot + 1) {
                    if later > o as i32 { end = (later as usize).min(buffer.len()); break; }
                }
                println!("  cell col {slot} bytes[{o}..{end}]: {}", buffer[o..end].hex());
            }
        }
    }
}

trait Hex { fn hex(&self) -> String; }
impl Hex for [u8] {
    fn hex(&self) -> String {
        self.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
    }
}