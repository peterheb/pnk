//! wasm-bindgen wrapper around pnk2json: raw document bytes in, pnk JSON
//! string out. The viewer drops a file in the browser, reads it as bytes, and
//! calls `convert` — no backend, no upload (AGENTS.md hackathon model).
//!
//! Media bytes are NOT inlined in the JSON envelope (that would base64-inflate
//! every asset); instead the viewer fetches them per `dataId` via
//! `media_bytes`, which serves raw bytes from the last-converted document's
//! `Data/` store. Rejections (legacy / encrypted / corrupt) throw a JS Error
//! carrying the iwadump layer-tagged message.

use std::cell::RefCell;
use std::collections::HashMap;

use wasm_bindgen::prelude::*;

thread_local! {
    /// The context of the most recent successful conversion, kept so
    /// `media_bytes` can resolve Data/ assets without re-parsing.
    static LAST_CTX: RefCell<Option<pnk2json::ctx::Ctx>> = const { RefCell::new(None) };
    /// Per-asset transcode cache (TIFF -> PNG), cleared on each new convert.
    /// Lazy: an asset is transcoded the first time the viewer asks for it,
    /// so decks full of big TIFFs only pay for the ones actually shown.
    static TRANSCODE_CACHE: RefCell<HashMap<u64, Option<Vec<u8>>>> = RefCell::new(HashMap::new());
}

fn clear_transcode_cache() {
    TRANSCODE_CACHE.with(|c| c.borrow_mut().clear());
}

/// Drop the previous document's context and transcode cache. Called at the
/// START of every conversion, not just after success: a failed convert must
/// not leave the prior document's media queryable via `media_bytes`.
fn clear_document_state() {
    LAST_CTX.with(|slot| *slot.borrow_mut() = None);
    clear_transcode_cache();
}

/// Cap on total cached transcoded bytes. Past it, transcodes are still
/// served but recomputed per request instead of cached.
const MAX_CACHE_BYTES: usize = 256 * 1024 * 1024;

fn cache_bytes() -> usize {
    TRANSCODE_CACHE.with(|c| {
        c.borrow()
            .values()
            .map(|v| v.as_ref().map_or(0, |p| p.len()))
            .sum()
    })
}

/// TIFF magic: little-endian `II*\0` or big-endian `MM\0*` (TIFF 6.0 §2).
fn is_tiff(bytes: &[u8]) -> bool {
    bytes.len() >= 4
        && (bytes[..4] == [0x49, 0x49, 0x2a, 0x00] || bytes[..4] == [0x4d, 0x4d, 0x00, 0x2a])
}

/// Browsers cannot decode TIFF; re-encode as PNG (fast compression — the
/// bytes live only in a session blob URL). Blink/WebKit/Gecko pick raster
/// image decoders by content signature, not MIME type, so the PNG renders
/// even though the blob is still labeled image/tiff by its file name.
fn tiff_to_png(bytes: &[u8]) -> Option<Vec<u8>> {
    // Explicit generous limits, NOT no_limits: pasted TIFFs legitimately
    // reach 22413x4183 (375MB decoded — b52c89c1's word-art images), past
    // the decoder's 512MB default, but a crafted header must not be able to
    // request unbounded decode memory and kill the WASM instance.
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes));
    reader.set_format(image::ImageFormat::Tiff);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(50_000);
    limits.max_image_height = Some(50_000);
    limits.max_alloc = Some(1024 * 1024 * 1024);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    // Slides never need more than ~4K of texture: downscale monsters so the
    // PNG re-encode (and the browser blob) stays tractable.
    let img = if img.width().max(img.height()) > 4096 {
        let (w, h) = (img.width() as f64, img.height() as f64);
        let s = 4096.0 / w.max(h);
        img.thumbnail((w * s) as u32, (h * s) as u32)
    } else {
        img
    };
    let mut out = Vec::new();
    let enc = image::codecs::png::PngEncoder::new_with_quality(
        &mut out,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Adaptive,
    );
    img.write_with_encoder(enc).ok()?;
    Some(out)
}

#[wasm_bindgen]
pub fn convert(bytes: &[u8]) -> Result<String, JsError> {
    clear_document_state();
    let mut ctx =
        pnk2json::ctx::Ctx::from_bytes(bytes).map_err(|e| JsError::new(&e.to_string()))?;
    let doc = pnk2json::convert_ctx(&mut ctx).map_err(|e| JsError::new(&e.to_string()))?;
    LAST_CTX.with(|slot| *slot.borrow_mut() = Some(ctx));
    Ok(pnk2json::to_json_compact(&doc))
}

/// Pretty-printed JSON (debug/inspection use; ~3x larger than compact).
#[wasm_bindgen]
pub fn convert_pretty(bytes: &[u8]) -> Result<String, JsError> {
    clear_document_state();
    let mut ctx =
        pnk2json::ctx::Ctx::from_bytes(bytes).map_err(|e| JsError::new(&e.to_string()))?;
    let doc = pnk2json::convert_ctx(&mut ctx).map_err(|e| JsError::new(&e.to_string()))?;
    LAST_CTX.with(|slot| *slot.borrow_mut() = Some(ctx));
    Ok(pnk2json::to_json(&doc))
}

/// Markdown fallback dump from raw bytes.
#[wasm_bindgen]
pub fn convert_markdown(bytes: &[u8]) -> Result<String, JsError> {
    clear_document_state();
    let mut ctx =
        pnk2json::ctx::Ctx::from_bytes(bytes).map_err(|e| JsError::new(&e.to_string()))?;
    let doc = pnk2json::convert_ctx(&mut ctx).map_err(|e| JsError::new(&e.to_string()))?;
    LAST_CTX.with(|slot| *slot.borrow_mut() = Some(ctx));
    Ok(pnk2json::dumptext::to_markdown(&doc))
}

/// Raw bytes of the media asset with the given DataInfo id (decimal string),
/// from the last successfully converted document. `None` when the asset has
/// no DataInfo entry or its `Data/` bytes are absent (remote/unmaterialized).
#[wasm_bindgen]
pub fn media_bytes(data_id: &str) -> Option<Vec<u8>> {
    let id = data_id.parse::<u64>().ok()?;
    let raw = LAST_CTX.with(|slot| {
        let ctx = slot.borrow();
        let ctx = ctx.as_ref()?;
        let entry = ctx.datas.get(&id)?;
        let name = entry
            .file_name
            .clone()
            .or_else(|| entry.preferred_file_name.clone())?;
        ctx.members.data_file(&name)
    })?;
    // TIFF assets (pasted-image-*.tiff and TIFF-backed masks) transcode to
    // PNG so <img> can render them; anything else passes through untouched.
    // Failures fall back to the raw bytes (better a broken image icon than
    // nothing at all). Cached per asset id for the session.
    if is_tiff(&raw) {
        let cached = TRANSCODE_CACHE.with(|c| c.borrow().get(&id).cloned());
        let png = match cached {
            Some(p) => p,
            None => {
                let p = tiff_to_png(&raw);
                let added = p.as_ref().map_or(0, |v| v.len());
                if cache_bytes() + added <= MAX_CACHE_BYTES {
                    TRANSCODE_CACHE.with(|c| c.borrow_mut().insert(id, p.clone()));
                }
                p
            }
        };
        return Some(png.unwrap_or(raw));
    }
    Some(raw)
}
