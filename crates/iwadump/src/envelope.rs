//! Envelope layer: one decompressed IWA stream is a sequence of archive
//! segments (docs/format/objects.md), each:
//!
//! 1. a varint byte length,
//! 2. the serialized `TSP.ArchiveInfo` message,
//! 3. one payload blob per `MessageInfo` in `message_infos` order, each
//!    exactly `MessageInfo.length` bytes — **the only delimiter between
//!    payloads**. Undecodable payloads are skipped by declared length, never
//!    by parse (docs/format/gotchas.md #6); an undecodable archive therefore
//!    never desynchronizes the ones after it.
//!
//! There is NO `TSP.PrefixedMessage` wrapper (gotcha #2) and no per-segment CRC.

use crate::error::{Error, Kind, Layer};
use crate::proto::{self, Value};

/// One payload declaration + its raw bytes.
#[derive(Debug, Clone)]
pub struct MessageInfo {
    /// Registry type id (`MessageInfo.type`, field 1) naming the payload's
    /// proto message — not an object id (docs/format/objects.md).
    pub type_id: u32,
    /// Version vector of the writing app (field 2, packed).
    pub version: Vec<u32>,
    /// Payload byte length (field 3) — the only delimiter between payloads.
    pub length: u32,
    /// Hoisted object references (field 5, packed).
    pub object_references: Vec<u64>,
    /// Hoisted media/data references (field 6, packed).
    pub data_references: Vec<u64>,
    /// Patch-message fields 7–11 (base_message_index etc.) when present.
    pub patch: Vec<(u32, Value)>,
    /// The declared-length payload bytes, verbatim.
    pub payload: Vec<u8>,
}

/// One archive segment.
#[derive(Debug, Clone)]
pub struct ArchiveInfo {
    /// This archive's object id in the global id space (`identifier`, field 1).
    pub identifier: u64,
    /// Incremental patch flag (`should_merge`, field 3).
    pub should_merge: bool,
    pub messages: Vec<MessageInfo>,
    /// Byte offset of this segment's ArchiveInfo inside the decoded stream.
    pub offset: usize,
    /// Unknown/extra fields on ArchiveInfo itself (kept for `--message` walks).
    pub unknown_fields: Vec<proto::Field>,
}

impl ArchiveInfo {
    /// Best-effort decodability of each payload: known type ids are walked as
    /// protobuf; structural failure yields a reason. Unknown ids stay opaque
    /// (docs/format/registry.md: never guess a name).
    pub fn message_status(
        &self,
        registry: &crate::registry::Registry,
        app: crate::registry::App,
    ) -> Vec<MessageStatus> {
        self.messages
            .iter()
            .map(|m| match registry.name_for(app, m.type_id) {
                Some(name) => match proto::parse_fields(&m.payload, Layer::Message) {
                    Ok(_) => MessageStatus::Decoded { name },
                    Err(e) => MessageStatus::Undecodable { name, reason: e.message },
                },
                None => MessageStatus::UnknownType,
            })
            .collect()
    }
}

/// Decodability verdict for one payload.
#[derive(Debug, Clone)]
pub enum MessageStatus {
    /// Known registry name, payload walked cleanly.
    Decoded { name: String },
    /// Known registry name, payload failed the structural walk.
    Undecodable { name: String, reason: String },
    /// Registry has no entry: opaque hex only, never a guessed name.
    UnknownType,
}

fn env_err(msg: String) -> Error {
    Error::new(Kind::Corrupt, Layer::Envelope, msg)
}

