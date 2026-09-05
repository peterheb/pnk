//! Agent-N probe (round 4): dump chart value-axis archives and grouped
//! summary models. usage: nchartaxis <file> axes|summary
use pnk2json::pb::Msg;

fn dump(m: &Msg, prefix: &str, depth: u32) {
    for f in &m.fields {
        match &f.value {
            iwadump::proto::Value::Bytes(b) => {
                let s = String::from_utf8_lossy(&b[..b.len().min(60)]).to_string();
                let printable = s.chars().all(|c| !c.is_control()) && !s.is_empty();
                println!(
                    "{prefix}f{} len={} {}",
                    f.number,
                    b.len(),
                    if printable {
                        format!("str={s:?}")
                    } else {
                        String::new()
                    }
                );
                if depth > 0 {
                    if let Some(sub) = Msg::parse(b) {
                        dump(&sub, &format!("{prefix}  "), depth - 1);
                    }
                }
            }
            v => println!("{prefix}f{} {:?}", f.number, v),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (_doc, loaded) = pnk2json::loader::open_document(std::path::Path::new(&args[1])).unwrap();
    let mode = args.get(2).map(|s| s.as_str()).unwrap_or("axes");
    if mode == "summary" {
        for r in loaded.records.values() {
            if r.name.as_deref() != Some("TST.SummaryModelArchive") {
                continue;
            }
            println!("=== summary model {}", r.id);
            if let Some(m) = r.msg.as_ref() {
                dump(m, "  ", 1);
            }
        }
        return;
    }
    for r in loaded.records.values() {
        if r.name.as_deref() != Some("TSCH.ChartDrawableArchive") {
            continue;
        }
        let Some(ca) = r.msg.as_ref().and_then(|m| m.msg(10000)) else {
            continue;
        };
        let ca = &ca;
        let title = ca
            .reference(10)
            .and_then(|id| loaded.msg(id))
            .and_then(|m| m.msg(10000))
            .and_then(|e| e.string(23));
        println!(
            "=== chart {} type={:?} title={:?}",
            r.id,
            ca.varint(1),
            title
        );
        for (label, field) in [
            ("value_axis_nonstyle", 14u32),
            ("value_axis_style", 13),
            ("chart_nonstyle", 10),
            ("chart_style", 9),
        ] {
            for id in ca.references(field) {
                let Some(m) = loaded.msg(id) else { continue };
                println!("  {label} {id}:");
                if let Some(e) = m.msg(10000) {
                    dump(&e, "    ", 1);
                }
            }
        }
    }
}
