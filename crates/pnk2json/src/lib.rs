//! pnk2json — convert iWork '13+ documents (.pages/.numbers/.key) into the pnk
//! JSON document model (phases 4+5).
//!
//! Layer stack: iwadump (container → snappy → envelope → wire walk) →
//! this crate (object graph → resolved model). Everything the converter meets
//! and cannot model becomes a warning or an `UnknownDrawable` — never a silent
//! drop (docs/model-design.md §5). Rejections (legacy, encrypted, corrupt)
//! surface as iwadump layer-tagged errors.

pub mod charts;
pub mod colors;
pub mod ctx;
pub mod drawables;
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

/// Convert using an already-opened context (exposes media/fonts/warnings for
/// the wasm binding and tests).
pub fn convert_ctx(ctx: &mut ctx::Ctx) -> Result<PnkDocument, iwadump::Error> {
    // Aggregate loader findings into envelope warnings first.
    ctx.drain_loader_warnings();

    let root = ctx.loaded.msg(1).cloned();
    let empty = pb::Msg::default();
    let root_ref = root.as_ref().unwrap_or(&empty);
    let mut doc = match ctx.app_kind {
        model::AppKind::Pages => PnkDocument::Pages(pages::convert_document(ctx, root_ref)),
        model::AppKind::Numbers => PnkDocument::Numbers(numbers::convert_document(ctx, root_ref)),
        model::AppKind::Keynote => PnkDocument::Keynote(keynote::convert_document(ctx, root_ref)),
    };

    // Envelope: fonts (deduped, sorted), media inventory, warnings, styles.
    let fonts: Vec<String> = ctx.fonts.iter().cloned().collect();
    let media = ctx.build_media_assets();
    let warnings = std::mem::take(&mut ctx.warnings);
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
    serde_json::to_string_pretty(doc).unwrap_or_else(|_| "{}".to_string())
}

/// Serialize a document to compact JSON.
pub fn to_json_compact(doc: &PnkDocument) -> String {
    serde_json::to_string(doc).unwrap_or_else(|_| "{}".to_string())
}
