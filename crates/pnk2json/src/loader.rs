//! Object-graph loading: every `.iwa` stream of the container becomes
//! `Records[id] = (type_id, decoded fields)` (docs/format/objects.md §Global
//! id space). Unknown type ids and structurally undecodable payloads are
//! counted (aggregated into envelope warnings later) and skipped by declared
//! length — never desynchronizing their neighbours (docs/format/gotchas.md #6).

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;

use iwadump::proto::{Field, Value};
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
    /// Incremental-save patches applied onto their base messages.
    pub patches_applied: u64,
    /// Patches that could NOT be applied (multi-segment path, bad base…) —
    /// the affected object may show pre-edit content; surfaced as a warning.
    pub patches_dropped: u64,
    /// Backward-compat diffs (MessageInfo.diff_merge_version present) left
    /// unapplied on purpose — see apply_patches.
    pub patches_superseded: u64,
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
    // When iwadump's stream-name heuristic fails (app = Unknown), recover the
    // app from object-name evidence BEFORE naming records: scan type ids and
    // accept unambiguous TN./TP./KN.-prefixed names (name_for with
    // App::Unknown only returns unambiguous ids). Otherwise per-record names
    // would be baked as None and the converter would warn unknown for ids
    // that the correct app table resolves.
    let app = if app == App::Unknown {
        streams
            .iter()
            .flat_map(|s| s.archives.iter())
            .flat_map(|a| a.messages.iter())
            .find_map(|m| {
                let name = registry.name_for(App::Unknown, m.type_id)?;
                match name.as_str() {
                    n if n.starts_with("TN.") => Some(App::Numbers),
                    n if n.starts_with("TP.") => Some(App::Pages),
                    n if n.starts_with("KN.") => Some(App::Keynote),
                    _ => None,
                }
            })
            .unwrap_or(App::Unknown)
    } else {
        app
    };

    let mut records = HashMap::new();
    let mut unknown_ids: BTreeMap<u32, u64> = BTreeMap::new();
    let mut undecodable_ids: BTreeMap<u32, u64> = BTreeMap::new();
    let mut undecodable_bytes = HashMap::new();
    let mut patches_applied = 0u64;
    let mut patches_dropped = 0u64;
    let mut patches_superseded = 0u64;

    for stream in streams {
        for archive in &stream.archives {
            // Incremental-save patches (type 0 + should_merge) splice one
            // field into their base message (docs/format/incremental.md).
            // Merge FIRST so the record below carries post-edit content —
            // dropping patches silently returned plausible pre-edit state
            // (FINDINGS.md H-6).
            let mut patched = if archive.should_merge {
                apply_patches(
                    archive,
                    &mut patches_applied,
                    &mut patches_dropped,
                    &mut patches_superseded,
                )
            } else {
                HashMap::new()
            };
            for (msg_index, message) in archive.messages.iter().enumerate() {
                // The patch messages themselves are consumed by the merge
                // above (or counted dropped); never records of their own.
                if message.type_id == 0 && archive.should_merge {
                    continue;
                }
                // Object types verified non-content in the corpus (Numbers/
                // Pages annotation caches and guide indexes): decoded but
                // never warned about (model-design.md §6 spirit — they carry
                // no DataReferences that feed the media inventory).
                if crate::pb::ids::IGNORED.contains(&message.type_id) {
                    continue;
                }
                let name = registry.name_for(app, message.type_id);
                // Command/undo archives are dropped per docs/model-design.md §6
                // — decoded but never warned about.
                if name.as_deref().is_some_and(is_command_name) {
                    continue;
                }
                let parsed = match patched.remove(&msg_index) {
                    Some(m) => Some(m),
                    None => Msg::parse(&message.payload),
                };
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

    Loaded {
        records,
        unknown_ids,
        undecodable_ids,
        undecodable_bytes,
        patches_applied,
        patches_dropped,
        patches_superseded,
    }
}

/// Wire-level incremental-save merge for one archive segment
/// (docs/format/incremental.md §Patch messages). Two shapes occur:
///
/// - with `diff_field_path` (one segment): the payload is the serialized
///   VALUE of that one field [parser: psobot/keynote-parser@56a4d3b
///   codec.py:260-285 decodes it as the patched field's message class] —
///   applying it replaces the field in the base message;
/// - pathless [inferred: corpus probe — payload field numbers mirror the
///   base type and pair with `fields_to_remove` of the same numbers, e.g.
///   TN.UIStateArchive f28]: the payload is a PARTIAL message of the base's
///   own type — set fields replace their base occurrences (proto2 singular
///   merge), possibly empty for a pure removal.
///
/// In both shapes `fields_to_remove` clears base fields FIRST, so a patch
/// that clears-and-resets a field keeps the new value. Multi-segment paths
/// are unimplemented in every reference parser — counted dropped, never
/// guessed at.
///
/// Returns base-message-index → merged Msg for the messages that received
/// patches; the caller substitutes these for the freshly-parsed payloads.
fn apply_patches(
    archive: &iwadump::envelope::ArchiveInfo,
    applied: &mut u64,
    dropped: &mut u64,
    superseded: &mut u64,
) -> HashMap<usize, Msg> {
    let mut merged: HashMap<usize, Msg> = HashMap::new();
    for message in &archive.messages {
        if message.type_id != 0 {
            continue;
        }
        // A diff that names a `diff_merge_version` (MessageInfo 8) is a
        // backward-compatibility DOWNGRADE for an older reader generation,
        // not a user edit: the base message already holds the modern
        // representation. Corpus survey (agent K, 2026-09-02, 960 docs):
        // every type-0 diff carries field 8 with a 0xFFFFFFFF sentinel;
        // RIPE 85's ring charts are stored as type 25 with a diff to pie
        // type 5, and Keynote 15.3.1 draws rings; a KN.BuildArchive diff
        // rewrites "apple:bc-appear" to the pre-build-chooser spelling
        // "apple:appear". A maximal-version reader (we decode the newest
        // protos) must not merge these. Diffs WITHOUT field 8 keep the
        // merge path below. This supersedes FINDINGS.md H-6's premise,
        // which was a code review, not a ground-truth comparison. [inferred]
        if message.patch.iter().any(|(n, _)| *n == 8) {
            *superseded += 1;
            continue;
        }
        // base_message_index = MessageInfo field 7 (proto default 0).
        let base_index = message
            .patch
            .iter()
            .find(|(n, _)| *n == 7)
            .and_then(|(_, v)| match v {
                Value::Varint(x) => Some(*x as usize),
                _ => None,
            })
            .unwrap_or(0);
        let base_ok = archive
            .messages
            .get(base_index)
            .is_some_and(|b| b.type_id != 0);
        let paths = field_paths(&message.patch, 9); // diff_field_path
        let removes = field_paths(&message.patch, 10); // fields_to_remove
        let replace_one = paths.len() == 1 && paths[0].len() == 1;
        let partial_merge = paths.is_empty();
        if !base_ok || !(replace_one || partial_merge) || removes.iter().any(|p| p.len() != 1) {
            *dropped += 1;
            continue;
        }
        // Pathless payloads must parse as the base's own type before any
        // mutation — an unparseable one drops the whole patch, not half.
        let partial = if partial_merge && !message.payload.is_empty() {
            match Msg::parse(&message.payload) {
                Some(m) => Some(m),
                None => {
                    *dropped += 1;
                    continue;
                }
            }
        } else {
            None
        };
        let base = match merged.entry(base_index) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                match Msg::parse(&archive.messages[base_index].payload) {
                    Some(m) => e.insert(m),
                    None => {
                        *dropped += 1;
                        continue;
                    }
                }
            }
        };
        for r in &removes {
            let n = r[0] as u32;
            base.fields.retain(|f| f.number != n);
        }
        // Replace-in-place keeps field order stable (semantically irrelevant
        // in proto2, but deterministic output matters).
        let set_fields = |base: &mut Msg, replacements: Vec<Field>| {
            let mut nums: Vec<u32> = Vec::new();
            for f in &replacements {
                if !nums.contains(&f.number) {
                    nums.push(f.number);
                }
            }
            for n in nums {
                let group: Vec<Field> = replacements
                    .iter()
                    .filter(|f| f.number == n)
                    .cloned()
                    .collect();
                match base.fields.iter().position(|f| f.number == n) {
                    Some(i) => {
                        base.fields.retain(|f| f.number != n);
                        let at = i.min(base.fields.len());
                        for (k, f) in group.into_iter().enumerate() {
                            base.fields.insert(at + k, f);
                        }
                    }
                    None => base.fields.extend(group),
                }
            }
        };
        if replace_one {
            let n = paths[0][0] as u32;
            // A patch aimed at an EMBEDDED MESSAGE merges into it: the RIPE 85
            // deck patches its pie charts' TSCH unity extension (10000) with
            // a 2-byte `{ chart_type: 5 }`, and Keynote keeps the grid,
            // styles and axes already stored there — replacing the whole
            // field left a type and no data. Sub-fields named by the payload
            // replace their counterparts; everything else stays. Scalars
            // and fields the base lacks are set outright as before.
            let base_sub = match base.fields.iter().find(|f| f.number == n).map(|f| &f.value) {
                Some(Value::Bytes(b)) => Msg::parse(b),
                Some(Value::Group(fields)) => Some(Msg {
                    fields: fields.clone(),
                }),
                _ => None,
            };
            let payload_msg = if message.payload.is_empty() {
                None
            } else {
                Msg::parse(&message.payload)
            };
            match (base_sub, payload_msg) {
                (Some(mut sub), Some(patch)) => {
                    set_fields(&mut sub, patch.fields);
                    set_fields(
                        base,
                        vec![Field {
                            number: n,
                            wire: iwadump::proto::WIRE_LEN,
                            value: Value::Group(sub.fields),
                        }],
                    );
                }
                _ => set_fields(
                    base,
                    vec![Field {
                        number: n,
                        wire: iwadump::proto::WIRE_LEN,
                        value: Value::Bytes(message.payload.clone()),
                    }],
                ),
            }
        } else if let Some(partial) = partial {
            set_fields(base, partial.fields);
        }
        *applied += 1;
    }
    merged
}

