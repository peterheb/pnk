//! iwadump — decode layer for iWork '13+ files.
//!
//! Layer stack, bottom to top (each module names its layer in errors):
//! - `container`: ZIP container / package directory, nested `Index.zip`
//! - `iwa`: `.iwa` stream framing (4-byte `00` + u24 LE header)
//! - `snappy`: raw Snappy block decode
//! - `envelope`: `[varint][TSP.ArchiveInfo]` + length-delimited payloads
//! - `message`: per-payload best-effort protobuf field walk
