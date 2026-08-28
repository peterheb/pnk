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
use std::path::Path;

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

/// `OperationStorage.iwa` magic ("bvxn"): newer-iWork collaboration operation
/// log, not an IWA snappy stream. Observed in 10 of the 968 modern fixtures.
const OPERATION_STORAGE_MAGIC: &[u8] = b"bvxn";

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
            .finish()
    }
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

fn read_zip_member<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    index: usize,
) -> Result<Vec<u8>, Error> {
    let mut f = archive.by_index(index).map_err(|e| {
        Error::new(Kind::Corrupt, Layer::Container, format!("cannot open container member: {e}"))
    })?;
    let mut buf = Vec::with_capacity(f.size() as usize);
    f.read_to_end(&mut buf).map_err(|e| {
        Error::new(Kind::Corrupt, Layer::Container, format!("cannot read container member: {e}"))
    })?;
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
            let nested_name = archive.name_for_index(idx).unwrap_or("Index.zip").to_string();
            let nested_bytes = read_zip_member(&mut archive, idx)?;
            let inner = scan_zip(nested_bytes, &format!("{label} → {nested_name}"))?;
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
        })
    }
}
