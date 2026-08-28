# TSWP text model — storage, paragraphs, runs, and fields

All rich text in iWork '13+ lives in `TSWP.StorageArchive` objects: one character
buffer plus per-range attribute tables that point at `TSS` styles, attachments,
and smart fields (see [styles.md](styles.md), [objects.md](objects.md)).

## Where text lives

A storage is referenced by the object that owns the text:

- Word-processing flow (Pages): `TSWP.FlowInfoArchive { text_storage = 1, textboxes = 2 }`
  — `.scratch/otorp/Keynote/TSWPArchives.proto → TSWP.FlowInfoArchive`. Reachable
  from `TSWP.ShapeInfoArchive { super = 1 (TSD.ShapeArchive), text_flow = 3,
  owned_storage = 4, is_text_box = 6 }` (same file, `TSWP.ShapeInfoArchive`).
- Keynote placeholders/text boxes: `KN.PlaceholderArchive` extends
  `TSWP.ShapeInfoArchive` (`.scratch/otorp/Keynote/KNArchives.proto → KN.PlaceholderArchive`),
  so slide text resolves `text_flow → TSWP.FlowInfoArchive.text_storage` (or the
  deprecated direct-storage path).
- Numbers table cells: `TST.RichTextPayloadArchive.storage` → `TSWP.StorageArchive`
  (`.scratch/otorp/Keynote/TSTArchives.proto → TST.RichTextPayloadArchive`), reached
  via the rich-text payload bucket on a cell. `[parser: numbers-parser@3238795]`
  `docs/Numbers.md:252-254` documents the caption chain
  `TSA.CaptionInfoArchive.owned_storage → TSWP.StorageArchive`.
- Footnote bodies: `TSWP.FootnoteReferenceAttachmentArchive.contained_storage = 2`
  (`.scratch/otorp/Keynote/TSWPArchives.proto → TSWP.FootnoteReferenceAttachmentArchive`).
- Speaker notes: `KN.NoteArchive.containedStorage = 1`
  (`.scratch/iwork/proto/KNArchives.proto → KN.NoteArchive`).

Storage objects are type ids **2001 and 2005** in the archive registry — both map
to `TSWP.StorageArchive` (`.scratch/iwork/codegen/Common.json` entries `"2001"`,
`"2005"`; `[parser: numbers-parser@3238795] src/numbers_parser/generated/mapping.py:191,195`).

## The storage archive

`[proto]` `.scratch/otorp/Keynote/TSWPArchives.proto → TSWP.StorageArchive` (fields below
also present in the older readable extraction `.scratch/iwork/proto/TSWPArchives.proto:78-121`):

- `kind = 1` enum `KindType`: BODY=0, HEADER=1, FOOTNOTE=2, TEXTBOX=3 (default),
  NOTE=4, CELL=5, UNCLASSIFIED=6, TABLEOFCONTENTS=7, UNDEFINED=8.
- `style_sheet = 2` → `TSP.Reference` to the owning `TSS.StylesheetArchive`.
- `text = 3` — `repeated string`. In every real fixture examined there is exactly
  **one** string holding the entire text of the storage
  (`[parser: iwork@02c26ebf] iwork2html/iwork2html.go:259-264` asserts
  `len(texts) == 1`; keynote-parser replacement machinery likewise reads
  `text[0]` — `[parser: keynote-parser@56a4d3b0] keynote_parser/replacement.py:61,93`).
- `has_itext = 4`, `in_document = 10`.
- ~20 attribute tables (see below): `table_para_style = 5`, `table_para_data = 6`,
  `table_list_style = 7`, `table_char_style = 8`, `table_attachment = 9`,
  `table_smartfield = 11`, `table_layout_style = 12`, `table_para_starts = 14`,
  `table_bookmark = 15`, `table_footnote = 16`, `table_section = 17`,
  `table_rubyfield = 18`, `table_language = 19`, `table_dictation = 20`,
  `table_insertion = 21`, `table_deletion = 22`, `table_highlight = 23`,
  `table_para_bidi = 24`, `table_overlapping_highlight = 25`,
  `table_pencil_annotation = 26`, `table_tatechuyoko = 27`,
  `table_drop_cap_style = 28`.

Table message shapes (`[proto]` same file):

- `TSWP.ObjectAttributeTable.ObjectAttribute { character_index = 1, object = 2 (TSP.Reference) }`
  — the workhorse: style refs, attachments, smart fields.
- `TSWP.StringAttributeTable.StringAttribute { character_index = 1, object = 2 (string) }`
  — used for `table_language` / `table_dictation` (e.g. language code `"en"`).
- `TSWP.ParaDataAttributeTable.ParaDataAttribute { character_index = 1, first = 2, second = 3 }`
  — used for `table_para_data`, `table_para_starts`, `table_para_bidi`.
