# Pages Document Tree

The structure of a Pages (.pages) document: root archive, body storage, sections, page templates, and drawables, with registry type ids and proto message names.

All objects live in `Index.zip` as snappy-framed `.iwa` files; every object has a numeric identifier, and identifier **1** is always the app-level document root ([parser: dunhamsteve/iwork@02c26ebf] index/index.go:33-107, iwork2html/iwork2html.go:499). Type ids below are from `.scratch/iwork/codegen/Pages.json` (app-specific ids, from the obriensp/iWorkFileFormat registry) and `Common.json` (shared ids). The registry JSONs were captured from older app versions (Pages 5.0), so modern files may contain additional ids [parser: dunhamsteve/iwork@02c26ebf] codegen/README.md.

## Tree

```
Index.zip
└─ Index/*.iwa  (snappy-compressed proto archives, see gotchas in objects.md)
   └─ Object 1: TP.DocumentArchive                      [10000]
      ├─ super → TSA.DocumentArchive (.scratch/otorp/Pages/TSAArchives.proto → TSA.DocumentArchive { super = 1 })
      │  └─ super → TSK.DocumentArchive                 [200]  (.scratch/otorp/Pages/TSKArchives.proto → TSK.DocumentArchive)
      ├─ body_storage (4) → TSWP.StorageArchive         [2001] — main text (word-processing mode)
      ├─ section (5) → TP.SectionArchive                [10011] — page-layout mode content
      ├─ floating_drawables (3) → TP.FloatingDrawablesArchive [10010]
      │  └─ page_groups (1) → PageGroup { page_index, background/foreground/drawables → DrawableEntry { drawable = 1 } }
      ├─ drawables_zorder (20) → TP.DrawablesZOrderArchive [10015]
      ├─ stylesheet (2) → TSS.StylesheetArchive         [401]
      ├─ theme (6) → TP.ThemeArchive                    [10001]
      │  └─ super → TSS.ThemeArchive                    [402]
      ├─ settings (7) → TP.SettingsArchive              [10012]
      ├─ page_templates (48, repeated) → TP.PageTemplateArchive
      ├─ deprecated_layout_state (11) → TP.LayoutStateArchive  [10131]
      └─ deprecated_view_state (12) → TP.ViewStateArchive      [10133]

TSWP.StorageArchive [2001]  (body_storage, headers, footnotes, textboxes)
├─ kind (1): BODY / HEADER / FOOTNOTE / TEXTBOX / NOTE / CELL / TOC ...
├─ text (3, repeated string) — the character content
├─ style_sheet (2) → TSS.StylesheetArchive
├─ table_para_style (5), table_char_style (8), table_list_style (7), table_layout_style (12) — style runs by character index
├─ table_attachment (9) — entries → TSWP.DrawableAttachmentArchive [2003] → drawable (field 1)
│  └─ drawable → TSD.ImageArchive [3005] | TSD.ShapeArchive [3004] | TSD.GroupArchive [3008]
│              | TST.TableInfoArchive | TSWP.ShapeInfoArchive (textboxes) | TP.PlaceholderArchive [7]
└─ table_footnote (16), table_bookmark (15), table_section (17), table_smartfield (11) ...

TP.SectionArchive [10011]  (page-layout mode; one per "section" of the canvas)
├─ first_section_template_page (23) / even_section_template_page (24) / odd_section_template_page (25)
│  → page masters (old proto names: first_page_master/even_page_master/odd_page_master;
│    target type unnamed in extraction, likely TP.PageTemplateArchive [inferred])
├─ name (26), section_start_kind (20), section_page_number_kind (21), section_page_number_start (22)
├─ inherit_previous_header_footer (17), background_fill (30 → TSD.FillArchive)
└─ user_defined_guide_storage (29)

TP.PageTemplateArchive  (a page-master: headers/footers + master drawables)
├─ section_template_drawables (2, repeated) → drawables on the master page
├─ placeholder_drawables (3, repeated) → TagDrawablePair { tag, drawable, z_index }
├─ hide_headers_footers (5), guide_storage (7), background_fill (6 → TSD.FillArchive)
└─ headers_footers_match_previous_page (4, required)

TSWP.ShapeInfoArchive (textboxes; .scratch/otorp/Pages/TSWPArchives.proto → TSWP.ShapeInfoArchive { super = 1 })
├─ super → TSD.ShapeArchive → TSD.DrawableArchive [3002] { geometry = 1, parent = 2 }
└─ owned_storage (4) → TSWP.StorageArchive (the textbox text; is_text_box = 6)

TP.PlaceholderArchive [7] — template placeholder shape
└─ super → TSWP.ShapeInfoArchive → TSD.ShapeArchive
```

