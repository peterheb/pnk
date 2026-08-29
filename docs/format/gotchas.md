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

## 11. `OperationStorage.iwa` is a collaboration log, not an IWA stream

Newer iWork (15.x-era) documents carry `Index/OperationStorage.iwa` whose payload
begins with LZFSE magic bytes — `bvx-` observed in 8 fixtures, `bvxn` in 2
(variants `bvx1`/`bvx2` also exist). It is Apple's collaborative-editing
operation log, NOT a Snappy-compressed TSP archive: block-framing parsers must
detect the magic and skip/report it. iwadump treats it as container metadata.
`[inferred: fixture-verified on 10 files via the iwadump dataset gate, 2026-08-28]`

## 12. `.iwpv2` is a second, newer encryption marker

Besides `.iwph` (the marker numbers-parser documents), fixture `2dccc804…`
carries `Index/*.iwpv2` members with every stream ciphertext except
`DocumentStylesheet`. Same class as `.iwph`: reject cleanly as
password-protected. `[inferred: single fixture so far, 2026-08-28 — treat the
`.iwp*` family as the encrypted class pending more samples]`
## 13. Pre-BNC (storage_version 4) table cells are a separate cell layout

Modern Numbers writes BNC tiles (`TST.Tile.storage_version = 5`,
`last_saved_in_BNC = true`, cell layout documented in tables.md). But
Numbers 11.x-era documents in the corpus write **pre-BNC tiles**:
`storage_version = 4`, no `last_saved_in_BNC`, and the row data in the
`*_pre_bnc` fields (`cell_storage_buffer_pre_bnc = 3` /
`cell_offsets_pre_bnc = 4`) — which the modern proto names "pre-BNC" but
which are the ONLY storage in these files. numbers-parser rejects these
(`UnsupportedError: Pre-BNC storage is unsupported`), so there is no
reference decode; the layout below is fixture-verified against
`CC-MAIN-2026-34-cdx-00006-5` and cross-checked against the file's embedded
QuickLook preview.

Layout mechanics (offsets, wide-offset scaling) match the v5 description.
The per-cell blocks differ:

- The block starts with `version = 4` (byte 0) + `TST.CellType` (byte 1),
  then 32-bit slots: `[version|type][?][flags][cell style id][text style id]…`.
- **Text cells (type 3, flags `0x10`):** the string-table key is the u32 at
  **slot 6** (byte offset 24); when flag bit `0x40000` is set one extra u32
  follows the key. Keys resolve through the table's `TableDataList`
  (STRING type, including its *segmented* entries — segment lists are
  required, not optional: inline entries cover only part of the key space).
- **Duration (type 7) / date (type 5) / number (type 2) cells:** the value
  is an f64 in the 8 bytes before the trailing slot (fixture cells show
  seconds for durations; 51.5 / 970 / 2408 for a "Start time" column), with
  the number-format id at slot 5 (resolvable in the FORMAT-type
  `TableDataList`).
- **Empty stubs:** 12-byte v4 blocks with type 0 (genericCellType) and a
  zero payload — explicitly-stored empty cells; skip them.

Blast radius across the corpus: 45 of 358 tables were fully empty under the
v5-only walker; after v4 support, 11 remain — all genuinely blank
(explicit v4 empty stubs, no value payload).

`[fixture-verified: layout against cdx-00006-5 + QuickLook; parser side
unverified — numbers-parser refuses the variant and libetonyek was not
checked]`


## 14. List membership/level/restart ride in storage tables — theme not required

Paragraph list membership is decodable WITHOUT theme resolution, per
G1-golden-pages-wp.pages (Pages 26.3.1 fixture) [fixture-verified]:

- `table_list_style` (field 7) holds paragraph → list-style ranges: an entry
  at character offset X applies its `TSWP.ListStyleArchive` from paragraph X
  onward until the next entry; a null object clears membership.
- `table_para_data` (field 6) `.first` at a paragraph start = that
  paragraph's list LEVEL (0-based).
- `table_para_starts` (field 14) `.first` = list RESTART flag (1 = numbering
  restarts at this paragraph).
