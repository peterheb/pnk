# Embedded media handling (images, movies, audio)

Media bytes live outside the IWA object graph — as `Data/<file_name>` members of
the ZIP (flat form) or files in the package's `Data/` directory — while the IWA
objects hold only a registry of `TSP.DataInfo` descriptors and reference them by
numeric identifier via `TSP.DataReference`.

## Where the bytes are

- Flat file: media are plain ZIP members under `Data/`, e.g. `Data/picture.png`,
  interleaved with `Index/*.iwa` and `Metadata/*.plist` members
  (`[parser: numbers-parser@32387958]` .scratch/numbers-parser/src/numbers_parser/model.py:2631-2637
  writes them as `f"Data/{filename}"`; keynote-parser streams every non-`.iwa`
  member through as raw bytes,
  `[parser: keynote-parser@56a4d3b0]` .scratch/keynote-parser/keynote_parser/file_utils.py:92-108).
- Package directory: `Data/` sits at the bundle root, NOT inside `Index.zip` —
  numbers-parser's package `save()` writes IWA blobs into `Index.zip` and all
  other blobs (including `Data/*`) as real files on disk
  (`[parser: numbers-parser@32387958]` .scratch/numbers-parser/src/numbers_parser/iwork.py:153-166).
  litchi's `MediaManager` scans only the bundle's `Data/` directory
  (`[parser: litchi@92293640]` .scratch/litchi/crates/litchi-iwa/src/media.rs:118-141 —
  note it returns no assets for single-file zips; only directory bundles are scanned).
- Media bytes never appear inside `.iwa` streams or as inline protobuf bytes: the
  DataInfo descriptor carries a digest and filenames, and unmaterialized/remote
  content is described by fields like `remote_url` and `materialized_length`
  (`[proto]` .scratch/otorp/Keynote/TSPArchiveMessages.proto → `TSP.DataInfo`
  lines 140-164).

## The DataInfo registry

Every media asset is described by a `TSP.DataInfo` message:

```
[proto] .scratch/otorp/Keynote/TSPArchiveMessages.proto:140-164  (also readable
[proto] .scratch/iwork/proto/TSPArchiveMessages.proto:81-89, older extraction)
message DataInfo {
  required uint64 identifier = 1;
  required bytes digest = 2;              // SHA-1 of the file bytes (20 bytes)
  required string preferred_file_name = 3;
  optional string file_name = 4;          // name actually stored under Data/
  optional string document_resource_locator = 5;
  optional string remote_url = 7;
  optional uint64 materialized_length = 18;
  ...
}
```

The registry lives in `TSP.PackageMetadata.datas` (field 4) — the IWA archive with
identifier 2, which numbers-parser hardcodes as `PACKAGE_ID = 2`
(`[proto]` .scratch/otorp/Keynote/TSPArchiveMessages.proto:106-123;
`[parser: numbers-parser@32387958]` constants.py:62). Two more copies of the same
`datas` list shape exist for pasteboards and per-component serialization:
`TSP.PasteboardMetadata.datas` (field 3) and
`TSP.ObjectSerializationMetadata.datas` (field 5)
(`[proto]` .scratch/otorp/Keynote/TSPArchiveMessages.proto:115,131-141,197-211 —
`datas` fields 4, 3, and 5 respectively).
There is no `TSP.DataSource` and no `TSD.MediaArchive` message in the 15.3.1
extraction — grep across `.scratch/otorp/Keynote/*.proto` returns neither; media
objects are `TSD.ImageArchive` / `TSD.MovieArchive` `[proto]`
.scratch/otorp/Keynote/TSDArchives.proto:382,427.

## How objects point at media

Two reference forms exist side by side (a database-migration artifact):

- `TSP.DataReference { required uint64 identifier = 1 }`
  (`[proto]` .scratch/otorp/Keynote/TSPMessages.proto:32-34) — points at a
  `DataInfo.identifier` in the `datas` registry.
- `TSP.Reference { required uint64 identifier = 1 }`
  (`[proto]` .scratch/otorp/Keynote/TSPMessages.proto:26-31) — the `database_*`
  fields point at the same objects through the normal object graph.
  `TSD.ImageArchive` has both: `data` (11) as DataReference and `database_data`
  (2) as Reference; likewise `thumbnailData` (12) / `database_thumbnailData` (6)
  (`[proto]` .scratch/otorp/Keynote/TSDArchives.proto:382-412).
  `[inferred: the two forms reference the same DataInfo object; the database_*
  variant is the canonical TSP-database link and the DataReference form is the
  flat-file serialization — confirm which one writers emit on a fixture]`.

