# TSS styles — base archive, hierarchy, and theme presets

`TSS` (iWork Style System) defines the one base style record every app-specific
style wraps, the parent-chain inheritance model, the stylesheet and theme
containers, and the property-map delta commands used by TSWP/TST/TSD objects
(see [text.md](text.md), [objects.md](objects.md)).

## The base style archive

`[proto]` `.scratch/otorp/Keynote/TSSArchives.proto → TSS.StyleArchive` (identical in
`.scratch/iwork/proto/TSSArchives.proto`):

- `name = 1` (display name, e.g. "Body", "Title"), `style_identifier = 2` (stable
  string id).
- `parent = 3` — `TSP.Reference` to another style archive: the inheritance chain is a
  single-parent linked list. Unset properties resolve through it.
- `is_variation = 4`, `stylesheet = 5` — back-pointer to the owning
  `TSS.StylesheetArchive`.

Per-app style archives embed this base as field **1 (`super`)** and add a typed
properties payload plus `override_count = 10` (`[proto]` paths as noted):

- `TSWP.CharacterStyleArchive { super = 1, override_count = 10, char_properties = 11 }`
  where `char_properties` is `TSWP.CharacterStylePropertiesArchive` (bold, italic,
  `font_size`, `font_name`, `font_color`, underline/strikethru enums, capitalization,
  baseline shift, kerning/tracking, ligatures, font_features, writing_direction …) —
  `.scratch/otorp/Keynote/TSWPArchives.proto → TSWP.CharacterStyleArchive`,
  `TSWP.CharacterStylePropertiesArchive`.
- `TSWP.ParagraphStyleArchive { super = 1, override_count = 10, char_properties = 11,
  para_properties = 12 }` — paragraph payload
  `TSWP.ParagraphStylePropertiesArchive` (alignment, indents, `line_spacing`,
  `space_before/after`, tabs, `outline_level`, keep rules, `list_style` ref) — same file.
- `TSWP.ListStyleArchive { super = 1, label_types = 11, text_indents = 12, indents = 13,
  number_types = 15, strings = 16, images = 17, tiered_numbers = 25, … }` —
  per-level label arrays (`[proto]` same file).
- `TSWP.ColumnStyleArchive`, `TSWP.DropCapStyleArchive`, `TSWP.ShapeStyleArchive`
  (`super` here is `TSD.ShapeStyleArchive`, which itself wraps `TSS.StyleArchive`),
  `TSWP.TOCEntryStyleArchive { super = 1 (TSWP.ParagraphStyleArchive), toc_properties = 2 }`
  — same file.
