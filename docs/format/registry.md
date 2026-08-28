# Object Type-ID Registry

Maps the integer object type-ids carried by `TSP.MessageInfo.type` to protobuf message names, and documents the three available id→name tables and how much they disagree. (`TSP.ArchiveInfo.identifier` is the *object id* of the archive within its stream, not a type; it is the identifier namespace that per-app id tables are indexed alongside.)

## Where the ids live

Every unarchived object stream in an `.iwa` archive is prefixed by a `TSP.ArchiveInfo` message whose `message_infos` entries carry a `TSP.MessageInfo` with `type` (a uint32 object type-id), `length`, and the byte length of the payload to decode (`.scratch/otorp/Keynote/TSPArchiveMessages.proto → ArchiveInfo { identifier = 1; message_infos = 2 }`, `MessageInfo { type = 1; length = 3 }`; same fields in the older reference copy `.scratch/iwork/proto/TSPArchiveMessages.proto`). The `type` value selects which protobuf message the following payload decodes to; the mapping from type-id to message name is **not** in the protos — it lives in a separate table the apps keep at runtime (see [iwa.md](iwa.md) for stream framing).

The namespace is per-application: Keynote, Numbers, and Pages each ship their own table, with a large shared range (TSP/TSK/TST/TSD/TSWP/... prefixes) plus app-specific ids (KN./TN./TP.). [inferred: consistent across all three reference registries below]

## Registry 1: dunhamsteve/iwork JSON tables [parser: dunhamsteve/iwork@02c26ebf]

`.scratch/iwork/codegen/{Common,Keynote,Numbers,Pages}.json` are id→name maps. `.scratch/iwork/README.md` (lines 32-36) states they came from Sean Patrick O'Brien's (obriensp/iWorkFileFormat) extraction from the running apps via F-Script (`TSPRegistry sharedRegistry` — see the original README quoted in `.scratch/iwork/codegen/README.md`, taken from Keynote 6.0, Pages 5.0, Numbers 3.0; dunhamsteve only re-formatted them as valid JSON).

Counts (computed on 2026-08-28): Common.json **337** entries, Keynote.json **68**, Numbers.json **31**, Pages.json **47**. `Common.json` holds the shared TSP/TSK/TST/... range (ids 200+), each app JSON the app-specific small ids; the effective per-app table is Common + that app's JSON. `.scratch/iwork/codegen/codegen.go` consumes a JSON as a flat `{id: message-name}` map to emit a Go `decode(typ, payload)` switch (template at codegen.go:16-36), and the output is vendored into `.scratch/iwork/index/` (keynote.go, numbers.go, pages.go; iwork/README.md line 38).

## Registry 2: keynote-parser per-version registries [parser: psobot/keynote-parser@56a4d3b]

`.scratch/keynote-parser/protos/versions/{10.2,11.2,12.0,12.1,12.2.1,13.1,14.4,14.5}/registry.json` — one flat `{id: "MessageName"}` JSON per extracted Keynote version, written by `dumper/run.py` (`registry_path = os.path.join(proto_output_directory, "registry.json")`, run.py:202-214) from the app bundle's live registry, sitting beside the `.proto` files it describes (`protos/versions/14.5/` contains 33 `.proto` files + `registry.json`). At decode time it is indexed by `MessageInfo.type`: `klass = import_version(version)[0][message_info.type]` (`keynote_parser/codec.py:333`).

Entry counts per version: 10.2 → 576, 11.2 → 607, 12.0 → 609, 12.1 → 611, 12.2.1 → 629, 13.1 → 631, 14.4 → 631, 14.5 → 631; the 14.4 and 14.5 files are byte-identical as JSON maps. [inferred: counted/compared with python3 on 2026-08-28]

This is the fullest known registry. Prefix histogram for 14.5: TST 174, KN 95, TSWP 77, TSD 73, TSCH 64, TSCK 33, TSK 30, TSA 28, TSP 21, TSS 10, TSCE 10, plus SOS/sos-variant names. It contains **zero** TN.* and TP.* entries: it was extracted from Keynote, so Numbers- and Pages-specific types are absent — for those apps you must fall back to registry 1's Numbers.json/Pages.json. [inferred: counted prefixes with python3 on 2026-08-28]

**Implication: ids are not stable across app versions.** Between 10.2 and 14.5, 567 ids are shared and 40 of them changed name (e.g. id 119 `KN.CommandChangeMasterSlideArchive` → `KN.CommandChangeTemplateSlideArchive`; id 201 `TSK.CommandHistory` → `TSK.LocalCommandHistory`; id 4004 `TSCE.ReferenceTrackerArchive` → `TSCE.TrackedReferenceStoreArchive`). 14.4's registry carries 327 ids that do not exist in the dunhamsteve tables at all. [inferred: computed by comparing the two registries on 2026-08-28]

## Registry 3: litchi's hardcoded Rust table [parser: DevExzh/litchi@92293640]

`.scratch/litchi/crates/litchi-iwa/src/registry.rs` (305 lines) is a hand-written `MessageRegistry` (a single `HashMap<u32, MessageType>`, registry.rs:59, register = plain insert at :64-66) fed by five functions covering "Common/Keynote/Numbers/Pages/shared" groups. It contains 69 `register(...)` calls covering only 25 distinct ids (ids 1-7, 100-107, 110, 145-148, 200-203); because every group re-uses small ids in one map, later calls silently overwrite earlier ones (effective map ends up e.g. `1 → TSWP.DocumentArchive`, `100 → TP.CommandSetTextArchive`). Several comments contain literal question marks ("TSP - Telesphoreo?"), and the names look guessed rather than extracted. [inferred: replayed the register calls in file order with python3 on 2026-08-28]

