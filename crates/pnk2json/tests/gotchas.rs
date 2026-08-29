//! pnk2json tests — gotcha cases from docs/format/gotchas.md exercised on
//! real fixtures (skipped gracefully when the gitignored fixture corpus is
//! not present), plus model-shape unit tests.

use pnk2json::model::*;

fn fixture_path(rel: &str) -> Option<std::path::PathBuf> {
    // Tests run from crates/pnk2json; fixtures live at repo root.
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let p = base.join(rel);
    if p.exists() {
        Some(p)
    } else {
        None
    }
}

#[test]
fn gotcha12_encrypted_iwph_rejects_cleanly() {
    let Some(path) = fixture_path(
        "fixtures/crawl/c8c80b312267e024a58c21b579f8a91274a631266a60af2db3a4d915f2cdf054.key",
    ) else {
        eprintln!("fixture corpus absent; skipping");
        return;
    };
    let err = pnk2json::convert_path(&path).unwrap_err();
    assert_eq!(err.kind, iwadump::Kind::Encrypted);
    assert!(err.message.contains("encrypted"), "{}", err.message);
}

#[test]
fn gotcha12_encrypted_iwpv2_rejects_cleanly() {
    let Some(path) = fixture_path(
        "fixtures/crawl/2dccc804091e1ac60d0fd3aa727388b9d5b08051008828bf7cb23e88a5df9ab3.key",
    ) else {
        eprintln!("fixture corpus absent; skipping");
        return;
    };
    let err = pnk2json::convert_path(&path).unwrap_err();
    assert_eq!(err.kind, iwadump::Kind::Encrypted);
}

#[test]
fn legacy_document_rejects_cleanly() {
    let Some(path) = fixture_path(
        "fixtures/crawl_old/0189c9dc3da0c1477042f5e4e7594116b4baa7621f7ed747808f9833a168e14a.unknown",
    ) else {
        eprintln!("fixture corpus absent; skipping");
        return;
    };
    let err = pnk2json::convert_path(&path).unwrap_err();
    assert_eq!(err.kind, iwadump::Kind::Legacy);
    assert!(err.message.contains("legacy"), "{}", err.message);
}

#[test]
fn gotcha11_operation_storage_is_skipped_not_decoded() {
    // Real fixture carrying an LZFSE OperationStorage.iwa member: the
    // converter must ignore it (collab log, not an IWA stream) and convert.
    let Some(path) = fixture_path(
        "fixtures/crawl/249f9484d3359f8773b733b824ff12c5038e5c56d42223bc4c7c962ddc4c6a9d.key",
    ) else {
        eprintln!("fixture corpus absent; skipping");
        return;
    };
    let container = iwadump::Container::open(&path, false).unwrap();
    assert!(
        container.non_iwa.iter().any(|(n, _)| n.contains("OperationStorage")),
        "fixture should carry OperationStorage"
    );
    let doc = pnk2json::convert_path(&path).unwrap();
    match &doc {
        PnkDocument::Keynote(d) => assert!(!d.slides.is_empty()),
        _ => panic!("expected keynote"),
    }
}

#[test]
fn gotcha4_unknown_type_ids_become_warnings() {
    // G2 golden fixture (Pages 26.3.1 fresh save): its lone
    // unknown-object-type warning is type 10017 (0x2721) — genuinely absent
    // from both the embedded tables and keynote-parser 14.5's registry.
    // (The 14.5 registry upgrade resolved the older gantt-fixture ids.)
    let Some(path) = fixture_path("fixtures/golden/G2-golden-pages-layout.pages") else {
        eprintln!("fixture corpus absent; skipping");
        return;
    };
    let doc = pnk2json::convert_path(&path).unwrap();
    let PnkDocument::Pages(d) = &doc else { panic!("expected pages") };
    let unknown = d
        .warnings
        .iter()
        .find(|w| w.code == WarningCode::UnknownObjectType)
        .expect("unknown-object-type warning expected");
    assert!(unknown.detail.as_deref().map(|s| s.starts_with("0x")).unwrap_or(false));
}

