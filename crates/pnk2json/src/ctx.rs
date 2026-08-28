//! Converter context: warnings sink, font harvest, media/DataInfo registry,
//! metadata plists — everything the per-app tree walkers share
//! (docs/model-design.md §4/§5, docs/format/media.md).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use iwadump::registry::{App, Registry};

use crate::loader::{self, Loaded};
use crate::members::Members;
use crate::model::{
    AppKind, DocumentMeta, MediaAsset, MediaKind, MediaRef, Size, Warning, WarningCode,
};
use crate::pb::Msg;

pub struct Ctx {
    pub app: App,
    pub app_kind: AppKind,
    pub registry: Registry,
    pub loaded: Loaded,
    pub members: Members,
    pub warnings: Vec<Warning>,
    pub fonts: BTreeSet<String>,
    pub meta: DocumentMeta,
    /// TSP.PackageMetadata.datas, keyed by DataInfo.identifier.
    pub datas: HashMap<u64, DataInfoEntry>,
    /// DataInfo identifiers actually referenced from content (for `media[]`).
    pub referenced_datas: BTreeSet<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct DataInfoEntry {
    pub preferred_file_name: Option<String>,
    pub file_name: Option<String>,
    pub remote_url: Option<String>,
    pub materialized_length: Option<u64>,
    pub pixel_size: Option<Size>,
}

impl Ctx {
    pub fn open(path: &Path) -> Result<Ctx, iwadump::Error> {
        let (doc, loaded) = loader::open_document(path)?;
        let members = Members::from_container(doc.container.clone());
        let app = doc.app;
        let app_kind = match app {
            App::Keynote => AppKind::Keynote,
            App::Numbers => AppKind::Numbers,
            App::Pages => AppKind::Pages,
            App::Unknown => AppKind::Pages, // best effort; refined by root type below
        };
        let mut ctx = Ctx {
            app,
            app_kind,
            registry: doc.registry.clone(),
            loaded,
            members,
            warnings: Vec::new(),
            fonts: BTreeSet::new(),
            meta: DocumentMeta { app: app_kind, ..Default::default() },
            datas: HashMap::new(),
            referenced_datas: BTreeSet::new(),
        };
        ctx.refine_app();
        ctx.load_metadata_plists();
        ctx.load_package_metadata();
        Ok(ctx)
    }

    /// Cross-check the stream-name heuristic (iwadump::detect_app) against the
    /// actual root object's type id (10000 = TP.DocumentArchive → Pages).
    fn refine_app(&mut self) {
        if let Some(root) = self.loaded.msg(1) {
            // Root type id comes from the record, not the parsed fields.
            if let Some(rec) = self.loaded.record(1) {
                let app_kind = match rec.type_id {
                    10000 => AppKind::Pages,
                    _ => match self.registry.name_for(self.app, rec.type_id).as_deref() {
                        Some("KN.DocumentArchive") => AppKind::Keynote,
                        Some("TN.DocumentArchive") => AppKind::Numbers,
                        _ => self.app_kind,
                    },
                };
                self.app_kind = app_kind;
                self.meta.app = app_kind;
                self.app = match app_kind {
                    AppKind::Pages => App::Pages,
                    AppKind::Numbers => App::Numbers,
                    AppKind::Keynote => App::Keynote,
                };
                let _ = root;
            }
        }
    }

    // -- warnings -----------------------------------------------------------

    pub fn warn(&mut self, code: WarningCode, message: impl Into<String>) {
        self.warnings.push(Warning { code, message: message.into(), path: None, detail: None });
    }

    pub fn warn_detail(
        &mut self,
        code: WarningCode,
        message: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.warnings.push(Warning {
            code,
            message: message.into(),
            path: None,
            detail: Some(detail.into()),
        });
    }