- `TSWP.OverlappingFieldAttributeTable.OverlappingFieldAttribute { range = 1 (TSP.Range), field = 2 }`
  — overlapping highlights/pencil annotations.

## Paragraph model

- Paragraphs are delimited by newline characters **inside** `text[0]`; there is no
  paragraph struct. Verified in four keynote-parser fixtures decoded from real
  Keynote archives (e.g. `multiline-oneslide.iwa` text
  `"This is a multi-line text box with styles.\nThis line should be bolded entirely."`).
  `[parser: keynote-parser@56a4d3b0] tests/data/*.iwa`; its replacement code also
  re-derives paragraph entries by scanning for newlines —
  `[parser: keynote-parser@56a4d3b0] keynote_parser/replacement.py:61-78`.
- `table_para_style.entries[i].character_index` = start offset of paragraph *i*, and
  the entry's `object` is a `TSP.Reference` to a `TSWP.ParagraphStyleArchive`.
  `[proto]` `TSWP.StorageArchive.table_para_style` +
  `TSWP.ObjectAttributeTable`.
- A **null `object`** means "keep the previous paragraph's style" —
  `[parser: iwork@02c26ebf] iwork2html/iwork2html.go:290` ("A null style seems to
  imply 'use the previous class'"). Every storage seen in fixtures carries a
  `table_para_style` entry at `character_index = 0`.
- The run-to-paragraph mapping a renderer uses: paragraph *i* spans
  `[entries[i].character_index, entries[i+1].character_index)` (last one runs to end
  of text) — `[parser: iwork@02c26ebf] iwork2html/iwork2html.go:294-300`.
- Per-paragraph extras: `table_para_data` / `table_para_starts` / `table_para_bidi`
  (ParaData pairs), `table_list_style` → `TSWP.ListStyleArchive`,
  `table_drop_cap_style` → `TSWP.DropCapStyleArchive`. `[proto]` StorageArchive field list.
- Heading detection for HTML export uses
  `TSWP.ParagraphStylePropertiesArchive.outline_level = 27` —
  `[parser: iwork@02c26ebf] iwork2html/iwork2html.go:333-338`; `[proto]`
  `.scratch/otorp/Keynote/TSWPArchives.proto → TSWP.ParagraphStylePropertiesArchive`.

## Character runs

- `table_char_style.entries` split the text into styled runs. Run *i* spans
  `[entries[i].character_index, entries[i+1].character_index)` (last run to end of
  text) — `[parser: iwork@02c26ebf] iwork2html/iwork2html.go:343-394`.
- `object` is a `TSP.Reference` to `TSWP.CharacterStyleArchive`
  (`{ super = 1 (TSS.StyleArchive), override_count = 10, char_properties = 11 }` —
  `[proto]` `.scratch/otorp/Keynote/TSWPArchives.proto → TSWP.CharacterStyleArchive`).
  A null `object` run means "no character-level override" — dunhamsteve emits plain
  text for it (`iwork2html.go:391-393`).
- `TSWP.ParagraphStyleArchive` additionally carries `para_properties = 12`
  (`[proto]` `TSWP.ParagraphStyleArchive`), so paragraph styles can also set
  character-level properties for the whole paragraph.

## Attachments

- Inline/anchored objects occupy a position in the text via `table_attachment`
  entries. Inline image/movie/shape attachments are marked in the text by a single
  **U+FFFC OBJECT REPLACEMENT CHARACTER** at `character_index`; the entry's `object`
  is the attachment archive. Verified in fixture storages:
  `text: ["\uFFFC"]` + `tableAttachment.entries[0].characterIndex = 0` pointing at a
  type-2043 object — `[parser: keynote-parser@56a4d3b0] tests/data/emoji-oneslide.py2.yaml:408-444`
  (round-trip of a real Keynote archive).
- `TSWP.DrawableAttachmentArchive { drawable = 1, h_offset_type = 2, h_offset = 3,
  v_offset_type = 4, v_offset = 5 }` — anchored drawables with offset semantics
  (`[proto]` `.scratch/otorp/Keynote/TSWPArchives.proto → TSWP.DrawableAttachmentArchive`).
  dunhamsteve resolves `entry.Object → DrawableAttachmentArchive.drawable` and renders
  the drawable at that point (`[parser: iwork@02c26ebf] iwork2html/iwork2html.go:273-287`).
- Textual attachments: `TSWP.TextualAttachmentArchive { string_equivalent = 1,
  kind = 2 }` with `Kind { kKindPageNumber = 0, kKindPageCount = 1, kKindFootnoteMark = 2 }`;
  subtypes `TSWPTOCPageNumberAttachmentArchive`, `NumberAttachmentArchive`
  (slide/page numbers; type id 2043), `FootnoteReferenceAttachmentArchive`
  (`contained_storage = 2` → footnote body storage), `TOCAttachmentArchive` —
  `[proto]` same file, messages `TextualAttachmentArchive` … `NumberAttachmentArchive`.

## Fields (smart fields)

`table_smartfield` entries point at the `TSWP.SmartFieldArchive` family
(`[proto]` `.scratch/otorp/Keynote/TSWPArchives.proto`):

- `SmartFieldArchive { text_attribute_uuid_string = 1 }` — base.
- `HyperlinkFieldArchive { super = 1, url_ref = 2 }` — URL fields;
  `UnsupportedHyperlinkFieldArchive` adds `url_original_ref = 3`.
- `DateTimeSmartFieldArchive` (`update_plan = 6`: never/auto/once), `BookmarkFieldArchive`
  (`name = 2`, `ranged = 3`), `PlaceholderSmartFieldArchive` (localizable placeholders),
  `FilenameSmartFieldArchive`, `MergeSmartFieldArchive` (mail merge), `TOCSmartFieldArchive`,
  `RubyFieldArchive` (ruby_text), `TateChuYokoFieldArchive`,
  `BibliographySmartFieldArchive` / `CitationSmartFieldArchive` (`citation_records`).
- Overlapping highlights/pencil annotations use `table_overlapping_highlight` /
  `table_pencil_annotation` with explicit `TSP.Range` instead of a start index
  (`[proto]` `TSWP.OverlappingFieldAttributeTable`).

## Unicode handling — disk vs. logical text

- The protobuf `text` string is plain UTF-8 with real characters: supplementary-plane
  characters (emoji, flags) are stored as actual 4-byte UTF-8, not surrogate escapes.
  Verified by decoding `tests/data/multiline-surrogate.iwa`: `text[0]` contains the
  flag pair U+1F1E8 U+1F1E6 as raw 4-byte UTF-8 (the full string is 110 UTF-8 bytes
  for 104 code points).
  `[parser: keynote-parser@56a4d3b0] tests/data/multiline-surrogate.iwa` (fixture).
- **Attribute-table offsets are UTF-16 code-unit indices, not code points.** Verified
  against `multiline-surrogate.iwa`: `tableParaStyle` entries are `0, 67, 72` while the
  newlines sit at code points 66 and 69 with two astral chars between them — pure
  code-point indexing would give 0/67/70; UTF-16 gives 0/67/72. This matches
  keynote-parser, which adds +1 to paragraph offsets per character `> 0xFFFF`
  (`[parser: keynote-parser@56a4d3b0] keynote_parser/replacement.py:64-70`,
  `correct_multiline_replacement`). `[inferred: single-fixture verification — confirm
  on more astral-bearing documents]`.
- Parser discrepancy to beware: dunhamsteve treats the same offsets as **code points**
  (`rr := []rune(text)` — `[parser: iwork@02c26ebf] iwork2html/iwork2html.go:266-267`),
  which diverges from keynote-parser only when text contains supplementary-plane
  characters. litchi does no offset math at all and slices by **bytes**
  (`self.text[run.offset..end]` — `[parser: litchi@92293640]
  crates/litchi-iwa/src/text/storage.rs:49-64`), which would also mis-slice astral text.
- keynote-parser's YAML intermediate can contain **literal `\uD83C\uDDE8`-style
  surrogate-pair escapes** (written by the original Python 2 pipeline; see
  `tests/data/emoji-oneslide.py2.yaml:181-182`). `unicode_utils.py` rewrites valid
  high/low pairs to a single `\U0001F1E8`-style escape on load and leaves BMP escapes,
  lone surrogates, and wrong-order pairs untouched —
  `[parser: keynote-parser@56a4d3b0] keynote_parser/unicode_utils.py:24-55` and
  `keynote_parser/file_utils.py:230` (`yaml.load(fix_unicode(...))`).
- Tab characters are stored literally in `text` (no tab marker table exists; tab stops
  are style properties — `TSWP.ParagraphStylePropertiesArchive.tabs = 25` /
  `default_tab_stops = 4`, `[proto]`). `[inferred: no fixture with tabs in the local
  set; verify against a tab-heavy document]`.
- Logical-text extraction is lossy: a renderer must also splice in attachment
  placeholders (U+FFFC / textual `string_equivalent`) and field text to reproduce what
  the user sees. dunhamsteve splices drawable attachments at their paragraph positions
  (`[parser: iwork@02c26ebf] iwork2html/iwork2html.go:272-309`).

## Parser behavior notes (litchi)

litchi's `text` module is a simplified extractor, not a faithful model:
`TextExtractor::extract_from_bundle` scans a loose type-id list (`200-205`,
`2001-2005`, `2011`, `2012`, `2022` — which sweeps in Selection/attachment/style
archives, not just storages), pulls `extract_text()` from decoded messages, and joins
the `text`
repeated field with `\n` into a flat string without resolving any attribute tables
(`[parser: litchi@92293640] crates/litchi-iwa/src/text/extractor.rs:25-55` and
`text/storage.rs:118-125`). Its `TextRun { offset, length, style }` model is defined
but never populated from `table_char_style`. Treat litchi text output as a
plain-text fallback only.

Cross-reference: style objects and inheritance are covered in
[styles.md](styles.md); snappy framing of the `.iwa` streams that carry storages in
[iwa.md](iwa.md); known format traps in [gotchas.md](gotchas.md).

## Style payload field shapes (added for the JSON model)

### Alignment — the opaque TATvalue enum, mapped [parser: masaccio/numbers-parser@32387958]

`TSWP.ParagraphStylePropertiesArchive.TextAlignmentType` is named TATvalue0..4
with no semantic names in the proto. numbers-parser pins the mapping
(src/numbers_parser/cell.py:145-149): **TATvalue0 = LEFT, TATvalue1 = RIGHT,
TATvalue2 = CENTER, TATvalue3 = JUSTIFIED, TATvalue4 = AUTO**. Note the
surprising 1=right / 2=center order — do not "fix" it by intuition.

Vertical alignment uses `TSWP.ShapeStylePropertiesArchive.VerticalAlignmentType`
(TSWPArchives.proto:496-503): kFrameAlignTop=0 / Middle=1 / Bottom=2 /
Justify=3. `TST.CellStylePropertiesArchive.vertical_alignment` is an int32
with the same 0..3 order [inferred: same enum family, no fixture check yet].

### Character style payload [proto] TSWPArchives.proto:140-201 → TSWP.CharacterStylePropertiesArchive

bold(1), italic(2), font_size(3), font_name(5) with `font_name_null`(4),
font_color(7) with null flag(6), language(9), superscript(10), underline(11:
none/single/double/wavy), strikethru(12: none/single/double/triple),
capitalization(13: none/all-caps/small-caps/title), baseline_shift(14),
kerning(15), ligatures(16), outline(19)+outline_color(18), shadow(21),
strikethru_color(23)/width(24), background_color(26), tracking(27),
underline_color(29)/width(30), word_strikethru(31), word_underline(32),
font_features(34, repeated FontFeatureArchive), writing_direction(35),
emphasis_marks(37), compatibility_font_name(39), tate_chu_yoko(42),
tsd_stroke(44), tsd_fill(46). Most fields pair with an explicit `*_null`
boolean — the TSS null-flag pattern of [styles.md](styles.md) applies
throughout.

### Paragraph style payload [proto] TSWPArchives.proto:227-298 → TSWP.ParagraphStylePropertiesArchive

alignment(1), decimal_tab(3), default_tab_stops(4), fill(6 — paragraph
background), first_line_indent(7), hyphenate(8), keep_lines_together(9),
keep_with_next(10), left_indent(11), line_spacing(13), page_break_before(14),
deprecated_borders(15), right_indent(19), space_after(20), space_before(21),
tabs(25, TSWP.TabsArchive), widow_control(26), outline_level(27, uint32 —
heading depth per the iwork2html heading detection), outline_style(28),
following_style_id(30), stroke(32, paragraph border), show_in_toc(33),
writing_direction(38), list_style(40, TSP.Reference), border_positions(45).

- `TSWP.LineSpacingArchive` (448-460): `mode`
  kRelativeLineSpacing=0/kMinimum=1/kExact=2/kMaximum=3/kSpaceBetween=4 +
  `amount` (float; relative mode = multiple of line height [inferred]).
- `TSWP.TabArchive` (411-420): position(float), alignment
  left=0/center=1/right=2/decimal=3, `leader` (string — the fill characters).

### List styles [proto] TSWPArchives.proto → TSWP.ListStyleArchive

`label_types` (LabelType: none=0/image=1/string=2/number=3) per level,
`number_types` — a ~65-value NumberType enum covering decimal, roman
(upper/lower, 3 bracket styles each), alpha (upper/lower, 3 styles), and ~40
locale-specific kinds (hiragana, katakana, iroha, ideographic JP/SC/TC,
Korean, circled, Arabian, Hebrew), plus `strings` (literal marker text),
`indents`/`text_indents`, and image labels (DataReference). A converter can
map the latin/roman/alpha kinds by name and should degrade exotic locale
kinds to a generic numbered marker rather than fail [inferred: policy].