/// Decode every occurrence of MessageInfo field `n` as a `TSP.FieldPath`
/// (`repeated uint32 path = 1`, packed).
fn field_paths(patch: &[(u32, Value)], n: u32) -> Vec<Vec<u64>> {
    patch
        .iter()
        .filter(|(num, _)| *num == n)
        .filter_map(|(_, v)| match v {
            Value::Bytes(b) => Msg::parse(b).map(|m| m.packed_varints(1)),
            _ => None,
        })
        .collect()
}

/// Command/undo/history classes (docs/format/incremental.md §safe-ignore).
fn is_command_name(name: &str) -> bool {
    name.contains("Command") || name.ends_with("History") || name.contains("Selection")
}

// ---------------------------------------------------------------------------
// Bytes-based loading (used by the wasm binding, where there is no filesystem)
// ---------------------------------------------------------------------------

/// Ceiling on zip entry count — real documents carry at most a few thousand
/// members; a many-entry archive is an algorithmic-complexity vector.
const MAX_ZIP_ENTRIES: usize = 100_000;

/// Total inflated `.iwa` budget: generous multiple of the compressed input
/// (DEFLATE on already-snappy data stays well under 20x) with a floor so
/// tiny documents aren't over-constrained. Bounds classic zip bombs.
fn iwa_budget(compressed_len: usize) -> u64 {
    (compressed_len as u64)
        .saturating_mul(20)
        .max(256 * 1024 * 1024)
}

