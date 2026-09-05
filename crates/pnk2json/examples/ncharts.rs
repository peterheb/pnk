//! Agent-N census (round 4): chart mediator binding formulas — node kinds,
//! function ids, host fields. usage: ncharts <file>...
use pnk2json::pb::Msg;

fn nodes_desc(f: &Msg) -> String {
    let mut out = Vec::new();
    if let Some(arr) = f.msg(1) {
        for n in arr.msgs(1) {
            let k = n.varint(1).unwrap_or(0);
            match k {
                16 => out.push(format!("F{}({})", n.varint(2).unwrap_or(0), n.varint(3).unwrap_or(0))),
                36 => out.push(format!(
                    "REF(c{:?}{} r{:?}{} x{})",
                    n.msg(26).and_then(|c| c.varint(1)),
                    if n.msg(26).and_then(|c| c.boolean(2)) == Some(true) { "$" } else { "" },
                    n.msg(27).and_then(|c| c.varint(1)),
                    if n.msg(27).and_then(|c| c.boolean(2)) == Some(true) { "$" } else { "" },
                    n.has(28)
                )),
                _ => out.push(format!("N{k}")),
            }
        }
    }
    format!(
        "host=({:?},{:?},{:?},{:?}) uid7={} [{}]",
        f.varint(2), f.varint(3), f.varint(4), f.varint(5), f.has(7),
        out.join(" ")
    )
}

fn main() {
    for path in std::env::args().skip(1) {
        let p = std::path::Path::new(&path);
        let Ok((_doc, loaded)) = pnk2json::loader::open_document(p) else { continue };
        let short = p.file_name().unwrap().to_string_lossy()[..12].to_string();
        for r in loaded.records.values() {
            if r.type_id != 12006 {
                continue;
            }
            let Some(m) = r.msg.as_ref() else { continue };
            let Some(fs) = m.msg(3) else {
                println!("{short}\tmediator {} no formulas", r.id);
                continue;
            };
            println!(
                "{short}\tmediator {} cols_are_series={:?} direction={:?} scheme={:?} data={} row_labels={} col_labels={}",
                r.id, m.boolean(4), fs.varint(5), fs.varint(10), fs.msgs(1).len(), fs.msgs(3).len(), fs.msgs(4).len()
            );
            for (i, f) in fs.msgs(1).iter().enumerate().take(3) {
                println!("{short}\t  data[{i}] {}", nodes_desc(f));
            }
            for (i, f) in fs.msgs(3).iter().enumerate().take(2) {
                println!("{short}\t  rowlab[{i}] {}", nodes_desc(f));
            }
            for (i, f) in fs.msgs(4).iter().enumerate().take(2) {
                println!("{short}\t  collab[{i}] {}", nodes_desc(f));
            }
        }
    }
}