- The `label_types` value discriminates the marker: 0 = none, 2 = string
  bullet (`strings` holds the glyph, e.g. "•"), 3 = number.

G1 example: "One/Two/Three" = one numbered segment (restart flag on "One"
only; "Two"/"Three" continue); "Bullet Level 1" level 0, "Bullet Level 2"
level 1 via `para_data`. Renderer numbering must also accumulate across
non-restarting paragraphs (the converter emits `start` only on restarts).

## 15. Trailing paragraph whitespace is not persisted; LS/PS need JSON escaping

Two independent editor-layer behaviors verified against the same fixture:

- Pages 26.3.1 does NOT store trailing paragraph whitespace: the saved
  `TSWP.StorageArchive` text buffer for a paragraph typed as
  `"   a\tb  c\u00a0d  "` ends at `d` (bytes `... 63 c2 a0 64`). The paste
  source has the spaces; the saved storage does not [fixture-verified]. A
  converter preserves byte-exact what the storage says — trailing spaces
  beyond that are un-recoverable.
- U+2028/U+2029 occur raw in storage text (soft line breaks). They are
  spec-legal raw in JSON strings but trip editor "unusual line terminator"
  warnings that mask real review signals — pnk2json escapes them as
  `\u2028`/`\u2029` in JSON output [inferred: policy; decode semantics
  unchanged]. Dumpers render them as spaces (text) / markdown hard breaks.


## 16. Pages flavor: body_storage exists in BOTH flavors — never infer flavor from it

Pages 26.3.1 page-layout documents still carry a `TP.DocumentArchive.body_storage`
(field 4) reference with a live, decodable text flow. The "Convert to Page
Layout" dialog's "body will be discarded" warning is about RENDERING, not
storage — find/replace still reaches the hidden flow [fixture-verified:
G2-golden-pages-layout.pages built in Pages 26.3.1; UI evidence
g2-menu-screenshot.png shows the File menu offering "Convert to Word
Processing", which only appears on layout docs]. The discriminator is
`TP.SettingsArchive.body` (settings reached via root field 7; field 1,
default true): 1/absent = word-processing, 0 = page-layout. Corpus sweep
2026-08-29: 17 of 325 Pages fixtures flip from the body-storage heuristic to
page-layout. pnk2json preserves the layout doc's hidden flow as
`hiddenBody` (omitted when empty) — no silent data loss.


## 17. Invisible Unicode in text runs — escaped in JSON output

Real documents carry NBSP, ZWSP/ZWNJ/ZWJ, bidi marks, BOM, soft hyphens and
ideographic spaces inside text runs (fixture-verified: G1 torture paragraph
uses NBSP). They are spec-legal raw in JSON strings, but reviewers cannot
visually distinguish them from ASCII spaces, so pnk2json's serializer escapes
every Unicode space separator (Zs) and format character (Cf) plus
U+2028/U+2029 as `\uXXXX` (plain space/tab stay raw). Decoded data is
byte-identical — this is a review-hygiene policy, not a format constraint.
The corpus sweep caught no surprises: escapes appear only where the source
files carry the code points.


## 18. Numbers merge ranges have TWO storage generations

Legacy (pre-uid): `DataStore.merge_region_map` (field 13) →
`TST.MergeRegionMapArchive` with packed-int `CellRange`s (col<<16|row) —
decoded by pnk2json (tables.md §Merges). Modern Numbers additionally writes
a uid-based merge table on the model itself (field 70 in the observed G5
layout): UUIDRect-style coordinate pairs (row-uid pair + col-uid pair) that
resolve through the table's row/column UID maps. pnk2json does not yet
decode the uid form: when it is present and the legacy map produced no
merges, the converter emits a `table-degraded` warning naming the covered
null-cell count — the grid nulls already mark covered cells, only span
info is missing [fixture-verified: G5 Table 2, Pages 26.3.1].

Related: `FormatStructArchive.decimal_places = 253` (also -3 as u32
4294967293 in fraction_accuracy) is the app's AUTO-DECIMALS sentinel across
decimal/currency/percent/scientific formats — NOT malformed. pnk2json emits
such formats with `decimals` absent (fixture-verified: G5 Table 2 cells
r2c1-c3/r3c2/r4c3).
