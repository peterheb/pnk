//! Converter context: warnings sink, font harvest, media/DataInfo registry,
//! metadata plists — everything the per-app tree walkers share
//! (docs/model-design.md §4/§5, docs/format/media.md).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;

use iwadump::registry::{App, Registry};

use crate::loader::{self, Loaded};
use crate::members::Members;
use crate::model::{
    AppKind, BaselineScript, Capitalization, CharStyle, DocumentMeta, HorizontalAlignment,
    ListMarkerKind, MediaAsset, MediaKind, MediaRef, ParaStyle, Size, StrikethroughStyle,
    TableCellStyle, UnderlineStyle, Warning, WarningCode,
};
use crate::pb::Msg;

/// Hash-cons pool with canonical JSON (sorted keys, compact) as dedup key;
/// first-use order preserved.
#[derive(Debug, Default)]
pub struct StylePool<T> {
    pub items: Vec<T>,
    keys: std::collections::HashMap<String, u32>,
}

impl<T: serde::Serialize + PartialEq> StylePool<T> {
    /// Intern `item`; returns its index. `None` for the empty/default value
    /// ("{}" canonical form) — absent index means unstyled per the contract.
    pub fn intern(&mut self, item: T) -> Option<u32> {
        let key = canonical_json(&item);
        if key == "{}" {
            return None;
        }
        if let Some(i) = self.keys.get(&key) {
            return Some(*i);
        }
        let idx = self.items.len() as u32;
        self.items.push(item);
        self.keys.insert(key, idx);
        Some(idx)
    }
}

/// Strip documented default values from a resolved ParaStyle so pooled
/// entries carry only overrides (absent = default per docs/model-design.md
/// §1.5: indents/spacing 0, alignment auto, list none/level 0, booleans
/// false). Fixture-verified: G1 body para style emits 12→5 fields.
pub fn strip_para_defaults(mut s: ParaStyle) -> ParaStyle {
    if s.left_indent_pt == Some(0.0) {
        s.left_indent_pt = None;
    }
    if s.right_indent_pt == Some(0.0) {
        s.right_indent_pt = None;
    }
    if s.first_line_indent_pt == Some(0.0) {
        s.first_line_indent_pt = None;
    }
    if s.space_before_pt == Some(0.0) {
        s.space_before_pt = None;
    }
    if s.space_after_pt == Some(0.0) {
        s.space_after_pt = None;
    }
    if s.horizontal_alignment == Some(HorizontalAlignment::Auto) {
        s.horizontal_alignment = None;
    }
    if s.keep_lines_together == Some(false) {
        s.keep_lines_together = None;
    }
    if s.keep_with_next == Some(false) {
        s.keep_with_next = None;
    }
    if s.hyphenate == Some(true) {
        s.hyphenate = None;
    }
    if s.page_break_before == Some(false) {
        s.page_break_before = None;
    }
    if s.outline_level == Some(0) {
        s.outline_level = None;
    }
    // List: none/level 0 is default
    if let Some(l) = &s.list {
        if l.marker_kind == ListMarkerKind::None
            && l.level == 0
            && l.marker_text.is_none()
            && l.number_kind.is_none()
            && l.marker_image.is_none()
            && l.start.is_none()
            && l.marker_indent_pt == Some(0.0)
        {
            s.list = None;
        }
        if let Some(l) = &mut s.list {
            if l.marker_indent_pt == Some(0.0) {
                l.marker_indent_pt = None;
            }
        }
    }
    if s.default_tab_stop_pt == Some(36.0) {
        s.default_tab_stop_pt = None;
    }
    if s.writing_direction.is_none() {} // already Option
    s
}

/// Strip documented defaults from a resolved TableCellStyle.
pub fn strip_cell_defaults(mut s: TableCellStyle) -> TableCellStyle {
    if s.text_wrap == Some(false) {
        s.text_wrap = None;
    }
    s
}

/// Strip documented defaults from a resolved CharStyle.
pub fn strip_char_defaults(mut s: CharStyle) -> CharStyle {
    if s.font_size_pt == Some(12.0) {
        s.font_size_pt = None;
    }
    if s.bold == Some(false) {
        s.bold = None;
    }
    if s.italic == Some(false) {
        s.italic = None;
    }
    if s.underline == Some(UnderlineStyle::None) {
        s.underline = None;
    }
    if s.strikethrough == Some(StrikethroughStyle::None) {
        s.strikethrough = None;
    }
    if s.capitalization == Some(Capitalization::None) {
        s.capitalization = None;
    }
    if s.baseline == Some(BaselineScript::Normal) {
        s.baseline = None;
    }
    if s.baseline_shift_pt == Some(0.0) {
        s.baseline_shift_pt = None;
    }
    if s.tracking_pt == Some(0.0) {
        s.tracking_pt = None;
    }
    s
}

/// Canonical form: serde_json::Value with alphabetically sorted keys,
/// compact serialization.
fn canonical_json<T: serde::Serialize>(item: &T) -> String {
    let value: serde_json::Value = serde_json::to_value(item).unwrap_or(serde_json::Value::Null);
    serde_json::to_string(&value).unwrap_or_default()
}

