# Objects — TSP archive envelope and object graph

After Snappy decompression (see [snappy.md](snappy.md)), every `.iwa` file is a flat
stream of protobuf `TSP.ArchiveInfo` envelopes, each followed by the raw payloads its
`MessageInfo` entries declare. All document state lives in these payloads; object
identity and cross-references are uint64 ids resolved through a global id space.

## Decompressed stream layout

One decompressed `.iwa` stream is a sequence of archive segments, each:

1. a `uint32` varint (`_DecodeVarint32`) giving the byte length of the ArchiveInfo message,
2. the serialized `TSP.ArchiveInfo` message,
3. immediately concatenated after it, one payload blob per `MessageInfo` in
   `message_infos` order, each exactly `MessageInfo.length` bytes long.

Verified in two independent parsers:

- [parser: psobot/keynote-parser@56a4d3b] `.scratch/keynote-parser/keynote_parser/codec.py:456-461`
  (`get_archive_info_and_remainder` reads the varint then `FromString`s the ArchiveInfo)
  and `codec.py:306-364` (`IWAArchiveSegment.from_buffer` slices the remainder into
  per-`MessageInfo` payloads using `message_info.length` as the only delimiter — so an
  undecodable archive never desynchronizes the ones after it).
- [parser: dunhamsteve/iwork@02c26eb] `.scratch/iwork/index/index.go:117-157` (`loadIWA`:
  `binary.ReadUvarint`, unmarshal `TSP.ArchiveInfo`, then read `*info.Length` payload
  bytes per `MessageInfo`).

There is no outer framing message and no per-segment CRC. The envelope is exactly the
varint + `TSP.ArchiveInfo` form above.

**Obsolete claim warning.** Some third-party writeups describe a `TSP.PrefixedMessage`
wrapper. No message of that name exists in any current proto
(`.scratch/otorp/{Keynote,Numbers,Pages}/*.proto` — grep finds zero hits) nor in the
older reference protos in `.scratch/iwork/proto/`. The envelope is the varint+ArchiveInfo
form; treat any `PrefixedMessage` description as obsolete [inferred: verified absent from
all local proto sources, and both parsers read the varint+ArchiveInfo form directly].

## ArchiveInfo and MessageInfo

`.scratch/otorp/Keynote/TSPArchiveMessages.proto:6-24` [proto]:

- `TSP.ArchiveInfo`:
  - `identifier = 1` (uint64) — this archive's object id in the global id space.
  - `message_infos = 2` (repeated MessageInfo) — one entry per payload that follows.
  - `should_merge = 3` (bool) — enables the incremental "patch" decode path (see
    [incremental.md](incremental.md)).
- `TSP.MessageInfo`:
  - `type = 1` (uint32, required) — registry type id naming the payload's proto message
    (e.g. 201 → `TSK.CommandHistory`, per `.scratch/iwork/codegen/Common.json`); see
    [registry.md](registry.md).
  - `version = 2` (repeated uint32, packed) — version vector of the app that wrote it.
  - `length = 3` (uint32, required) — payload byte length (the only delimiter between
    payloads).
  - `field_infos = 4` — per-field forward/backward-compatibility metadata.
  - `object_references = 5` / `data_references = 6` (repeated uint64, packed) — ids of
    referenced objects/media, hoisted into the header.
  - Fields 7–11 (`base_message_index`, `diff_merge_version`, `diff_field_path`,
    `fields_to_remove`, `diff_read_version`) — only meaningful for patch messages;
    see [incremental.md](incremental.md).

`FieldInfo` and `FieldPath` (`.scratch/otorp/Keynote/TSPArchiveMessages.proto:26-56`)
carry schema-evolution hints: a `FieldPath` is a packed list of field numbers, and
`FieldInfo` classifies a field (`Value=0`, `ObjectReference=1`, `DataReference=2`,
`Message=3`) with `unknown_field_rule` / `known_field_rule` telling newer writers how a
newer reader should treat fields it does not understand. Parsers can ignore all of it
when the type id already resolves to a concrete message.

## References and the object graph

Every edge between objects is a `TSP.Reference` carrying a global object id:

- `.scratch/otorp/Keynote/TSPMessages.proto:26-30` [proto]:
  `TSP.Reference { identifier = 1 (uint64, required); deprecated_type = 2;
  deprecated_is_external = 3 }`. Only `identifier` matters; fields 2–3 are deprecated.
- `.scratch/otorp/Keynote/TSPMessages.proto:32-34`: `TSP.DataReference { identifier = 1 }`
  points at media/data objects instead.