#[test]
fn keynote_fixture_converts_with_envelope() {
    let Some(path) = fixture_path(
        "fixtures/crawl/6b8460d2240a2fdf7a7c3fb2b9ce5ddfee064439faad05f21a5b041d3ca528b3.key",
    ) else {
        eprintln!("fixture corpus absent; skipping");
        return;
    };
    let doc = pnk2json::convert_path(&path).unwrap();
    let PnkDocument::Keynote(d) = &doc else { panic!("expected keynote") };
    assert_eq!(d.kind, "keynote");
    assert_eq!(d.meta.app, AppKind::Keynote);
    assert_eq!(d.slides.len(), 2);
    assert_eq!(d.slide_size.width, 1920.0);
    // Storage text must survive conversion (UTF-16 offset splitting works).
    let texts: Vec<String> = d
        .slides
        .iter()
        .flat_map(|s| s.drawables.iter())
        .filter_map(|dr| match dr {
            Drawable::Textbox { text, .. } => Some(crate_text(text)),
            _ => None,
        })
        .collect();
    assert!(
        texts.iter().any(|t| t.contains("Edit the Keynote Gantt chart template")),
        "expected storage text in converted drawables: {texts:?}"
    );
    // Meta from container plists.
    assert!(d.meta.application.is_some() || d.meta.build_version_history.is_some());
}