Media-carrying messages (Keynote 15.3.1 extraction):

- `TSD.ImageArchive`: `data` (11), `thumbnailData` (12), `originalData` (13),
  `originalSVGData` (23), `enhancedImageData` (17), `adjustedImageData` (15),
  `thumbnailAdjustedImageData` (16) — all `TSP.DataReference`
  (`[proto]` .scratch/otorp/Keynote/TSDArchives.proto:382-412; the same message
  with fewer fields is in `[proto]` .scratch/iwork/proto/TSDArchives.proto:349-373).
- `TSD.MovieArchive`: `movieData` (14), `importedAuxiliaryMovieData` (22),
  `posterImageData` (15), `audioOnlyImageData` (16); plus `movieRemoteURL` (17),
  `audioOnly` (9) — audio-only movies keep a poster image reference
  (`[proto]` .scratch/otorp/Keynote/TSDArchives.proto:427-470).
- `TSD.ImageFillArchive`: `imagedata` (6) — the fill-image path used for table
  cell backgrounds (`[proto]` .scratch/otorp/Keynote/TSDArchives.proto:139-157).

## Resolution chain (the only one a parser needs)

numbers-parser reading a cell background image is the complete, worked chain:

1. Cell style object → `cell_properties.cell_fill.image.imagedata.identifier`
   (a `TSP.DataReference`).
2. Find the `TSP.DataInfo` with that identifier in `objects[PACKAGE_ID].datas`.
3. Look up ZIP member exactly `Data/{x.file_name}` in the file store; if absent,
   warn using `preferred_file_name` ("Cannot find file '{preferred_filename}' in
   Numbers archive").
4. Computes the SHA-1 of the returned bytes to key its internal image cache (not
   a verification against `DataInfo.digest` — no parser validates the stored
   digest on read `[inferred: cell.py:1172-1174 only populates a cache; no
   digest check exists in any of the four parsers]`).
   (`[parser: numbers-parser@32387958]`
   .scratch/numbers-parser/src/numbers_parser/cell.py:1150-1176.)

## Naming and writing conventions parsers rely on

- `file_name` is the on-disk/zip name; `preferred_file_name` is the user-facing
  original name. The two can differ (dedup/renaming by the app) — numbers-parser
  treats `Data/{file_name}` as the lookup key and `preferred_file_name` as display
  only (`[parser: numbers-parser@32387958]` cell.py:1156-1168).
- When writing, numbers-parser sets `file_name = preferred_file_name` and stores
  the bytes at `Data/{filename}`, refusing duplicates
  (`[parser: numbers-parser@32387958]` model.py:2631-2637,1921-1928).
- New DataInfo creation: `digest = sha1(data).digest()`,
  `materialized_length = len(data)`, and the new identifier is
  `max(datas identifiers) + 1` — "datas never appears to be an empty list (default
  themes include images)" (`[parser: numbers-parser@32387958]`
  model.py:1914-1931,2639-2644).
- The digest is a raw 20-byte SHA-1, not hex `[inferred: numbers-parser compares
  it against Python's `sha1().digest()` output, cell.py:1172-1174 — a hex string
  would never match]`.
- No directory structure below `Data/` is assumed by numbers-parser; the member
  name is matched exactly. litchi does recurse into `Data/` subdirectories when
  scanning a package (`[parser: litchi@92293640]` media.rs:134-166).

## Type detection

No media-type field exists in DataInfo; parsers infer type from file extension.
litchi's table: png/jpg/jpeg/gif/tiff/bmp/heic/heif → image, mp4/mov/m4v/avi/mkv →
video, mp3/aac/m4a/wav/aiff → audio, pdf → PDF
(`[parser: litchi@92293640]` media.rs:29-53). Extended pixel-level metadata is a
proto extension on `TSP.DataAttributes`: `TSD.ImageDataAttributes` (ext 100)
carries `pixel_size`, `image_is_srgb`, `media_library_asset_id`
(`[proto]` .scratch/otorp/Keynote/TSDArchives.proto:414-425;
`TSP.DataAttributes` extension point at
`[proto]` .scratch/otorp/Keynote/TSPMessages.proto:209-211, wired into DataInfo
field 10 `attributes`).

## Related

- ZIP member layout and `Index.zip` vs package structure: [container.md](container.md).
- The registry archive itself (identifier 2, `TSP.PackageMetadata`) and reference
  resolution mechanics: [objects.md](objects.md).
- Filename-encoding pitfalls that break `Data/` lookups: [container.md](container.md)
  ("Zip entry-name encoding") and [gotchas.md](gotchas.md).
