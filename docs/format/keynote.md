# Keynote Document Tree

The structure of a Keynote (.key) document: root archive, show, slide tree, slides, and their builds/drawables, with registry type ids and proto message names.

All objects live in `Index.zip` as snappy-framed `.iwa` files; every object has a numeric identifier, and identifier **1** is the app-level document root — the Go index decodes object 1 as `KN.DocumentArchive` [parser: dunhamsteve/iwork@02c26ebf] index/keynote.go:11-14. Keynote uses the smallest type ids of the three apps (1-25 for content objects). Type ids below are from `.scratch/iwork/codegen/Keynote.json` and `Common.json` (captured from Keynote 6.0-era registries, so modern files may carry additional ids [parser: dunhamsteve/iwork@02c26ebf] codegen/README.md).

## Tree

```
Index.zip
└─ Index/*.iwa  (snappy-compressed proto archives)
   └─ Object 1: KN.DocumentArchive                     [1]
      ├─ show (2, required) → KN.ShowArchive           [2]
      ├─ super (3) → TSA.DocumentArchive (.scratch/otorp/Keynote/TSAArchives.proto → TSA.DocumentArchive)
      │  └─ super → TSK.DocumentArchive                [200]
      └─ tables_custom_format_list (4)

KN.ShowArchive [2]   (.scratch/otorp/Keynote/KNArchives.proto → KN.ShowArchive)
├─ slideTree (3, required) → KN.SlideTreeArchive
│  ├─ slides (2, repeated) → KN.SlideArchive          [5] (and [6], a second registered slide id)
│  └─ rootSlideNode (1, deprecated) → KN.SlideNodeArchive [4]
│     └─ children (1, repeated) → KN.SlideNodeArchive (recursive tree)
│        └─ slide (2) → KN.SlideArchive
├─ slideList (19) — newer flat list of slides; target message not defined in the
│  15.3.1 extraction [inferred: replacement for slideTree, type not exposed]
├─ theme (2, required) → KN.ThemeArchive              [10]
│  ├─ super → TSS.ThemeArchive                        [402]
│  ├─ templates (2, repeated) → KN.SlideArchive — master/template slides
│  └─ classicThemeRecords (4) → KN.ClassicThemeRecordArchive [20]
├─ stylesheet (5, required) → TSS.StylesheetArchive   [401]
├─ size (4, required) → TSP.Size — slide dimensions
├─ uiState (1) → KN.UIStateArchive                    [3]
├─ recording (7) → KN.RecordingArchive                [16]
│  ├─ event_tracks (1) → KN.RecordingEventTrackArchive  [17]
│  └─ movie_track (2) → KN.RecordingMovieTrackArchive   [18]
├─ soundtrack (17) → KN.Soundtrack                    [21]
├─ slideNumbersVisible (6), mode (9 → KNShowMode),
│  loop_presentation (8), autoplay_transition_delay (10), autoplay_build_delay (11)

KN.SlideArchive [5]/[6]   (.scratch/otorp/Keynote/KNArchives.proto → KN.SlideArchive)
├─ style (1, required) → KN.SlideStyleArchive         [9]
├─ builds (2, repeated) → KN.BuildArchive             [8]
│  └─ drawable (1), delivery (2, required), attributes (4 → KN.BuildAttributesArchive)
├─ transition (4, required) → KN.TransitionArchive
├─ titlePlaceholder (5) / bodyPlaceholder (6) / objectPlaceholder (30) /
│  slideNumberPlaceholder (20) → KN.PlaceholderArchive [7] (also [12])
├─ owned_drawables (7, repeated) — shapes, images, tables, text boxes on the slide
├─ drawables_z_order (42, repeated) — stacking order
├─ note (27) → KN.NoteArchive                         [15]
│  └─ containedStorage (1) → TSWP.StorageArchive      [2001] — presenter notes text
├─ template_slide (17) → KN.SlideArchive — the master slide this slide follows
├─ name (10), inDocument (19, required), staticGuides (18), userDefinedGuideStorage (36)
└─ classicStylesheetRecord (29) → KN.ClassicStylesheetRecordArchive [19]

KN.PlaceholderArchive [7]/[12]   (.scratch/otorp/Keynote/KNArchives.proto → KN.PlaceholderArchive)
├─ super (1, required) → TSWP.ShapeInfoArchive → TSD.ShapeArchive → TSD.DrawableArchive [3002]
└─ kind (2): kKindPlaceholder / kKindSlideNumberPlaceholder / kKindTitlePlaceholder /
   kKindBodyPlaceholder / kKindObjectPlaceholder
```

## Field notes

