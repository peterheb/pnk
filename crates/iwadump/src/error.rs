//! Layer-tagged errors. Every malformed input surfaces as an `Error` naming
//! the decode layer that rejected it (container / iwa / snappy / envelope /
//! message) — never a panic (docs/format/gotchas.md #7: "block failed to
//! decompress", not "corrupt file", when Snappy fails).

use std::fmt;

/// Decode layers, bottom to top. Used for error routing and exit messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Container,
    Iwa,
    Snappy,
    Envelope,
    Message,
}

impl fmt::Display for Layer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Layer::Container => "container",
            Layer::Iwa => "iwa",
            Layer::Snappy => "snappy",
            Layer::Envelope => "envelope",
            Layer::Message => "message",
        })
    }
}

/// Rejection class, so callers can distinguish "wrong kind of file" from
/// "file is damaged" (docs/format/legacy.md: unsupported-version is its own
/// class, distinct from corrupt and unknown-format).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Io,
    /// Pre-'13 legacy document (index.xml / apxl / tef / index.db signals).
    Legacy,
    /// Password-protected (`.iwph`).
    Encrypted,
    /// Recognized container family but no decodable iWork '13+ content.
    Unsupported,
    /// Damaged beyond the layer's recovery.
    Corrupt,
}

#[derive(Debug, Clone)]
pub struct Error {
    pub layer: Layer,
    pub kind: Kind,
    pub message: String,
}

impl Error {
    pub fn new(kind: Kind, layer: Layer, message: impl Into<String>) -> Error {
        Error {
            layer,
            kind,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}]", self.message, self.layer)
    }
}

impl std::error::Error for Error {}