## Field notes

- **TP.DocumentArchive** [10000] — `.scratch/otorp/Pages/TPArchives.proto → TP.DocumentArchive`. Two content modes share this root: word-processing documents put the text in `body_storage` (field 4) and leave `section` empty; page-layout documents use `section` (field 5). `floating_drawables` (field 3) holds floating images/shapes/tables grouped by page. Document-level page geometry lives here: `page_width` (30), `page_height` (31), margins (32-37), `page_scale` (38), `orientation` (42). `super` (15) chains to the shared TSA/TSK document records.
- **TSA.DocumentArchive / TSK.DocumentArchive** — shared across all three apps. TSA adds `calculation_engine` (4), `view_state` (5), `custom_format_list` (12) (.scratch/otorp/Pages/TSAArchives.proto → TSA.DocumentArchive). TSK adds `locale_identifier` (4), `annotation_author_storage` (7) (.scratch/otorp/Pages/TSKArchives.proto → TSK.DocumentArchive).
- **TP.SectionArchive** [10011] — `.scratch/otorp/Pages/TPArchives.proto → TP.SectionArchive`. Most legacy per-section margin/paper fields (1-16) are marked `OBSOLETE_` — layout moved to TP.DocumentArchive. Active fields: the three page-template references (23-25), `inherit_previous_header_footer` (17), numbering controls (20-22), `name` (26).
- **Page templates** — `.scratch/otorp/Pages/TPArchives.proto → TP.PageTemplateArchive`. These are the "page masters": repeating headers/footers and background drawables. The registry also lists id 10143 = TP.PageMasterArchive, but that message is absent from the 15.3.1 extraction; it appears only in the older reference proto (.scratch/iwork/proto/TPArchives.proto → TP.PageMasterArchive { headers = 1, footers = 2, master_drawables = 3 }) — treat it as legacy [inferred: absent from current otorp extraction, present in older dunhamsteve proto].
- **TSWP.StorageArchive** [2001] — `.scratch/otorp/Pages/TSWPArchives.proto → TSWP.StorageArchive`. All text (body, headers, footers, footnotes, textboxes, table cells) is stored here: a flat string (field 3) plus "attribute tables" that map character indexes to styles, attachments, and fields. Attachments (field 9) are how images/tables/shapes sit inline in the text. Registry ids 2001 and 2005 both decode to TSWP.StorageArchive [parser: dunhamsteve/iwork@02c26ebf] index/common.go:48-71.
- **TP.PlaceholderArchive** [7] — template "dummy" shapes; extends TSWP.ShapeInfoArchive, which owns its text via `owned_storage` (.scratch/otorp/Pages/TSWPArchives.proto → TSWP.ShapeInfoArchive { super = 1, owned_storage = 4 }).
- **Decoding** — the Go index dispatches on type id: `index/pages.go` handles the TP ids (10000-10157, 7) and `index/common.go` the shared ids, then everything is reachable by dereferencing `TSP.Reference` identifiers against the loaded record map [parser: dunhamsteve/iwork@02c26ebf] index/index.go:110-115, index/pages.go:9-255. The Go HTML converter walks exactly the path shown above: Records[1] → body_storage → StorageArchive → paragraphs, and warns that floating drawables are not yet handled [parser: dunhamsteve/iwork@02c26ebf] iwork2html/iwork2html.go:492-526.

Cross-references: shared object-envelope mechanics and TSP.Reference resolution in [objects.md](objects.md); drawable message details in [drawables.md](drawables.md); inline tables (TST.TableInfoArchive → TST.TableModelArchive) in [tables.md](tables.md); stylesheets/themes in [styles.md](styles.md).
