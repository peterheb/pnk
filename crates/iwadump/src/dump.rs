//! Document assembly and rendering: container → streams → archives →
//! messages, presented the way docs/model-design.md wants to consume them —
//! structure first (ids, lengths, decodability), payloads opaque.

use std::path::Path;

use crate::container::{Container, ContainerForm};
use crate::envelope::{ArchiveInfo, MessageStatus};
use crate::error::Error;
use crate::iwa::IwaStream;
use crate::registry::{App, Registry};

/// One archive segment flattened for display.
#[derive(Debug, Clone)]
pub struct MessageView {
    /// `ArchiveInfo.identifier` — the segment's object id (shown as "local id"
    /// in the dump; the stream-local handle into the global id space).
    pub local_id: u64,
    /// Position of the payload within its archive (0-based `message_infos` order).
    pub index: usize,
    pub type_id: u32,
    /// Resolved registry name, or `None` (unknown ids stay opaque).
    pub name: Option<String>,
    pub length: u32,
    pub status: MessageStatus,
    /// The payload bytes (hex-dumped by `--message`).
    pub payload: Vec<u8>,
}

impl MessageView {
    /// `Name` or `unknown:0x…` — never a guessed name (docs/format/registry.md).
    pub fn display_name(&self) -> String {
        match &self.name {
            Some(n) => n.clone(),
            None => format!("unknown:0x{:x}", self.type_id),
        }
    }

    pub fn status_reason(&self) -> String {
        match &self.status {
            MessageStatus::Decoded { .. } => "ok".to_string(),
            MessageStatus::UnknownType => "unknown type id; payload not walked".to_string(),
            MessageStatus::Undecodable { reason, .. } => format!("undecodable: {reason}"),
        }
    }
}

/// One decoded IWA stream plus its archive segments.
#[derive(Debug, Clone)]
pub struct StreamView {
    pub name: String,
    pub iwa: IwaStream,
    pub archives: Vec<ArchiveInfo>,
}

impl StreamView {
    /// Flattened message views in stream order.
    pub fn messages(&self, registry: &Registry, app: App) -> Vec<MessageView> {
        let mut out = Vec::new();
        for a in &self.archives {
            let statuses = a.message_status(registry, app);
            for (i, m) in a.messages.iter().enumerate() {
                out.push(MessageView {
                    local_id: a.identifier,
                    index: i,
                    type_id: m.type_id,
                    name: registry.name_for(app, m.type_id),
                    length: m.length,
                    status: statuses
                        .get(i)
                        .cloned()
                        .unwrap_or(MessageStatus::UnknownType),
                    payload: m.payload.clone(),
                });
            }
        }
        out
    }
}

/// An opened, fully decoded document.
pub struct Document {
    pub path: String,
    pub form: ContainerForm,
    pub container: Container,
    pub streams: Vec<StreamView>,
    pub app: App,
    pub registry: Registry,
}

impl Document {
    /// Open and fully decode a document (all streams decompressed and
    /// envelope-parsed). Rejections surface as layer-tagged errors.
    pub fn open(path: &Path, legacy_ok: bool) -> Result<Document, Error> {
        let container = Container::open(path, legacy_ok)?;
        let registry = Registry::embedded()?;
        let mut streams = Vec::with_capacity(container.iwas.len());
        for (name, bytes) in &container.iwas {
            let iwa = IwaStream::parse(name, bytes)?;
            let archives = crate::envelope::parse_stream(&iwa.decoded)?;
            streams.push(StreamView {
                name: name.clone(),
                iwa,
                archives,
            });
        }
        let app = detect_app(&streams);
        Ok(Document {
            path: path.display().to_string(),
            form: container.form,
            container,
            streams,
            app,
            registry,
        })
    }

    /// Raw container (used by `--list` and `--legacy-ok`, which skip decode).
    pub fn open_container_only(path: &Path, legacy_ok: bool) -> Result<Container, Error> {
        Container::open(path, legacy_ok)
    }

    /// Find a message by archive local id (`--message`). Returns the stream
    /// name and the view. The first match in stream order wins (ids are
    /// document-global in well-formed files).
    pub fn find_message(&self, id: u64) -> Option<(&StreamView, MessageView)> {
        for s in &self.streams {
            for m in s.messages(&self.registry, self.app) {
                if m.local_id == id {
                    return Some((s, m));
                }
            }
        }
        None
    }