    /// Aggregate loader findings into envelope warnings (docs/model-design.md
    /// §5: unknown ids in hex, never a guessed name).
    pub fn drain_loader_warnings(&mut self) {
        let unknown = self.loaded.unknown_ids.clone();
        for (id, count) in unknown {
            self.warnings.push(Warning {
                code: WarningCode::UnknownObjectType,
                message: format!(
                    "{count} object(s) with type id {id} (0x{id:x}) have no trusted registry entry; payloads skipped"
                ),
                path: None,
                detail: Some(format!("0x{:x}", id)),
            });
        }
        let undecodable = self.loaded.undecodable_ids.clone();
        for (id, count) in undecodable {
            let name = self
                .registry
                .name_for(self.app, id)
                .unwrap_or_else(|| format!("unknown:0x{id:x}"));
            self.warnings.push(Warning {
                code: WarningCode::UndecodableObject,
                message: format!("{count} object(s) of type {name} failed to decode structurally; payloads skipped by declared length"),
                path: None,
                detail: Some(name),
            });
        }
    }

    // -- metadata plists (docs/format/container.md §Metadata files) ----------

    fn load_metadata_plists(&mut self) {
        if let Some(bytes) = self.members.get("Metadata/Properties.plist") {
            match plist::Value::from_reader(std::io::Cursor::new(bytes)) {
                Ok(v) => {
                    let dict = v.as_dictionary();
                    self.meta.application = dict
                        .and_then(|d| d.get("Application"))
                        .and_then(|p| p.as_string())
                        .map(str::to_string);
                    self.meta.file_format_version = dict
                        .and_then(|d| d.get("fileFormatVersion"))
                        .and_then(plist_value_as_string);
                }
                Err(e) => self.warn(
                    WarningCode::UnsupportedFeature,
                    format!("Metadata/Properties.plist unreadable: {e}"),
                ),
            }
        }
        if let Some(bytes) = self.members.get("Metadata/BuildVersionHistory.plist") {
            match plist::Value::from_reader(std::io::Cursor::new(bytes)) {
                Ok(v) => {
                    // Array of strings, or array of dicts with Version/Build
                    // (docs/format/container.md).
                    if let Some(arr) = v.as_array() {
                        let entries: Vec<String> = arr
                            .iter()
                            .filter_map(|item| match item.as_dictionary() {
                                Some(d) => {
                                    let ver = d.get("Version").and_then(|p| p.as_string());
                                    let build = d.get("Build").and_then(|p| p.as_string());
                                    match (ver, build) {
                                        (Some(v), Some(b)) => Some(format!("{v} ({b})")),
                                        (Some(v), None) => Some(v.to_string()),
                                        (None, Some(b)) => Some(b.to_string()),
                                        _ => item.as_string().map(str::to_string),
                                    }
                                }
                                None => item.as_string().map(str::to_string),
                            })
                            .collect();
                        // Last entries, oldest → newest (model contract).
                        let last: Vec<String> =
                            entries.iter().rev().take(10).rev().cloned().collect();
                        self.meta.build_version_history = Some(last);
                    }
                }
                Err(e) => self.warn(
                    WarningCode::UnsupportedFeature,
                    format!("Metadata/BuildVersionHistory.plist unreadable: {e}"),
                ),
            }
        }
        if let Some(bytes) = self.members.get("Metadata/DocumentIdentifier") {
            let id = String::from_utf8_lossy(&bytes).trim().to_string();
            if !id.is_empty() {
                self.meta.document_id = Some(id);
            }
        }
    }

    // -- TSP.PackageMetadata (object 2) --------------------------------------

