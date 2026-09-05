//! pnk2json — convert iWork '13+ documents (.pages/.numbers/.key) into the pnk
//! JSON document model (phases 4+5).
//!
//! Layer stack: iwadump (container → snappy → envelope → wire walk) →
//! this crate (object graph → resolved model). Everything the converter meets
//! and cannot model becomes a warning or an `UnknownDrawable` — never a silent
//! drop (docs/model-design.md §5). Rejections (legacy, encrypted, corrupt)
//! surface as iwadump layer-tagged errors.

pub mod categories;
pub mod charts;
pub mod colors;
pub mod ctx;
pub mod drawables;
pub mod formulas;
pub mod function_names;
pub mod dumptext;
pub mod keynote;
pub mod loader;
pub mod members;
pub mod model;
pub mod numbers;
pub mod pages;
pub mod pb;
pub mod styles;
pub mod tables;
pub mod text;
pub mod tsd;

pub use model::PnkDocument;

/// Convert a document at `path` (flat file or package directory) to the pnk
/// model. Encrypted and legacy documents are rejected with layer-tagged
/// errors (docs/format/legacy.md, docs/format/container.md).
pub fn convert_path(path: &std::path::Path) -> Result<PnkDocument, iwadump::Error> {
    let mut ctx = ctx::Ctx::open(path)?;
    convert_ctx(&mut ctx)
}

/// Collapse warning floods (docs/model-review.md §1 Leak A): rows sharing
/// (code, message-with-digit-runs-normalized) merge into the first row with
/// `count` = total and up to 5 distinct example `paths`. Degraded corpus docs
/// carry hundreds of per-cell rows ("cell r1c1 dropped" / "cell r0c3 dropped")
/// that differ only in coordinates; normalizing digit runs to '#' is the
/// dedupe key, while the surviving row keeps its original message verbatim.
fn aggregate_warnings(warnings: Vec<model::Warning>) -> Vec<model::Warning> {
    use std::collections::HashMap;
    let mut order: Vec<model::Warning> = Vec::new();
    let mut index: HashMap<(model::WarningCode, String), usize> = HashMap::new();
    // Codes whose `detail` IS the warning's identity (a type id, an asset
    // name): digit-normalized messages would merge distinct identities into
    // the first row (FINDINGS.md M-9 — e.g. type ids 0x2222 and 0x3333 both
    // normalize to "0x#"). Their details join the key; coordinate-flood
    // codes keep the message-only key so per-cell spam still collapses.
    let identity_coded = |c: model::WarningCode| {
        matches!(
            c,
            model::WarningCode::UnknownObjectType
                | model::WarningCode::UndecodableObject
                | model::WarningCode::MediaMissing
        )
    };
    let normalize = |m: &str| {
        let mut out = String::with_capacity(m.len());
        let mut in_digits = false;
        for c in m.chars() {
            if c.is_ascii_digit() {
                if !in_digits {
                    out.push('#');
                    in_digits = true;
                }
            } else {
                in_digits = false;
                out.push(c);
            }
        }
        out
    };
    for w in warnings {
        let mut key_text = normalize(&w.message);
        if identity_coded(w.code) {
            if let Some(d) = &w.detail {
                key_text.push('\u{1f}');
                key_text.push_str(d);
            }
        }
        let key = (w.code, key_text);
        match index.get(&key) {
            Some(&i) => {
                let first = &mut order[i];
                first.count = Some(first.count.unwrap_or(1) + 1);
                if let Some(p) = &w.path {
                    let paths = first
                        .paths
                        .get_or_insert_with(|| first.path.iter().cloned().collect());
                    if paths.len() < 5 && !paths.contains(p) {
                        paths.push(p.clone());
                    }
                }
            }
            None => {
                index.insert(key, order.len());
                order.push(w);
            }
        }
    }
    order
}

