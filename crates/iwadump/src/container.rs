//! Container layer: turn a filesystem path into iWork '13+ IWA streams.
//!
//! Two physical forms exist (docs/format/container.md):
//! - **package directory**: `Index.zip` at the bundle root holds the `.iwa` streams;
//!   `Metadata/`, `Data/`, previews are real files beside it.
//! - **flat file**: the `.pages`/`.numbers`/`.key` file IS a ZIP holding everything.
//!   Two flat sub-variants: `.iwa` members directly under `Index/`, or a nested
//!   member literally named `Index.zip` (early '13) that itself is a ZIP of the
//!   `.iwa` members — a zip inside a zip (docs/format/gotchas.md #5).
//!
//! Rejections happen at open time, before any IWA work (docs/format/legacy.md):
//! `.iwph` member → encrypted; `index.xml` / `index.apxl` / `index.numbers` /
//! `index.db` / `index.xml.gz` / `*-tef` → legacy pre-'13; no IWA payload at all
//! → unsupported.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{Error, Kind, Layer};

/// How the container was physically laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerForm {
    /// The input file is itself the ZIP; `.iwa` members read directly.
    FlatZip,
    /// The input is a directory; `Index.zip` inside it holds the IWA streams.
    PackageDir,
    /// A flat ZIP that carries a nested `Index.zip` member (early '13 variant).
    FlatZipNested,
    /// Legacy input opened with `--legacy-ok`: listed as a raw ZIP, not decoded.
    LegacyRaw,
}

/// LZFSE block magic prefix ("bvx…"): newer-iWork `OperationStorage.iwa`
/// streams start with one of `bvx-` (uncompressed block) / `bvx1` / `bvx2` /
/// `bvxn` (end marker) — a collaboration operation log, not an IWA snappy
/// stream. Observed in 10 of the 968 modern fixtures (8× `bvx-`, 2× `bvxn`).
const OPERATION_STORAGE_MAGIC: &[u8] = b"bvx";

/// One entry of the container's member listing. Names are UTF-8: the zip crate
/// re-decodes cp437 names when the ZIP UTF-8 flag is absent (the cp437 hazard
/// in docs/format/container.md), so display names are safe to print.
#[derive(Debug, Clone)]
pub struct Member {
    pub name: String,
    pub size: u64,
    pub compressed_size: u64,
}

impl std::fmt::Debug for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Container")
            .field("form", &self.form)
            .field("members", &self.members.len())
            .field("iwas", &self.iwas.iter().map(|(n, b)| (n, b.len())).collect::<Vec<_>>())
            .field("nested", &self.nested_members.is_some())
            .field("source", &self.source.is_some())
            .finish()
    }
}

/// Retained container source for non-IWA member reads (`Metadata/`, `Data/`):
/// the flat file's raw bytes or the package directory root. Package-form
/// members are real files beside `Index.zip` (docs/format/container.md).
#[derive(Clone, Debug)]
pub enum Source {
    /// Full bytes of the flat-file ZIP (outer zip; also the FlatZipNested case).
    Flat(Vec<u8>),
    /// Package directory root.
    Dir(PathBuf),
}

 /// An opened iWork '13+ container: IWA streams ready for decode, plus the raw
#[derive(Clone)]
pub struct Container {
    pub form: ContainerForm,
    /// Member listing shown for `--list` (outer zip, or package `Index.zip`).
    pub members: Vec<Member>,
    /// IWA stream members in stream order: (member name, raw bytes).
    pub iwas: Vec<(String, Vec<u8>)>,
    /// `.iwa`-named members skipped as non-IWA (OperationStorage `bvxn`):
    /// (member name, size). Shown in dumps, never decoded.
    pub non_iwa: Vec<(String, u64)>,
    /// When the early-'13 nested `Index.zip` variant supplied the streams, the
    /// nested zip's own member listing.
    pub nested_members: Option<Vec<Member>>,
    /// Retained source for `read_member`. `None` only for `LegacyRaw` listings.
    pub source: Option<Source>,
}

/// Result of walking one ZIP: member listing, decodable IWA streams, and any
/// non-IWA members skipped by magic.
struct ScanOutcome {
    members: Vec<Member>,
    iwas: Vec<(String, Vec<u8>)>,
    non_iwa: Vec<(String, u64)>,
    nested_members: Option<Vec<Member>>,
}

/// Member base names that mark a pre-'13 legacy document
/// (docs/format/legacy.md §3/§4).
const LEGACY_MARKERS: [&str; 5] = [
    "index.xml",     // Pages '09 flat zip
    "index.xml.gz",  // Pages '08 bundle
    "index.apxl",    // legacy Keynote package
    "index.numbers", // legacy Numbers package
    "index.db",      // iOS .pages-tef sqlite bundle
];

