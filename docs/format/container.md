# ZIP container layout (.pages / .numbers / .key)

An iWork '13+ document is either a package directory or a single ZIP file; in both
forms the object database lives in `Index.zip` (or `Index/*.iwa` members), metadata
in `Metadata/*.plist`, and media in `Data/`. Per-app differences are in the IWA
payload, not the container.

## Two physical forms

- Package directory: `<Doc>.numbers/` containing `Index.zip`, `Metadata/`,
  `Data/`, previews as real files. litchi validates by requiring `Index.zip` at the
  bundle root (`[parser: litchi@92293640]` .scratch/litchi/crates/litchi-iwa/src/bundle.rs:163-177);
  numbers-parser requires `Index.zip` inside a `.numbers` directory before it will
  save (`[parser: numbers-parser@32387958]` .scratch/numbers-parser/src/numbers_parser/iwork.py:136-146).
- Flat file: the `.pages`/`.numbers`/`.key` file IS the ZIP. dunhamsteve falls back
  to opening the file itself as a zip when `<doc>/Index.zip` is not found
  (`[parser: iwork@02c26ebf]` .scratch/iwork/index/index.go:34-44); keynote-parser
  dispatches on extension — `.key` → zip reader, else directory reader
  (`[parser: keynote-parser@56a4d3b0]` .scratch/keynote-parser/keynote_parser/file_utils.py:85-88);
  litchi `Bundle::open` branches on `is_dir()` vs `is_file()`
  (`[parser: litchi@92293640]` bundle.rs:33-43).

## Member names the parsers actually look up

| Member | Read by | Citation |
|---|---|---|
| `Index.zip` (in package dir) | all four | `[parser: iwork@02c26ebf]` index.go:35; `[parser: litchi@92293640]` bundle.rs:180-189; `[parser: numbers-parser@32387958]` iwork.py:141,154,196-198 |
| `*.iwa` members (suffix match, inside Index.zip or the flat file) | Go/Rust/Python all | `[parser: iwork@02c26ebf]` index.go:86-100; `[parser: litchi@92293640]` zip_utils.rs:43-63; `[parser: keynote-parser@56a4d3b0]` file_utils.py:220 (`".iwa" in filename`); `[parser: numbers-parser@32387958]` iwork.py:227 |
| nested `Index.zip` member inside a flat ZIP | numbers-parser only | `[parser: numbers-parser@32387958]` iwork.py:216-218 (recurses into any member whose name ends with `index.zip`) |
| `Metadata/Properties.plist` | numbers-parser, litchi | `[parser: numbers-parser@32387958]` iwork.py:66,74-84; `[parser: litchi@92293640]` bundle.rs:218-227 |
| `Metadata/BuildVersionHistory.plist` | numbers-parser, litchi | `[parser: numbers-parser@32387958]` iwork.py:67,74-84; `[parser: litchi@92293640]` bundle.rs:234-238 |
| `Metadata/DocumentIdentifier` (plain text) | litchi | `[parser: litchi@92293640]` bundle.rs:242-249 |
| `Data/<file_name>` media members | numbers-parser | `[parser: numbers-parser@32387958]` cell.py:1157, model.py:2633 |
| `.iwph` member (encrypted-document marker) | numbers-parser rejects | `[parser: numbers-parser@32387958]` iwork.py:205-212 (`UnsupportedError`) |
| `preview.jpg` / `preview.png` / `preview.pdf` QuickLook previews | none of the four parse them | `[parser: litchi@92293640]` bundle.rs:7 (comment only: "Preview images at root level"); `[inferred: exact preview filenames and placement are a QuickLook convention no reference parser verifies — confirm on a fixture]` |

Note there is no `Metadata.plist` file; the metadata files are
`Metadata/Properties.plist` and `Metadata/BuildVersionHistory.plist`.

## Inside Index.zip

- IWA member paths use the `Index/` prefix and carry the object class in the name,
  e.g. `Index/Tables/Tile-{id}.iwa`, `Index/Tables/DataList-{id}.iwa`,
  `Index/Tables/HeaderStorageBucket-{id}.iwa`
  (`[parser: numbers-parser@32387958]` model.py:1284,1306,1469 — these are the names
  it *writes*), and `Index/Document.iwa`, `Index/CalculationEngine.iwa` appear in
  real documents (`[parser: keynote-parser@56a4d3b0]` codec.py:51).
- In package form, `Index.zip` contains only the `.iwa` members; every other blob
  (`Metadata/*.plist`, `Data/*`) is written as a real file in the package directory.
  numbers-parser's `save()` shows this split explicitly: IWAFile blobs go into
  `Index.zip`, everything else to `filepath / blob_path`
  (`[parser: numbers-parser@32387958]` iwork.py:153-166).
