# AGENTS.md — pnk

Client-side Apple iWork (`.pages` / `.numbers` / `.key`) document viewer, built for
[Hackyard Yard #1](https://hackyard.tech/yards/yard-1). Everything runs in the browser:
drop a file, parse it to JSON in-page via Rust→WASM, render it. No backend, no login, no
email, no upload.

## Hackathon constraints

- Theme "no accounts": zero sign-up / login / email to use the product.
- 48 hours, solo: kickoff 2026-08-28 18:00 UTC, ship deadline 2026-08-30 18:00 UTC,
  voting closes 2026-09-02 18:00 UTC.
- All code written during the 48 hours; repo is public (github.com/peterheb/pnk); demo video required.
- Project license: MIT / Apache-2.0 dual. Only reference license-compatible third-party work,
  and record EVERY reference for attribution — vendored or merely browsed — including the
  exact git commit hashes (so we can debug against them six months from now).

## Repo conventions

- Commit straight to `main`. No branches, no PRs, no ceremony. Commit often, in small
  steps, with educational messages — peers will read this history.
- Provenance tagging for all format documentation: every claim gets one of
  - `proto` — structure verified in the protobuf definitions,
  - `parser` — behavior confirmed in third-party parser code (name which),
  - `inferred` — our own reasoning, not yet verified.
  Correctness beats completeness: one wrong "fact" poisons every line of code downstream.
- Layout: `crates/iwadump` (CLI structure inspector), `crates/pnk2json` (lib + wasm +
  text/markdown dumpers), `viewer/` (TS web app), `docs/format/` (format reference — start
  at `INDEX.md`), `fixtures/` (gitignored binaries; `provenance.json` is committed),
  `scripts/` (research/fetch tooling), `.scratch/` (local reference checkouts, gitignored).

## Format primer (iWork '13+)

- The file is a ZIP archive: `Index.zip` containing `Index/*.iwa` streams, QuickLook
  previews (`preview.jpg`/`png`/`pdf`), `Metadata.plist`, `buildVersionHistory.plist`, media.
- Each `.iwa` is a sequence of Snappy-compressed blocks with a 4-byte header:
  u16 LE compressed size, u16 *big-endian* uncompressed size (historical quirk), then a raw
  Snappy payload (NOT the Snappy "framing format").
- Each block is a stream of protobuf messages wrapped in a TSP envelope:
  varint length + `TSP.PrefixedMessage { 1: identifier, 2: length, 3: payload }`.
  The first message is `TSP.ArchiveInfo`, mapping local ids → global registry type ids.
- Namespaces: TSWP (text), TST (tables), TSD (drawables), TSCH (charts), TSCE (formulas),
  KN (Keynote), TSP (shared storage).
- Legacy iWork (pre-13) is out of scope: detect and reject with a clear error.
- Full reference lives in `docs/format/INDEX.md` (built in phase 1 from
  `scripts/docs_fetch_sources.py` + `npx otorp` extraction of the local apps' protos).

## Phases — each independently verifiable

0. **Env & repo** ✅ — toolchain validated/installed, repo linked to github.com/peterheb/pnk.
1. **Format docs** — `docs/format/*` from primary sources; every claim provenance-tagged.
   Gate: INDEX.md covers all topics; sources recorded with SHAs and licenses.
1b. **Fixtures** (parallel with 1) — real `.pages`/`.numbers`/`.key` from Common Crawl into
   `fixtures/` + `provenance.json` (URL, capture id, sha256). Gate: ≥5 files per format.
2. **JSON models** — `docs/MODEL.md` + TS types: document → sheets/sections → objects/tiles,
   incl. shared subobjects (picture/chart/table). Gate: iwadump output maps onto it cleanly.
3. **iwadump (Rust CLI)** — zip → snappy → protobuf → readable dump. Gate: dumps every
   fixture without panicking.
4. **pnk2json (Rust, wasm-friendly)** — typed JSON model; builds natively AND for
   `wasm32-unknown-unknown`. Gate: `cargo test` green + wasm build emits JSON for fixtures.
5. **Fallback: dump-to-text / dump-to-markdown** in pnk2json. Gate: readable markdown for
   every fixture. This is the shippable fallback if the viewer gets too ambitious.
6. **Viewer (TS + pnk2json.wasm)** — drag-drop file, render pages/sheets/slides.
   Gate: Playwright screenshot of a rendered fixture.

## Cross-validation

Real Keynote / Numbers / Pages as ground truth, installed locally. On disk they are the 2026
"Creator Studio"-era bundles (`/Applications/Keynote Creator Studio.app`, `Numbers Creator Studio.app`,
`Pages Creator Studio.app`; display names unchanged; bundle IDs moved from `com.apple.iWork.*`
to `com.apple.*` per Daring Fireball 2026-04-14 — verify in Info.plist). Open fixtures in the
apps with the `computer` tool and compare against our render, plus each file's embedded
QuickLook `preview.pdf` as an offline reference. Playwright drives viewer screenshots.

## Environment (validated 2026-08-28)

| Tool | Status |
| --- | --- |
| macOS 26.6.2 arm64, CLT, git 2.50.1 | ok |
| node 22.23.2 / npm·npx 11.19.0 | ok |
| gh 2.98.0, authed as peterheb (repo scope, https) | ok |
| uv 0.12.1 / brew 6.0.20 | ok |
| rustc·cargo 1.98.0 + wasm32-unknown-unknown | ok (rustup) |
| awscli 2.36.33 | ok — credentials NOT configured (run `aws login` or provide keys/SSO) |
| mas 7.0.0 | ok — real Keynote/Numbers/Pages 15.3.1 installed via `sudo mas install --force`; on-disk bundles are `Keynote Creator Studio.app` etc. (2026 "Creator Studio" rebrand — these ARE Apple's apps) |
| Playwright + chromium 151 | ok — smoke: `/tmp/pw-smoke/example.png` sha256 `8294b47e1b936d08f4743c826fcff63e20f943298d96d41ba2ac94e76ba406e4` |
| `computer` tool | enabled via project `.omp/config.yml`; needs `/computer on` (or new session) + Screen Recording & Accessibility grants |

## Harness notes for agents

- `computer` tool: read `omp://computer-use.md`. Prefer AX over pixels; `read_only` for inspection.
- `browser` (xd://browser) for web/DOM checks; Playwright for user-facing screenshot validation.
- Sources of truth for the format: `docs/format/` (provenance-tagged) — do not re-derive from memory.