/// Is `name` an IWA stream? Reference parsers suffix-match (keynote-parser:
/// `".iwa" in filename`; litchi: ends-with); we require the suffix and skip
/// directory entries (trailing `/`).
fn is_iwa(name: &str) -> bool {
    !name.ends_with('/') && name.to_ascii_lowercase().ends_with(".iwa")
}

fn is_legacy_marker(name: &str) -> bool {
    let base = name.rsplit('/').next().unwrap_or(name).to_lowercase();
    LEGACY_MARKERS.contains(&base.as_str())
}

/// Treat any `pages|key|numbers -tef` suffix as legacy; do not special-case the
/// app (docs/format/legacy.md signal 1 — the TrimSuffix in dunhamsteve's index.go
/// is format-agnostic, but only -tef suffixes on the three iWork extensions are
/// attested).
fn ext_is_tef(ext: &str) -> bool {
    match ext.rsplit_once('-') {
        Some((base, "tef")) => matches!(base, "pages" | "key" | "numbers"),
        _ => false,
    }
}

/// Per-member inflation ceiling. Real members top out in the hundreds of MB.
const MAX_MEMBER_BYTES: u64 = 1024 * 1024 * 1024;

fn read_zip_member<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    index: usize,
) -> Result<Vec<u8>, Error> {
    let mut f = archive.by_index(index).map_err(|e| {
        Error::new(Kind::Corrupt, Layer::Container, format!("cannot open container member: {e}"))
    })?;
    // The declared size is untrusted input twice over: don't pre-allocate
    // from it, and don't let the actual stream inflate past the ceiling
    // either — a bomb can declare small and expand huge (FINDINGS.md H-2).
    if f.size() > MAX_MEMBER_BYTES {
        return Err(Error::new(
            Kind::Corrupt,
            Layer::Container,
            format!("container member declares {} bytes (limit {MAX_MEMBER_BYTES})", f.size()),
        ));
    }
    let mut buf = Vec::with_capacity((f.size().min(16 * 1024 * 1024)) as usize);
    let mut limited = (&mut f).take(MAX_MEMBER_BYTES + 1);
    limited.read_to_end(&mut buf).map_err(|e| {
        Error::new(Kind::Corrupt, Layer::Container, format!("cannot read container member: {e}"))
    })?;
    if buf.len() as u64 > MAX_MEMBER_BYTES {
        return Err(Error::new(
            Kind::Corrupt,
            Layer::Container,
            format!("container member inflates past {MAX_MEMBER_BYTES} bytes"),
        ));
    }
    Ok(buf)
}

fn not_a_zip(label: &str, e: impl std::fmt::Display) -> Error {
    Error::new(Kind::Corrupt, Layer::Container, format!("{label}: not a readable ZIP container: {e}"))
}

/// Walk one ZIP held in memory: reject encrypted/legacy, then collect its
/// `.iwa` members; if there are none, recurse into a nested member named
/// `Index.zip` (any directory prefix), mirroring numbers-parser iwork.py:216-218.
/// Returns (member listing, iwa streams, nested listing when used).
fn scan_zip(bytes: Vec<u8>, label: &str) -> Result<ScanOutcome, Error> {
    scan_zip_at_depth(bytes, label, 0)
}

/// The early-'13 variant nests exactly ONE `Index.zip` inside the document
/// zip; a crafted chain of nested index zips must not recurse further
/// (FINDINGS.md H-4).
const MAX_NESTED_INDEX_DEPTH: u32 = 1;