/// Container-level decode from raw document bytes: reject encrypted/legacy,
/// collect `.iwa` members (descending into a nested `Index.zip`), frame and
/// envelope-parse each stream. Mirrors iwadump's container semantics.
/// Single pass over each zip's entries with an expansion budget — the old
/// shape reopened and reparsed the central directory once per member and
/// inflated without bound (FINDINGS.md H-2).
pub fn streams_from_bytes(bytes: &[u8]) -> Result<Vec<StreamView>, iwadump::Error> {
    use iwadump::error::{Error, Kind, Layer};

    let layer = Layer::Container;
    let label = "document";
    let cursor = std::io::Cursor::new(bytes);
    let mut zip = zip::ZipArchive::new(cursor).map_err(|e| {
        Error::new(
            Kind::Unsupported,
            layer,
            format!("{label}: not a readable ZIP container: {e}"),
        )
    })?;
    if zip.len() > MAX_ZIP_ENTRIES {
        return Err(Error::new(
            Kind::Unsupported,
            layer,
            format!(
                "{label}: {} zip entries exceeds the {MAX_ZIP_ENTRIES}-entry limit",
                zip.len()
            ),
        ));
    }

    let names: Vec<String> = zip.file_names().map(|n| n.to_string()).collect();

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
    const LEGACY_MARKERS: [&str; 5] = [
        "index.xml",
        "index.xml.gz",
        "index.apxl",
        "index.numbers",
        "index.db",
    ];
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

    let mut remaining = iwa_budget(bytes.len());
    let over_budget = || {
        Error::new(
            Kind::Corrupt,
            layer,
            format!("{label}: inflated .iwa members exceed the expansion budget; refusing further decompression"),
        )
    };

    // One pass by index: inflate each `.iwa` (bounded), skipping directories
    // and LZFSE operation logs (gotcha #11: `bvx` magic, not snappy IWA).
    let collect = |zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
                   remaining: &mut u64|
     -> Result<Vec<(String, Vec<u8>)>, Error> {
        let mut out = Vec::new();
        for i in 0..zip.len() {
            let Ok(mut f) = zip.by_index(i) else { continue };
            if f.is_dir() || !f.name().to_ascii_lowercase().ends_with(".iwa") {
                continue;
            }
            let name = f.name().to_string();
            let mut buf = Vec::new();
            let mut limited = std::io::Read::take(&mut f, remaining.saturating_add(1));
            if std::io::Read::read_to_end(&mut limited, &mut buf).is_err() {
                continue;
            }
            if buf.len() as u64 > *remaining {
                return Err(over_budget());
            }
            *remaining -= buf.len() as u64;
            if buf.starts_with(b"bvx") {
                continue;
            }
            out.push((name, buf));
        }
        Ok(out)
    };

    let mut iwas = collect(&mut zip, &mut remaining)?;
    if iwas.is_empty() {
        // Nested Index.zip variant (gotcha #5) — exactly one level deep.
        let nested_name = names
            .iter()
            .find(|n| n.to_lowercase().ends_with("index.zip") && !n.ends_with('/'))
            .cloned();
        if let Some(nname) = nested_name {
            let mut nested_bytes = Vec::new();
            if let Ok(mut f) = zip.by_name(&nname) {
                let mut limited = std::io::Read::take(&mut f, remaining.saturating_add(1));
                let _ = std::io::Read::read_to_end(&mut limited, &mut nested_bytes);
            }
            if nested_bytes.len() as u64 > remaining {
                return Err(over_budget());
            }
            remaining -= nested_bytes.len() as u64;
            if let Ok(mut inner) =
                zip::ZipArchive::new(std::io::Cursor::new(nested_bytes.as_slice()))
            {
                if inner.len() > MAX_ZIP_ENTRIES {
                    return Err(Error::new(
                        Kind::Unsupported,
                        layer,
                        format!("{label}: nested Index.zip entry count exceeds the {MAX_ZIP_ENTRIES}-entry limit"),
                    ));
                }
                // Encrypted/legacy markers apply INSIDE the nested index
                // too — skipping the re-check accepted encrypted nested
                // documents as empty ones (FINDINGS.md M-4).
                for n in inner.file_names() {
                    let base = n.rsplit('/').next().unwrap_or(n);
                    if base.starts_with(".iwph") || base.starts_with(".iwpv") {
                        return Err(Error::new(
                            Kind::Encrypted,
                            layer,
                            format!("{label}: encrypted iWork document (nested member `{base}`) — password-protected files are not supported"),
                        ));
                    }
                    if LEGACY_MARKERS.contains(&base.to_lowercase().as_str()) {
                        return Err(Error::new(
                            Kind::Legacy,
                            layer,
                            format!("{label}: legacy iWork document (nested marker `{base}`) — re-save with a current version of Pages, Numbers, or Keynote"),
                        ));
                    }
                }
                iwas = collect(&mut inner, &mut remaining)?;
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

    let mut streams = Vec::with_capacity(iwas.len());
    for (name, raw) in &iwas {
        let iwa = iwadump::IwaStream::parse(name, raw)?;
        let archives = iwadump::envelope::parse_stream(&iwa.decoded)?;
        streams.push(StreamView {
            name: name.clone(),
            iwa,
            archives,
        });
    }
    Ok(streams)
}