Treat it as illustrative only: **none of its 25 effective id→name pairs matches the keynote-parser 14.5 registry**, and it is partial even by intent (25 ids vs 631). [inferred: computed by comparing the two on 2026-08-28]

## Drift analysis: dunhamsteve vs keynote-parser 14.4 [inferred: computed by comparing the two registries with python3 on 2026-08-28]

Comparing dunhamsteve's Keynote namespace (Common.json + Keynote.json, 405 distinct ids) against `.scratch/keynote-parser/protos/versions/14.4/registry.json` (631 ids):

- Shared ids: **304** — of these, **291 agree** on the message name and **13 disagree** (same id, different name).
- Only in keynote-parser 14.4: **327** ids.
- Only in dunhamsteve tables: **101** ids (older Keynote 6.0-era types since removed/renamed).

The 13 conflicts, with dunhamsteve name → keynote-parser 14.4 name:

| id | dunhamsteve (Keynote 6.0 era) | keynote-parser 14.4 |
|---|---|---|
| 111 | KN.CommandSlideMoveBuildChunkArchive | KN.CommandSlideMoveBuildChunksArchive |
| 119 | KN.CommandChangeMasterSlideArchive | KN.CommandChangeTemplateSlideArchive |
| 134 | KN.CommandMoveMastersArchive | KN.CommandMoveTemplatesArchive |
| 135 | KN.CommandInsertMasterArchive | KN.CommandInsertTemplateArchive |
| 140 | KN.CommandRemoveMasterArchive | KN.CommandRemoveTemplateArchive |
| 142 | KN.CommandMasterSetThumbnailTextArchive | KN.CommandTemplateSetThumbnailTextArchive |
| 144 | KN.CommandSlidePrimitiveSetMasterArchive | KN.CommandSlidePrimitiveSetTemplateArchive |
| 145 | KN.CommandMasterSetBodyStylesArchive | KN.CommandTemplateSetBodyStylesArchive |
| 146 | KN.CommandSlideReapplyMasterArchive | KNSOS.CommandSlideReapplyTemplateSlideArchive |
| 201 | TSK.CommandHistory | TSK.LocalCommandHistory |
| 215 | TSK.SetAnnotationAuthorColorCommandArchive | TSCK.SetAnnotationAuthorColorCommandArchive |
| 4004 | TSCE.ReferenceTrackerArchive | TSCE.TrackedReferenceStoreArchive |
| 6256 | TST.CommandNotifyForTransformingArchive | TST.CommandJustForNotifyingArchive |

Pattern: renames of Master→TemplateSlide, package moves such as `TSK.SetAnnotationAuthorColorCommandArchive` → `TSCK.SetAnnotationAuthorColorCommandArchive`, and outright renames like `TSCE.ReferenceTrackerArchive` → `TSCE.TrackedReferenceStoreArchive`. [inferred: listed from the python3 diff on 2026-08-28]

Restricting to Common.json alone (245 shared ids): 241 agree, only 4 disagree (201, 215, 4004, 6256) — the shared TSP-family namespace is far more stable than the app-specific one; drift concentrates in KN.* command archives.

## Drift risk: the 15.3.1 otorp extraction has no registry

The current-format protos in `.scratch/otorp/{Keynote,Numbers,Pages}/` were pulled with otorp from the installed 15.3.1 app binaries and carry **no id table** — otorp recovers message/field structure but not the runtime `TSPRegistry` type-id mapping (no registry.* file exists in any otorp directory). [inferred: directory listings of .scratch/otorp/Keynote/ and siblings on 2026-08-28]

Consequences:

1. Every id→name mapping in this doc set comes from the reference repos, whose newest data is keynote-parser 14.5 (fresher than dunhamsteve's Keynote 6.0-era tables; confirmed by the drift analysis above). Currency ranking: **keynote-parser 14.5 > dunhamsteve codegen JSONs > litchi registry.rs**.
2. The installed apps are 15.3.1, one major step beyond 14.5. Apple has repeatedly renamed/reassigned ids across versions (576 → 631 entries and 40 renames in the observed range), so 15.3.1 ids **may differ again**. [inferred: extrapolated from the observed 10.2→14.5 drift]
3. Recommendation for iwadump and any consumer: **treat unknown type-ids as opaque** — decode the payload as raw bytes and display the id in hex (e.g. `type 0x1a2b`), never guess a message name from a stale table, and prefer the 14.5 table for Keynote, Common.json+Numbers.json for Numbers, Common.json+Pages.json for Pages, with the caveat that Numbers/Pages ids are anchored to much older app versions and therefore the least trustworthy. [inferred]

## Cross-references

- Stream/archives framing that carries these ids: [iwa.md](iwa.md), [objects.md](objects.md).
- Per-app object models: [keynote.md](keynote.md), [numbers.md](numbers.md), [pages.md](pages.md).
- Known format traps incl. outdated header descriptions: [gotchas.md](gotchas.md).