fn scan_zip_at_depth(bytes: Vec<u8>, label: &str, depth: u32) -> Result<ScanOutcome, Error> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| not_a_zip(label, e))?;
    let members: Vec<Member> = (0..archive.len())
        .filter_map(|i| {
            let f = archive.by_index_raw(i).ok()?;
            Some(Member {
                name: f.name().to_string(),
                size: f.size(),
                compressed_size: f.compressed_size(),
            })
        })
        .collect();

    // Encrypted documents: `.iwph` is the classic password-protection marker
    // (numbers-parser iwork.py:205-212 rejects it the same way; real fixtures
    // carry `.iwph` alongside otherwise-readable members). `.iwpv2` marks the
    // newer iWork encryption — fixture 2dccc804 has it with every stream but
    // DocumentStylesheet high-entropy ciphertext and no `.iwph` present.
    for marker in [".iwph", ".iwpv2"] {
        if let Some(name) = members
            .iter()
            .map(|m| m.name.as_str())
            .find(|n| n.to_lowercase().ends_with(marker) && !n.ends_with('/'))
        {
            return Err(Error::new(
                Kind::Encrypted,
                Layer::Container,
                format!(
                    "{label}: encrypted iWork document (member `{name}`) — password-protected files are not supported"
                ),
            ));
        }
    }

    // Legacy pre-'13 signals: legacy zips unzip fine but carry an XML/apxl/
    // sqlite index instead of the IWA object database (docs/format/gotchas.md #9).
    if let Some(name) = members.iter().map(|m| m.name.as_str()).find(|n| is_legacy_marker(n)) {
        return Err(Error::new(
            Kind::Legacy,
            Layer::Container,
            format!(
                "{label}: legacy iWork document (pre-'13 index member `{name}`) — re-save with a current version of Pages, Numbers, or Keynote"
            ),
        ));
    }

    let mut iwas = Vec::new();
    let mut non_iwa = Vec::new();
    for (i, m) in members.iter().enumerate() {
        if is_iwa(&m.name) {
            let bytes = read_zip_member(&mut archive, i)?;
            // `OperationStorage.iwa` (newer iWork) is NOT an IWA snappy
            // stream: it carries the collaboration operation log behind an
            // LZFSE-style `bvxn` magic, and postdates all four reference
            // parsers. Skip it by magic, visibly, never decode it as IWA.
            if bytes.starts_with(OPERATION_STORAGE_MAGIC) {
                non_iwa.push((m.name.clone(), m.size));
            } else {
                iwas.push((m.name.clone(), bytes));
            }
        }
    }

    if iwas.is_empty() {
        // named `Index.zip` that is itself a zip of the `.iwa` members —
        // gotcha #5, "a zip inside a zip".
        let nested = (0..archive.len()).find(|&i| {
            archive
                .name_for_index(i)
                .map(|n| {
                    !n.ends_with('/')
                        && n.rsplit('/').next().map(|b| b.to_lowercase()).as_deref() == Some("index.zip")
                })
                .unwrap_or(false)
        });
        if let Some(idx) = nested {
            if depth >= MAX_NESTED_INDEX_DEPTH {
                return Err(Error::new(
                    Kind::Corrupt,
                    Layer::Container,
                    format!("{label}: Index.zip nests deeper than the format allows"),
                ));
            }
            let nested_name = archive.name_for_index(idx).unwrap_or("Index.zip").to_string();
            let nested_bytes = read_zip_member(&mut archive, idx)?;
            let inner = scan_zip_at_depth(nested_bytes, &format!("{label} → {nested_name}"), depth + 1)?;
            return Ok(ScanOutcome {
                members,
                iwas: inner.iwas,
                non_iwa: inner.non_iwa,
                nested_members: Some(inner.members),
            });
        }
    }

    Ok(ScanOutcome { members, iwas, non_iwa, nested_members: None })
}

impl Container {
    /// Open a document path (flat file or package directory) and extract its
    /// IWA streams. Legacy and encrypted inputs are rejected with a message
    /// naming the signal; `legacy_ok` downgrades legacy flat files to a raw
    /// member listing instead of rejecting.
    pub fn open(path: &Path, legacy_ok: bool) -> Result<Container, Error> {
        if path.is_dir() {
            return Self::open_package_dir(path);
        }
        let bytes = fs::read(path).map_err(|e| {
            Error::new(Kind::Io, Layer::Container, format!("cannot read {}: {e}", path.display()))
        })?;
        let label = path.display().to_string();

        // Legacy signal that needs no ZIP parse: the `-tef` iOS bundle suffix
        // (docs/format/legacy.md signal 1). Extension `pages-tef` arrives here
        // as a single `Extension` component `pages-tef`? No: `Path::extension`
        // yields `pages-tef` for `doc.pages-tef`.
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if ext_is_tef(&ext.to_lowercase()) {
                return Err(Error::new(
                    Kind::Legacy,
                    Layer::Container,
                    format!(
                        "{label}: iOS iWork bundle (*-tef) is not supported — open and re-save in a current version of Pages"
                    ),
                ));
            }
        }

