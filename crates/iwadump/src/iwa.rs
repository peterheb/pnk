//! IWA stream framing: a `.iwa` member is a concatenation of Snappy-compressed
//! blocks (docs/format/iwa.md):
//!
//! ```text
//! +--------+---------------------------+--------+---------------------------+
//! | 4-byte | compressed payload        | 4-byte | compressed payload        |
//! | header |                           | header |                           |
//! +--------+---------------------------+--------+---------------------------+
//! ```
//!
//! Header = one zero chunk-type byte + u24 **little-endian** compressed length.
//! The widely-copied "u16 LE + u16 BE uncompressed size" folklore is wrong
//! (docs/format/gotchas.md #1); the header carries NO uncompressed size.
//! Blocks larger than 64 KiB occur (libetonyek documents header `06 00 01` =
//! compressed length 0x010006), hence u24, not u16.

use crate::error::{Error, Kind, Layer};
use crate::snappy;

/// One decoded block: how many compressed bytes it consumed, and the
/// decompressed payload.
#[derive(Debug, Clone)]
pub struct Block {
    pub compressed_len: u32,
    pub data: Vec<u8>,
}

/// A fully framed IWA stream: blocks decompressed and concatenated, ready for
/// envelope parsing.
#[derive(Debug, Clone)]
pub struct IwaStream {
    pub name: String,
    pub blocks: Vec<Block>,
    /// Blocks concatenated — the protobuf message stream of docs/format/objects.md.
    pub decoded: Vec<u8>,
}

/// Cumulative decoded-bytes ceiling per stream. The largest real corpus
/// streams decode to a few tens of MiB; a crafted file chaining many
/// maximum-ratio blocks must not be able to grow `decoded` without bound.
pub const MAX_STREAM_DECODED: u64 = 1024 * 1024 * 1024;

impl IwaStream {
    /// Frame + decompress one `.iwa` member's raw bytes.
    pub fn parse(name: &str, raw: &[u8]) -> Result<IwaStream, Error> {
        let mut blocks = Vec::new();
        let mut decoded = Vec::new();
        let mut pos = 0usize;
        let mut index = 0usize;
        while pos < raw.len() {
            if raw.len() - pos < 4 {
                return Err(Error::new(
                    Kind::Corrupt,
                    Layer::Iwa,
                    format!(
                        "truncated block header at byte {pos} ({} byte{} left, need 4)",
                        raw.len() - pos,
                        if raw.len() - pos == 1 { "" } else { "s" }
                    ),
                ));
            }
            let header = &raw[pos..pos + 4];
            if header[0] != 0x00 {
                // All four reference implementations require byte 0 == 0
                // (iwa.md). The old "u16+u16" reading would misparse here.
                return Err(Error::new(
                    Kind::Corrupt,
                    Layer::Iwa,
                    format!(
                        "block {index} header at byte {pos} does not start with 0x00 (got 0x{:02x}) — not an IWA snappy stream",
                        header[0]
                    ),
                ));
            }
            let compressed_len =
                header[1] as u32 | (header[2] as u32) << 8 | (header[3] as u32) << 16;
            if compressed_len == 0 {
                return Err(Error::new(
                    Kind::Corrupt,
                    Layer::Iwa,
                    format!("block {index} at byte {pos} declares zero compressed length"),
                ));
            }
            let start = pos + 4;
            let end = start + compressed_len as usize;
            if end > raw.len() {
                return Err(Error::new(
                    Kind::Corrupt,
                    Layer::Iwa,
                    format!(
                        "block {index} at byte {pos} declares {compressed_len} compressed bytes but only {} remain",
                        raw.len() - start
                    ),
                ));
            }
            let data = snappy::decode_block(&raw[start..end], index)?;
            if (decoded.len() as u64).saturating_add(data.len() as u64) > MAX_STREAM_DECODED {
                return Err(Error::new(
                    Kind::Corrupt,
                    Layer::Iwa,
                    format!(
                        "stream exceeds {MAX_STREAM_DECODED} decoded bytes at block {index}; refusing further decompression"
                    ),
                ));
            }
            decoded.extend_from_slice(&data);
            blocks.push(Block {
                compressed_len,
                data,
            });
            pos = end;
            index += 1;
        }
        Ok(IwaStream {
            name: name.to_string(),
            blocks,
            decoded,
        })
    }
}
