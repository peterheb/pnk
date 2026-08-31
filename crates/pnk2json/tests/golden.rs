//! Golden-fixture regression tripwire: convert the hand-built Pages 26.3.1
//! fixture and semantic-diff against the expected JSON (structure + values;
//! JSON text formatting and key order are irrelevant).

use std::path::PathBuf;

fn golden_path(rel: &str) -> Option<PathBuf> {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let p = base.join(rel);
    p.exists().then_some(p)
}

/// Deep-compare two serde_json::Values with array order significant and
/// object key order irrelevant. Returns a list of divergence paths.
fn diff(a: &serde_json::Value, b: &serde_json::Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (serde_json::Value::Object(x), serde_json::Value::Object(y)) => {
            for k in x
                .keys()
                .chain(y.keys())
                .collect::<std::collections::BTreeSet<_>>()
            {
                let p = format!("{path}.{k}");
                match (x.get(k), y.get(k)) {
                    (Some(u), Some(v)) => diff(u, v, &p, out),
                    (Some(u), None) => out.push(format!("MISSING {p} = {}", short(u))),
                    (None, Some(v)) => out.push(format!("EXTRA {p} = {}", short(v))),
                    _ => {}
                }
            }
        }
        (serde_json::Value::Array(x), serde_json::Value::Array(y)) => {
            if x.len() != y.len() {
                out.push(format!("LEN {path}: expected {}, got {}", x.len(), y.len()));
            }
            for (i, (u, v)) in x.iter().zip(y.iter()).enumerate() {
                diff(u, v, &format!("{path}[{i}]"), out);
            }
        }
        _ => {
            if a != b {
                out.push(format!(
                    "VALUE {path}: expected {}, got {}",
                    short(a),
                    short(b)
                ));
            }
        }
    }
}

fn short(v: &serde_json::Value) -> String {
    let s = v.to_string();
    if s.len() > 70 {
        // byte-safe truncation: back up to a char boundary so multi-byte
        // chars (emoji, CJK) in mismatch snippets never panic
        let mut end = 70.min(s.len());
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &s[..end])
    } else {
        s
    }
}

#[test]
fn golden_g1_word_processing_matches_expected() {
    let Some(fixture) = golden_path("fixtures/golden/G1-golden-pages-wp.pages") else {
        eprintln!("golden fixture absent; skipping");
        return;
    };
    let Some(expected_path) = golden_path("fixtures/golden/expected/G1-golden-pages-wp.json")
    else {
        eprintln!("expected JSON absent; skipping");
        return;
    };

    let ours: serde_json::Value = serde_json::from_str(&pnk2json::to_json(
        &pnk2json::convert_path(&fixture).unwrap(),
    ))
    .unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(expected_path).unwrap()).unwrap();

    let mut out = Vec::new();
    diff(&expected, &ours, "", &mut out);
    assert!(
        out.is_empty(),
        "golden G1 diverged ({}):\n{}",
        out.len(),
        out.join("\n")
    );
}

#[test]
fn golden_g2_page_layout_matches_expected() {
    let Some(fixture) = golden_path("fixtures/golden/G2-golden-pages-layout.pages") else {
        eprintln!("golden fixture absent; skipping");
        return;
    };
    let Some(expected_path) = golden_path("fixtures/golden/expected/G2-golden-pages-layout.json")
    else {
        eprintln!("expected JSON absent; skipping");
        return;
    };

    let ours: serde_json::Value = serde_json::from_str(&pnk2json::to_json(
        &pnk2json::convert_path(&fixture).unwrap(),
    ))
    .unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(expected_path).unwrap()).unwrap();

    let mut out = Vec::new();
    diff(&expected, &ours, "", &mut out);
    assert!(
        out.is_empty(),
        "golden G2 diverged ({}):\n{}",
        out.len(),
        out.join("\n")
    );
}

/// Byte-level determinism: converting the same document twice must produce
/// identical JSON (FINDINGS.md H-7 — randomized HashMap drain order once
/// leaked into style-pool indices). Runs the goldens plus a handful of
/// corpus Numbers fixtures when the (gitignored) crawl symlink is present.
#[test]
fn conversion_is_byte_deterministic() {
    let mut candidates: Vec<PathBuf> = [
        "fixtures/golden/G1-golden-pages-wp.pages",
        "fixtures/golden/G2-golden-pages-layout.pages",
        "fixtures/golden/G5-golden-pages-acid.pages",
    ]
    .iter()
    .filter_map(|r| golden_path(r))
    .collect();
    if let Some(crawl) = golden_path("fixtures/crawl") {
        if let Ok(rd) = std::fs::read_dir(crawl) {
            let mut numbers: Vec<PathBuf> = rd
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|x| x == "numbers"))
                .collect();
            numbers.sort();
            candidates.extend(numbers.into_iter().take(5));
        }
    }
    if candidates.is_empty() {
        eprintln!("no fixtures present; skipping");
        return;
    }
    for path in candidates {
        let a = pnk2json::to_json(&pnk2json::convert_path(&path).unwrap());
        let b = pnk2json::to_json(&pnk2json::convert_path(&path).unwrap());
        assert_eq!(a, b, "nondeterministic conversion: {}", path.display());
    }
}