/// Parse a full decoded IWA stream into archive segments.
pub fn parse_stream(decoded: &[u8]) -> Result<Vec<ArchiveInfo>, Error> {
    let mut archives = Vec::new();
    let mut pos = 0usize;
    while pos < decoded.len() {
        let start = pos;
        // 1. varint byte length of the ArchiveInfo message
        let len = proto::read_varint(decoded, &mut pos, Layer::Envelope)? as usize;
        if len == 0 {
            return Err(env_err(format!(
                "zero-length ArchiveInfo header at stream offset {start}"
            )));
        }
        if decoded.len() - pos < len {
            return Err(env_err(format!(
                "ArchiveInfo at stream offset {start} declares {len} bytes but {} remain",
                decoded.len() - pos
            )));
        }
        let info_bytes = &decoded[pos..pos + len];
        pos += len;

        // 2. the ArchiveInfo message
        let fields = proto::parse_fields(info_bytes, Layer::Envelope).map_err(|e| {
            env_err(format!(
                "ArchiveInfo at stream offset {start} does not parse: {}",
                e.message
            ))
        })?;
        let mut identifier = 0u64;
        let mut should_merge = false;
        let mut message_infos: Vec<Vec<u8>> = Vec::new();
        let mut unknown_fields = Vec::new();
        for f in &fields {
            match (f.number, &f.value) {
                (1, Value::Varint(v)) => identifier = *v,
                (2, Value::Bytes(b)) => message_infos.push(b.clone()),
                (3, Value::Varint(v)) => should_merge = *v != 0,
                _ => unknown_fields.push(f.clone()),
            }
        }

        // 3. payload blobs, delimited ONLY by MessageInfo.length
        let mut messages = Vec::with_capacity(message_infos.len());
        for (i, raw) in message_infos.iter().enumerate() {
            let mi = parse_message_info(raw).map_err(|e| {
                env_err(format!(
                    "MessageInfo {i} of archive at offset {start} does not parse: {}",
                    e.message
                ))
            })?;
            if decoded.len() - pos < mi.length as usize {
                return Err(env_err(format!(
                    "payload {i} (type {}) of archive {} at stream offset {pos} declares {} bytes but {} remain",
                    mi.type_id,
                    identifier,
                    mi.length,
                    decoded.len() - pos
                )));
            }
            let payload = decoded[pos..pos + mi.length as usize].to_vec();
            pos += mi.length as usize;
            messages.push(mi.with_payload(payload));
        }

        archives.push(ArchiveInfo {
            identifier,
            should_merge,
            messages,
            offset: start,
            unknown_fields,
        });
    }
    Ok(archives)
}

/// `TSP.MessageInfo` header only (payload sliced separately by `length`).
fn parse_message_info(buf: &[u8]) -> Result<MessageInfo, Error> {
    let fields = proto::parse_fields(buf, Layer::Envelope)?;
    let mut type_id = 0u32;
    let mut length = 0u32;
    let mut version = Vec::new();
    let mut object_references = Vec::new();
    let mut data_references = Vec::new();
    let mut patch = Vec::new();
    for f in &fields {
        match (f.number, &f.value) {
            (1, Value::Varint(v)) => type_id = *v as u32,
            (3, Value::Varint(v)) => length = *v as u32,
            (2, Value::Bytes(b)) => version.extend(proto::packed_u64s(b, Layer::Envelope)?.into_iter().map(|v| v as u32)),
            (2, Value::Varint(v)) => version.push(*v as u32),
            (5, Value::Bytes(b)) => object_references.extend(proto::packed_u64s(b, Layer::Envelope)?),
            (5, Value::Varint(v)) => object_references.push(*v),
            (6, Value::Bytes(b)) => data_references.extend(proto::packed_u64s(b, Layer::Envelope)?),
            (6, Value::Varint(v)) => data_references.push(*v),
            (4, _) => {} // field_infos: schema-evolution hints; ignorable (objects.md)
            (n @ 7..=11, v) => patch.push((n, v.clone())),
            _ => {} // unknown fields on MessageInfo: skipped, not an error
        }
    }
    Ok(MessageInfo {
        type_id,
        length,
        version,
        object_references,
        data_references,
        patch,
        payload: Vec::new(),
    })
}

impl MessageInfo {
    fn with_payload(mut self, payload: Vec<u8>) -> MessageInfo {
        self.payload = payload;
        self
    }
}