    fn load_package_metadata(&mut self) {
        let Some(pkg) = self.loaded.msg(2) else { return };
        // DocumentRevision.identifier (field 2 → DocumentRevision.identifier=2)
        if self.meta.document_id.is_none() {
            if let Some(rev) = pkg.msg(2) {
                self.meta.document_id = rev.string(2);
            }
        }
        // datas = 4 (TSPArchiveMessages.proto:106-123)
        for data in pkg.msgs(4) {
            let id = data.varint(1).unwrap_or(0);
            let entry = DataInfoEntry {
                preferred_file_name: data.string(3),
                file_name: data.string(4),
                remote_url: data.string(7),
                materialized_length: data.varint(18),
                pixel_size: data.msg(10).and_then(|attrs| attributes_pixel_size(&attrs)),
            };
            self.datas.insert(id, entry);
        }
    }

    // -- media ---------------------------------------------------------------

    /// Resolve a `TSP.DataReference` id to a `MediaRef` (docs/format/media.md
    /// resolution chain, minus the byte reads the JSON model doesn't carry).
    pub fn media_ref(&mut self, data_id: u64) -> MediaRef {
        self.referenced_datas.insert(data_id);
        let entry = self.datas.get(&data_id);
        let file_name = entry.and_then(|e| e.file_name.clone());
        let preferred = entry.and_then(|e| e.preferred_file_name.clone());
        let pixel = entry.and_then(|e| e.pixel_size);
        let found = entry.is_some();
        let r = MediaRef {
            data_id: data_id.to_string(),
            file_name,
            preferred_file_name: preferred,
            pixel_size: pixel,
        };
        if !found {
            self.warn_detail(
                WarningCode::MediaMissing,
                format!("media data id {data_id} has no TSP.DataInfo registry entry"),
                data_id.to_string(),
            );
        }
        r
    }

    /// The envelope `media[]`: every referenced DataInfo (dead formats are
    /// dropped per docs/model-design.md §6). Assets whose Data/ bytes are
    /// absent get a `media-missing` warning.
    pub fn build_media_assets(&mut self) -> Vec<MediaAsset> {
        let mut assets = Vec::new();
        let referenced: Vec<u64> = self.referenced_datas.iter().copied().collect();
        for id in referenced {
            let Some(entry) = self.datas.get(&id).cloned() else { continue };
            let file_name = entry.file_name.clone().or_else(|| entry.preferred_file_name.clone());
            let kind = file_name.as_deref().map(media_kind).unwrap_or(MediaKind::Other);
            let byte_length = entry.materialized_length;
            let remote = entry.remote_url.clone();
            let preferred = entry.preferred_file_name.clone();
            let pixel = entry.pixel_size;
            // Materialized bytes absent → media-missing (docs/format/media.md
            // resolution chain step 3).
            if let Some(name) = &file_name {
                if self.members.data_file(name).is_none()
                    && remote.is_none()
                    && byte_length.is_some_and(|l| l > 0)
                {
                    self.warn_detail(
                        WarningCode::MediaMissing,
                        format!(
                            "media bytes for `{}` (data id {id}) are not in the container",
                            preferred.unwrap_or_else(|| name.clone())
                        ),
                        name.clone(),
                    );
                }
            }
            assets.push(MediaAsset {
                data_id: id.to_string(),
                file_name,
                preferred_file_name: entry.preferred_file_name.clone(),
                kind,
                byte_length,
                pixel_size: entry.pixel_size,
            });
        }
        assets.sort_by(|a, b| a.data_id.cmp(&b.data_id));
        assets
    }

    /// Locale from TSK.DocumentArchive.locale_identifier (field 4), reached
    /// through the TSA super chain off the app root.
    pub fn finish_meta(&mut self, root_locale: Option<String>) {
        if self.meta.locale.is_none() {
            self.meta.locale = root_locale;
        }
    }
}

fn plist_value_as_string(v: &plist::Value) -> Option<String> {
    match v {
        plist::Value::String(s) => Some(s.clone()),
        plist::Value::Array(a) => a.iter().filter_map(|i| i.as_string()).next().map(str::to_string),
        plist::Value::Integer(i) => Some(i.as_signed().unwrap_or(0).to_string()),
        _ => None,
    }
}

/// Media kind from file extension (litchi's table, docs/format/media.md).
pub fn media_kind(name: &str) -> MediaKind {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "tiff" | "tif" | "bmp" | "heic" | "heif" | "webp"
        | "svg" => MediaKind::Image,
        "mp4" | "mov" | "m4v" | "avi" | "mkv" | "mpg" | "mpeg" | "wmv" => MediaKind::Movie,
        "mp3" | "aac" | "m4a" | "wav" | "aiff" | "aif" | "caf" | "oga" | "ogg" | "flac"
        | "m4b" => MediaKind::Audio,
        "pdf" => MediaKind::Pdf,
        _ => MediaKind::Other,
    }
}