fn crate_text(st: &StyledText) -> String {
    st.paragraphs
        .iter()
        .flat_map(|p| p.items.iter())
        .filter_map(|i| match i {
            ParagraphItem::Plain(text) | ParagraphItem::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("")
}

#[test]
fn serde_field_names_match_ts_contract() {
    // Discriminants and field names must match model/src (kebab-case tags,
    // camelCase fields). round-trip a small document.
    let doc = KeynoteDocument {
        kind: "keynote".to_string(),
        meta: DocumentMeta {
            app: AppKind::Keynote,
            application: Some("Keynote".into()),
            ..Default::default()
        },
        warnings: vec![Warning {
            code: WarningCode::UnknownObjectType,
            message: "test".into(),
            path: None,
            detail: Some("0xde".into()),
        }],
        fonts: vec!["Helvetica".into()],
        styles: StylePools::default(),
        media: vec![MediaAsset {
            data_id: "1".into(),
            file_name: Some("a.png".into()),
            preferred_file_name: None,
            kind: MediaKind::Image,
            byte_length: Some(10),
            pixel_size: Some(Size { width: 2.0, height: 2.0 }),
        }],
        slide_size: Size { width: 1280.0, height: 720.0 },
        slides: vec![Slide {
            name: Some("S".into()),
            skipped: Some(false),
            master_name: None,
            drawables: vec![Drawable::Shape {
                common: DrawableCommon {
                    position: Some(Point { x: 1.0, y: 2.0 }),
                    angle_deg: Some(90.0),
                    keynote_build: Some(BuildSpec {
                        delivery: BuildDelivery::In,
                        ..Default::default()
                    }),
                    placeholder: Some(PlaceholderInfo { role: "title".into(), inherited: None }),
                    ..Default::default()
                },
                geometry: ShapeGeometry {
                    preset: Some("star".into()),
                    ..Default::default()
                },
                text: None,
                vertical_alignment: None,
                text_insets: None,
            }],
            notes: None,
            transition: None,
            slide_number_visible: None,
            background: None,
        }],
        masters: Vec::new(),
        theme_name: None,
        playback: None,
        soundtrack: None,
        recording: None,
    };
    let json = serde_json::to_value(&doc).unwrap();
    assert_eq!(json["kind"], "keynote");
    assert_eq!(json["styles"]["para"], serde_json::json!([]));
    assert_eq!(json["styles"]["char"], serde_json::json!([]));
    assert_eq!(json["meta"]["app"], "keynote");
    assert_eq!(json["meta"]["application"], "Keynote");
    assert_eq!(json["warnings"][0]["code"], "unknown-object-type");
    assert_eq!(json["warnings"][0]["detail"], "0xde");
    assert_eq!(json["media"][0]["dataId"], "1");
    assert_eq!(json["media"][0]["byteLength"], 10);
    assert_eq!(json["media"][0]["pixelSize"]["width"], 2.0);
    assert_eq!(json["slideSize"]["width"], 1280.0);
    assert_eq!(json["slides"][0]["drawables"][0]["type"], "shape");
    assert_eq!(json["slides"][0]["drawables"][0]["geometry"]["preset"], "star");
    assert_eq!(json["slides"][0]["drawables"][0]["common"]["angleDeg"], 90.0);
    assert_eq!(
        json["slides"][0]["drawables"][0]["common"]["keynoteBuild"]["delivery"],
        "in"
    );
    assert_eq!(
        json["slides"][0]["drawables"][0]["common"]["placeholder"]["role"],
        "title"
    );
    // Round-trip through strict JSON.
    let back: KeynoteDocument = serde_json::from_value(json).unwrap();
    assert_eq!(back, doc);
}

#[test]
fn cell_value_union_shape() {
    let cell = CellValue::Currency { value: 3.5, currency_code: Some("USD".into()) };
    let json = serde_json::to_value(&cell).unwrap();
    assert_eq!(json["type"], "currency");
    assert_eq!(json["value"], 3.5);
    assert_eq!(json["currencyCode"], "USD");

    let empty = serde_json::to_value(CellValue::Empty).unwrap();
    assert_eq!(empty["type"], "empty");
}

#[test]
fn json_escapes_zs_cf_code_points() {
    // gotchas #15/#17: space separators (Zs) and format chars (Cf) escape as
    // \uXXXX in JSON output; space/tab stay raw; decoded strings identical.
    use pnk2json::model::*;
    let doc = KeynoteDocument {
        kind: "keynote".to_string(),
        meta: DocumentMeta { app: AppKind::Keynote, ..Default::default() },
        warnings: vec![],
        fonts: vec![],
        media: vec![],
        styles: StylePools::default(),
        slide_size: Size { width: 100.0, height: 100.0 },
        slides: vec![Slide {
            name: None,
            skipped: None,
            master_name: None,
            drawables: vec![Drawable::Textbox {
                common: DrawableCommon::default(),
                text: StyledText {
                    paragraphs: vec![Paragraph {
                        p_style: None,
                        items: vec![
                            ParagraphItem::Plain("a\u{00a0}b\u{200b}c\u{200d}d\u{202f}e\u{3000}f".into()),
                        ],
                    }],
                },
                vertical_alignment: None,
                text_insets: None,
            }],
            notes: None,
            transition: None,
            slide_number_visible: None,
            background: None,
        }],
        masters: Vec::new(),
        theme_name: None,
        playback: None,
        soundtrack: None,
        recording: None,
    };
    let doc = PnkDocument::Keynote(doc);
    let json = pnk2json::to_json(&doc);
    // raw Zs/Cf bytes must be absent
    for (name, cp) in [
        ("NBSP", '\u{00a0}'),
        ("ZWSP", '\u{200b}'),
        ("ZWNJ", '\u{200c}'),
        ("ZWJ", '\u{200d}'),
        ("narrow-NBSP", '\u{202f}'),
        ("ideographic-space", '\u{3000}'),
        ("LS", '\u{2028}'),
        ("PS", '\u{2029}'),
    ] {
        assert!(
            !json.contains(cp),
            "raw {name} (U+{:04X}) leaked into JSON output",
            cp as u32
        );
    }
    // escaped forms present
    assert!(json.contains("\\u00a0"), "NBSP escape missing");
    assert!(json.contains("\\u200b"), "ZWSP escape missing");
    assert!(json.contains("\\u200d"), "ZWJ escape missing");
    assert!(json.contains("\\u202f"), "narrow-NBSP escape missing");
    assert!(json.contains("\\u3000"), "ideographic-space escape missing");
    // plain space + tab stay raw
    assert!(json.contains(' '), "plain space must stay raw");
    // decoded string is identical to input
    let back: serde_json::Value = serde_json::from_str(&json).unwrap();
    let text = back["slides"][0]["drawables"][0]["text"]["paragraphs"][0]["items"][0]
        .as_str()
        .unwrap();
    assert_eq!(text, "a\u{00a0}b\u{200b}c\u{200d}d\u{202f}e\u{3000}f");
}
