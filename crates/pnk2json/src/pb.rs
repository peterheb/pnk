//! Typed accessors over iwadump's generic protobuf field walk.
//!
//! The protos carry no schema registry (docs/format/gotchas.md #4), so every
//! message is decoded by hand-written field numbers (docs/format/*.md). These
//! helpers keep that decoding terse and tolerant: a wrong wire type yields
//! `None`, never a panic.

use iwadump::proto::{self, Field, Value};

/// A decoded payload: the flat field list of one TSP archive message.
#[derive(Debug, Clone, Default)]
pub struct Msg {
    pub fields: Vec<Field>,
}

impl Msg {
    pub fn parse(payload: &[u8]) -> Option<Msg> {
        proto::parse_fields(payload, iwadump::error::Layer::Message)
            .ok()
            .map(|fields| Msg { fields })
    }

    /// Last occurrence of field `n` (proto2 optional: last wins).
    pub fn get(&self, n: u32) -> Option<&Value> {
        self.fields.iter().rev().find(|f| f.number == n).map(|f| &f.value)
    }

    /// Every occurrence of field `n` in order.
    pub fn all(&self, n: u32) -> Vec<&Value> {
        self.fields.iter().filter(|f| f.number == n).map(|f| &f.value).collect()
    }

    pub fn has(&self, n: u32) -> bool {
        self.fields.iter().any(|f| f.number == n)
    }

    pub fn varint(&self, n: u32) -> Option<u64> {
        match self.get(n) {
            Some(Value::Varint(v)) => Some(*v),
            _ => None,
        }
    }

    /// Signed enum/int field (proto2 int32/int64/enum: sign-extended varint).
    pub fn int(&self, n: u32) -> Option<i64> {
        self.varint(n).map(|v| v as i64)
    }

    pub fn boolean(&self, n: u32) -> Option<bool> {
        self.varint(n).map(|v| v != 0)
    }

    pub fn f64v(&self, n: u32) -> Option<f64> {
        match self.get(n)? {
            Value::Fixed64(b) => Some(f64::from_le_bytes(*b)),
            _ => None,
        }
    }

    pub fn f32v(&self, n: u32) -> Option<f32> {
        match self.get(n)? {
            Value::Fixed32(b) => Some(f32::from_le_bytes(*b)),
            _ => None,
        }
    }

