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

    /// Bytes of a media asset stored at `Data/<file_name>`. Falls back to a
    /// suffix match for zipped-package layouts where members carry the bundle
    /// directory prefix (e.g. `MyDoc.key/Data/pic.jpeg`).
    pub fn data_file(&self, file_name: &str) -> Option<Vec<u8>> {
        let exact = format!("Data/{file_name}");
        if let Some(b) = self.get(&exact) {
            return Some(b);
        }
        let suffix = format!("/Data/{file_name}");
        match &self.inner {
            MembersInner::Container(_) => self.get(&exact),
            MembersInner::Map(map) => map
                .iter()
                .find(|(k, _)| k.ends_with(&suffix))
                .map(|(_, v)| v.clone()),
        }
    }
}
