//! Object-graph loading: every `.iwa` stream of the container becomes
//! `Records[id] = (type_id, decoded fields)` (docs/format/objects.md §Global
//! id space). Unknown type ids and structurally undecodable payloads are
//! counted (aggregated into envelope warnings later) and skipped by declared
//! length — never desynchronizing their neighbours (docs/format/gotchas.md #6).

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;

use iwadump::registry::{App, Registry};
use iwadump::{Document, StreamView};

use crate::pb::Msg;

#[derive(Debug, Clone)]
pub struct Record {
    pub id: u64,
    pub type_id: u32,
    /// Trusted registry name when one exists (never guessed —
    /// docs/format/registry.md).
    pub name: Option<String>,
    /// Parsed payload; `None` when the walk failed structurally.
    pub msg: Option<Msg>,
}

pub struct Loaded {
    pub records: HashMap<u64, Record>,
    /// unknown-object-type aggregation: type id → object count.
    pub unknown_ids: BTreeMap<u32, u64>,
    /// undecodable-object aggregation: type id → object count.
    pub undecodable_ids: BTreeMap<u32, u64>,
    /// Undecodable payloads kept verbatim, for diagnostics.
    pub undecodable_bytes: HashMap<u64, u32>,
}

impl Loaded {
    pub fn record(&self, id: u64) -> Option<&Record> {
        self.records.get(&id)
    }

    pub fn msg(&self, id: u64) -> Option<&Msg> {
        self.records.get(&id)?.msg.as_ref()
    }
}

/// Open + fully decode a document. Rejections (legacy / encrypted / corrupt)
/// surface as iwadump layer errors; this layer adds none of its own.
pub fn open_document(path: &Path) -> Result<(Document, Loaded), iwadump::Error> {
    let doc = Document::open(path, false)?;
    let loaded = load(&doc.streams, &doc.registry, doc.app);
    Ok((doc, loaded))
}

pub fn load(streams: &[StreamView], registry: &Registry, app: App) -> Loaded {
    let mut records = HashMap::new();
    let mut unknown_ids: BTreeMap<u32, u64> = BTreeMap::new();
    let mut undecodable_ids: BTreeMap<u32, u64> = BTreeMap::new();
    let mut undecodable_bytes = HashMap::new();

    for stream in streams {
        for archive in &stream.archives {
            for message in &archive.messages {
                // Incremental-save patch messages (type 0 + should_merge,
                // docs/format/incremental.md) are editing machinery: skip by
                // declared length, silently (model-design.md §6 dropped table).
                if message.type_id == 0 && archive.should_merge {
                    continue;
                }
                let name = registry.name_for(app, message.type_id);
                // Command/undo archives are dropped per docs/model-design.md §6
                // — decoded but never warned about.
                if name.as_deref().is_some_and(is_command_name) {
                    continue;
                }
                let parsed = Msg::parse(&message.payload);
                if name.is_none() {
                    *unknown_ids.entry(message.type_id).or_insert(0) += 1;
                }
                if parsed.is_none() {
                    *undecodable_ids.entry(message.type_id).or_insert(0) += 1;
                    undecodable_bytes.insert(archive.identifier, message.length);
                }
                // One archive segment = one object (docs/format/objects.md);
                // well-formed files carry one payload per segment. When a
                // segment holds several, keep the first decodable known-type
                // payload.
                let record = records.entry(archive.identifier).or_insert_with(|| Record {
                    id: archive.identifier,
                    type_id: message.type_id,
                    name: name.clone(),
                    msg: parsed.clone(),
                });
                let better = match (&record.msg, &parsed) {
                    (None, Some(_)) => true,
                    (Some(_), Some(_)) => record.name.is_none() && name.is_some(),
                    _ => false,
                };
                if better {
                    record.type_id = message.type_id;
                    record.name = name;
                    record.msg = parsed;
                }
            }
        }
    }

    Loaded { records, unknown_ids, undecodable_ids, undecodable_bytes }
}

/// Command/undo/history classes (docs/format/incremental.md §safe-ignore).
fn is_command_name(name: &str) -> bool {
    name.contains("Command") || name.ends_with("History") || name.contains("Selection")
}
