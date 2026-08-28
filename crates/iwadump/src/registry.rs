//! Object type-id → message-name registry (placeholder; full tables next).

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Registry {
    map: std::collections::HashMap<u32, String>,
}

impl Registry {
    pub fn empty() -> Registry {
        Registry { map: std::collections::HashMap::new() }
    }
    pub fn lookup(&self, id: u32) -> Option<&str> {
        self.map.get(&id).map(|s| s.as_str())
    }
    #[allow(dead_code)]
    pub fn insert(&mut self, id: u32, name: String) {
        self.map.insert(id, name);
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
    /// Names sorted by id (for `--json` stability).
    pub fn entries(&self) -> Vec<(u32, &str)> {
        let mut v: Vec<(u32, &str)> = self.map.iter().map(|(k, v)| (*k, v.as_str())).collect();
        v.sort_by_key(|(k, _)| *k);
        v
    }
    /// Union of all tables' ids (for ambiguity analysis).
    #[allow(dead_code)]
    pub fn ids(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.map.keys().copied().collect();
        v.sort_unstable();
        v
    }
    pub fn tables(&self) -> Vec<&str> {
        Vec::new()
    }
    pub fn names_for(&self, _id: u32) -> Vec<&str> {
        Vec::new()
    }
    pub fn from_json(_json: &str) -> Result<Registry, crate::error::Error> {
        Ok(Registry::empty())
    }
}

pub type Table = std::collections::HashMap<String, String>;
pub fn unused(_m: &HashMap<u32, String>) {}
