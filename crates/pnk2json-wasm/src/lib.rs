//! wasm-bindgen wrapper around pnk2json: raw document bytes in, pnk JSON
//! string out. The viewer drops a file in the browser, reads it as bytes, and
//! calls `convert` — no backend, no upload (AGENTS.md hackathon model).
//!
//! Rejections (legacy / encrypted / corrupt) throw a JS Error carrying the
//! iwadump layer-tagged message.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub fn convert(bytes: &[u8]) -> Result<String, JsError> {
    let mut ctx = pnk2json::ctx::Ctx::from_bytes(bytes)
        .map_err(|e| JsError::new(&e.to_string()))?;
    let doc = pnk2json::convert_ctx(&mut ctx).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(pnk2json::to_json(&doc))
}

/// Markdown fallback dump from raw bytes.
#[wasm_bindgen]
pub fn convert_markdown(bytes: &[u8]) -> Result<String, JsError> {
    let mut ctx = pnk2json::ctx::Ctx::from_bytes(bytes)
        .map_err(|e| JsError::new(&e.to_string()))?;
    let doc = pnk2json::convert_ctx(&mut ctx).map_err(|e| JsError::new(&e.to_string()))?;
    Ok(pnk2json::dumptext::to_markdown(&doc))
}
