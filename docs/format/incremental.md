# Incremental save — patch messages and command archives

iWork saves documents incrementally: instead of rewriting every object, a save writes
patch messages that update single fields of already-stored objects, and separately
persists undo/redo state as command archives. Both mechanisms ride on the same
[objects.md](objects.md) envelope.

## MessageInfo patch fields

`.scratch/otorp/Keynote/TSPArchiveMessages.proto:12-24` [proto] — `TSP.MessageInfo`
fields 7-11:

- `base_message_index = 7` (uint32) — index into this segment's `message_infos` of the
  message being patched.
- `diff_merge_version = 8` (repeated uint32, packed) — version vector the patch was
  produced against.
- `diff_field_path = 9` (`TSP.FieldPath` = packed list of field numbers,
  `.scratch/otorp/Keynote/TSPArchiveMessages.proto:54-56`) — the one field the patch
  replaces.
- `fields_to_remove = 10` (repeated FieldPath) — fields to drop from the base message.
- `diff_read_version = 11` (repeated uint32, packed) — version the reader is expected to
  merge at.

The enabling switch is on the envelope: `TSP.ArchiveInfo.should_merge = 3` (bool)
[proto, `.scratch/otorp/Keynote/TSPArchiveMessages.proto:6-10`].

## Patch messages (type == 0)

A patch payload is identified by `MessageInfo.type == 0` combined with
`ArchiveInfo.should_merge == true` and at least one earlier message in the same segment.
There is no registry entry for type 0 — the payload is not a full message but a one-field
sub-message to be spliced into the base message at `diff_field_path` (a "ProtobufPatch").
[parser: psobot/keynote-parser@56a4d3b] `.scratch/keynote-parser/keynote_parser/codec.py:322-331`:

```python
if message_info.type == 0 and archive_info.should_merge and payloads:
    base_message = archive_info.message_infos[message_info.base_message_index]
    base_klass = import_version(version)[0][base_message.type]
    klass = partial(ProtobufPatch.FromString, message_info, base_klass)
```

Decode algorithm, as implemented:

1. Decode the payload as the field message type found by looking up
   `diff_field_path.path[0]` in the *base* class's descriptor (extension lookup fallback
   included) — `codec.py:260-285`.
2. Replace or set the base message's field at that path with the decoded value.
3. Any `fields_to_remove` entries are removed from the merged result.

Known limitations in the reference parsers — a reader must handle these or skip the
patch without desynchronizing (payload lengths still come from `MessageInfo.length`):

- `len(diff_field_path.path) != 1` → `NotImplementedError`
  [parser: psobot/keynote-parser@56a4d3b] codec.py:261-265; same limitation documented in
  [parser: masaccio/numbers-parser@323879] `.scratch/numbers-parser/src/numbers_parser/iwafile.py:133-135`.
- non-empty `fields_to_remove` → `NotImplementedError` (keynote-parser codec.py:266-269).

If the type id is unknown (not 0/patch and not in the registry), the payload must be
preserved verbatim to keep later segments aligned; keynote-parser wraps it in an
`UnknownArchive` that round-trips byte-for-byte [parser: psobot/keynote-parser@56a4d3b]
codec.py:83-126, 335-343. See [registry.md](registry.md) for type-id resolution.

`version` arrays and `diff_merge_version`/`diff_read_version` are app-version vectors;
keynote-parser selects the whole proto schema set by the app version string
(`import_version`, codec.py:27-44) and ignores per-message vectors beyond that
[inferred: parser code resolves versions per-document, never per-MessageInfo].

## Command history and undo/redo state

Undo/redo is persisted as ordinary objects in the global id space, in the document's
regular `.iwa` archives (not a separate envelope format).

Base type, `.scratch/otorp/Keynote/TSKArchives.proto` [proto]:

- `TSK.CommandArchive` (lines 135-143): the common base of every recorded operation —
  `undoCollection = 2`, `remote = 5`, `server_originated = 7` (collaboration), plus
  deprecated `undoRedoState = 1`. Per-app commands embed it as `super = 1`.
