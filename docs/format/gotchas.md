# Gotchas — oddities discovered during research

Collected oddities that have bitten (or will bite) parsers. Provenance rules per
[INDEX.md](INDEX.md).

## 1. The Snappy block header is NOT "u16 LE + u16 BE"

The most-copied format description (including our AGENTS.md primer at kickoff)
says the 4-byte block header is "u16 LE compressed size, u16 *big-endian*
uncompressed size". Every real implementation disagrees: byte 0 is a zero
chunk-type byte and bytes 1–3 are the compressed length as **u24 little-endian**;
no uncompressed size appears in the header at all (it is the leading varint of
the raw Snappy block). Evidence: `[parser: keynote-parser@56a4d3b0]
keynote_parser/codec.py:186-210` and `:232-243` (round-tripping writer),
`[parser: iwork@02c26ebf] index/index.go:181-197`, `[parser: litchi@92293640]
crates/litchi-iwa/src/snappy.rs:27-52`. Full write-up: [iwa.md](iwa.md).

## 2. There is no `TSP.PrefixedMessage`

Some third-party writeups describe an envelope message
`TSP.PrefixedMessage { 1: identifier, 2: length, 3: payload }`. No such message
exists in any proto we hold — not in the 15.3.1 otorp extraction
(`.scratch/otorp/*/TSPArchiveMessages.proto`) nor in the reference protos
(`.scratch/iwork/proto/TSPArchiveMessages.proto`) `[proto]`. The real envelope is
`[varint length][TSP.ArchiveInfo]` followed by the payloads declared in its
`MessageInfo`s — see [objects.md](objects.md).

## 3. `npx otorp` extracts nothing from the 2026 15.3.1 apps

otorp 0.0.1's reference heuristic (x86_64 `LEA` scan + classic absolute-pointer
scan) fails on binaries linked with `LC_DYLD_CHAINED_FIXUPS` — both slices of
every `*.app/Contents/MacOS` and `Frameworks/*.framework` binary in the
"Creator Studio" 15.3.1 bundles. `scripts/docs_fetch_sources.py` works around it
by running otorp's own engine with the reference check relaxed and validating
candidates by strict `FileDescriptorProto` wire parsing. Recorded in
[ATTRIBUTION.md](ATTRIBUTION.md). `[inferred: diagnosed from otorp source at
~/.npm/_npx/*/node_modules/otorp/index.node.js and repeated empty extractions]`

## 4. Recovered protos carry no id registry

`TSP.MessageInfo.type` ids are resolved through out-of-band registry tables
(numeric id → message name). The 15.3.1 otorp protos contain none; ids come from
the reference repos, which document *older* app versions — and the ids differ
across versions (keynote-parser even ships per-version registries:
`.scratch/keynote-parser/protos/versions/<ver>/registry.json`). See
[registry.md](registry.md) for the drift analysis. Unknown ids must be displayed
as opaque numbers, never guessed.

## 5. `Index.zip` is a zip inside a zip

The outer zip contains `Index.zip`, which contains the `Index/*.iwa` streams.
Single-level unzip is a classic first mistake. `[parser: iwork@02c26ebf]
index/index.go` and `[parser: keynote-parser@56a4d3b0]
keynote_parser/bundle_utils.py` both handle the nesting — see
[container.md](container.md).

## 6. Undecodable objects must not desynchronize the stream

`MessageInfo.length` delimits each object payload *regardless of decodability* —
keynote-parser preserves undecodable objects verbatim (`UnknownArchive`,
`codec.py:47-60`, `:312-321`) rather than skipping by parse. Any pnk dumper must
use the declared length to skip, never "consume until it parses".
`[parser: keynote-parser@56a4d3b0]`

## 7. Some "compressed" blocks may not be compressed

keynote-parser falls back to emitting the block bytes verbatim when Snappy
decoding fails (`codec.py:204-210`). Whether real documents rely on this is
unknown `[inferred: no fixture confirms it yet]` — pnk should error, but the
error message should say "block failed to decompress", not "corrupt file".

## 8. The 2026 "Creator Studio" rebrand

The locally installed bundles are `/Applications/{Keynote,Numbers,Pages} Creator
Studio.app` — these ARE Apple's iWork apps (v15.3.1, display names unchanged,
bundle IDs `com.apple.Keynote` / `com.apple.Numbers` / `com.apple.Pages`,
verified via `Contents/Info.plist`; recorded in [ATTRIBUTION.md](ATTRIBUTION.md)).
Scripts that glob for `/Applications/Keynote.app` will find nothing.
`[inferred: verified on this machine 2026-08-28]`

## 9. Legacy formats look deceptively similar

Pre-'13 files are also zips (or directory bundles) and often unzip successfully —
but contain `index.xml` (optionally gzipped) instead of `Index.zip`. Detection
and rejection guidance: [legacy.md](legacy.md).

## 10. Sos protos are a separate namespace

The 15.3.1 extraction contains parallel `*.sos.proto` files (e.g.
`TSTArchives.sos.proto`, `KNArchives.sos.proto`) alongside the plain ones —
Apple's internal "SOS"/sync-related schema variants. They are additive reading
material, not a different on-disk format `[inferred: presence pattern only;
not yet fixture-verified]`.