        // Retained so `read_member` can fetch non-IWA members (Metadata/, Data/)
        // without re-reading the file; scan_zip consumes the bytes.
        let keep = bytes.clone();
        match scan_zip(bytes, &label) {
            Ok(outcome) if !outcome.iwas.is_empty() => {
                let has_direct = outcome.members.iter().any(|m| is_iwa(&m.name));
                let form =
                    if has_direct { ContainerForm::FlatZip } else { ContainerForm::FlatZipNested };
                Ok(Container {
                    form,
                    members: outcome.members,
                    iwas: outcome.iwas,
                    non_iwa: outcome.non_iwa,
                    nested_members: outcome.nested_members,
                    source: Some(Source::Flat(keep)),
                })
            }
            Ok(_) => Err(Error::new(
                Kind::Unsupported,
                Layer::Container,
                format!("{label}: no iWork '13+ data found (missing Index.zip / *.iwa members)"),
            )),
            Err(e) if e.kind == Kind::Legacy && legacy_ok => {
                let bytes = fs::read(path).map_err(|x| {
                    Error::new(Kind::Io, Layer::Container, format!("cannot read {}: {x}", path.display()))
                })?;
                Self::legacy_raw(bytes, label)
            }
            Err(e) => Err(e),
        }
    }

    /// Legacy downgrade (`--legacy-ok`): raw zip member listing only.
    pub fn legacy_raw(bytes: Vec<u8>, label: String) -> Result<Container, Error> {
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| not_a_zip(&label, e))?;
        let members: Vec<Member> = (0..archive.len())
            .filter_map(|i| {
                let f = archive.by_index_raw(i).ok()?;
                Some(Member {
                    name: f.name().to_string(),
                    size: f.size(),
                    compressed_size: f.compressed_size(),
                })
            })
            .collect();
        Ok(Container {
            form: ContainerForm::LegacyRaw,
            members,
            iwas: Vec::new(),
            non_iwa: Vec::new(),
            nested_members: None,
            source: None,
        })
    }

    fn open_package_dir(dir: &Path) -> Result<Container, Error> {
        let index_zip = dir.join("Index.zip");
        if !index_zip.is_file() {
            // Directory without Index.zip: legacy bundle signals (legacy.md §4).
            for name in ["index.xml.gz", "index.xml", "index.db", "index.apxl", "index.numbers"] {
                if dir.join(name).exists() {
                    return Err(Error::new(
                        Kind::Legacy,
                        Layer::Container,
                        format!(
                            "{}: legacy iWork document (bundle marker `{name}`) — re-save with a current version of Pages, Numbers, or Keynote",
                            dir.display()
                        ),
                    ));
                }
            }
            return Err(Error::new(
                Kind::Unsupported,
                Layer::Container,
                format!(
                    "{}: not an iWork '13+ package (no Index.zip, no legacy markers)",
                    dir.display()
                ),
            ));
        }
        let bytes = fs::read(&index_zip).map_err(|e| {
            Error::new(Kind::Io, Layer::Container, format!("cannot read {}: {e}", index_zip.display()))
        })?;
        let outcome = scan_zip(bytes, &index_zip.display().to_string())?;
        if outcome.iwas.is_empty() {
            return Err(Error::new(
                Kind::Unsupported,
                Layer::Container,
                format!("{}: Index.zip holds no `*.iwa` members", dir.display()),
            ));
        }
        Ok(Container {
            form: ContainerForm::PackageDir,
            members: outcome.members,
            iwas: outcome.iwas,
            non_iwa: outcome.non_iwa,
            nested_members: outcome.nested_members,
            source: Some(Source::Dir(dir.to_path_buf())),
        })
    }

    /// Read one non-IWA member by name (e.g. `Metadata/Properties.plist`,
    /// `Data/photo.jpg`). Flat forms read the outer ZIP; under the early-'13
    /// nested `Index.zip` variant a member the outer zip hides is looked up in
    /// the nested zip. Package dirs read the real file beside `Index.zip`.
    pub fn read_member(&self, name: &str) -> Result<Vec<u8>, Error> {
        match &self.source {
            Some(Source::Flat(bytes)) => {
                match Self::member_from_zip(bytes, name) {
                    Ok(bytes) => Ok(bytes),
                    Err(first) if self.form == ContainerForm::FlatZipNested => {
                        let Some(nested_name) = self
                            .members
                            .iter()
                            .find(|m| m.name.ends_with("Index.zip"))
                            .map(|m| m.name.clone())
                        else {
                            return Err(first);
                        };
                        let nested = Self::member_from_zip(bytes, &nested_name)?;
                        Self::member_from_zip(&nested, name).map_err(|_| first)
                    }
                    Err(e) => Err(e),
                }
            }
            Some(Source::Dir(root)) => {
                // Containment (FINDINGS.md M-1): `name` is document-derived
                // (DataInfo.file_name) — an absolute path, `..`, or a
                // symlink pointing outside the package must never disclose
                // files beyond the package root.
                let rel = sanitized_member_path(name).ok_or_else(|| {
                    Error::new(
                        Kind::Io,
                        Layer::Container,
                        format!("member {name}: not a plain relative member name"),
                    )
                })?;
                let io_err = |e: std::io::Error| {
                    Error::new(Kind::Io, Layer::Container, format!("member {name}: {e}"))
                };
                let canon_root = root.canonicalize().map_err(io_err)?;
                let canon = root.join(rel).canonicalize().map_err(io_err)?;
                if !canon.starts_with(&canon_root) || !canon.is_file() {
                    return Err(Error::new(
                        Kind::Io,
                        Layer::Container,
                        format!("member {name}: escapes the package root or is not a regular file"),
                    ));
                }
                fs::read(canon).map_err(io_err)
            }
            None => Err(Error::new(
                Kind::Io,
                Layer::Container,
                "no retained source (legacy raw listing only)".to_string(),
            )),
        }
    }

    /// Whether a member exists, WITHOUT inflating it (flat forms check the
    /// central-directory listing; package dirs stat the contained path).
    pub fn has_member(&self, name: &str) -> bool {
        match &self.source {
            Some(Source::Flat(_)) => {
                let suffix = format!("/{name}");
                let in_list = |ms: &[Member]| {
                    ms.iter().any(|m| m.name == name || m.name.ends_with(&suffix))
                };
                in_list(&self.members)
                    || self.nested_members.as_deref().is_some_and(in_list)
            }
            Some(Source::Dir(root)) => {
                let Some(rel) = sanitized_member_path(name) else { return false };
                let Ok(canon_root) = root.canonicalize() else { return false };
                root.join(rel)
                    .canonicalize()
                    .is_ok_and(|c| c.starts_with(&canon_root) && c.is_file())
            }
            None => false,
        }
    }

    /// Read one member from in-memory ZIP bytes: exact name first, then a
    /// directory-prefix-insensitive suffix match (`Data/x.jpg` vs `x.jpg`).
    fn member_from_zip(bytes: &[u8], name: &str) -> Result<Vec<u8>, Error> {
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
            .map_err(|e| not_a_zip("member source", e))?;
        let mut exact = None;
        let mut suffix = None;
        for i in 0..archive.len() {
            let Ok(f) = archive.by_index_raw(i) else { continue };
            let fname = f.name().to_string();
            if fname == name {
                exact = Some(i);
                break;
            }
            if suffix.is_none() && fname.ends_with(&format!("/{name}")) {
                suffix = Some(i);
            }
        }
        let index = exact.or(suffix).ok_or_else(|| {
            Error::new(Kind::Io, Layer::Container, format!("member not found: {name}"))
        })?;
        read_zip_member(&mut archive, index)
    }
}