/// Convert using an already-opened context (exposes media/fonts/warnings for
/// the wasm binding and tests).
pub fn convert_ctx(ctx: &mut ctx::Ctx) -> Result<PnkDocument, iwadump::Error> {
    // Aggregate loader findings into envelope warnings first.
    ctx.drain_loader_warnings();

    // Object 1 is the document root by convention (docs/format/objects.md);
    // every file in the 964-file corpus carries a decodable one. A missing
    // root means this is not a usable iWork document — reject instead of
    // fabricating an empty Pages document from Msg::default()
    // (FINDINGS.md M-4: corrupt/arbitrary ZIPs were accepted as empty docs).
    let root = ctx.loaded.msg(1).cloned();
    let Some(root_ref) = root.as_ref() else {
        return Err(iwadump::Error::new(
            iwadump::error::Kind::Unsupported,
            iwadump::error::Layer::Message,
            "no decodable document root (object 1) — not a usable iWork document",
        ));
    };
    let mut doc = match ctx.app_kind {
        model::AppKind::Pages => PnkDocument::Pages(pages::convert_document(ctx, root_ref)),
        model::AppKind::Numbers => PnkDocument::Numbers(numbers::convert_document(ctx, root_ref)),
        model::AppKind::Keynote => PnkDocument::Keynote(keynote::convert_document(ctx, root_ref)),
    };

    // Envelope: fonts (deduped, sorted), media inventory, warnings, styles.
    let fonts: Vec<String> = ctx.fonts.iter().cloned().collect();
    let media = ctx.build_media_assets();
    let warnings = aggregate_warnings(std::mem::take(&mut ctx.warnings));
    let styles = model::StylePools {
        para: std::mem::take(&mut ctx.para_pool.items),
        char: std::mem::take(&mut ctx.char_pool.items),
    };
    match &mut doc {
        PnkDocument::Pages(d) => {
            d.fonts = fonts;
            d.media = media;
            d.warnings = warnings;
            d.styles = styles;
        }
        PnkDocument::Numbers(d) => {
            d.fonts = fonts;
            d.media = media;
            d.warnings = warnings;
            d.styles = styles;
        }
        PnkDocument::Keynote(d) => {
            d.fonts = fonts;
            d.media = media;
            d.warnings = warnings;
            d.styles = styles;
        }
    }
    Ok(doc)
}

/// Serialize a document to pretty JSON.
pub fn to_json(doc: &PnkDocument) -> String {
    escape_review_hazards(&serde_json::to_string_pretty(doc).unwrap_or_else(|_| "{}".to_string()))
}

/// Serialize a document to compact JSON.
pub fn to_json_compact(doc: &PnkDocument) -> String {
    escape_review_hazards(&serde_json::to_string(doc).unwrap_or_else(|_| "{}".to_string()))
}

/// Escape code points that JSON allows raw but that corrupt human review:
/// - U+2028/U+2029 (LS/PS): editor "unusual line terminator" warnings that
///   mask real stray-LS bugs (gotchas #15);
/// - Unicode space separators (Zs) and format characters (Cf): invisible or
///   width-ambiguous (NBSP, ZWSP, ZWNJ, ZWJ, BOM, bidi marks) — reviewers
///   cannot distinguish them from ASCII spaces by eye.
///
/// Spec-legal raw in JSON strings; escaping is a no-op semantically
/// (json.loads of old vs new output yields identical strings). Safe as a
/// whole-text pass: these code points only occur inside string literals.
/// Regular space and tab are real whitespace and stay raw (serde_json's
/// control escaping is untouched).
fn escape_review_hazards(json: &str) -> String {
    fn needs_escape(c: char) -> bool {
        if matches!(c, '\u{2028}' | '\u{2029}') {
            return true;
        }
        matches!(c,
            // Zs — space separators
            '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
            // Cf — format characters
            | '\u{00ad}' | '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206f}' | '\u{feff}'
        )
    }
    if !json.chars().any(needs_escape) {
        return json.to_string();
    }
    let mut out = String::with_capacity(json.len() + 32);
    for c in json.chars() {
        if needs_escape(c) {
            out.push_str(&format!("\\u{:04x}", c as u32));
        } else {
            out.push(c);
        }
    }
    out
}