    pub fn bytes(&self, n: u32) -> Option<&[u8]> {
        match self.get(n)? {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }

    pub fn string(&self, n: u32) -> Option<String> {
        self.bytes(n).map(|b| String::from_utf8_lossy(b).into_owned())
    }

    /// A nested message (LEN bytes re-walked as fields, or an inline group).
    pub fn msg(&self, n: u32) -> Option<Msg> {
        match self.get(n)? {
            Value::Bytes(b) => Msg::parse(b),
            Value::Group(fields) => Some(Msg { fields: fields.clone() }),
            _ => None,
        }
    }

    /// All occurrences of field `n` as nested messages.
    pub fn msgs(&self, n: u32) -> Vec<Msg> {
        self.all(n)
            .into_iter()
            .filter_map(|v| match v {
                Value::Bytes(b) => Msg::parse(b),
                Value::Group(fields) => Some(Msg { fields: fields.clone() }),
                _ => None,
            })
            .collect()
    }

    /// `TSP.Reference` / `TSP.DataReference` identifier (field 1 inside the
    /// LEN bytes — TSPMessages.proto:26-34).
    pub fn reference(&self, n: u32) -> Option<u64> {
        match self.get(n)? {
            Value::Bytes(b) => {
                let m = Msg::parse(b)?;
                m.varint(1)
            }
            Value::Varint(v) => Some(*v), // some writers inline the identifier
            _ => None,
        }
    }

    /// All occurrences of field `n` as reference ids.
    pub fn references(&self, n: u32) -> Vec<u64> {
        self.all(n)
            .into_iter()
            .filter_map(|v| match v {
                Value::Bytes(b) => Msg::parse(b).and_then(|m| m.varint(1)),
                Value::Varint(v) => Some(*v),
                _ => None,
            })
            .collect()
    }

    /// Packed repeated varints (or single varint).
    pub fn packed_varints(&self, n: u32) -> Vec<u64> {
        self.all(n)
            .into_iter()
            .filter_map(|v| match v {
                Value::Bytes(b) => proto::packed_u64s(b, iwadump::error::Layer::Message).ok(),
                Value::Varint(v) => Some(vec![*v]),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Packed repeated floats (fixed32) or single float.
    pub fn packed_f32s(&self, n: u32) -> Vec<f32> {
        self.all(n)
            .into_iter()
            .filter_map(|v| match v {
                Value::Bytes(b) => Some(
                    b.chunks_exact(4)
                        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                        .collect::<Vec<f32>>(),
                ),
                Value::Fixed32(b) => Some(vec![f32::from_le_bytes(*b)]),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// Packed repeated doubles (fixed64) or single double.
    pub fn packed_f64s(&self, n: u32) -> Vec<f64> {
        self.all(n)
            .into_iter()
            .filter_map(|v| match v {
                Value::Bytes(b) => Some(
                    b.chunks_exact(8)
                        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
                        .collect::<Vec<f64>>(),
                ),
                Value::Fixed64(b) => Some(vec![f64::from_le_bytes(*b)]),
                _ => None,
            })
            .flatten()
            .collect()
    }

    /// `TSP.Point { x = 1, y = 2 }` (floats, TSPMessages.proto:45-48).
    pub fn point(&self, n: u32) -> Option<(f64, f64)> {
        let m = self.msg(n)?;
        Some((m.f32v(1)? as f64, m.f32v(2)? as f64))
    }

    /// `TSP.Size { width = 1, height = 2 }` (TSPMessages.proto:61-64).
    pub fn size(&self, n: u32) -> Option<(f64, f64)> {
        let m = self.msg(n)?;
        Some((m.f32v(1)? as f64, m.f32v(2)? as f64))
    }
}

/// Message type ids used across the converter (docs/format registry tables).
/// Only ids with distinct meanings per docs are named here; anything else is
/// dispatched through the registry name.
pub mod ids {
    // TSP / shared
    pub const PACKAGE_METADATA: u32 = 11006;
    pub const STORAGE: u32 = 2001; // TSWP.StorageArchive (2005 = same message)
    pub const STORAGE_ALT: u32 = 2005;
    pub const DRAWABLE_ATTACHMENT: u32 = 2003;

    // TSD
    pub const SHAPE: u32 = 3004;
    pub const IMAGE: u32 = 3005;
    pub const MASK: u32 = 3006;
    pub const MOVIE: u32 = 3007;
    pub const GROUP: u32 = 3008;
    pub const CONNECTION_LINE: u32 = 3009;

    // TSCH
    pub const CHART_DRAWABLE: u32 = 5021;
    pub const CHART_MEDIATOR: u32 = 5004;
    pub const PRE_UFF_CHART: u32 = 5000;

    // TP (Pages)
    pub const TP_DOCUMENT: u32 = 10000;
    pub const TP_PLACEHOLDER: u32 = 7;
    pub const TP_SECTION: u32 = 10011;
    pub const TP_FLOATING_DRAWABLES: u32 = 10010;
    pub const TP_DRAWABLES_ZORDER: u32 = 10015;

    // KN (Keynote)
    pub const KN_DOCUMENT: u32 = 1;
    pub const KN_SHOW: u32 = 2;
    pub const KN_SLIDE: u32 = 5;
    pub const KN_SLIDE_ALT: u32 = 6;
    pub const KN_THEME: u32 = 10;
    pub const KN_NOTE: u32 = 15;
    pub const KN_BUILD: u32 = 8;

    // TN (Numbers)
    pub const TN_DOCUMENT: u32 = 1;
    pub const TN_SHEET: u32 = 2;
    pub const TN_FORM_BASED_SHEET: u32 = 3;
}