    /// Human-readable summary tree.
    pub fn render_tree(&self, limit: Option<usize>) -> String {
        let mut out = String::new();
        let form = match self.form {
            ContainerForm::FlatZip => "flat zip",
            ContainerForm::FlatZipNested => "flat zip (nested Index.zip)",
            ContainerForm::PackageDir => "package dir",
            ContainerForm::LegacyRaw => "legacy raw listing",
        };
        let app = match self.app {
            App::Keynote => "keynote",
            App::Numbers => "numbers",
            App::Pages => "pages",
            App::Unknown => "unknown app",
        };
        out.push_str(&format!(
            "{} — {}, {} members, {} iwa streams, app: {}\n",
            self.path,
            form,
            self.container.members.len(),
            self.streams.len(),
            app
        ));
        for s in &self.streams {
            let raw_len: usize = s
                .iwa
                .blocks
                .iter()
                .map(|b| b.compressed_len as usize + 4)
                .sum();
            out.push_str(&format!(
                "  {} — {} blocks, {} B compressed → {} B decoded, {} archives\n",
                s.name,
                s.iwa.blocks.len(),
                raw_len,
                s.iwa.decoded.len(),
                s.archives.len()
            ));
            let msgs = s.messages(&self.registry, self.app);
            let shown = msgs.len().min(limit.unwrap_or(msgs.len()));
            for m in &msgs[..shown] {
                out.push_str(&format!(
                    "    id={:<6} {:<45} len={:<8} {}\n",
                    m.local_id,
                    m.display_name(),
                    m.length,
                    m.status_reason()
                ));
            }
            if shown < msgs.len() {
                out.push_str(&format!(
                    "    … {} more (use --limit)\n",
                    msgs.len() - shown
                ));
            }
        }
        for (name, size) in &self.container.non_iwa {
            out.push_str(&format!(
                "  {name} — operation storage (bvxn), not an IWA snappy stream; skipped ({size} B)\n"
            ));
        }
        out
    }
    /// Machine-readable JSON dump.
    pub fn render_json(&self, limit: Option<usize>) -> String {
        let mut j = Json::new();
        j.obj_start();
        let form = match self.form {
            ContainerForm::FlatZip => "flat-zip",
            ContainerForm::FlatZipNested => "flat-zip-nested",
            ContainerForm::PackageDir => "package-dir",
            ContainerForm::LegacyRaw => "legacy-raw",
        };
        j.field_str("form", form);
        j.field_num("member_count", self.container.members.len() as u64);
        j.field_str("app", self.app.label());
        j.field_num(
            "registry_common_entries",
            self.registry.common_size() as u64,
        );
        j.key("members");
        j.arr_start();
        for m in &self.container.members {
            j.obj_start();
            j.field_str("name", &m.name);
            j.field_num("size", m.size);
            j.field_num("compressed_size", m.compressed_size);
            j.obj_end();
        }
        j.arr_end();
        j.key("streams");
        j.arr_start();
        for s in &self.streams {
            j.obj_start();
            j.field_str("name", &s.name);
            j.field_num("block_count", s.iwa.blocks.len() as u64);
            j.field_num(
                "compressed_bytes",
                s.iwa
                    .blocks
                    .iter()
                    .map(|b| b.compressed_len as u64 + 4)
                    .sum(),
            );
            j.field_num("decoded_bytes", s.iwa.decoded.len() as u64);
            j.field_num("archive_count", s.archives.len() as u64);
            let msgs = s.messages(&self.registry, self.app);
            let shown = msgs.len().min(limit.unwrap_or(msgs.len()));
            j.key("messages");
            j.arr_start();
            for m in &msgs[..shown] {
                j.obj_start();
                j.field_num("local_id", m.local_id);
                j.field_num("index", m.index as u64);
                j.field_num("type", m.type_id as u64);
                match &m.name {
                    Some(n) => j.field_str("name", n),
                    None => j.field_str("name", &format!("unknown:0x{:x}", m.type_id)),
                }
                j.field_num("length", m.length as u64);
                match &m.status {
                    MessageStatus::Decoded { name } => {
                        j.field_str("status", "decoded");
                        j.field_str("registry_name", name);
                    }
                    MessageStatus::UnknownType => {
                        j.field_str("status", "unknown-type");
                    }
                    MessageStatus::Undecodable { name, reason } => {
                        j.field_str("status", "undecodable");
                        j.field_str("registry_name", name);
                        j.field_str("reason", reason);
                    }
                }
                j.obj_end();
            }
            j.arr_end();
            if shown < msgs.len() {
                j.field_num("messages_omitted", (msgs.len() - shown) as u64);
            }
            j.obj_end();
        }
        j.arr_end();
        j.key("skipped_non_iwa");
        j.arr_start();
        for (name, size) in &self.container.non_iwa {
            j.obj_start();
            j.field_str("name", name);
            j.field_num("size", *size);
            j.field_str("reason", "operation storage (bvxn magic)");
            j.obj_end();
        }
        j.arr_end();
        j.obj_end();
        j.finish()
    }
}

