# data/ — object type-id registries

Flat `{ "<id>": "MessageName" }` JSON tables mapping `TSP.MessageInfo.type`
ids to protobuf message names, embedded into the `iwadump` binary with
`include_str!` and parsed by `src/registry.rs`.

| file | entries | covers |
|---|---|---|
| `Common.json` | 337 | shared TSP/TSK/TST/TSD/TSWP/TSCE range (ids 200+) |
| `Keynote.json` | 68 | `KN.*` app-specific ids |
| `Numbers.json` | 31 | `TN.*` app-specific ids |
| `Pages.json` | 47 | `TP.*` app-specific ids |

The effective per-app table is `Common` + that app's JSON. App-specific ids
overlap across apps (ids 1, 2, 3, 7 collide between KN/TN/TP with different
names), so `iwadump` only applies an app table after detecting the app from
the document's own IWA content (`src/dump.rs::detect_app`); with no detected
app, an id whose name differs across tables stays **unknown** — ids are
displayed as opaque `unknown:0x…`, never guessed (docs/format/registry.md).

## Provenance (record per AGENTS.md attribution culture)

- Source repo: `dunhamsteve/iwork`, path `codegen/*.json`,
  commit `02c26ebf`, license MIT. Files copied verbatim on 2026-08-28.
  https://github.com/dunhamsteve/iwork
- Original data: `obriensp/iWorkFileFormat` — the tables were extracted from
  **Keynote 6.0, Pages 5.0, Numbers 3.0** by attaching to the running apps
  with F-Script (`TSPRegistry sharedRegistry`), per the original README
  (quoted in `codegen/README.md` upstream). dunhamsteve only re-formatted
  them as valid JSON.
- Age caveat: these tables anchor to 2013-era app versions; ids drift across
  versions (docs/format/registry.md drift analysis: 13 KN renames between the
  dunhamsteve tables and keynote-parser 14.4). Unknown ids stay opaque.