- **KN.DocumentArchive** [1] — `.scratch/otorp/Keynote/KNArchives.proto → KN.DocumentArchive`. Deliberately tiny: a `super` chain to the shared TSA/TSK document records (field 3), one required `show` reference (field 2), and `tables_custom_format_list` (4). Everything presentation-specific hangs off KN.ShowArchive.
- **KN.ShowArchive** [2] — `.scratch/otorp/Keynote/KNArchives.proto → KN.ShowArchive`. Holds the theme, the slide tree, the stylesheet, slide size, and playback/recording state. `slideTree` (field 3) → KN.SlideTreeArchive whose `slides` list (field 2) is the authoritative slide order; `rootSlideNode` (field 1) is the deprecated tree form. A newer `slideList` (field 19) exists alongside it.
- **KN.SlideNodeArchive** [4] — `.scratch/otorp/Keynote/KNArchives.proto → KN.SlideNodeArchive`. A navigator node per slide, not the slide content itself: `children` (1) forms the hierarchy (used for grouping/outdent), `slide` (2) points at the content, plus `isSkipped` (4), `hasBuilds` (6), `hasTransition` (7), `isSlideNumberVisible` (18), and cached thumbnails (`thumbnails` = 16, TSP.DataReferences).
- **KN.SlideArchive** [5]/[6] — `.scratch/otorp/Keynote/KNArchives.proto → KN.SlideArchive`. The slide content: placeholders for title/body/object/slide-number (fields 5, 6, 30, 20), user-added `owned_drawables` (7) with `drawables_z_order` (42) for stacking, animations via `builds` (2) and `buildChunks` (43), one `transition` (4), speaker notes via `note` (27), and a `template_slide` (17) link to its master. The registry registers two ids (5 and 6) that both decode to KN.SlideArchive [parser: dunhamsteve/iwork@02c26ebf] index/keynote.go:321-334 — likely normal vs. template slides [inferred: registry lists both ids with the same message].
- **Master slides** — live in KN.ThemeArchive.templates (field 2), each a KN.SlideArchive whose placeholders define title/body/object/slide-number positions; regular slides inherit geometry and styles from them via `template_slide` [proto: .scratch/otorp/Keynote/KNArchives.proto → KN.ThemeArchive { templates = 2 }].
- **Placeholders** — `.scratch/otorp/Keynote/KNArchives.proto → KN.PlaceholderArchive`. Extend TSWP.ShapeInfoArchive, so each placeholder owns its text via a TSWP.StorageArchive [2001] (see [pages.md](pages.md) for the storage layout). The `kind` enum distinguishes title/body/object/slide-number.
- **Builds and transitions** — KN.BuildArchive [8] binds a drawable (field 1) to a delivery (field 2, e.g. build-in/out) and its KN.BuildAttributesArchive; chunks (KN.BuildChunkArchive) allow staged builds. KN.TransitionArchive on field 4 holds the slide transition.
- **How keynote-parser walks this tree** — it does not hard-code a tree walk; it decodes every `.iwa` file generically and resolves each archive's type id through per-version registries bundled as descriptor sets extracted from the Keynote app binaries [parser: psobot/keynote-parser@56a4d3b0]: `keynote_parser/versions/archive.py` (`registry_for`, `compute_maps`), and `keynote_parser/codec.py` (`IWAFile.from_buffer`, `IWAArchiveSegment.from_buffer` resolving message classes at codec.py:375). Those registries were produced by attaching a debugger to the running Keynote app and dumping `TSPRegistry sharedRegistry` [parser: psobot/keynote-parser@56a4d3b0] dumper/extract_mapping.py:1-9, 54-63 — the same registry mechanism our codegen JSONs come from.
- **Decoding** — the Go index maps ids 1-25 (content) and 100-148 (commands) [parser: dunhamsteve/iwork@02c26ebf] index/keynote.go:9-354. The Go HTML converter takes a different route to the same slides: it reads TSP.PackageMetadata (object 2, id 11006), filters components whose `PreferredLocator == "Slide"`, sorts by identifier, and renders each KN.SlideArchive's bodyPlaceholder + drawables [parser: dunhamsteve/iwork@02c26ebf] iwork2html/iwork2html.go:563-608.

Cross-references: object-envelope mechanics and TSP.Reference resolution in [objects.md](objects.md); drawable geometry/styles in [drawables.md](drawables.md); TSWP.StorageArchive text layout in [pages.md](pages.md); stylesheets/themes in [styles.md](styles.md); chart drawables in [charts.md](charts.md). For comparison with the other apps, see [pages.md](pages.md) and [numbers.md](numbers.md).