Resolution is a flat map from `ArchiveInfo.identifier` to the decoded payload object —
there are no per-document nested scopes. [parser: dunhamsteve/iwork@02c26eb]
`.scratch/iwork/index/index.go:109-115` (`Deref` looks up `Records[*ref.Identifier]`,
keyed at `index.go:151-178` by `(*ai.Identifier, *info.Type)`).

Two container messages group objects [proto, .scratch/otorp/Keynote/TSPMessages.proto]:

- `TSP.ObjectContainer { identifier = 1 (uint32); objects = 2 (repeated Reference) }`
  (lines 204-207) — an ordered bag of references.
- `TSP.ObjectCollection { objects = 1 }` (lines 200-202) and `TSP.SparseReferenceArray`
  (lines 36-43) for indexed/sparse collections.

## Large arrays and segments

Big collections are not stored inline. A `TSP.LargeArray` header lists byte-range
descriptors plus references to segment objects [proto,
.scratch/otorp/Keynote/TSPMessages.proto:213-273]:

- `TSP.LargeArray` (lines 247-257): `ranges = 1` (TSP.Range element index span per
  segment), `segments = 2` (repeated `TSP.Reference` to segment objects), sizing caps
  (`max_segment_element_count = 3`, `max_segment_size = 4`) and archiving hints
  (`should_delay_archiving = 5`, `store_outside_object_archive = 7`).
- Typed wrappers all embed `large_array = 1`: `TSP.LargeNumberArray` (lines 259-261),
  `TSP.LargeStringArray` (263-265), `TSP.LargeLazyObjectArray` (267-269),
  `TSP.LargeObjectArray` (271-273).
- Matching segment messages: `TSP.LargeNumberArraySegment { elements = 2 (repeated
  double) }` (219-222), `TSP.LargeStringArraySegment` with `OptionalElement` wrappers
  (224-230), `TSP.LargeUUIDArraySegment { elements = 2 (repeated TSP.UUID) }` (232-235),
  and `TSP.LargeObjectArraySegment` / `TSP.LargeLazyObjectArraySegment`
  `{ elements = 2 (repeated TSP.Reference) }` (237-245). All share a
  `TSP.LargeArraySegment` prefix (213-217) with `package_locator = 3`, which lets a
  segment's bytes be written into a separate package entry.

Segmentation is transparent to readers that walk `ranges` + `segments`; segment splits
are a writer-side space/time tradeoff [inferred: caps are optional and parsers never
enforce them].

## TST tiles — the generic big-array storage

Spreadsheet cell storage uses the same segmented pattern via tiles [proto,
.scratch/otorp/Keynote/TSTArchives.proto:123-153]:

- `TST.TileStorage` (145-153): `tiles = 1` (repeated `Tile { tileid = 1 (uint32);
  tile = 2 (TSP.Reference) }`), `tile_size = 2` (uint32), `should_use_wide_rows = 3`.
  Each tile is itself a separate object in the global id space, reached through
  `TSP.Reference`.
- `TST.Tile` (134-143): `maxColumn`, `maxRow`, `numCells`, `numrows`, and
  `rowInfos = 5` (repeated `TST.TileRowInfo`).
- `TST.TileRowInfo` (123-132): per row, a `cell_storage_buffer` (opaque bytes holding the
  row's values) plus `cell_offsets` locating each cell inside the buffer;
  `*_pre_bnc` fields are pre-Brushed-Metal-Calc engine legacy copies, and
  `has_wide_offsets` selects a wider offset encoding.

The same tile mechanism stores other large per-object state (e.g. style tables); any
message owning a `TSP.TileStorage`-style field is using it as generic big-array storage
[inferred: verified for table cells; other uses seen in protos but not yet
fixture-verified].

## Global id space

- `ArchiveInfo.identifier` is the object's id — one object per archive segment, assigned
  document-wide. Ids are stable across saves and are what `TSP.Reference.identifier`
  points at [parser: dunhamsteve/iwork@02c26eb] `.scratch/iwork/index/index.go:138-153`
  (each segment contributes one `Records[id] = value` entry).
- `MessageInfo.type` is *not* an object id — it is the registry type id that maps to the
  payload's proto message class; see [registry.md](registry.md).
- `MessageInfo.object_references` / `data_references` and `FieldInfo` variants hoist the
  same ids into the envelope, letting an app compute reachability without decoding
  payloads [proto, `.scratch/otorp/Keynote/TSPArchiveMessages.proto:17-18,47-48`];
  keynote-parser does not use them for decoding [parser: psobot/keynote-parser@56a4d3b]
  (codec.py slices payloads by `length` alone).

Practical consequence for writers: you can append a fresh archive segment with a new
global id and repoint references to it, without rewriting the whole stream — this is what
makes both the segmented `LargeArray` layout and the incremental save path
([incremental.md](incremental.md)) cheap.
