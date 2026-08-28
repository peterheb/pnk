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

// ---------------------------------------------------------------------------
// Bytes-based loading (used by the wasm binding, where there is no filesystem)
// ---------------------------------------------------------------------------

/// Container-level decode from raw document bytes: reject encrypted/legacy,
/// collect `.iwa` members (recursing into a nested `Index.zip`), frame and
/// envelope-parse each stream. Mirrors iwadump's container semantics.
pub fn streams_from_bytes(
    bytes: &[u8],
) -> Result<Vec<StreamView>, iwadump::Error> {
    use iwadump::error::{Error, Kind, Layer};

    let layer = Layer::Container;
    let label = "document";
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).map_err(|e| {
        Error::new(Kind::Unsupported, layer, format!("{label}: not a readable ZIP container: {e}"))
    })?;

    let mut names: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        if let Ok(f) = zip.by_index_raw(i) {
            names.push(f.name().to_string());
        }
    }

    // Encrypted: .iwph / .iwp* members (gotchas #12).
    for n in &names {
        let base = n.rsplit('/').next().unwrap_or(n);
        if base.starts_with(".iwph") || base.starts_with(".iwpv") {
            return Err(Error::new(
                Kind::Encrypted,
                layer,
                format!("{label}: encrypted iWork document (member `{base}`) — password-protected files are not supported"),
            ));
        }
    }
    // Legacy markers (docs/format/legacy.md).
    const LEGACY_MARKERS: [&str; 5] =
        ["index.xml", "index.xml.gz", "index.apxl", "index.numbers", "index.db"];
    for n in &names {
        let base = n.rsplit('/').next().unwrap_or(n).to_lowercase();
        if LEGACY_MARKERS.contains(&base.as_str()) {
            return Err(Error::new(
                Kind::Legacy,
                layer,
                format!("{label}: legacy iWork document (bundle marker `{base}`) — re-save with a current version of Pages, Numbers, or Keynote"),
            ));
        }
    }

    // Collect .iwa members; if none at top level, recurse into a nested
    // Index.zip (gotcha #5).
    let mut read_member = |name: &str| -> Option<Vec<u8>> {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
        let mut f = z.by_name(name).ok()?;
        let mut buf = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut buf).ok()?;
        Some(buf)
    };

    let mut collect = |names: &[String], read: &mut dyn FnMut(&str) -> Option<Vec<u8>>| -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        for n in names {
            if !n.ends_with('/') && n.to_ascii_lowercase().ends_with(".iwa") {
                if let Some(b) = read(n) {
                    // OperationStorage.iwa is an LZFSE collaboration log, not a
                    // snappy IWA stream (gotcha #11): skip by magic.
                    if b.starts_with(b"bvx") {
                        continue;
                    }
                    out.push((n.clone(), b));
                }
            }
        }
        out
    };

    let mut iwas = collect(&names, &mut read_member);
    if iwas.is_empty() {
        // Nested Index.zip variant.
        let nested_name = names.iter().find(|n| n.ends_with("Index.zip") && !n.ends_with('/')).cloned();
        if let Some(nname) = nested_name {
            if let Some(nested_bytes) = read_member(&nname) {
                if let Ok(mut inner) = zip::ZipArchive::new(std::io::Cursor::new(&nested_bytes)) {
                    let mut inner_names = Vec::new();
                    for i in 0..inner.len() {
                        if let Ok(f) = inner.by_index_raw(i) {
                            inner_names.push(f.name().to_string());
                        }
                    }
                    let mut inner_read = |name: &str| -> Option<Vec<u8>> {
                        let mut z = zip::ZipArchive::new(std::io::Cursor::new(&nested_bytes)).ok()?;
                        let mut f = z.by_name(name).ok()?;
                        let mut buf = Vec::new();
                        std::io::Read::read_to_end(&mut f, &mut buf).ok()?;
                        Some(buf)
                    };
                    iwas = collect(&inner_names, &mut inner_read);
                }
            }
        }
    }
    if iwas.is_empty() {
        return Err(Error::new(
            Kind::Unsupported,
            layer,
            format!("{label}: no iWork '13+ data found (missing Index.zip / *.iwa members)"),
        ));
    }

    let registry = Registry::embedded()?;
    let mut streams = Vec::with_capacity(iwas.len());
    for (name, raw) in &iwas {
        let iwa = iwadump::IwaStream::parse(name, raw)?;
        let archives = iwadump::envelope::parse_stream(&iwa.decoded)?;
        streams.push(StreamView { name: name.clone(), iwa, archives });
    }
    Ok(streams)
}
