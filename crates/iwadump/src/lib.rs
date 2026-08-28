//! iwadump — decode layer for iWork '13+ files.
//!
//! Layer stack, bottom to top (each module names its layer in errors):
//! - [`container`]: ZIP container / package directory, nested `Index.zip`
//! - [`iwa`]: `.iwa` stream framing (4-byte `00` + u24 LE header)
//! - [`snappy`]: raw Snappy block decode
//! - [`envelope`]: `[varint][TSP.ArchiveInfo]` + length-delimited payloads
//! - [`proto`]: generic protobuf wire walker (group wire types included)
//! - [`registry`]: object type-id → message-name tables
//! - [`dump`]: summary tree / JSON rendering
//!
//! Everything above `container` is reusable as a library: `Container::open`
//! yields IWA streams, `IwaStream::parse` frames + decompresses them,
//! `envelope::parse_stream` splits archives and payloads, `proto` walks
//! payload fields structurally.

pub mod container;
pub mod dump;
pub mod envelope;
pub mod error;
pub mod iwa;
pub mod proto;
pub mod registry;
pub mod snappy;

pub use container::{Container, ContainerForm, Member};
pub use dump::{Document, MessageView, StreamView};
pub use envelope::{ArchiveInfo, MessageInfo, MessageStatus};
pub use error::{Error, Kind, Layer};
pub use iwa::{Block, IwaStream};
pub use registry::{App, Registry};
