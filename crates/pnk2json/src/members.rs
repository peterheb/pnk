//! Access to non-IWA container members (`Metadata/*.plist`, `Data/*`).
//!
//! Three sources: a path-opened iwadump `Container` (delegates to
//! `Container::read_member`), a prebuilt map (tests), or a LAZY zip view for
//! the bytes-based wasm path — the compressed document is retained once and
//! members inflate on demand, so unreferenced media (movies, junk entries)
//! never occupies memory (FINDINGS.md H-2).

use std::collections::HashMap;

/// Ceiling for one inflated member read. Real `Data/` assets top out in the
/// hundreds of MB; a crafted DEFLATE bomb must not expand unbounded.
const MAX_MEMBER_BYTES: u64 = 1024 * 1024 * 1024;

pub struct Members {
    inner: MembersInner,
}

enum MembersInner {
    Container(Box<iwadump::Container>),
    Map(HashMap<String, Vec<u8>>),
    Zip(std::cell::RefCell<zip::ZipArchive<std::io::Cursor<Vec<u8>>>>),
}

impl Members {
    pub fn from_container(container: iwadump::Container) -> Members {
        Members {
            inner: MembersInner::Container(Box::new(container)),
        }
    }

    pub fn from_map(map: HashMap<String, Vec<u8>>) -> Members {
        Members {
            inner: MembersInner::Map(map),
        }
    }

    /// Lazy zip-backed view over the raw document bytes (wasm path). Falls
    /// back to an empty map when the bytes are not a readable zip (the
    /// caller has already parsed streams out of them, so this is unusual).
    pub fn from_zip_bytes(bytes: Vec<u8>) -> Members {
        match zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
            Ok(z) => Members {
                inner: MembersInner::Zip(std::cell::RefCell::new(z)),
            },
            Err(_) => Members {
                inner: MembersInner::Map(HashMap::new()),
            },
        }
    }

    /// Bytes of a named member; `None` when absent.
    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        match &self.inner {
            MembersInner::Container(c) => match c.read_member(name) {
                Ok(b) => Some(b),
                Err(_) => None,
            },
            MembersInner::Map(m) => m.get(name).cloned(),
            MembersInner::Zip(z) => {
                let mut z = z.borrow_mut();
                let f = z.by_name(name).ok()?;
                read_bounded(f)
            }
        }
    }

    /// Whether a media asset exists at `Data/<file_name>` — a name check
    /// only, never inflating the member (FINDINGS.md H-2: existence probes
    /// used to materialize and clone whole assets).
    pub fn has_data_file(&self, file_name: &str) -> bool {
        let exact = format!("Data/{file_name}");
        let suffix = format!("/Data/{file_name}");
        match &self.inner {
            MembersInner::Container(c) => c.has_member(&exact),
            MembersInner::Map(m) => {
                m.contains_key(&exact) || m.keys().any(|k| k.ends_with(&suffix))
            }
            MembersInner::Zip(z) => z
                .borrow()
                .file_names()
                .any(|n| n == exact || n.ends_with(&suffix)),
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
            MembersInner::Zip(z) => {
                let name = z
                    .borrow()
                    .file_names()
                    .find(|n| n.ends_with(&suffix))
                    .map(|n| n.to_string())?;
                self.get(&name)
            }
        }
    }
}

/// Inflate one zip member with the bomb ceiling applied.
fn read_bounded(f: zip::read::ZipFile<'_>) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut buf = Vec::new();
    let mut limited = f.take(MAX_MEMBER_BYTES + 1);
    limited.read_to_end(&mut buf).ok()?;
    if buf.len() as u64 > MAX_MEMBER_BYTES {
        return None;
    }
    Some(buf)
}
