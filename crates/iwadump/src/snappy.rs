//! Raw Snappy block decode.
//!
//! `.iwa` payloads are **raw** Snappy blocks — NOT the Snappy framing format
//! (no stream identifier, no CRC-32C; docs/format/iwa.md). The uncompressed
//! length is not in the block header; it is the leading varint inside the raw
//! block itself, which the `snap` decoder consumes internally.

use crate::error::{Error, Kind, Layer};

/// Per-block decoded-size ceiling. Real writers emit ~64 KiB decoded blocks
/// (docs/format/iwa.md; the u24 header caps compressed size at 16 MiB), so
/// 64 MiB is a 1000x margin. Without this check a five-byte payload whose
/// leading varint declares ~4 GiB makes `decompress_vec` allocate that much
/// before validating a single Snappy command.
pub const MAX_BLOCK_DECODED: u64 = 64 * 1024 * 1024;

/// Decode one raw Snappy block. On failure the message says the block failed
/// to decompress — keynote-parser would silently emit the compressed bytes
/// here; we surface the corruption instead (docs/format/iwa.md implementation
/// note: pnk wants corruption surfaced, not masked).
pub fn decode_block(payload: &[u8], block_index: usize) -> Result<Vec<u8>, Error> {
    // Same phrasing as the decode failure below — gotcha #7 wants "block N
    // failed to decompress", and an unreadable length varint IS that.
    let declared = snap::raw::decompress_len(payload).map_err(|e| {
        Error::new(
            Kind::Corrupt,
            Layer::Snappy,
            format!("block {block_index} failed to decompress ({e}); stream may be truncated or corrupt"),
        )
    })? as u64;
    if declared > MAX_BLOCK_DECODED {
        return Err(Error::new(
            Kind::Corrupt,
            Layer::Snappy,
            format!(
                "block {block_index} declares {declared} decoded bytes (limit {MAX_BLOCK_DECODED}); refusing the allocation"
            ),
        ));
    }
    let mut decoder = snap::raw::Decoder::new();
    decoder.decompress_vec(payload).map_err(|e| {
        Error::new(
            Kind::Corrupt,
            Layer::Snappy,
            format!("block {block_index} failed to decompress ({e}); stream may be truncated or corrupt"),
        )
    })
}

/// Compress one raw Snappy block (test helper and round-trip proof).
pub fn encode_block(data: &[u8]) -> Vec<u8> {
    snap::raw::Encoder::new()
        .compress_vec(data)
        .expect("snappy encode cannot fail on valid input")
}