/// Validate a document-derived member name for filesystem use: only plain
/// relative path components — no root, no drive prefix, no `..`, no `.`.
fn sanitized_member_path(name: &str) -> Option<std::path::PathBuf> {
    use std::path::Component;
    let p = std::path::Path::new(name);
    let mut out = std::path::PathBuf::new();
    for c in p.components() {
        match c {
            Component::Normal(part) => out.push(part),
            _ => return None,
        }
    }
    (!out.as_os_str().is_empty()).then_some(out)
}

#[cfg(test)]
mod member_tests {
    use super::*;
    use std::io::Write as _;

    /// Hermetic early-'13 nested fixture: outer zip = `Index.zip` (one fake
    /// .iwa inside) + `Metadata/Properties.plist`. Streams must come from the
    /// nested member; `read_member` must fetch the plist from the outer zip.
    fn nested_fixture_bytes() -> Vec<u8> {
        let mut inner = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut inner));
            w.start_file("Index/Document.iwa", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(b"not-really-snappy").unwrap();
            w.finish().unwrap();
        }
        let mut outer = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut outer));
            w.start_file("Index.zip", zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(&inner).unwrap();
            w.start_file("Metadata/Properties.plist", zip::write::SimpleFileOptions::default())
                .unwrap();
            w.write_all(b"<plist/>").unwrap();
            w.finish().unwrap();
        }
        outer
    }

    #[test]
    fn read_member_reads_outer_zip_in_nested_variant() {
        let dir = std::env::temp_dir().join(format!("iwadump-member-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.key");
        std::fs::write(&path, nested_fixture_bytes()).unwrap();
        let c = Container::open(&path, false).unwrap();
        assert_eq!(c.form, ContainerForm::FlatZipNested);
        assert_eq!(c.read_member("Metadata/Properties.plist").unwrap(), b"<plist/>");
        assert!(c.read_member("Data/missing.jpg").is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