- In flat-file form the single ZIP holds everything: `Index/*.iwa`,
  `Metadata/*.plist`, `Data/*`, previews. keynote-parser and litchi stream all
  members of the outer zip; non-IWA members are passed through as raw bytes
  (`[parser: keynote-parser@56a4d3b0]` file_utils.py:92-108,194-221;
  `[parser: litchi@92293640]` bundle.rs:191-209).
- The `.iwa` payload framing (snappy blocks, protobuf ArchiveInfo) is covered in
  [iwa.md](iwa.md).

## Nested Index.zip variant

numbers-parser alone handles a flat ZIP that contains a *member* named `Index.zip`
(it re-opens that member as a zip and iterates it). The three other reference
parsers read `Index/*.iwa` members directly from the outer zip and have no nesting
code (`[parser: iwork@02c26ebf]` index.go:86-100; `[parser: keynote-parser@56a4d3b0]`
file_utils.py:92-108; `[parser: litchi@92293640]` zip_utils.rs:43-63). Both layouts
therefore exist in the wild; the nested variant is handled as backward
compatibility by numbers-parser
(`[parser: numbers-parser@32387958]` iwork.py:216-218)
`[inferred: the nested form is the early iWork '13 flat layout; verify which app
versions emit it once fixtures exist]`. Single-level unzip is a classic failure —
see [gotchas.md](gotchas.md).

## Metadata files

- `Metadata/Properties.plist`: XML plist. The one key parsers rely on is
  `fileFormatVersion` — numbers-parser's version gate reads it and warns (not
  fails) when outside `SUPPORTED_NUMBERS_VERSIONS`; a malformed plist degrades to
  an empty version with a warning (`[parser: numbers-parser@32387958]`
  iwork.py:54-92). litchi additionally surfaces `Application` from the same plist
  to detect the originating app (`[parser: litchi@92293640]` bundle.rs:226-227).
- `Metadata/BuildVersionHistory.plist`: array of build-version strings, or array of
  dicts with `Version`/`Build` keys — litchi accepts both shapes
  (`[parser: litchi@92293640]` bundle.rs:291-311). numbers-parser only requires the
  file to exist (`[parser: numbers-parser@32387958]` iwork.py:74-84).
- `Metadata/DocumentIdentifier`: plain-text document UUID, read by litchi only
  (`[parser: litchi@92293640]` bundle.rs:242-249). There is also a proto-level
  `TSP.DocumentRevision.identifier` in the object database
  (`[proto]` .scratch/otorp/Keynote/TSPArchiveMessages.proto → `TSP.PackageMetadata.revision`,
  field 2, message `TSP.DocumentRevision` at lines 125-129).

## Encrypted documents

A member named `.iwph` in the zip marks a password-protected document;
numbers-parser raises `UnsupportedError` before parsing anything else
(`[parser: numbers-parser@32387958]` iwork.py:205-212). No other reference parser
checks for it `[inferred: grep for `iwph` across all four repos returns only that
one hit]`.

## Zip entry-name encoding

Non-ASCII member names (media filenames with accented/CJK characters) are a real
hazard: keynote-parser re-decodes every entry name from cp437 to UTF-8
(`[parser: keynote-parser@56a4d3b0]` file_utils.py:93-95), and numbers-parser opts
into `metadata_encoding="utf-8"` on Python 3.11+
(`[parser: numbers-parser@32387958]` iwork.py:176-185). Reading the same document
without these workarounds yields mojibake `Data/` paths and failed media lookups.

- The container layer is app-agnostic: none of the four parsers branches on
  `.pages` vs `.numbers` vs `.key` at the ZIP level (extension checks are only
  format gates: `[parser: numbers-parser@32387958]` containers.py:65-67;
  `[parser: keynote-parser@56a4d3b0]` file_utils.py:85-88). Differences start at
  the IWA object graph — see [objects.md](objects.md) and the app-specific
  archive types in Keynote (`KN.DocumentArchive`,
  `[proto]` .scratch/otorp/Keynote/KNArchives.proto:473) vs Numbers
  (`TSA.DocumentArchive`, `[proto]` .scratch/otorp/Keynote/TSAArchives.proto:37).
- Legacy containers ('08/'09 XML, `.pages-tef` sqlite, iWork 5.5 flat zips) and
  their detection signals are covered in [legacy.md](legacy.md); dunhamsteve's
  loader falls back through them (`[parser: iwork@02c26ebf]` index.go:35-64).

## What the container does NOT contain

No SQL database, no manifest beyond `TSP.PackageMetadata` inside the IWA object
graph, and no encryption envelope in the clear-file case. The authoritative
component/`DataInfo` registry lives in the IWA archive with identifier 2
(`TSP.PackageMetadata` with `components` and `datas` fields,
`[proto]` .scratch/otorp/Keynote/TSPArchiveMessages.proto:106-123); how references
resolve into it is documented in [media.md](media.md).