/// `TSP.DataAttributes` + `TSD.ImageDataAttributes` (ext field 100): pixel
/// size of an image asset (docs/format/media.md §Type detection).
fn attributes_pixel_size(attrs: &Msg) -> Option<Size> {
    let img = attrs.msg(100)?;
    let m = img.msg(1)?; // pixel_size: TSP.Size { width = 1, height = 2 }? see proto
    Some(Size { width: m.f32v(1)? as f64, height: m.f32v(2)? as f64 })
}

/// Fonts harvested from resolved CharStyles, deduped + sorted (model contract
/// FontList).
pub fn collect_font(fonts: &mut BTreeSet<String>, name: &str) {
    if !name.is_empty() {
        fonts.insert(name.to_string());
    }
}

/// Counted-warning helper for repeated unresolved references: keeps the
/// envelope small while staying non-silent.
pub struct Counter {
    counts: BTreeMap<String, u64>,
}

impl Counter {
    pub fn new() -> Counter {
        Counter { counts: BTreeMap::new() }
    }
    pub fn bump(&mut self, key: impl Into<String>) {
        *self.counts.entry(key.into()).or_insert(0) += 1;
    }
    pub fn drain(self) -> Vec<(String, u64)> {
        self.counts.into_iter().collect()
    }
}

impl Ctx {
    /// Build a context from raw document bytes (wasm path): no filesystem.
    pub fn from_bytes(bytes: &[u8]) -> Result<Ctx, iwadump::Error> {
        use iwadump::error::{Error, Kind, Layer};
        let streams = loader::streams_from_bytes(bytes)?;
        // Collect non-IWA members (metadata plists, Data/) for the envelope.
        let mut map: HashMap<String, Vec<u8>> = HashMap::new();
        if let Ok(mut zip) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) {
            for i in 0..zip.len() {
                let Ok(mut f) = zip.by_index(i) else { continue };
                if f.is_dir() || f.name().to_ascii_lowercase().ends_with(".iwa") {
                    continue;
                }
                let name = f.name().to_string();
                let mut buf = Vec::new();
                if std::io::Read::read_to_end(&mut f, &mut buf).is_ok() {
                    map.insert(name, buf);
                }
            }
        }
        let registry = Registry::embedded()?;
        let app = iwadump::dump::detect_app(&streams);
        let app_kind = match app {
            App::Keynote => AppKind::Keynote,
            App::Numbers => AppKind::Numbers,
            _ => AppKind::Pages,
        };
        let loaded = loader::load(&streams, &registry, app);
        let mut ctx = Ctx {
            app,
            app_kind,
            registry,
            loaded,
            members: Members::from_map(map),
            warnings: Vec::new(),
            fonts: std::collections::BTreeSet::new(),
            meta: DocumentMeta { app: app_kind, ..Default::default() },
            datas: HashMap::new(),
            referenced_datas: std::collections::BTreeSet::new(),
        };
        ctx.refine_app();
        ctx.load_metadata_plists();
        ctx.load_package_metadata();
        if !ctx.loaded.unknown_ids.is_empty() || !ctx.loaded.undecodable_ids.is_empty() {
            let _ = Error::new(Kind::Corrupt, Layer::Message, ""); // types kept imported
        }
        Ok(ctx)
    }
}