- Text-frame fit: `TSWP.ShapeStylePropertiesArchive.shrink_to_fit = 1` (bool,
  Keynote's "shrink text on overflow"; `vertical_alignment = 2`) on the TSWP
  wrapper's OWN `shape_properties = 11`; the older `TSWP.ColumnStyleArchive`
  keeps the same pair in `column_properties = 11` at `shrink_to_fit = 2` /
  `vertical_alignment = 5` [proto: TSWPArchives.proto:495-513 + 468-493].
  Both resolve through the TSS parent chain; theme presets carry the flag on
  ancestors (fixture: f99b78dd's 78pt title shrinks into its 150pt box in
  Apple's render).
- Tables: `TST.TableStyleArchive { super = 1 (TSS.StyleArchive), table_properties = 11 }`
  and `TST.CellStyleArchive { super = 1, cell_properties = 11 }` —
  `.scratch/otorp/Keynote/TSTArchives.proto`.
- Drawables: `TSD.ShapeStyleArchive { super = 1, override_count = 10,
  shape_properties = 11 }` and `TSD.MediaStyleArchive` —
  `.scratch/otorp/Keynote/TSDArchives.proto`.

Property payloads use an explicit **null-flag pattern** for "explicitly unset"
(e.g. `font_name_null = 4` alongside `font_name = 5`, `font_color_null = 6` /
`font_color = 7`) — `[proto]` `TSWP.CharacterStylePropertiesArchive`. Absence of both
means "inherit from parent"; the null flag means "clear the inherited value".

## Inheritance semantics

- Effective properties = child payload overriding parent along `super.parent`, walking
  to the root. dunhamsteve merges one parent level explicitly when rendering:
  "Some properties are inherited (e.g. if you apply a style and then tweak it)" —
  `[parser: iwork@02c26ebf] iwork2html/iwork2html.go:318-329` (multi-level chains are
  unhandled — the code hits `panic("Need recursion here")` at `iwork2html.go:327`;
  treat deep chains as unverified).
- `override_count` tracks how many properties the instance overrides
  (`[proto]` per-app style archives, field 10).
- Named styles are looked up by `super.name` —
  `[parser: numbers-parser@3238795] src/numbers_parser/model.py:1735-1744` builds its
  style map by keying theme `paragraph_style_presets` entries on
  `self.objects[x.identifier].super.name`.

## Stylesheet

`[proto]` `.scratch/otorp/Keynote/TSSArchives.proto → TSS.StylesheetArchive`:

- `styles = 1` (repeated `TSP.Reference`), `identifier_to_style_map = 2` —
  repeated `IdentifiedStyleEntry { identifier = 1 (string), style = 2 }` for
  stable-id lookup.
- `parent = 3` (stylesheet inheritance), `is_locked = 4`, `can_cull_styles = 6`,
  `parent_to_children_style_map = 5` (`StyleChildrenEntry { parent = 1, children = 2 }`).
- Versioned style snapshots `styles_for_10_0 = 7` … `styles_for_15_3 = 26`
  (`VersionedStyles { styles, identifier_to_style_map, parent_to_children_style_map }`) —
  the theme's style set as of each iWork release.

Wiring: `TSWP.StorageArchive.style_sheet = 2` points at the stylesheet
(`[proto]` TSWPArchives.proto), each style's `super.stylesheet = 5` points back
(`[proto]` TSS.StyleArchive); numbers-parser re-links new styles by merging the
document stylesheet reference into `para_style.super.stylesheet` —
`[parser: numbers-parser@3238795] src/numbers_parser/model.py:1820-1824`.

## Theme and presets

`[proto]` `.scratch/otorp/Keynote/TSSArchives.proto → TSS.ThemeArchive`:

- `legacy_stylesheet = 1`, `theme_identifier = 3`, `document_stylesheet = 4`,
  `old_uuids_for_preset_replacements = 5` / `new_uuids_for_preset_replacements = 6`,
  `color_presets = 10`.
- App payloads attach via **proto2 extensions** on `TSS.ThemeArchive`:
  - `TSWP.ThemePresetsArchive` at extension **110**:
    `list_style_presets = 1`, `text_style_presets = 2`, `imported_text_style_presets = 3`,
    `toc_entry_style_presets = 4`, `character_style_presets = 6`,
    `paragraph_style_presets = 7`, `dropcap_style_presets = 8` —
    `.scratch/otorp/Keynote/TSWPArchives.proto → TSWP.ThemePresetsArchive`.
  - `TSD.ThemePresetsArchive` at extension **100**: gradient/image fill presets,
    `shadow_presets`, `line_style_presets`, `shape_style_presets`,
    `textbox_style_presets`, `image_style_presets`, `movie_style_presets` —
    `.scratch/otorp/Keynote/TSDArchives.proto → TSD.ThemePresetsArchive`.
  - `TSA.ThemePresetsArchive` at extension **210**: `caption_style_presets`,
    `svg_import_style_presets` — `.scratch/otorp/Keynote/TSAArchives.proto`.
- App themes subclass the base: `KN.ThemeArchive { super = 1 (TSS.ThemeArchive),
  templates = 2, classicThemeRecords = 4, … }` —
  `.scratch/otorp/Keynote/KNArchives.proto → KN.ThemeArchive`; Numbers/Pages have
  equivalent `TN./TP.ThemeArchive` subclasses
  (`.scratch/iwork/proto/TNArchives.proto`, `TPArchives.proto`).
- Document wiring: Keynote `KN.ShowArchive { theme = 2, stylesheet = 5 }` —
  `.scratch/otorp/Keynote/KNArchives.proto → KN.ShowArchive`; numbers-parser resolves
  the document theme via `self.objects[DOCUMENT_ID].theme` —
  `[parser: numbers-parser@3238795] src/numbers_parser/model.py:1736`.
- Text presets bundle a paragraph + list style:
  `TSWP.TextStylePresetArchive { preset_identifier = 1, paragraph_style = 2,
  list_style = 3 }` (`[proto]` TSWPArchives.proto); the Numbers app document lists
  them for the UI as `TSWP.TextPresetDisplayItemArchive { preset = 1,
  display_name = 2 }` via `TSA.DocumentArchive.text_preset_display_items = 2`
  (`.scratch/otorp/Keynote/TSAArchives.proto`).
- Preset mutations run through `TSS.ThemeAddStylePresetCommandArchive` /
  `ThemeRemoveStylePresetCommandArchive` / `ThemeMovePresetCommandArchive` /
  `ThemeReplaceColorPresetCommandArchive` (`[proto]` TSSArchives.proto).

## Property maps and style deltas

The live style archives store properties as typed fields (above); **property maps**
appear on the command/delta side:

- `TSS.CommandPropertyEntryArchive { property = 1 (uint32 property id), type = 2
  (PropertyType), integer_value = 3, float_value = 4, double_value = 5,
  string_value = 6, tsp_reference = 7 }` with extension `color = 8`; gathered in
  `TSS.CommandPropertyMapArchive.property_entries = 1` —
  `[proto]` `.scratch/otorp/Keynote/TSSArchives.proto` (with `ValueType`/`PropertyType`
  enums at the top of the file).
- `TSS.StyleUpdatePropertyMapCommandArchive { current_style = 2,
  style_with_old_property_map = 3, style_with_new_property_map = 4, style_diff = 7 }`
  swaps a style's property map — `[proto]` same file. TSWP mirrors it as
  `TSWP.StyleUpdatePropertyMapCommandArchive` (`.scratch/otorp/Keynote/
  TSWPCommandArchives.proto → TSWP.StyleUpdatePropertyMapCommandArchive`, super =
  TSS.StyleUpdatePropertyMapCommandArchive; registry id 2406 —
  `[parser: keynote-parser@56a4d3b0] protos/versions/14.4/registry.json:213`).
- Range-level styling deltas live in the SOS layer:
  `TSWP.CharacterStyleChangePropertyCommand_GArchive { super = 1
  (StorageActionCommandArchive), range_list = 2, change_list = 4 }` and the paragraph
  equivalent —
  `.scratch/iwork/proto/TSWPCommandArchives_sos.proto`; the per-property change sets
  (`TSWP.CharacterStylePropertyChangeSetArchive`, `…ParagraphStylePropertyChangeSetArchive`,
  `…ListStylePropertyChangeSetArchive`, `TSWP.StyleDiffArchive`) are in
  `.scratch/otorp/Keynote/TSWPArchives.sos.proto`, built from
  `TSSSOS.SpecSet{Bool,Color,Double,Integer,String}Archive` value/unset pairs —
  `.scratch/otorp/Keynote/TSSArchives.sos.proto`.
- TST reuses the map for cell diffs:
  `TST.CellDiffArchive { property_map_to_set = 1, property_map_to_reset = 2 }` with TST
  extensions on `TSS.CommandPropertyEntryArchive` (`import_warning_set = 500`,
  `format_and_value = 501`, `cell_border = 503`) —
  `.scratch/otorp/Keynote/TSTArchives.proto`.

## How styles are referenced from documents

Every style reference is a `TSP.Reference` (object-identifier pointer; see
[objects.md](objects.md), [iwa.md](iwa.md)):

- **TSWP text**: `table_para_style`, `table_char_style`, `table_list_style`,
  `table_drop_cap_style` entries → `ParagraphStyleArchive` /
  `CharacterStyleArchive` / `ListStyleArchive` / `DropCapStyleArchive`
  (`[proto]` `TSWP.StorageArchive`); resolved by
  `[parser: iwork@02c26ebf] iwork2html/iwork2html.go:314-316, 368-369`.
- **TST tables**: `TST.TableStyleNetworkArchive { body_text_style = 1,
  header_row_text_style = 2, header_column_text_style = 3, footer_row_text_style = 4,
  body_cell_style = 5, header_row_style = 6, …, table_style = 9, preset_id = 12 }` —
  `.scratch/otorp/Keynote/TSTArchives.proto`; per-cell overrides via
  `TST.DataStore`/bucket `cell_style` / `text_style` references
  (`TSTArchives.proto` header-bucket and cell messages).
- **TSD drawables**: `TSD.ShapeArchive { super = 1, style = 2, pathsource = 3 }` —
  `.scratch/otorp/Keynote/TSDArchives.proto → TSD.ShapeArchive`; visible in real
  archives as `style: { identifier: '7891' }` inside a shape's `super` block —
  `[parser: keynote-parser@56a4d3b0] tests/data/emoji-oneslide.py2.yaml:372-373`.
- **Charts** follow the same pattern: `TSCH.ChartStyleArchive { super = 1
  (TSS.StyleArchive), extensions 10000+ }` — `.scratch/otorp/Keynote/
  TSCHArchives.Common.proto → TSCH.ChartStyleArchive`; pasteboard copies carry the
  style network (`TSCH.StylePasteboardDataArchive { super = 1, style_network = 2 }` —
  `.scratch/otorp/Keynote/TSCHArchives.proto`).

Verification note: the five .iwa fixtures in `.scratch/keynote-parser/tests/data/`
were decoded during this phase and confirm storage-level references (para/char/list
style entries, `styleSheet` back-pointer, U+FFFC attachment entries); the
theme-extension layout (110/100/210) is proto-verified but not yet fixture-verified —
`[inferred: extension fields not present in the small local fixtures; confirm against
a Theme.iwa from the fixtures/ corpus]`.

Cross-reference: text storage tables that carry these references in
[text.md](text.md); object/reference addressing in [objects.md](objects.md);
format traps in [gotchas.md](gotchas.md).