pub struct Ctx {
    pub app: App,
    pub app_kind: AppKind,
    pub registry: Registry,
    pub loaded: Loaded,
    pub members: Members,
    pub warnings: Vec<Warning>,
    pub fonts: BTreeSet<String>,
    /// Document-wide text-style pools (first-use order via emission order).
    pub para_pool: StylePool<ParaStyle>,
    pub char_pool: StylePool<CharStyle>,
    pub meta: DocumentMeta,
    /// TSP.PackageMetadata.datas, keyed by DataInfo.identifier.
    pub datas: HashMap<u64, DataInfoEntry>,
    /// DataInfo identifiers actually referenced from content (for `media[]`).
    pub referenced_datas: BTreeSet<u64>,
    /// Live nesting depth of text-storage extraction (footnote bodies pull
    /// contained storages, which can reference attachments that pull more
    /// storages). Guards a crafted cyclic graph — FINDINGS.md H-4.
    pub text_extract_depth: u32,
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
            para_pool: StylePool::default(),
            char_pool: StylePool::default(),
            meta: DocumentMeta {
                app: app_kind,
                ..Default::default()
            },
            datas: HashMap::new(),
            referenced_datas: BTreeSet::new(),
            text_extract_depth: 0,
        };
        ctx.refine_app();
        ctx.load_metadata_plists();
        ctx.load_package_metadata();
        Ok(ctx)
    }

    /// Cross-check the stream-name heuristic (iwadump::detect_app) against the
    /// actual root object's type id (10000 = TP.DocumentArchive → Pages).
    fn refine_app(&mut self) {
        // 1. Root type: 10000 = TP.DocumentArchive → Pages; a root resolving
        //    to KN./TN.DocumentArchive in the CURRENT app namespace → that app.
        if let Some(rec) = self.loaded.record(1) {
            let by_root = match rec.type_id {
                10000 => Some(AppKind::Pages),
                _ => self
                    .registry
                    .name_for(self.app, rec.type_id)
                    .as_deref()
                    .and_then(|n| match n {
                        "KN.DocumentArchive" => Some(AppKind::Keynote),
                        "TN.DocumentArchive" => Some(AppKind::Numbers),
                        _ => None,
                    }),
            };
            if let Some(kind) = by_root {
                self.set_app(kind);
                return;
            }
        }
        // 2. Object-name evidence: some documents (older Numbers saves with
        //    unregistered root types) defeat iwadump's stream-name heuristic.
        //    Resolve every object type id in the unambiguous-Common namespace;
        //    the first TN./TP./KN.-prefixed name wins. TN./TP./KN. ids can
        //    collide across app tables, so only accept names when the id is
        //    unambiguous (name_for with App::Unknown already enforces that).
        for rec in self.loaded.records.values() {
            let Some(name) = self.registry.name_for(App::Unknown, rec.type_id) else {
                continue;
            };
            let kind = match name.as_str() {
                n if n.starts_with("TN.") => Some(AppKind::Numbers),
                n if n.starts_with("TP.") => Some(AppKind::Pages),
                n if n.starts_with("KN.") => Some(AppKind::Keynote),
                _ => None,
            };
            if let Some(kind) = kind {
                self.set_app(kind);
                return;
            }
        }
    }

    fn set_app(&mut self, kind: AppKind) {
        self.app_kind = kind;
        self.meta.app = kind;
        self.app = match kind {
            AppKind::Pages => App::Pages,
            AppKind::Numbers => App::Numbers,
            AppKind::Keynote => App::Keynote,
        };
    }

    // -- warnings -----------------------------------------------------------

    pub fn warn(&mut self, code: WarningCode, message: impl Into<String>) {
        self.warnings.push(Warning {
            code,
            message: message.into(),
            path: None,
            detail: None,
            count: None,
            paths: None,
        });
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
            count: None,
            paths: None,
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
                count: None,
                paths: None,
            });
        }
        if self.loaded.patches_dropped > 0 {
            let n = self.loaded.patches_dropped;
            self.warnings.push(Warning {
                code: WarningCode::UnsupportedFeature,
                message: format!(
                    "{n} incremental-save patch(es) could not be applied; affected objects may show pre-edit content"
                ),
                path: None,
                detail: Some("incremental-save".to_string()),
                count: None,
                paths: None,
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
                count: None,
                paths: None,
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
        let Some(pkg) = self.loaded.msg(2) else {
            return;
        };
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

    /// Are the bytes for this DataInfo actually reachable in the container?
    /// A `remote_url` does NOT count: this viewer is explicitly offline and
    /// never fetches, so a remote-only original must lose to a materialized
    /// alternative such as a packaged `-small` preview (FINDINGS.md M-10;
    /// template packages ship only those variants — 00C Textbook fixture).
    /// Name-only check — never inflates the member (FINDINGS.md H-2).
    pub fn data_available(&self, data_id: u64) -> bool {
        self.datas
            .get(&data_id)
            .and_then(|e| e.file_name.as_deref().or(e.preferred_file_name.as_deref()))
            .map(|n| self.members.has_data_file(n))
            .unwrap_or(false)
    }

    /// The envelope `media[]`: every referenced DataInfo (dead formats are
    /// dropped per docs/model-design.md §6). Assets whose Data/ bytes are
    /// absent get a `media-missing` warning.
    pub fn build_media_assets(&mut self) -> Vec<MediaAsset> {
        let mut assets = Vec::new();
        let referenced: Vec<u64> = self.referenced_datas.iter().copied().collect();
        for id in referenced {
            let Some(entry) = self.datas.get(&id).cloned() else {
                continue;
            };
            let file_name = entry
                .file_name
                .clone()
                .or_else(|| entry.preferred_file_name.clone());
            let kind = file_name
                .as_deref()
                .map(media_kind)
                .unwrap_or(MediaKind::Other);
            let byte_length = entry.materialized_length;
            let remote = entry.remote_url.clone();
            let preferred = entry.preferred_file_name.clone();
            let _pixel = entry.pixel_size;
            // Materialized bytes absent → media-missing (docs/format/media.md
            // resolution chain step 3). Independent of the optional length
            // metadata (FINDINGS.md L-4), and a remote-only asset warns too:
            // this viewer never fetches (FINDINGS.md M-10).
            if let Some(name) = &file_name {
                if !self.members.has_data_file(name) {
                    let shown = preferred.unwrap_or_else(|| name.clone());
                    let message = if remote.is_some() {
                        format!(
                            "media `{shown}` (data id {id}) is remote-only; this offline viewer cannot fetch it"
                        )
                    } else {
                        format!("media bytes for `{shown}` (data id {id}) are not in the container")
                    };
                    self.warn_detail(WarningCode::MediaMissing, message, name.clone());
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
        plist::Value::Array(a) => a
            .iter()
            .filter_map(|i| i.as_string())
            .next()
            .map(str::to_string),
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
        "mp3" | "aac" | "m4a" | "wav" | "aiff" | "aif" | "caf" | "oga" | "ogg" | "flac" | "m4b" => {
            MediaKind::Audio
        }
        "pdf" => MediaKind::Pdf,
        _ => MediaKind::Other,
    }
}

/// `TSP.DataAttributes` + `TSD.ImageDataAttributes` (ext field 100): pixel
/// size of an image asset (docs/format/media.md §Type detection).
fn attributes_pixel_size(attrs: &Msg) -> Option<Size> {
    let img = attrs.msg(100)?;
    let m = img.msg(1)?; // pixel_size: TSP.Size { width = 1, height = 2 }? see proto
    Some(Size {
        width: m.f32v(1)? as f64,
        height: m.f32v(2)? as f64,
    })
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
        Counter {
            counts: BTreeMap::new(),
        }
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
        // Non-IWA members (metadata plists, Data/) stay LAZY: the compressed
        // document is retained once and members inflate on demand, so unused
        // media and junk entries never occupy memory (FINDINGS.md H-2).
        let members = Members::from_zip_bytes(bytes.to_vec());
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
            members,
            warnings: Vec::new(),
            fonts: std::collections::BTreeSet::new(),
            para_pool: StylePool::default(),
            char_pool: StylePool::default(),
            meta: DocumentMeta {
                app: app_kind,
                ..Default::default()
            },
            datas: HashMap::new(),
            referenced_datas: std::collections::BTreeSet::new(),
            text_extract_depth: 0,
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

impl Ctx {
    /// Document locale: TSK.DocumentArchive.locale_identifier (field 4) via
    /// the TSA super chain (Pages root super = 15, KN/TN = 8), with the TSA's
    /// own `document_language` (field 3) as fallback — both observed in the
    /// corpus.
    pub fn resolve_locale(&self, root: &Msg) -> Option<String> {
        for f in [15u32, 8, 3] {
            // The app root's `super` is the TSA.DocumentArchive payload —
            // INLINE (the Apple-protobuf superclass idiom embeds the parent,
            // it is not a TSP.Reference), with the TSK payload in turn
            // inline at TSA field 1. Fall back to a referenced archive when
            // a writer does use one.
            let tsa = root.msg(f).or_else(|| {
                root.reference(f)
                    .and_then(|id| self.loaded.msg(id).cloned())
            });
            let Some(tsa) = tsa else { continue };
            // TSK.DocumentArchive.locale_identifier = 4 (inline at TSA.1 or
            // referenced).
            let tsk = tsa
                .msg(1)
                .or_else(|| tsa.reference(1).and_then(|id| self.loaded.msg(id).cloned()));
            if let Some(loc) = tsk.as_ref().and_then(|m| m.string(4)) {
                return Some(loc);
            }
            // TSA.document_language (3).
            if let Some(loc) = tsa.string(3) {
                if is_locale_token(&loc) {
                    return Some(loc);
                }
            }
        }
        None
    }
}

fn is_locale_token(s: &str) -> bool {
    s.len() <= 8
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && !s.chars().any(char::is_whitespace)
}
