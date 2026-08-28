//! Access to non-IWA container members (`Metadata/*.plist`, `Data/*`).
//!
//! Delegates to `iwadump::Container::read_member` (added for pnk2json):
//! exact-name lookup with directory-prefix-insensitive suffix fallback;
//! flat/nested forms read the outer zip and fall back into the nested
//! `Index.zip`; package directories read real files beside `Index.zip`.

pub struct Members {
    container: iwadump::Container,
}

impl Members {
    pub fn new(container: iwadump::Container) -> Members {
        Members { container }
    }

    /// Bytes of a named member; `None` when absent (Kind::Io).
    pub fn get(&self, name: &str) -> Option<Vec<u8>> {
        match self.container.read_member(name) {
            Ok(b) => Some(b),
            Err(e) if e.kind == iwadump::Kind::Io => None,
            Err(_) => None,
        }
    }

    /// Bytes of a media asset stored at `Data/<file_name>`.
    pub fn data_file(&self, file_name: &str) -> Option<Vec<u8>> {
        self.get(&format!("Data/{file_name}"))
    }
}
