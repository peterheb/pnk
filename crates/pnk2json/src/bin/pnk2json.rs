//! pnk2json CLI — iWork '13+ document → pnk JSON (or text/markdown dump).
//!
//! Exit codes: 0 = converted, 1 = rejected (legacy / encrypted / corrupt /
//! unsupported), 2 = usage error.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;

#[derive(Parser)]
#[command(
    name = "pnk2json",
    version,
    about = "Convert an iWork '13+ document (.pages/.numbers/.key) to pnk JSON, or dump it as readable text/markdown"
)]
struct Args {
    /// Document: flat .pages/.numbers/.key file or package directory
    file: PathBuf,

    /// Write JSON to this file instead of stdout
    #[arg(long, short)]
    output: Option<PathBuf>,

    /// Emit a readable plain-text dump instead of JSON
    #[arg(long)]
    text: bool,

    /// Emit a readable markdown dump instead of JSON
    #[arg(long)]
    markdown: bool,

    /// Compact JSON (no pretty-printing)
    #[arg(long)]
    compact: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(out) => {
            let result = match &args.output {
                Some(p) => std::fs::write(p, out).map_err(|e| e.to_string()),
                None => std::io::stdout().write_all(out.as_bytes()).map_err(|e| e.to_string()),
            };
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("pnk2json: write failed: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Err(e) => {
            eprintln!("pnk2json: {e}");
            ExitCode::from(1)
        }
    }
}

fn run(args: &Args) -> Result<String, String> {
    if args.text && args.markdown {
        return Err("--text and --markdown are mutually exclusive".into());
    }
    let doc = pnk2json::convert_path(&args.file).map_err(|e| e.to_string())?;
    if args.text {
        Ok(pnk2json::dumptext::to_text(&doc))
    } else if args.markdown {
        Ok(pnk2json::dumptext::to_markdown(&doc))
    } else if args.compact {
        Ok(pnk2json::to_json_compact(&doc))
    } else {
        Ok(pnk2json::to_json(&doc))
    }
}
