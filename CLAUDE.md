# CLAUDE.md — pnk (Claude Code entry point)

@AGENTS.md

## Claude-session conventions (established 2026-08-29/30)

- **Git** (PR workflow since v0.2, 2026-09-02): branch per piece of work,
  commit small + educational on the branch, `git push -u origin <branch>`,
  then `gh pr create` into `main`. Never rebase, force-push, stash, or
  `git add -A`; stage files by explicit path. The hackathon-era
  "commit straight to main" rule in AGENTS.md is historical.
- **Build**: `cargo build --release -p pnk2json` (native converter the harness
  uses) and `bash scripts/build_viewer.sh` (wasm + viewer dist). TS-only
  changes need only build_viewer.sh.
- **Ground truth loop**: `uv run --with pillow --with pyobjc-framework-Quartz
  --with pymupdf python3 scripts/visual_diff.py --app {pages|numbers|keynote}
  --fixture <file> --out <dir> [--base-url http://127.0.0.1:<port>]` — opens a
  renamed COPY in the real app in the background (no focus steal), exports
  PDF, renders our viewer via Playwright, writes side-by-side composites.
  Judge composites BY EYE (Read the PNGs). `--skip-apple` = embedded-preview
  fallback for quick iteration. Concurrent agents use distinct ports.
- **Gates before a final commit**: `python3 scripts/conformance.py` (all
  fixtures must convert, no panics), `cd viewer && npm test` (strict tsc +
  Playwright 6/6), `cargo test -p pnk2json --release` (includes byte-value
  golden tests).
- **Golden guard**: converter changes that alter G1/G2/G5 output require
  visual verification FIRST, then re-sync `fixtures/golden/expected/*.json`
  in the same commit.
- **Model stewardship**: `model/src/*.ts` + `crates/pnk2json/src/model.rs`
  (and ctx.rs/loader.rs/lib.rs) change only through the orchestrating session
  — subagents send a proposal (field, why, shape, proof fixture) and keep
  working; the steward lands TS + serde in sync, additive-only. Design
  rulings live in `docs/model-review.md` — headline: **the viewer never walks
  an inheritance chain**; everything is resolved at emission.
- **Multi-agent file ownership** (when subagents run concurrently): split by
  app+layer — K: keynote.rs/tsd.rs/drawables.rs/colors.rs + viewer
  keynote.ts/drawables.ts; N: numbers.rs/tables.rs/charts.rs + viewer
  numbers.ts/tables.ts; P: pages.rs/text.rs/styles.rs + viewer
  pages.ts/text.ts. viewer/styles.css is append-only under a per-agent
  marker; main.ts/index.html belong to the orchestrator.
- **Golden fixtures**: hand-built by Peter in the real apps from checklists in
  `fixtures/golden/G*-checklist.md` (one feature per item). Ask for a new
  fixture by writing a checklist, don't wait blocked on it.
