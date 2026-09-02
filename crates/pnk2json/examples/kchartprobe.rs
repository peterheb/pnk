//! Agent K: dump every TSCH.ChartArchive of a document with its non-style,
//! axis non-style and series non-style Generated extensions.
//! usage: kchartprobe <file> [--all-fields]
use iwadump::proto::Value;
use pnk2json::pb::Msg;

fn show(m: &Msg, indent: &str) {
    for f in &m.fields {
        match &f.value {
            Value::Bytes(b) => {
                let s = String::from_utf8_lossy(b);
                let printable = s.chars().all(|c| !c.is_control()) && !b.is_empty();
                if let Some(sub) = Msg::parse(b) {
                    if sub.fields.len() == 1 && sub.fields[0].number == 1 {
                        if let Value::Varint(v) = sub.fields[0].value {
                            println!("{indent}f{} = ref({v})", f.number);
                            continue;
                        }
                    }
                    println!(
                        "{indent}f{} = msg({} B){}",
                        f.number,
                        b.len(),
                        if printable {
                            format!(" str={s:?}")
                        } else {
                            String::new()
                        }
                    );
                    show(&sub, &format!("{indent}  "));
                    continue;
                }
                println!(
                    "{indent}f{} = bytes({} B) {:?}",
                    f.number,
                    b.len(),
                    &s[..s.len().min(60)]
                );
            }
            Value::Varint(v) => println!("{indent}f{} = {v}", f.number),
            Value::Group(_) => println!("{indent}f{} = group", f.number),
            Value::Fixed32(v) => println!(
                "{indent}f{} = f32 {} / u32 {}",
                f.number,
                f32::from_le_bytes(*v),
                u32::from_le_bytes(*v)
            ),
            Value::Fixed64(v) => println!(
                "{indent}f{} = f64 {} / u64 {}",
                f.number,
                f64::from_le_bytes(*v),
                u64::from_le_bytes(*v)
            ),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&args[1]);
    let only: Option<u64> = args.get(2).and_then(|s| s.parse().ok());
    let (_doc, loaded) = pnk2json::loader::open_document(path).unwrap();
    let mut ids: Vec<u64> = loaded
        .records
        .values()
        .filter(|r| {
            r.name.as_deref() == Some("TSCH.ChartDrawableArchive")
                || r.name.as_deref() == Some("TSCH.ChartArchive")
        })
        .map(|r| r.id)
        .collect();
    ids.sort();
    for id in ids {
        if let Some(o) = only {
            if o != id {
                continue;
            }
        }
        let rec = loaded.record(id).unwrap();
        let m = rec.msg.as_ref().unwrap();
        let ca = m.msg(10000).unwrap_or_else(|| m.clone());
        println!(
            "=== chart drawable {id} ({:?}) type={:?} dir={:?}",
            rec.name,
            ca.varint(1),
            ca.varint(5)
        );
        let dump = |label: &str, rid: Option<u64>| {
            let Some(rid) = rid else { return };
            let Some(mm) = loaded.msg(rid) else {
                println!("  {label} ref({rid}) MISSING");
                return;
            };
            println!(
                "  {label} ref({rid}) name={:?}",
                loaded.record(rid).and_then(|r| r.name.clone())
            );
            if let Some(ext) = mm.msg(10000) {
                show(&ext, "    ");
            } else {
                println!("    (no 10000 ext); raw:");
                show(mm, "    ");
            }
        };
        dump("preset", ca.reference(4));
        dump("owned_preset", ca.reference(23));
        dump("chart_non_style", ca.reference(10));
        dump("chart_style", ca.reference(9));
        for (i, r) in ca.references(14).into_iter().enumerate() {
            dump(&format!("value_axis_nonstyle[{i}]"), Some(r));
        }
        for (i, r) in ca.references(16).into_iter().enumerate() {
            dump(&format!("category_axis_nonstyle[{i}]"), Some(r));
        }
        if let Some(sparse) = ca.msg(19) {
            for e in sparse.msgs(2) {
                let idx = e.varint(1).unwrap_or(0);
                dump(&format!("series_non_style[{idx}]"), e.reference(2));
            }
        }
        // grid summary
        if let Some(g) = ca.msg(7) {
            let rn: Vec<String> = g
                .all(1)
                .into_iter()
                .filter_map(|v| match v {
                    Value::Bytes(b) => Some(String::from_utf8_lossy(&b).into_owned()),
                    _ => None,
                })
                .collect();
            let cn: Vec<String> = g
                .all(2)
                .into_iter()
                .filter_map(|v| match v {
                    Value::Bytes(b) => Some(String::from_utf8_lossy(&b).into_owned()),
                    _ => None,
                })
                .collect();
            println!("  grid rows={rn:?} cols={cn:?}");
            for (i, r) in g.msgs(3).into_iter().enumerate() {
                let vals: Vec<String> = r
                    .msgs(1)
                    .into_iter()
                    .map(|v| {
                        format!(
                            "{:?}",
                            v.fields
                                .iter()
                                .map(|f| (f.number, f.value.clone()))
                                .collect::<Vec<_>>()
                        )
                    })
                    .collect();
                println!("    row {i}: {}", vals.join(" | "));
            }
            println!("  grid other fields:");
            for f in &g.fields {
                if f.number > 3 {
                    println!("    f{} {:?}", f.number, f.value);
                }
            }
        }
    }
}
