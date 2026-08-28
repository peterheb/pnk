//! iwadump CLI — iWork '13+ file structure inspector.
//!
//! Exit codes: 0 = dumped, 1 = rejected file (legacy / encrypted / corrupt /
//! unsupported), 2 = usage error (clap default).

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

use iwadump::proto::{self, Field, Value};
use iwadump::{App, Document, Error, Kind, Layer, MessageStatus};

#[derive(Parser)]
#[command(
    name = "iwadump",
    version,
    about = "Dump the structure of an iWork '13+ document (.pages/.numbers/.key)"
)]
struct Args {
    /// Document: flat .pages/.numbers/.key file or package directory
    file: PathBuf,

    /// List container members only (no IWA decode)
    #[arg(long)]
    list: bool,

    /// Decode a single IWA stream (member name, or a unique suffix like `Document.iwa`)
    #[arg(long)]
    archive: Option<String>,

    /// Dump one payload as hex + best-effort field walk, by archive local id
    #[arg(long)]
    message: Option<u64>,

    /// Machine-readable JSON output
    #[arg(long)]
    json: bool,

    /// Limit messages listed per stream (default: all)
    #[arg(long)]
    limit: Option<usize>,

    /// List legacy (pre-'13) files as raw zip members instead of rejecting
    #[arg(long)]
    legacy_ok: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("iwadump: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(args: &Args) -> Result<String, Error> {
    if args.list {
        let container = iwadump::Container::open(&args.file, args.legacy_ok)?;
        let mut out = String::new();
        if container.form == iwadump::ContainerForm::LegacyRaw {
            out.push_str("legacy document — raw zip member listing (--legacy-ok):\n");
        }
        for m in &container.members {
            out.push_str(&format!("{:>10}  {}\n", m.size, m.name));
        }
        if let Some(nested) = &container.nested_members {
            out.push_str("nested Index.zip members:\n");
            for m in nested {
                out.push_str(&format!("{:>10}  {}\n", m.size, m.name));
            }
        }
        return Ok(out);
    }

    let doc = Document::open(&args.file, args.legacy_ok)?;

    if let Some(id) = args.message {
        return render_message(&doc, id);
    }

    if let Some(sel) = &args.archive {
        let matches: Vec<_> = doc
            .streams
            .iter()
            .filter(|s| s.name == *sel || s.name.ends_with(&format!("/{sel}")) || s.name.ends_with(sel))
            .collect();
        if matches.is_empty() {
            return Err(Error::new(
                Kind::Unsupported,
                Layer::Container,
                format!(
                    "no IWA stream matches `{sel}` (have: {})",
                    doc.streams.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", ")
                ),
            ));
        }
        let sub = Document {
            path: doc.path.clone(),
            form: doc.form,
            container: doc.container.clone(),
            streams: matches.into_iter().cloned().collect(),
            app: doc.app,
            registry: doc.registry.clone(),
        };
        return Ok(if args.json { sub.render_json(args.limit) } else { sub.render_tree(args.limit) });
    }

    if args.json {
        Ok(doc.render_json(args.limit) + "\n")
    } else {
        Ok(doc.render_tree(args.limit))
    }
}

fn render_message(doc: &Document, id: u64) -> Result<String, Error> {
    let (stream, msg) = doc.find_message(id).ok_or_else(|| {
        Error::new(
            Kind::Unsupported,
            Layer::Message,
            format!("no message with local id {id} in any stream"),
        )
    })?;
    let mut out = String::new();
    out.push_str(&format!("stream: {}\n", stream.name));
    out.push_str(&format!("local id: {}\n", msg.local_id));
    out.push_str(&format!("type: {} ({})\n", msg.type_id, msg.display_name()));
    out.push_str(&format!("payload length: {} bytes\n", msg.length));
    match &msg.status {
        MessageStatus::Decoded { name } => out.push_str(&format!("status: decoded ({name})\n")),
        MessageStatus::UnknownType => {
            out.push_str("status: unknown type id — payload kept as opaque bytes\n")
        }
        MessageStatus::Undecodable { name, reason } => {
            out.push_str(&format!("status: undecodable ({name}): {reason}\n"))
        }
    }
    out.push_str("\nhex:\n");
    out.push_str(&iwadump::dump::hex_dump(&msg.payload, 64));
    out.push_str("\nbest-effort field walk:\n");
    match proto::parse_fields(&msg.payload, Layer::Message) {
        Ok(fields) => {
            if fields.is_empty() {
                out.push_str("  (no fields)\n");
            }
            for f in &fields {
                walk_field(&mut out, f, 1);
            }
        }
        Err(e) => out.push_str(&format!("  (payload does not walk cleanly: {})\n", e.message)),
    }
    Ok(out)
}

fn walk_field(out: &mut String, f: &Field, depth: usize) {
    let indent = "  ".repeat(depth);
    let wire = proto::wire_name(f.wire);
    match &f.value {
        Value::Varint(v) => out.push_str(&format!("{indent}{:>3}: {} = {v}\n", f.number, wire)),
        Value::Fixed32(b) => {
            out.push_str(&format!("{indent}{:>3}: fixed32 = 0x{:08x}\n", f.number, u32::from_le_bytes(*b)))
        }
        Value::Fixed64(b) => out.push_str(&format!(
            "{indent}{:>3}: fixed64 = 0x{:016x}\n",
            f.number,
            u64::from_le_bytes(*b)
        )),
        Value::Bytes(b) => {
            // Try a nested message; if it walks cleanly, show it nested.
            match proto::parse_fields(b, Layer::Message) {
                Ok(inner) if !inner.is_empty() => {
                    out.push_str(&format!(
                        "{indent}{:>3}: len ({} B) → nested message:\n",
                        f.number,
                        b.len()
                    ));
                    for g in &inner {
                        walk_field(out, g, depth + 1);
                    }
                }
                _ => {
                    let text = std::str::from_utf8(b).ok().filter(|s| {
                        !s.is_empty() && s.chars().all(|c| c.is_ascii_graphic() || c == ' ')
                    });
                    match text {
                        Some(s) => out.push_str(&format!(
                            "{indent}{:>3}: len ({} B) = \"{}\"\n",
                            f.number,
                            b.len(),
                            truncate(s, 60)
                        )),
                        None => out.push_str(&format!(
                            "{indent}{:>3}: len ({} B) = {}\n",
                            f.number,
                            b.len(),
                            short_hex(b)
                        )),
                    }
                }
            }
        }
        Value::Group(inner) => {
            out.push_str(&format!("{indent}{:>3}: group:\n", f.number));
            for g in inner {
                walk_field(out, g, depth + 1);
            }
        }
    }
}

fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn short_hex(b: &[u8]) -> String {
    if b.len() <= 8 {
        b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
    } else {
        format!("{} B [{:02x} {:02x} …]", b.len(), b[0], b[1])
    }
}

const _: () = {
    // Silence unused-import churn while flags evolve; App is used by --json
    // output via Document.
    let _ = App::Unknown;
};