- `TSK.CommandGroupArchive` (145-151): `super = 1` plus `commands = 2` (repeated
  TSP.Reference to child commands), `action_string = 4`, `can_coalesce_group = 5` —
  a composite undo step.
- `TSK.LocalCommandHistory` (31-35): `undo_count = 1` (how many entries from the front
  are undoable) and `items_array = 2` → a `TSP.LargeObjectArray` of
  `TSK.LocalCommandHistoryItem { command = 1, behavior = 2 }` (18-29). Registry type id
  201 [parser: dunhamsteve/iwork@02c26eb] `.scratch/iwork/codegen/Common.json`
  (`"201": "TSK.CommandHistory"`) decodes it in `index/common.go:93-96`.
  In the older reference proto `.scratch/iwork/proto/TSKArchives.proto:12-18` this
  message is `TSK.CommandHistory { undo_count = 1; commands = 2; marked_redo_commands = 3;
  pending_preflight_command = 4 }` — the current extraction replaced the flat
  `commands` list with the segmented `items_array` [inferred: comparing both protos;
  same field 1 semantics].
- Supporting records: `TSK.CommandContainerArchive` (176-178) and
  `TSK.ProgressiveCommandGroupArchive` (180-182); registry ids 202/203/206 in
  Common.json. `TSK.CommandSelectionBehaviorHistoryArchive` (id 208) restores selection
  alongside undo.

## Per-app command archives

Every app layer defines `*CommandArchive` messages that record what an operation did, so
it can be undone/redone without keeping pre-images of whole objects. Each embeds
`required .TSK.CommandArchive super = 1` and stores old/new values:

- Keynote: `.scratch/otorp/Keynote/KNCommandArchives.proto` — e.g.
  `CommandSlideNodeSetPropertyArchive { super = 1; slideNode = 2; property = 3;
  oldValue = 4; newValue = 5 }` (lines 67-78); 55 `CommandArchive` mentions.
- Numbers: `TNCommandArchives.proto` lives in `.scratch/otorp/Numbers/` (the Keynote
  extraction carries only TSD/TST/TSWP/TSCH/KN command protos); the sheet commands in
  the older reference proto `.scratch/iwork/proto/TNCommandArchives.proto:16-81` follow
  the same `super = 1` pattern, with undo scaffolding like
  `formula_rewrite_command_for_undo = 6`.
- Pages: `.scratch/otorp/Pages/TPCommandArchives.proto` (see
  `.scratch/iwork/proto/TPCommandArchives.proto:13-60`) — insert/move/paste drawables,
  section breaks, etc.

App-level counts [proto, `.scratch/otorp/Keynote/`]: `TSDCommandArchives.proto` (102
`CommandArchive` refs), `TSWPCommandArchives.proto` (80), `TSTCommandArchives.proto`
(77), `KNCommandArchives.proto` (55), `TSCHCommandArchives.proto` (37). Per-app type ids
resolve through the app registries — e.g. KN 100-119, TN 12002-12027, TP 10101+ (see
`.scratch/iwork/codegen/{Keynote,Numbers,Pages}.json` and [registry.md](registry.md)).
Collaboration-only (`remote`/`server_originated`): `TSK.Operation` /
`TSK.OperationTransformer` / `TSK.OperationStorageEntry`
(`.scratch/otorp/Keynote/TSKArchives.proto:341-419`) store OT-style operations per
document revision; single-user docs do not populate them [inferred: fields sit in the
collaboration path, not the local undo path].

## How viewers can ignore all of it safely

- Command archives and command history are *not* on the rendering path. The document
  model is the set of non-command objects (e.g. `TSK.DocumentArchive`, slide/sheet/word
  processor storages) reached via `TSP.Reference` from the component roots.
- Safe-ignore recipe [inferred, consistent with parser behavior]: skip messages whose
  type resolves to a `*CommandArchive`/`*CommandGroupArchive`/history class; for
  `type == 0` patch messages, either apply the merge (section above) or skip exactly
  `MessageInfo.length` bytes — never guess payload boundaries.
- The one thing you cannot skip blindly is undo state that references live objects:
  `TSK.CommandHistory` ids can appear in the id space, and `undo_count` is the only
  field a resumable editor needs. Viewers that never undo can drop it entirely.
