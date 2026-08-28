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

use wasm_bindgen::prelude::*;

thread_local! {
    /// The context of the most recent successful conversion, kept so
    /// `media_bytes` can resolve Data/ assets without re-parsing.
    static LAST_CTX: RefCell<Option<pnk2json::ctx::Ctx>> = const { RefCell::new(None) };
}

#[wasm_bindgen]
pub fn convert(bytes: &[u8]) -> Result<String, JsError> {
    let mut ctx = pnk2json::ctx::Ctx::from_bytes(bytes)
        .map_err(|e| JsError::new(&e.to_string()))?;
    let doc = pnk2json::convert_ctx(&mut ctx).map_err(|e| JsError::new(&e.to_string()))?;
    LAST_CTX.with(|slot| *slot.borrow_mut() = Some(ctx));
    Ok(pnk2json::to_json(&doc))
}

/// Markdown fallback dump from raw bytes.
#[wasm_bindgen]
pub fn convert_markdown(bytes: &[u8]) -> Result<String, JsError> {
    let mut ctx = pnk2json::ctx::Ctx::from_bytes(bytes)
        .map_err(|e| JsError::new(&e.to_string()))?;
    let doc = pnk2json::convert_ctx(&mut ctx).map_err(|e| JsError::new(&e.to_string()))?;
    LAST_CTX.with(|slot| *slot.borrow_mut() = Some(ctx));
    Ok(pnk2json::dumptext::to_markdown(&doc))
}

/// Raw bytes of the media asset with the given DataInfo id (decimal string),
/// from the last successfully converted document. `None` when the asset has
/// no DataInfo entry or its `Data/` bytes are absent (remote/unmaterialized).
#[wasm_bindgen]
pub fn media_bytes(data_id: &str) -> Option<Vec<u8>> {
    LAST_CTX.with(|slot| {
        let ctx = slot.borrow();
        let ctx = ctx.as_ref()?;
        let id = data_id.parse::<u64>().ok()?;
        let entry = ctx.datas.get(&id)?;
        let name = entry
            .file_name
            .clone()
            .or_else(|| entry.preferred_file_name.clone())?;
        ctx.members.data_file(&name)
    })
}
