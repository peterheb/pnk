//! Generic protobuf wire-format walker.
//!
//! We have no per-message schemas (the recovered protos carry no registry —
//! docs/format/gotchas.md #4), so payloads are walked structurally: every
//! field is reported by (number, wire type, value). Wire types 3/4 (start/end
//! group) occur in real TSP data and are nested, never an error.

use crate::error::{Error, Kind, Layer};

pub const WIRE_VARINT: u8 = 0;
pub const WIRE_I64: u8 = 1;
pub const WIRE_LEN: u8 = 2;
pub const WIRE_SGROUP: u8 = 3;
pub const WIRE_EGROUP: u8 = 4;
pub const WIRE_I32: u8 = 5;

#[derive(Debug, Clone)]
pub enum Value {
    Varint(u64),
    Fixed32([u8; 4]),
    Fixed64([u8; 8]),
    /// Length-delimited bytes (nested message, string, packed, or opaque).
    Bytes(Vec<u8>),
    /// A parsed group (wire type 3): fields up to the matching end-group tag.
    Group(Vec<Field>),
}

/// Human-readable wire-type name (field walks).
pub fn wire_name(wire: u8) -> &'static str {
    match wire {
        WIRE_VARINT => "varint",
        WIRE_I64 => "fixed64",
        WIRE_LEN => "len",
        WIRE_SGROUP => "group",
        WIRE_EGROUP => "end-group",
        WIRE_I32 => "fixed32",
        _ => "?",
    }
}

#[derive(Debug, Clone)]
pub struct Field {
    pub number: u32,
    pub wire: u8,
    pub value: Value,
}

fn err(layer: Layer, msg: String) -> Error {
    Error::new(Kind::Corrupt, layer, msg)
}

/// Read one base-128 varint; `pos` advances past it.
pub fn read_varint(buf: &[u8], pos: &mut usize, layer: Layer) -> Result<u64, Error> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    loop {
        if *pos >= buf.len() {
            return Err(Error::new(Kind::Corrupt, layer, String::from("truncated varint")));
        }
        if shift >= 64 {
            return Err(Error::new(Kind::Corrupt, layer, String::from("varint exceeds 64 bits")));
        }
        let b = buf[*pos];
        *pos += 1;
        value |= ((b & 0x7f) as u64) << shift;
        if b & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
    }
}

/// Outcome of a field-scan: the fields found, how many input bytes they
/// consumed, and whether the scan stopped at a matching end-group tag.
struct Scan {
    fields: Vec<Field>,
    consumed: usize,
    closed: bool,
}

/// Parse a buffer into a flat field list. Groups are nested recursively into
/// `Value::Group`. Any structural error (bad wire type, truncated field,
/// unmatched group) fails the walk — callers treat that as "payload not
/// decodable"; the *stream* stays synchronized because payloads are
/// length-delimited (docs/format/gotchas.md #6).
pub fn parse_fields(buf: &[u8], layer: Layer) -> Result<Vec<Field>, Error> {
    scan(buf, None, layer).map(|s| s.fields)
}

/// Scan fields until the buffer ends, or until an end-group tag matching
/// `group_field` appears (`closed == true`).
fn scan(buf: &[u8], group_field: Option<u32>, layer: Layer) -> Result<Scan, Error> {
    let mut fields = Vec::new();
    let mut pos = 0usize;
    while pos < buf.len() {
        let start = pos;
        let tag = read_varint(buf, &mut pos, layer)?;
        let number = (tag >> 3) as u32;
        let wire = (tag & 0x7) as u8;
        if number == 0 {
            return Err(err(layer, "field number 0 is invalid".into()));
        }
        let value = match wire {
            WIRE_VARINT => Value::Varint(read_varint(buf, &mut pos, layer)?),
            WIRE_I64 => {
                if buf.len() - pos < 8 {
                    return Err(err(layer, "truncated fixed64".into()));
                }
                let mut b = [0u8; 8];
                b.copy_from_slice(&buf[pos..pos + 8]);
                pos += 8;
                Value::Fixed64(b)
            }
            WIRE_LEN => {
                let len = read_varint(buf, &mut pos, layer)? as usize;
                if buf.len() - pos < len {
                    return Err(err(
                        layer,
                        format!("length-delimited field {number} declares {len} bytes but {} remain", buf.len() - pos),
                    ));
                }
                let bytes = buf[pos..pos + len].to_vec();
                pos += len;
                Value::Bytes(bytes)
            }
            WIRE_SGROUP => {
                // Group content runs to the matching end-group tag; recurse.
                let inner = scan(&buf[pos..], Some(number), layer)?;
                if !inner.closed {
                    return Err(err(layer, format!("group field {number} is never closed")));
                }
                let group = inner.fields;
                pos += inner.consumed;
                Value::Group(group)
            }
            WIRE_EGROUP => {
                return match group_field {
                    Some(g) if g == number => Ok(Scan { fields, consumed: pos, closed: true }),
                    Some(g) => Err(err(
                        layer,
                        format!("end-group tag for field {number} inside group {g}"),
                    )),
                    None => Err(err(
                        layer,
                        format!("end-group tag for field {number} with no open group"),
                    )),
                };
            }
            WIRE_I32 => {
                if buf.len() - pos < 4 {
                    return Err(err(layer, "truncated fixed32".into()));
                }
                let mut b = [0u8; 4];
                b.copy_from_slice(&buf[pos..pos + 4]);
                pos += 4;
                Value::Fixed32(b)
            }
            other => {
                return Err(err(layer, format!("invalid wire type {other} on field {number}")));
            }
        };
        fields.push(Field { number, wire, value });
        debug_assert!(pos > start);
    }
    match group_field {
        Some(_) => Err(err(
            layer,
            format!("group {group_field:?} reaches end of buffer without end-group tag"),
        )),
        None => Ok(Scan { fields, consumed: buf.len(), closed: false }),
    }
}

/// Interpret length-delimited bytes as packed repeated varints (used for
/// `MessageInfo.version` / `object_references` / `data_references`, packed
/// uint32/uint64 per TSPArchiveMessages.proto).
pub fn packed_u64s(bytes: &[u8], layer: Layer) -> Result<Vec<u64>, Error> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        out.push(read_varint(bytes, &mut pos, layer)?);
    }
    Ok(out)
}
