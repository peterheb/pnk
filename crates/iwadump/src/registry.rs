//! Object type-id → message-name registry.
//!
//! `TSP.MessageInfo.type` ids resolve through out-of-band tables (the protos
//! carry no registry — docs/format/gotchas.md #4). We embed the
//! dunhamsteve/iwork codegen JSONs (see `data/README.md` for provenance).
//! The namespace is per-app: Common + one of Keynote/Numbers/Pages; the
//! app-specific ids overlap between apps, so an id is only named once the
//! app is known — otherwise ambiguity means **unknown**, never a guess.

use std::collections::HashMap;

use crate::error::{Error, Kind, Layer};

const COMMON_JSON: &str = include_str!("../data/Common.json");
const KEYNOTE_JSON: &str = include_str!("../data/Keynote.json");
const NUMBERS_JSON: &str = include_str!("../data/Numbers.json");
const PAGES_JSON: &str = include_str!("../data/Pages.json");

/// Which app's id namespace to apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    Keynote,
    Numbers,
    Pages,
    /// App not detected: only unambiguous ids (shared name everywhere, or
    /// present in exactly one table) get names.
    Unknown,
}

impl App {
    pub fn label(self) -> &'static str {
        match self {
            App::Keynote => "keynote",
            App::Numbers => "numbers",
            App::Pages => "pages",
            App::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Registry {
    common: HashMap<u32, String>,
    keynote: HashMap<u32, String>,
    numbers: HashMap<u32, String>,
    pages: HashMap<u32, String>,
}

impl Registry {
    /// Load the embedded tables (parse failure is a build-time bug; errors
    /// still surface as layer errors rather than panics).
    pub fn embedded() -> Result<Registry, Error> {
        Ok(Registry {
            common: parse_string_map(COMMON_JSON)?,
            keynote: parse_string_map(KEYNOTE_JSON)?,
            numbers: parse_string_map(NUMBERS_JSON)?,
            pages: parse_string_map(PAGES_JSON)?,
        })
    }

    pub fn table_size(&self, app: App) -> usize {
        match app {
            App::Keynote => self.keynote.len(),
            App::Numbers => self.numbers.len(),
            App::Pages => self.pages.len(),
            App::Unknown => 0,
        }
    }

    pub fn common_size(&self) -> usize {
        self.common.len()
    }

    /// Resolve a type id. Resolution order: app table, then Common. With no
    /// detected app, an id whose candidate names disagree across tables stays
    /// unknown (returns `None`) — display as opaque hex, never guess
    /// (docs/format/registry.md).
    pub fn name_for(&self, app: App, id: u32) -> Option<String> {
        let app_table = |a: App| match a {
            App::Keynote => Some(&self.keynote),
            App::Numbers => Some(&self.numbers),
            App::Pages => Some(&self.pages),
            App::Unknown => None,
        };
        if let Some(table) = app_table(app) {
            if let Some(name) = table.get(&id) {
                return Some(name.clone());
            }
        }
        if let Some(name) = self.common.get(&id) {
            return Some(name.clone());
        }
        // No app table (or id absent from it): is the id unambiguous across
        // all three app tables?
        let mut names: Vec<&String> = Vec::new();
        for table in [&self.keynote, &self.numbers, &self.pages] {
            if let Some(name) = table.get(&id) {
                names.push(name);
            }
        }
        match names.len() {
            0 => None,
            1 => Some(names[0].clone()),
            _ if names.iter().all(|n| *n == names[0]) => Some(names[0].clone()),
            _ => None, // KN.DocumentArchive vs TN.DocumentArchive etc.: unknown
        }
    }

    /// All distinct names any table offers for `id` (diagnostics/tests).
    pub fn names_for(&self, id: u32) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for table in [&self.common, &self.keynote, &self.numbers, &self.pages] {
            if let Some(name) = table.get(&id) {
                if !names.contains(&name.as_str()) {
                    names.push(name);
                }
            }
        }
        names
    }

    /// Total distinct ids across all tables (for the `--json` header).
    pub fn total_ids(&self) -> usize {
        let mut ids: Vec<u32> = self
            .common
            .keys()
            .chain(self.keynote.keys())
            .chain(self.numbers.keys())
            .chain(self.pages.keys())
            .copied()
            .collect();
        ids.sort_unstable();
        ids.dedup();
        ids.len()
    }
}

/// Parse a flat `{"123": "Name"}` JSON object. The tables are exactly this
/// shape (dunhamsteve re-formatted them to valid JSON), so a tiny parser is
/// enough — no serde dependency.
pub fn parse_string_map(json: &str) -> Result<HashMap<u32, String>, Error> {
    let err = |msg: String| {
        Error::new(
            Kind::Corrupt,
            Layer::Message,
            format!("registry table malformed: {msg}"),
        )
    };
    let bytes = json.as_bytes();
    let mut i = 0usize;
    let skip_ws = |i: &mut usize| {
        while *i < bytes.len() && bytes[*i].is_ascii_whitespace() {
            *i += 1;
        }
    };
    skip_ws(&mut i);
    if i >= bytes.len() || bytes[i] != b'{' {
        return Err(err("expected `{` at document start".into()));
    }
    i += 1;
    let mut map = HashMap::new();
    loop {
        skip_ws(&mut i);
        if i >= bytes.len() {
            return Err(err("unterminated object".into()));
        }
        if bytes[i] == b'}' {
            return Ok(map);
        }
        if bytes[i] == b',' {
            i += 1;
            continue;
        }
        let key = parse_json_string(bytes, &mut i).map_err(|e| err(e))?;
        skip_ws(&mut i);
        if i >= bytes.len() || bytes[i] != b':' {
            return Err(err(format!("expected `:` after key {key:?} at byte {i}")));
        }
        i += 1;
        skip_ws(&mut i);
        let value = parse_json_string(bytes, &mut i).map_err(err)?;
        let id: u32 = key.parse().map_err(|_| err(format!("non-numeric id key {key:?}")))?;
        map.insert(id, value);
    }
}

fn parse_json_string(bytes: &[u8], i: &mut usize) -> Result<String, String> {
    if *i >= bytes.len() || bytes[*i] != b'"' {
        return Err(format!("expected string at byte {i}"));
    }
    *i += 1;
    let mut out = String::new();
    while *i < bytes.len() {
        match bytes[*i] {
            b'"' => {
                *i += 1;
                return Ok(out);
            }
            b'\\' => {
                *i += 1;
                if *i >= bytes.len() {
                    break;
                }
                match bytes[*i] {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        if *i + 4 >= bytes.len() {
                            return Err("truncated \\u escape".into());
                        }
                        let hex = std::str::from_utf8(&bytes[*i + 1..*i + 5])
                            .map_err(|_| "bad \\u escape".to_string())?;
                        let cp = u32::from_str_radix(hex, 16)
                            .map_err(|_| "bad \\u escape".to_string())?;
                        out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                        *i += 4;
                    }
                    other => return Err(format!("bad escape \\{}", other as char)),
                }
                *i += 1;
            }
            _ => {
                // Consume one UTF-8 code point.
                let rest = std::str::from_utf8(&bytes[*i..])
                    .map_err(|_| "invalid UTF-8 in string".to_string())?;
                let ch = rest.chars().next().unwrap();
                out.push(ch);
                *i += ch.len_utf8();
            }
        }
    }
    Err("unterminated string".into())
}
