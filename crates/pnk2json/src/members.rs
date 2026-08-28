//! Access to non-IWA container members (`Metadata/*.plist`, `Data/*`).
//!
//! Two sources: a path-opened iwadump `Container` (delegates to
//! `Container::read_member`), or a prebuilt map for the bytes-based wasm path.

use std::collections::HashMap;

pub struct Members {
    inner: MembersInner,
}

enum MembersInner {
    Container(Box<iwadump::Container>),
    Map(HashMap<String, Vec<u8>>),
}

impl Members {
    pub fn from_container(container: iwadump::Container) -> Members {
        Members { inner: MembersInner::Container(Box::new(container)) }
    }

    pub fn from_map(map: HashMap<String, Vec<u8>>) -> Members {
        Members { inner: MembersInner::Map(map) }
    }

    /// Bytes of a named member; `None` when absent.
    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        match &self.inner {
            MembersInner::Container(c) => match c.read_member(name) {
                Ok(b) => Some(b),
                Err(_) => None,
            },
            MembersInner::Map(m) => m.get(name).cloned(),
        }
    }

    /// Bytes of a media asset stored at `Data/<file_name>`.
    pub fn data_file(&self, file_name: &str) -> Option<Vec<u8>> {
        self.get(&format!("Data/{file_name}"))
    }
}
