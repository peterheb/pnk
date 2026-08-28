# iWork File Format Reference — INDEX

Reference documentation for the Apple iWork '13+ file format (`.pages`,
`.numbers`, `.key`), as used by the **pnk** viewer. Built during phase 1 from
primary sources: protobuf definitions extracted from the locally installed
15.3.1 apps (`npx otorp`, in `.scratch/otorp/`, gitignored) and four
license-compatible third-party parsers (checkouts in `.scratch/`, gitignored).

**Provenance.** Every claim carries one of:
- `[proto]` — verified in a `.proto` file (path + message/field cited),
- `[parser: <repo>@<sha>]` — confirmed in third-party parser code (file cited),
- `[inferred: reason]` — our reasoning, not yet fixture-verified.

Sources, licenses, and commit SHAs: [ATTRIBUTION.md](ATTRIBUTION.md).
Known gotchas live in [gotchas.md](gotchas.md) — read it before writing parser code.

## Reading order

1. **[container.md](container.md)** — the ZIP container: package dirs vs flat
   ZIPs, `Index.zip` (nested zip variant), `Metadata/Properties.plist`,
   `BuildVersionHistory.plist`, previews, encrypted `.iwph` marker.
2. **[iwa.md](iwa.md)** — `.iwa` stream framing: Snappy blocks
   (`00` + u24 LE compressed length, raw Snappy, **not** framing format).
3. **[objects.md](objects.md)** — the object database: varint + `TSP.ArchiveInfo`
   envelope, `MessageInfo`, `TSP.Reference` object graph, `LargeArray` segments,
   `TST.TileStorage`, global vs local ids.
4. **[registry.md](registry.md)** — object type-id registries (dunhamsteve JSON
   tables, keynote-parser per-version registries), drift analysis between them,
   and the 15.3.1 no-registry risk.
5. **[gotchas.md](gotchas.md)** — ten traps: the wrong "u16+u16" header folklore,
   the nonexistent `TSP.PrefixedMessage`, otorp vs chained fixups, and more.

## Domain namespaces

- **[text.md](text.md)** — TSWP text model: `StorageArchive`, attribute tables,
  paragraphs/runs/styles, attachments (U+FFFC), fields, UTF-16 offset semantics.
- **[styles.md](styles.md)** — TSS styles: `StyleArchive` parent chains, property
  maps, themes and per-app preset extensions.
- **[tables.md](tables.md)** — TST tables: `TableArchive`, tiles/rows, RB-trees,
  cell values, merges.
- **[drawables.md](drawables.md)** — TSD drawables: `DrawableArchive` base +
  Shape/Image/Mask/Movie/Group subclasses, media references.
- **[charts.md](charts.md)** — TSCH chart model (2D/3D), series/categories,
  mediator chain to table data. Rendering explicitly deferred.
- **[calcengine.md](calcengine.md)** — TSCE formula archives as stored ASTs;
  why the calc engine is out of viewer scope.
- **[media.md](media.md)** — embedded media: `Data/` members, `TSP.DataInfo`
  registry, `DataReference` resolution chains.

## Per-app document trees

- **[pages.md](pages.md)** — Pages: `TP.DocumentArchive` → sections/page
  templates → drawables; word-processing vs page-layout mode.
- **[numbers.md](numbers.md)** — Numbers: `TN.DocumentArchive` → sheets →
  tables/drawables; how numbers-parser walks it.
- **[keynote.md](keynote.md)** — Keynote: `KN.DocumentArchive` → show → slide
  tree → slides, builds, placeholders, notes, themes.

## Operational topics

- **[incremental.md](incremental.md)** — incremental saves: patch messages
  (`should_merge`, `base_message_index`), command archives (undo/redo), how a
  viewer can safely ignore them.
- **[legacy.md](legacy.md)** — pre-'13 formats (Pages '08/'09, `.pages-tef`):
  detection signals and rejection guidance. Out of scope otherwise.
- **[ATTRIBUTION.md](ATTRIBUTION.md)** — every reference repo with URL, commit
  SHA, and license; the otorp fallback note; libetonyek (MPL-2.0) consult-only policy.

## Source checkouts (gitignored, `.scratch/`)

| checkout | what we use it for | provenance prefix |
| --- | --- | --- |
| `.scratch/iwork/` | protos + primary id registry JSONs | `[parser: iwork@02c26ebf]` |
| `.scratch/litchi/` | Rust cross-check parser | `[parser: litchi@92293640]` |
| `.scratch/numbers-parser/` | Numbers behavior | `[parser: numbers-parser@32387958]` |
| `.scratch/keynote-parser/` | Keynote behavior, per-version registries | `[parser: keynote-parser@56a4d3b0]` |
| `.scratch/otorp/<App>/` | 15.3.1 protos (no registry) | `[proto]` |

Regenerate the checkouts + otorp extraction any time with
`python3 scripts/docs_fetch_sources.py` (idempotent).