/// Detect the owning app (drives which app registry table applies).
///
/// Signals, cheap-first:
/// 1. the root type of `Document.iwa`'s first archive: 10000 = `TP.DocumentArchive`
///    → Pages (verified in the fixture set; Pages rows of success.tsv record it);
/// 2. IWA member names: `Slide`/`TemplateSlide`/`Theme` → Keynote,
///    `Tables/` → Numbers (Numbers stores tables under `Index/Tables/`);
/// 3. otherwise Unknown — ambiguous ids then stay unnamed rather than guessed.
pub fn detect_app(streams: &[StreamView]) -> App {
    if let Some(doc) = streams.iter().find(|s| {
        s.name
            .rsplit('/')
            .next()
            .map(|b| b.eq_ignore_ascii_case("Document.iwa"))
            .unwrap_or(false)
    }) {
        if let Some(root_type) = doc
            .archives
            .first()
            .and_then(|a| a.messages.first())
            .map(|m| m.type_id)
        {
            if root_type == 10000 {
                return App::Pages;
            }
        }
    }
    let names: Vec<&str> = streams.iter().map(|s| s.name.as_str()).collect();
    let has = |pred: &dyn Fn(&str) -> bool| names.iter().any(|n| pred(n));
    // Slide member names vary by writer: `Slide-1.iwa`, `Slide7.iwa`,
    // `MasterSlide.iwa`, `TemplateSlide-…` all occur in the corpus — match
    // the bare prefix, not a dashed form.
    if has(&|n| {
        let base = n.rsplit('/').next().unwrap_or(n).to_lowercase();
        base.starts_with("slide")
            || base.starts_with("masterslide")
            || base.starts_with("templateslide")
    }) {
        return App::Keynote;
    }
    // `Tables/` must outrank the theme signal: Numbers documents also carry
    // a `ThemeStylesheet.iwa` (TN.ThemeArchive), and xlsx-derived Numbers
    // files in the corpus have Tables/ + theme members but no slides — the
    // old theme-first order rendered those spreadsheets as one bogus slide.
    if has(&|n| n.contains("/Tables/") || n.starts_with("Tables/")) {
        return App::Numbers;
    }
    if has(&|n| {
        let base = n.rsplit('/').next().unwrap_or(n).to_lowercase();
        base.starts_with("theme")
    }) {
        return App::Keynote;
    }
    App::Unknown
}

/// Minimal JSON writer with proper escaping (no serde dependency — the
/// emitted shapes are fully controlled by this crate).
pub struct Json {
    buf: String,
    needs_comma: Vec<bool>,
}

impl Json {
    pub fn new() -> Json {
        Json {
            buf: String::new(),
            needs_comma: Vec::new(),
        }
    }

    fn sep(&mut self) {
        if let Some(last) = self.needs_comma.last_mut() {
            if *last {
                self.buf.push(',');
            }
            *last = true;
        }
    }

    pub fn obj_start(&mut self) {
        self.sep();
        self.buf.push('{');
        self.needs_comma.push(false);
    }

    pub fn obj_end(&mut self) {
        self.buf.push('}');
        self.needs_comma.pop();
    }

    pub fn arr_start(&mut self) {
        self.sep();
        self.buf.push('[');
        self.needs_comma.push(false);
    }

    pub fn arr_end(&mut self) {
        self.buf.push(']');
        self.needs_comma.pop();
    }

    pub fn key(&mut self, k: &str) {
        self.sep();
        self.buf.push_str(&quote(k));
        self.buf.push(':');
        if let Some(last) = self.needs_comma.last_mut() {
            *last = false;
        }
    }

    pub fn field_str(&mut self, k: &str, v: &str) {
        self.key(k);
        self.buf.push_str(&quote(v));
        if let Some(last) = self.needs_comma.last_mut() {
            *last = true;
        }
    }

    pub fn field_num(&mut self, k: &str, v: u64) {
        self.key(k);
        self.buf.push_str(&v.to_string());
        if let Some(last) = self.needs_comma.last_mut() {
            *last = true;
        }
    }

    pub fn finish(self) -> String {
        self.buf
    }
}

fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Hex + ASCII dump used by `--message`.
pub fn hex_dump(bytes: &[u8], max_rows: usize) -> String {
    let mut out = String::new();
    let total_rows = bytes.len().div_ceil(16);
    for (row, chunk) in bytes.chunks(16).take(max_rows).enumerate() {
        out.push_str(&format!("{:08x}  ", row * 16));
        for i in 0..16 {
            match chunk.get(i) {
                Some(b) => out.push_str(&format!("{:02x} ", b)),
                None => out.push_str("   "),
            }
            if i == 7 {
                out.push(' ');
            }
        }
        out.push_str(" |");
        for b in chunk {
            out.push(if (0x20..0x7f).contains(b) {
                *b as char
            } else {
                '.'
            });
        }
        out.push_str("|\n");
    }
    if total_rows > max_rows {
        out.push_str(&format!(
            "… {} more bytes ({} total)\n",
            bytes.len() - max_rows * 16,
            bytes.len()
        ));
    }
    out
}
