# TSD Drawables — shapes, images, masks, movies, groups

Everything the user can select and drag on a canvas (shape, image, mask, movie,
group, connection line, chart, even a Numbers table) is a `TSD` drawable: a
`TSD.DrawableArchive` base record carrying geometry, wrapped by a subclass message
via a required `super` field, and placed into a canvas through `TSP.Reference`
containment lists. Provenance: the 15.3.1 proto extraction at
`.scratch/otorp/Keynote/TSDArchives.proto` (`package TSD`), cross-checked against
`.scratch/iwork/proto/TSD/TSDArchives.pb.go` and parser behavior in
dunhamsteve/iwork and DevExzh/litchi.

## The drawable base pattern

`TSD.DrawableArchive` (`.scratch/otorp/Keynote/TSDArchives.proto:321-335`) [proto]:

- `geometry = 1` — inline `TSD.GeometryArchive`.
- `parent = 2` — `TSP.Reference` to the containing drawable (a group, or the
  canvas owner's container).
- `exterior_text_wrap = 3` — `TSD.ExteriorTextWrapArchive` (lines 466-473:
  `type`/`direction`/`fit_type`/`margin`/`alpha_threshold`/`is_html_wrap`) for
  wrap-around-object text flow. [proto]
- `hyperlink_url = 4`, `locked = 5`, `comment = 6` (`TSP.Reference`),
  `aspect_ratio_locked = 7`, `accessibility_description = 8`,
  `pencil_annotations = 9` (repeated `TSP.Reference`), `title = 10`,
  `caption = 11`, `title_hidden = 12`, `caption_hidden = 13`. [proto]

Geometry lives in `TSD.GeometryArchive` (lines 21-26): `position = 1`
(`TSP.Point { x = 1, y = 2 }`, floats — `TSPMessages.proto:45-48`),
`size = 2` (`TSP.Size { width = 1, height = 2 }` — `TSPMessages.proto:61-64`),
`flags = 3` (uint32 bitfield; semantics not documented in any local proto
[inferred: flags exist but no local source defines the bits]),
`angle = 4` (float, **degrees** [inferred→fixture-verified 2026-08-29: the
24_Briefing.key master's tick rules store `angle = 90.0` and Keynote's own PDF
export renders them vertical — 90 radians would display as ≡116.6°]). [proto]

Every concrete drawable embeds the base as `super` (required, field 1) — the
Apple-protobuf subclassing idiom used throughout iWork (same pattern as
`TST.TableInfoArchive` → `TSD.DrawableArchive` in [tables.md](tables.md)):

- `TSD.ShapeArchive { super = 1, style = 2, pathsource = 3, ... }` (lines 365-372). [proto]
- `TSD.ImageArchive { super = 1, ... }` (lines 382-407). [proto]
- `TSD.MaskArchive { super = 1, pathsource = 2 }` (lines 409-412). [proto]
- `TSD.MovieArchive { super = 1, ... }` (lines 427-464). [proto]
- `TSD.GroupArchive { super = 1, children = 2, fake_shape_for_empty_group = 3 }`
  (lines 343-348). [proto]
- `TSD.ConnectionLineArchive { super (a ShapeArchive) = 1, connected_from = 2,
  connected_to = 3, connected_to_uuid = 4, connected_from_uuid = 5 }`
  (lines 374-380) — the two endpoints are `TSP.Reference`s to other drawables,
  with UUID alternatives for anchor tracking. [proto]
- `TSCH.ChartDrawableArchive { super (DrawableArchive) = 1 }`
  (`.scratch/otorp/Keynote/TSCHArchives.proto:14-17`) — charts are drawables
  too; see [charts.md](charts.md). [proto]

Registry type ids (needed to decode each payload; see [registry.md](registry.md))
— [parser: dunhamsteve/iwork@02c26eb] `.scratch/iwork/index/common.go:460-480`:
3002 = `TSD.DrawableArchive`, 3003 = `TSD.ContainerArchive`, 3004 = `ShapeArchive`,
3005 = `ImageArchive`, 3006 = `MaskArchive`, 3007 = `MovieArchive`,
3008 = `GroupArchive`, 3009 = `ConnectionLineArchive`. litchi independently
dispatches on 3004/3005 for shapes
([parser: DevExzh/litchi@9229364] `.scratch/litchi/crates/litchi-iwa/src/shapes/text_extractor.rs:139-141`).

## Containment: how drawables live on a canvas

There is no per-canvas archive message in TSD itself — `TSD.CanvasArchive` does
not exist in the 15.3.1 extraction [proto: absent from
`.scratch/otorp/Keynote/TSDArchives.proto`; the nearest analogues are
`TSD.ContainerArchive` (lines 337-341: `geometry`, `parent`, `children =
3 (repeated TSP.Reference)`) and `TSD.CanvasSelectionArchive` (lines 542-546,
whose deprecated field 2 `container` suggests earlier versions had a real
container object)]. Canvases are owned by app-level objects:

- Keynote: `KN.SlideArchive` owns its drawables with two lists —
  `owned_drawables = 7` and `drawables_z_order = 42` (both repeated
  `TSP.Reference`, `.scratch/otorp/Keynote/KNArchives.proto:250-251`) [proto].
  The z-order list is the paint order [inferred: name plus parallel owned list;
  confirm fixture-verified]. `KN.BuildArchive.drawable = 1` also references a
  drawable for build animations (KNArchives.proto:194-200). [proto]
- Numbers: `TN.SheetArchive.drawable_infos = 2` (repeated `TSP.Reference`,
  `.scratch/iwork/proto/TN/TNArchives.pb.go:559`) [parser:
  dunhamsteve/iwork@02c26eb]; the 15.3.1 otorp Keynote extraction has no
  TNArchives.proto, so the Go proto is the citation for Numbers sheets.
- Groups: `TSD.GroupArchive.children` (repeated `TSP.Reference`) nest drawables
  recursively; each child's `DrawableArchive.parent` points back up. [proto]
  Child geometry is stored in GROUP-LOCAL coordinates (origin = the group's
  own `geometry.position`), NOT canvas-absolute. [inferred→fixture-verified:
  G2-golden-pages-layout.pages plus 6 crawl .key decks — every raw child
  position lands inside [0, group size]; subtracting the group origin again
  visibly threw G2's grouped block arrow to the canvas corner.]

A table on a Numbers sheet is itself a drawable: `TST.TableInfoArchive.super`
is a required `TSD.DrawableArchive` — see [tables.md](tables.md).

## Shapes and path sources

`TSD.ShapeArchive` (TSDArchives.proto:365-372): `style = 2` is a `TSP.Reference`
to a `TSD.ShapeStyleArchive`, `pathsource = 3` an inline `TSD.PathSourceArchive`;
fields 4/5 (`head_line_end`, `tail_line_end`) are deprecated duplicates of style
properties. `strokePatternOffsetDistance = 6` (float). [proto]

`TSD.PathSourceArchive` (lines 98-109) is a one-of style union — exactly one of
the source fields is meaningful [inferred: fields are optional siblings, app
code picks the first present]:

- `point_path_source = 3` → `TSD.PointPathSourceArchive` (lines 28-39): preset
  arrow/star/plus shapes, enum `PointPathSourceType` (LeftSingleArrow = 0,
  RightSingleArrow = 1, DoubleArrow = 10, Star = 100, Plus = 200), plus a
  `point` and `naturalSize`. [proto]
- `scalar_path_source = 4` → `TSD.ScalarPathSourceArchive` (lines 41-51):
  parameterized shapes (RoundedRectangle = 0, RegularPolygon = 1, Chevron = 2)
  with a float `scalar` (corner radius / pointiness [inferred: semantic]) and
  `naturalSize`. [proto]
- `bezier_path_source = 5` → `TSD.BezierPathSourceArchive` (lines 53-57):
  deprecated `path_string = 1` plus modern `path = 3` (`TSP.Path`,
  `TSPMessages.proto:103`). [proto]
- `callout_path_source = 6` → `TSD.CalloutPathSourceArchive` (lines 59-65):
  `natural_size`, `tail_position`, `tail_size`, `corner_radius`, `center_tail`. [proto]
- `connection_line_path_source = 7` → `TSD.ConnectionLinePathSourceArchive`
  (lines 67-76): quadratic or orthogonal routing between two drawables. [proto]
- `editable_bezier_path_source = 8` → `TSD.EditableBezierPathSourceArchive`
  (lines 78-96): user-editable node list — repeated `Subpath { nodes = 1,
  closed = 2 }`, each `Node` carrying `inControlPoint`, `nodePoint`,
  `outControlPoint`, and `NodeType` (sharp = 1, bezier = 2, smooth = 3). [proto]
- `horizontalFlip = 1` / `verticalFlip = 2` flip the source; `localizationKey = 9`
  and `userDefinedName = 10` label preset shapes. [proto]

Shape styling: `TSD.ShapeStyleArchive` (lines 279-283) embeds
`TSS.StyleArchive super = 1` (stylesheet machinery — see
[registry.md](registry.md)) plus `shape_properties = 11`, an inline
`TSD.ShapeStylePropertiesArchive` (lines 269-277: `fill = 1` (`TSD.FillArchive`,
lines 158-163: color, `GradientArchive`, `ImageFillArchive`), `stroke = 2`
(`TSD.StrokeArchive`, lines 177-192), `opacity`, `shadow`, `reflection`,
line ends). [proto]

Shape text content is *not* in the shape: a text box is a
`TSWP.ShapeInfoArchive` (`super` = required `TSD.ShapeArchive`, `text_flow = 3`
/ `owned_storage = 4` referencing a `TSWP.StorageArchive`, `is_text_box = 6` —
`.scratch/otorp/Keynote/TSWPArchives.proto:697-704`) [proto]. litchi confirms
the split: geometry/styling in the shape, text in the separately-referenced
storage ([parser: DevExzh/litchi@9229364]
`.scratch/litchi/crates/litchi-iwa/src/shapes/text_extractor.rs:12-16,110-133` —
its `parse_shape_text` falls back to `accessibility_description` because full
text requires traversing the storage references).

## Images: TSD.ImageArchive media fields

`TSD.ImageArchive` (TSDArchives.proto:382-407). Two generations of media
pointers coexist — modern files use `TSP.DataReference` (a bare
`identifier = 1` uint64 into the data-id space, `TSPMessages.proto:32-34`),
legacy files use `TSP.Reference` into the object graph:

- `data = 11` (`TSP.DataReference`) vs `database_data = 2` (`TSP.Reference`) —
  the primary image bytes. [proto]
- `thumbnailData = 12` / `database_thumbnailData = 6`,
  `originalData = 13` / `database_originalData = 8` (original, pre-adjustment
  bytes), `originalSVGData = 23`, `enhancedImageData = 17`,
  `adjustedImageData = 15`, `thumbnailAdjustedImageData = 16`. [proto]
- `style = 3` → `TSD.MediaStyleArchive` (lines 292-296; media properties =
  stroke/opacity/shadow/reflection, lines 285-290). [proto]
- `originalSize = 4` and `naturalSize = 9` (`TSP.Size`) — pixel size vs
  displayed natural size [inferred: naming convention]. [proto]
- `mask = 5` — `TSP.Reference` to a `TSD.MaskArchive` (drawable with a
  `pathsource`), cropping the image to a path. [proto]
- `instantAlphaPath = 10` (`TSP.Path`): the region that stays visible after
  Instant Alpha, a closed path (several subpaths for islands) in
  `naturalSize` pixel space — icecube c3582f31 stores 1 move, 18 lines and
  313 cubics spanning 37-766 x 0-1285 on a 768x1284 image, and Keynote's
  export draws only the inside. pnk2json emits it as
  `ImageDrawable.instantAlphaPath`. [proto for the field; inferred for the
  space and the keep-inside semantics, one deck, 2026-09-05]
- `traced_path = 19` (`TSP.Path`), `flags = 7`,
  `imageAdjustments = 14` (`TSD.ImageAdjustmentsArchive`, lines 252-267:
  exposure/saturation/contrast/... gamma), `attribution = 20`,
  `background_removed = 22`, `should_trace_pdf_content = 21`,
  `interpretsUntaggedImageDataAsGeneric = 18`. [proto]

Equation images. Insert > Equation stores a `TSD.ImageArchive` whose data is
a PDF (`equation-N.pdf`, Quartz-rendered, no raster twin) plus
`TSWP.EquationInfoArchive` extension fields on the ImageArchive
(TSWPArchives.proto: `extend .TSD.ImageArchive`): `equation_source_old = 100`
(string), `equation_text_properties = 101`
(`TSWP.CharacterStylePropertiesArchive`, inline: font_size 3, font_name 5,
font_color 7), `equation_depth = 102` (float), `equation_source_text = 103`
(string). [proto] The source text is the expression as typed — LaTeX in every
corpus deck sampled (52 of 484 Keynote decks carry equation members; e.g.
`P(T \cap C) = P(C)P(T|C)`), MathML being the other input Keynote accepts;
100 and 103 are identical when both are present, older files carry only
100. `equation_depth` is the baseline depth in points for inline placement
(the image's bottom edge sits that far below the text baseline).
[fixture-verified: atnf 0ddd627b, kcsrk-adjacent decks in the survey]

Inline equation display size. The stored PDF is set in STIXGeneral at
`font_size` (e.g. 45pt glyphs on a 203x40pt page), and the ImageArchive
geometry equals that page. Keynote does not draw an inline equation at
that size: its PDF export re-sets the same expression at
`font_size * xheight(font_name) / xheight(STIXGeneral-Italic)`, i.e. so the
math's x-height matches the run font's. Measured: HelveticaNeue 45 -> 54.36pt
(x-height 0.517 / 0.428 = 1.208), HelveticaNeue-Light 40 -> 48.88 (0.523,
1.222), AvenirNext-Regular 50 -> 54.67 (0.468, 1.093), TimesNewRomanPS-
ItalicMT 30 -> 30.15 (0.4302, 1.005); each matches the CoreText x-height
ratio to four digits. Canvas-level equations (not inside a text storage)
are drawn 1:1 at their geometry. pnk2json applies the factor to inline
equations from a table of x-heights (drawables.rs `FONT_X_HEIGHT`) and
records it in `equation.displayScale`. [inferred: exports of 0ddd627b and
3775cc34, 2026-09-05]

Per-data metadata rides on the data object itself:
`TSD.ImageDataAttributes` (lines 414-425) extends `TSP.DataAttributes` via
extension field 100 with `pixel_size`, `image_is_srgb`,
`media_library_asset_id`, etc. [proto]

`TSD.MovieArchive` (lines 427-464) mirrors the pattern: `movieData = 14`
(`TSP.DataReference`; legacy `database_movieData = 2`), `posterImageData = 15`,
`audioOnlyImageData = 16`, `movieRemoteURL = 17`, trim range `startTime = 3` /
`endTime = 4` / `posterTime = 5`, `loop_option = 24` (None/Repeat/BackAndForth),
`volume = 7`, `audioOnly = 9`, `streaming = 18`, `is_live_video = 30`. [proto]

## Freehand drawings

`TSD.FreehandDrawingArchive` (lines 355-363) extends `TSD.GroupArchive` via
extension field 100: `spacer_shape = 1` (`TSP.Reference`), `opacity = 2`,
`animation = 3` (`FreehandDrawingAnimationArchive`: `duration`, `should_loop`),
`last_clamped_scale = 4`. The actual strokes are group children (shape
drawables) [inferred: group-container semantics; strokes not separately
modeled in this proto]. [proto]

## Selection

`TSD.DrawableSelectionArchive` (lines 548-551) is the base selection record;
`TSD.GroupSelectionArchive` (553-557) and `TSD.PathSelectionArchive`
(558-560) extend it; `TSD.CanvasSelectionArchive` (542-546) holds
`infos = 1` + `non_interactive_infos = 3` for multi-select. Chart selection
adds `TSCH.ChartSelectionArchive` with a `super = 3` back-reference — see
[charts.md](charts.md). [proto]

## Visual property payloads — verified field shapes (added for the JSON model)

The sections below record the exact proto shapes a converter needs to emit
resolved styles. All claims are from the 15.3.1 otorp extraction unless noted.

### TSP.Color [proto] .scratch/otorp/Keynote/TSPMessages.proto → TSP.Color

- `model = 1`: `rgb = 1` / `cmyk = 2` / `white = 3`; `rgbspace = 12`:
  `srgb = 1` / `p3 = 2` (wide gamut exists in the format).
- rgb form: `r/g/b` floats 0..1 (fields 3-5); `a` float default **1** (field 6).
- `headroom` float default **1** (field 13) — HDR headroom on top of the
  rgb channels.
- cmyk form: `c/m/y/k` (7-10); white form: `w` (11).
- Not documented anywhere else in this doc set before this addition; a
  converter must handle all three models plus p3/headroom (or degrade with a
  warning) [inferred: conversion policy is ours].

### TSP.Path — the universal curve form [proto] TSPMessages.proto → TSP.Path

`Element { type = 1, points = 2 (repeated TSP.Point) }` with
`ElementType { moveTo = 1, lineTo = 2, quadCurveTo = 3, curveTo = 4,
closeSubpath = 5 }`. This is what `bezier_path_source.path` carries and what
`LineEndArchive.path` uses — the natural target for "shapes as curves".

### Fills [proto] TSDArchives.proto

- `TSD.GradientArchive` (121-137): `type` Linear=0/Radial=1, repeated stops
  `{ color = TSP.Color, fraction = float, inflection = float }`, `opacity`,
  `advancedGradient`, plus `anglegradient = 5` → `TSD.AngleGradientArchive
  { gradientangle = 2 (float, RADIANS — fixture-verified 2026-08-29:
  22_ColorGradient.key stores exactly 3π/2 for its vertical backdrop; note the
  contrast with TSD.Geometry `angle`, which is degrees) }` and `transformgradient = 6` →
  `TSD.TransformGradientArchive { start = TSP.Point, end = TSP.Point,
  baseNaturalSize = TSP.Size }` (111-119). The angle/transform sub-messages
  are how a linear gradient's direction is stored.
- `TSD.ImageFillArchive` (139-156): `technique` enum NaturalSize=0 (default),
  Stretch=1, Tile=2, ScaleToFill=3, ScaleToFit=4; `tint` (TSP.Color),
  `fillsize` (TSP.Size), `imagedata` (TSP.DataReference).

### Strokes [proto] TSDArchives.proto

- `TSD.StrokeArchive` (177-192): `color`, `width` (float), `cap` enum
  ButtCap=0/RoundCap=1/SquareCap=2, `join` (`TSD.LineJoin` MiterJoin=0/
  RoundJoin=1/BevelJoin=2, lines 8-12), `miter_limit`, `pattern`
  (`TSD.StrokePatternArchive`: type TSDSolidPattern=1/TSDEmptyPattern=2,
  `phase`, `count`, repeated float `pattern` = the dash array),
  `smart_stroke` (named texture stroke with a parameter dictionary), `frame`,
  `patterned_stroke`. A stroke whose pattern type is TSDEmptyPattern=2 draws
  NOTHING (not solid): theme preset styles ship a fully-parameterized 1pt
  black stroke with an empty pattern and Apple paints no border
  [inferred→fixture-verified: G2 'captions-0-shapestyle' textbox presets].
- `TSD.LineEndArchive` (210-216): `path` (TSP.Path), `line_join` (default
  MiterJoin), `end_point`, `is_filled`, `identifier` (string) — preset
  arrowhead identity + optional explicit outline.
  Rendering semantics [inferred→fixture-verified 2026-08-30: cdx-00243-21
  vs Pages' own PDF export vectors]:
  - The glyph path is a canonical shape in its own small coordinate space
    ("simple arrow" = triangle (0,0)-(3,6)-(6,0); "filled arrow" adds a
    (3,1.5) notch), with **+y pointing outward** past the line tip. The
    glyph's bbox-top row (max y — the arrow apex) sits exactly AT the
    path's geometric endpoint.
  - `end_point` (e.g. (3,0) for simple arrow) marks where the line's own
    stroke terminates in glyph space; Apple's export cuts the stroke back
    to it plus half a stroke width.
  - Glyph scale tracks stroke width sub-linearly: the 6x6 canonical glyph
    rasterizes 6.0pt tall at a 1pt stroke and 9.6pt at a 2pt stroke —
    `scale = 0.4 + 0.6 * stroke_width` fits both observations.
  - `head_line_end` decorates the path's END point, `tail_line_end` its
    START (the "head" is the end the user dragged to).
  - `identifier = "none"` is the explicit no-decoration preset; it still
    carries an empty `path` message and renders nothing.

### Shadows [proto] TSDArchives.proto 218-246

`TSD.ShadowArchive`: `color`, `angle` float default **315**, `offset` float
default **5**, `radius` **int32** default 1, `opacity` default 1,
`is_enabled` default true, `type`: TSDDropShadow=0/TSDContactShadow=1/
TSDCurvedShadow=2, plus per-type payloads: `TSD.DropShadowArchive` (empty),
`TSD.ContactShadowArchive { height = 2 default 0.2, offset = 4 default 0 }`,
`TSD.CurvedShadowArchive { curve = 1 default 0.6 }`. Preset styles carry
disabled shadows with `is_enabled = 0` — honor the flag before painting
[fixture: G2 caption presets]. Drop-shadow angle renders as dx=cos(θ),
dy=sin(θ) with screen y-down (angle 45 + offset 5 = down-right in Apple's
raster) [fixture: G2 pentagon]. An EMPTY `TSD.ReflectionArchive` in a preset
means "no reflection"; a real reflection stores `opacity` explicitly even at
the 0.5 default [fixture: G2 pentagon vs caption presets].

### Other style payloads [proto] TSDArchives.proto

- `TSD.EdgeInsetsArchive` (14-19): required `top/left/bottom/right` floats.
- `TSD.ReflectionArchive` (248-250): `opacity` default **0.5**.
- `TSD.ShapeStylePropertiesArchive` (269-277): `fill`, `stroke`, `opacity`,
  `shadow`, `reflection`, `head_line_end`, `tail_line_end` — the complete
  shape style payload (fields 4/5 on ShapeArchive are deprecated duplicates
  of the line ends).
- `TSD.MediaStylePropertiesArchive` (285-290): same minus fill/line ends.

### Blur — a negative result [inferred: grep over .scratch/otorp, 2026-08-28]

There is **no static blur property** on any TSD/TSS style payload. The only
blur in the format is the motion-blur parameter of Keynote build/transition
effects (`KN.TransitionAttributesArchive.custom_motion_blur` +
`custom_blur_amount`, KNArchives.proto:54-57,174-184; `KNArchives.sos.proto`
carries `blur`/`color_blur_sigma` SOS spec values). Converters must not
invent a static blur; the pnk model carries motion blur only on builds.

## Connection lines: TSD.ConnectionLineArchive routing

`TSD.ConnectionLineArchive { super = 1 (ShapeArchive), connected_from = 2,
connected_to = 3, connected_to_uuid = 4, connected_from_uuid = 5 }`; the
shape's `pathsource.connection_line_path_source = 7` is
`TSD.ConnectionLinePathSourceArchive { super = 1 (BezierPathSourceArchive),
type = 2 (kTSDConnectionLineTypeQuadratic = 0, kTSDConnectionLineTypeOrthogonal
= 1), outset_from = 3, outset_to = 4 }`. [proto] An end with no
`connected_from`/`connected_to` (both the reference and the UUID absent) is a
free end drawn where the stored path puts it. [fixture-verified: kcsrk
121be18d slide 8, lines 2892605/2892690 lack field 2]

Quadratic type: the stored `TSP.Path` is moveTo + lineTo + lineTo, in the
line's local frame (origin = the drawable's geometry position). The end
points are the connected shapes' CENTRES; the middle point is ON the drawn
curve (not a Bezier control point), and the drawable's geometry is the
bounding box of the part outside both shapes. Keynote's export draws the
quadratic through the middle point, center to center, clipped at each
shape's outline. [inferred; measured on the export: line 2892641 stores
(−35.78, 42.43)→(57.64, 0)→(151.05, 42.43) at position (761.70, 334.80),
which are the centres of the two 71.56pt-wide boxes; the exported curve
peaks at y ≈ 335 and meets the boxes at y ≈ 351, which only a curve through
the middle point produces]

Stroke dash units: `TSD.StrokePatternArchive.pattern` (repeated float,
padded to six entries, `count = 3` real ones) is in multiples of the stroke
width, not points — the "dotted" preset stores `[1, 1]` on a 2pt stroke and
exports as 2pt on / 2pt off. [fixture-verified: same slide, 150dpi export,
measured on/off 2.4/1.44pt with anti-aliasing]
