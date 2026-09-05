# Fonts — what documents ask for, and what the viewer can show

iWork documents store a PostScript FACE name (`HelveticaNeue-Light`,
`Calibri-Bold`, `TimesNewRomanPSMT`, `HiraKakuProN-W3`), not a file. On a Mac
with the fonts installed the browser resolves most of them and the render is
faithful. Off macOS — the common case for a reader of pnk.vu — the name
resolves to nothing and the browser silently substitutes its default, which
turns a condensed display face into Helvetica and a screen serif into Times.

This document records (1) which fonts the corpus actually uses, (2) the
substitute chosen for each family and why, and (3) what loading those
substitutes costs.

The mapping itself lives in `viewer/src/fontmap.ts`; `viewer/src/webfonts.ts`
turns it into one stylesheet request per document. The font licenses and
sources are recorded in `docs/format/ATTRIBUTION.md`.

## The census

`scripts/font_census.py` converted every corpus fixture and collected the
`fonts` array of each envelope: **960 documents** (323 Pages, 157 Numbers,
480 Keynote), **491 distinct PostScript names**, **253 families** after
normalizing face and foundry suffixes (`familyKey()` in `fontmap.ts`:
`ArialMT`, `Arial-BoldMT` and `Arial-Black` all key as `arial`). Raw data is
in the script's JSON output (not committed), the per-name table in `docs/fonts-census.md`.

`docs` below is the number of documents whose font list contains the name;
a family's count is the maximum over its names, which is a lower bound on
the union.

Most-used families, per app (documents):

| rank | Pages (323) | Numbers (157) | Keynote (480) |
|---:|---|---|---|
| 1 | Helvetica Neue 227 | Helvetica Neue 143 | Helvetica Neue 383 |
| 2 | Helvetica 130 | Calibri 97 | Helvetica 209 |
| 3 | Arial 61 | Helvetica 67 | Arial 143 |
| 4 | Times New Roman 55 | Arial 18 | Calibri 86 |
| 5 | Calibri 39 | Hiragino Kaku Gothic 6 | Gill Sans 68 |
| 6 | Times 32 | Trebuchet MS 5 | Lucida Grande 60 |
| 7 | Avenir Next 19 | Times New Roman 3 | Times New Roman 52 |
| 8 | Cambria 16 | Verdana 2 | Baskerville 48 |
| 9 | Trebuchet MS 14 | Yu Gothic 2 | Times 46 |
| 10 | Tahoma 10 | Gill Sans 1 | Palatino 45 |

The distribution is steep: eleven families cover most documents, and the tail
is 179 families used by one or two documents each — school-worksheet
handwriting fonts, one-off display faces, and fonts nobody can identify.

## How a family is mapped

In priority order:

1. **metric-clone** — a font drawn to the original's advance widths, so line
   breaks land in the same places and the document's layout survives. Only
   seven such clones exist on Google Fonts, and they happen to cover the
   Microsoft and PostScript core fonts that dominate the corpus. The same
   label is used when the document's family IS a Google Fonts family (a
   document that asks for Open Sans gets Open Sans).
2. **close** — same classification, similar x-height, width and weight range,
   but its own metrics: text will run shorter or longer than Apple's layout.
   Every one of these carries a stated reason.
3. **generic** — no substitute beats the browser's own default for that
   class, so the stack falls through to `sans-serif`, `serif`, `monospace`,
   `cursive` or `fantasy`. Symbol and dingbat fonts are here on purpose:
   substituting a text face prints letters where the document means symbols.

Coverage of the 253 families: **42 metric-clone, 155 close, 56 generic**.
Weighted by documents (summing each family's document count): 1,169
metric-clone, 1,753 close, 146 generic.

### The metric clones, and their sources

Every metric-compatibility claim below was verified against a primary source;
none is asserted from memory.

| clone | replaces | source of the claim |
|---|---|---|
| Arimo | Arial, Helvetica | google/fonts `ofl/arimo/DESCRIPTION.en_us.html`: "metrically compatible with Arial™"; Arial is itself "metrically compatible with Helvetica" ([Wikipedia: Arial](https://en.wikipedia.org/wiki/Arial)), and [Croscore fonts](https://en.wikipedia.org/wiki/Croscore_fonts) lists Arimo's compatibility as "Arial, Helvetica". |
| Tinos | Times New Roman, Times | `ofl/tinos/DESCRIPTION.en_us.html`: "metrically compatible with Times New Roman™"; Croscore lists "Times New Roman, Times". |
| Cousine | Courier New, Courier | `ofl/cousine/DESCRIPTION.en_us.html`: "metrically compatible with Courier New™"; Croscore lists "Courier New, Courier". |
| Carlito | Calibri | `ofl/carlito/DESCRIPTION.en_us.html`: "metric-compatible with Calibri"; Debian `fonts-crosextra-carlito`: "Carlito is metric-compatible with Calibri font." |
| Caladea | Cambria | Debian `fonts-crosextra-caladea`: "Caladea is metric-compatible with the Cambria font." (The google/fonts description does not mention Cambria — do not cite it for this.) |
| Gelasio | Georgia | `ofl/gelasio/DESCRIPTION.en_us.html`: "metrics compatible with Georgia in its Regular, Bold, Italic and Bold Italic weights". Its README adds that Gelasio ships **no kerning**, to stay a functional match, and that its Medium/SemiBold have no Georgia equivalent. |
| Comic Relief | Comic Sans MS | `ofl/comicrelief/DESCRIPTION.en_us.html`: "metrically equivalent to the popular Comic Sans MS … can be used in place of Comic Sans MS without having to move, resize, or reset any part of the copy." Regular and Bold only: the upstream README states there is no metrically-equivalent italic. |

Comic Neue is **not** metric-compatible with Comic Sans — its own
description claims only a reinterpretation, and an open upstream issue asks
for metric compatibility as a missing feature — so it is used for the
Chalkboard family (where only the style matters), not for Comic Sans itself.

### Where a metric clone exists but is not on Google Fonts

Recorded so the question is not reopened: Verdana (no clone exists anywhere;
the DejaVu claim is unsourced), Tahoma (Wine Tahoma, LGPL), Segoe UI
(Selawik, MIT), Consolas (DMCA Sans Serif, public domain), Palatino / Book
Antiqua (URW P052, TeX Gyre Pagella), Century Gothic / Avant Garde (URW
Gothic, TeX Gyre Adventor), Century Schoolbook (URW C059, TeX Gyre Schola),
Bookman Old Style (URW Bookman, TeX Gyre Bonum), Optima (URW Classico, AFPL —
non-commercial), Garamond (URW Garamond No.8, 8pt only). Helvetica Neue, Gill
Sans, Trebuchet MS, Futura, Impact, Rockwell, American Typewriter, Baskerville
and Lucida Grande have no metric clone anywhere, free or otherwise. Primary
source for the clone families: the
[ArchWiki metric-compatible fonts table](https://wiki.archlinux.org/title/Metric-compatible_fonts),
which is derived from fontconfig's `30-metric-aliases.conf`.

## Faces: PostScript suffix to CSS

`parseFace()` in `fontmap.ts` reads the weight, slope and width a face name
declares, because iWork often stores no separate bold flag — the face name is
the only weight information a run carries.

| suffix | CSS |
|---|---|
| `-Thin`, `-Hairline` | 100 |
| `-UltraLight`, `-ExtraLight` | 200 |
| `-Light` | 300 |
| (none), `-Regular`, `-Roman`, `-Normal`, `-Book`, `-Text` | 400 |
| `-Medium` | 500 |
| `-Semibold`, `-DemiBold`, `-Demi` | 600 |
| `-Bold` | 700 |
| `-ExtraBold`, `-UltraBold` | 800 |
| `-Black`, `-Heavy`, `-Ultra` | 900 |
| `-W3` … `-W8` (Hiragino, Yu) | 300 … 800 |
| `-300`, `-500`, `-700` (Museo Sans) | as written |
| `-Italic`, `-It`, `-Oblique` | `font-style: italic` |
| `-Condensed`, `-Cond`, `-Cn`, `Narrow` | `font-stretch: 75%` |
| `-SemiCondensed` / `-Wide`, `-Expanded` | 87.5% / 125% |

Two wrinkles worth knowing:

- **Apple orders UltraLight below Thin**, the opposite of the CSS names
  (where 100 is Thin and 200 ExtraLight). Helvetica Neue ships both, and the
  corpus uses both, so for Helvetica Neue, Avenir, Avenir Next and SF the two
  are swapped: UltraLight → 100, Thin → 200. Every other family follows the
  CSS naming.
- Web-subsetter names such as `Inter-Regular_Black` and
  `Rubik-LightItalic_Medium-Italic` name the file before the underscore and
  the face actually used after it; the parser reads the part after the last
  underscore.

Where a fallback does not ship the weight a face asks for, `nearestWeight()`
picks the closest one it has. The cases that matter in the corpus:
Arial Black and Helvetica Neue CondensedBlack (900) fall to Arimo 700;
Gill Sans Light (300, 34 documents) falls to Cabin 400; Produkt Extralight
(200) falls to Zilla Slab 300. Fallbacks without real italics (Cinzel,
Inconsolata, Anton, the Noto CJK families, Comic Relief) leave the browser to
synthesize an oblique.

## The mapping — families used by 4 or more documents

| family | PostScript names | docs | fallback | kind | weights | rationale |
|---|---|---:|---|---|---|---|
| Helvetica Neue | `HelveticaNeue`, `HelveticaNeue-Medium`, `HelveticaNeue-Bold`, `HelveticaNeue-Light` +8 | 753 | Inter | close | 100–900 + italic | No clone exists for Helvetica Neue’s own widths. Inter is a neutral grotesque with a similar large x-height and, unlike Arimo, covers the Thin/UltraLight/Light/Medium cuts this family uses in hundreds of documents. Arimo is the alternative if width fidelity matters more than weight. |
| Helvetica | `Helvetica`, `Helvetica-Light`, `Helvetica-Bold`, `Helvetica-LightOblique` +3 | 364 | Arimo | metric-clone | 400–700 + italic | Arimo clones Arial’s metrics, and Arial is metrically compatible with Helvetica. |
| Arial | `ArialMT`, `Arial-BoldMT`, `Arial-ItalicMT`, `Arial-Black` +1 | 222 | Arimo | metric-clone | 400–700 + italic | Metric clone by design; Arial Black (17 docs) has no 900 in Arimo and lands on 700. |
| Calibri | `Calibri`, `Calibri-Bold`, `Calibri-Italic`, `Calibri-Light` +2 | 222 | Carlito | metric-clone | 400/700 + italic | Metric clone by design; Carlito is a Lato derivative drawn to Calibri’s widths. |
| Times New Roman | `TimesNewRomanPSMT`, `TimesNewRomanPS-BoldMT`, `TimesNewRomanPS-ItalicMT`, `TimesNewRomanPS-BoldItalicMT` | 110 | Tinos | metric-clone | 400/700 + italic | Metric clone by design. |
| Times | `Times-Roman`, `Times-Bold`, `Times-Italic`, `Times-BoldItalic` | 78 | Tinos | metric-clone | 400/700 + italic | Tinos is metrically compatible with Times as well as Times New Roman (fontconfig’s alias set). |
| Gill Sans | `GillSans`, `GillSans-Light`, `GillSans-Bold`, `GillSans-SemiBold` +5 | 70 | Cabin | close | 400–700 + italic | No Gill Sans clone exists anywhere. Cabin is the closest humanist on Google Fonts — its own description names Johnston and Gill as its models. It has no 300, so Gill Sans Light (34 docs) renders at 400; Lato is the alternative when the Light cut matters more than the skeleton. |
| Lucida Grande | `LucidaGrande` | 60 | Source Sans 3 | close | 200–900 + italic | No clone. Source Sans 3 is humanist with a comparable generous x-height and open apertures; Lucida Grande is wider and looser, so text runs short. |
| Baskerville | `Baskerville`, `Baskerville-SemiBold`, `Baskerville-SemiBoldItalic`, `Baskerville-Bold` +2 | 52 | Libre Baskerville | close | 400–700 + italic | A revival, not a clone: Libre Baskerville has a taller x-height and wider counters by design, so text runs long. Baskervville is the more faithful proportional match but is drawn for display sizes. |
| Palatino | `Palatino-Roman`, `Palatino-Bold`, `Palatino-Italic`, `Palatino-BoldItalic` | 47 | Vollkorn | close | 400–900 + italic | Palatino’s metric clones (URW P052, TeX Gyre Pagella) are not on Google Fonts. Vollkorn is the closest in colour at text sizes: old-style, large x-height, moderate contrast. |
| Trebuchet MS | `TrebuchetMS`, `TrebuchetMS-Bold`, `TrebuchetMS-Italic`, `Trebuchet-BoldItalic` | 44 | Fira Sans | close | 100–900 + italic | No clone exists. Fira Sans is a humanist of similar warmth and x-height. |
| DIN Condensed | `DINCondensed-Bold` | 43 | Barlow Condensed | close | 100–900 + italic | No DIN clone on Google Fonts. Barlow Condensed is a low-contrast grotesk with the same squarish skeleton and covers 100–900. |
| DIN Alternate | `DINAlternate-Bold` | 41 | Barlow Semi Condensed | close | 100–900 + italic | DIN Alternate is the normal-width cut; Barlow Semi Condensed matches its narrower-than-normal proportion. |
| Iowan Old Style | `IowanOldStyle-Roman`, `IowanOldStyle-Italic`, `IowanOldStyle-BoldItalic` | 35 | Libre Baskerville | close | 400–700 + italic | Iowan Old Style is a sturdy old-style with a large x-height and short ascenders — Libre Baskerville’s screen-first proportions are the nearest fit. |
| Avenir Next | `AvenirNext-Regular`, `AvenirNext-Medium`, `AvenirNext-DemiBold`, `AvenirNext-Bold` +4 | 33 | Nunito Sans | close | 200–1000 + italic | Geometric humanist. Nunito Sans covers 200–1000, which this family needs (UltraLight, Medium, DemiBold, Heavy); Montserrat is wider and heavier in colour. |
| American Typewriter | `AmericanTypewriter`, `AmericanTypewriter-Bold` | 33 | Cutive | close | 400, no italic | Cutive is a typewriter-derived serif with the soft slabs and even spacing of ITC American Typewriter. Special Elite is a distressed typed-impression face and is the wrong reference. |
| Century Gothic | `CenturyGothic`, `CenturyGothic-Bold`, `CenturyGothic-Italic` | 31 | Poppins | close | 100–900 + italic | Century Gothic sits in the Avant Garde metric group, whose clones (URW Gothic, TeX Gyre Adventor) are not on Google Fonts. Poppins is monolinear geometric with a matching large x-height and circular bowls. |
| Open Sans | `OpenSans`, `OpenSans-Bold`, `OpenSans-Light`, `OpenSans-Semibold` +1 | 27 | Open Sans | metric-clone | 300–800 + italic | The document’s own family, published on Google Fonts. |
| Georgia | `Georgia`, `Georgia-Bold`, `Georgia-Italic`, `Georgia-BoldItalic` | 24 | Gelasio | metric-clone | 400–700 + italic | Metric clone in Regular/Bold/Italic/Bold Italic only; Gelasio’s added Medium and SemiBold have no Georgia equivalent, and it ships no kerning so that widths stay matched. |
| Cochin | `Cochin`, `Cochin-Italic` | 22 | EB Garamond | close | 400–800 + italic | French old-style with a small x-height and fine detail; EB Garamond is the closest on Google Fonts. |
| Chalkboard | `Chalkboard`, `Chalkboard-Bold` | 22 | Comic Neue | close | 300/400/700 + italic | Casual upright marker face. Comic Neue has a real 700 for Chalkboard-Bold; Patrick Hand is closer in letterform but is a single weight. |
| Hiragino Kaku Gothic | `HiraKakuProN-W3`, `HiraKakuProN-W6`, `HiraKakuStd-W8`, `HiraKakuPro-W6` | 22 | Noto Sans JP | close | 100–900, no italic | Japanese sans. Noto Sans JP covers the script and maps Hiragino’s W3/W6 weights onto 300/600. |
| Tahoma | `Tahoma`, `Tahoma-Bold` | 21 | Open Sans | close | 300–800 + italic | Tahoma’s metric clone (Wine Tahoma, LGPL) is not on Google Fonts. Open Sans is the closest humanist with a large x-height. |
| Papyrus | `Papyrus`, `Papyrus-Regular`, `Papyrus-Condensed` | 21 | none → `fantasy` | generic | — | Nothing on Google Fonts resembles Papyrus. The browser’s `fantasy` default is a more honest miss than a wrong specific face. |
| Copperplate | `Copperplate`, `Copperplate-Bold` | 21 | Cinzel | close | 400–900, no italic | Engraved Roman capitals. Cinzel is the nearest all-caps inscriptional face; Copperplate’s micro-serifs are not reproduced. |
| Verdana | `Verdana`, `Verdana-Bold`, `Verdana-Italic` | 20 | Open Sans | close | 300–800 + italic | No metric clone exists (the DejaVu claim is unsourced). Open Sans matches the humanist letterforms and large x-height but is materially narrower, so text runs short. |
| Cambria | `Cambria`, `Cambria-Bold`, `Cambria-Italic`, `Cambria-BoldItalic` | 19 | Caladea | metric-clone | 400/700 + italic | Metric clone by design (Caladea, from Chrome OS’s crosextra set). |
| Menlo | `Menlo-Regular`, `Menlo-Bold`, `Menlo-BoldItalic`, `Menlo-Italic` | 18 | Roboto Mono | close | 100–700 + italic | Menlo descends from Bitstream Vera Mono, which is not on Google Fonts. Roboto Mono is a neutral grotesque mono of similar width. |
| Courier | `Courier`, `Courier-BoldOblique`, `Courier-Bold` | 18 | Cousine | metric-clone | 400/700 + italic | Cousine is metrically compatible with Courier as well as Courier New. Courier Prime is the more faithful visual match to PostScript Courier if that matters more. |
| Hoefler Text | `HoeflerText-Regular`, `HoeflerText-Italic`, `HoeflerText-Black`, `HoeflerText-BlackItalic` +1 | 16 | Crimson Pro | close | 200–900 + italic | Janson-flavoured old-style with moderate contrast; Crimson Pro is the closest, and it has the 900 that Hoefler Text Black asks for. |
| Avenir | `Avenir-Roman`, `Avenir-Book`, `Avenir-Medium`, `Avenir-Light` +5 | 15 | Nunito Sans | close | 200–1000 + italic | Same reasoning as Avenir Next. |
| Thames Nude Roman | `ThamesNudeRoman` | 14 | none → `serif` | generic | — |  |
| Symbol | `Symbol` | 13 | none → `serif` | generic | — | Symbol is a legacy symbol encoding, not a text face: any text substitute prints letters where the document means symbols. |
| Courier New | `CourierNewPSMT`, `CourierNewPS-BoldMT`, `CourierNewPS-BoldItalicMT` | 13 | Cousine | metric-clone | 400/700 + italic | Metric clone by design. |
| Book Antiqua | `BookAntiqua`, `BookAntiqua-Bold` | 13 | Vollkorn | close | 400–900 + italic | Book Antiqua is in Palatino’s metric group; same substitute, same caveat. |
| Charcoal CY | `CharcoalCY` | 13 | Noto Sans | close | 100–900 + italic | A Cyrillic Apple system face. Noto Sans is chosen for coverage rather than style. |
| Wingdings | `Wingdings-Regular`, `Wingdings` | 12 | none → `sans-serif` | generic | — | A dingbat font. Substituting a text face shows letters instead of symbols. |
| Futura | `Futura-Medium`, `Futura-Bold`, `Futura-CondensedMedium` | 11 | Jost | close | 100–900 + italic | No Futura clone exists. Jost is an original geometric explicitly inspired by 1920s German sans-serifs — the closest analogue, with the full weight range. |
| Chalkduster | `Chalkduster` | 10 | Rock Salt | close | 400, no italic | Chalk texture cannot be reproduced by an outline font on Google Fonts. Rock Salt keeps the rough hand-drawn look and loses the texture. |
| Impact | `Impact` | 10 | Anton | close | 400, no italic | No clone. Anton is the standard stand-in: ultra-condensed, ultra-bold, tiny counters — but its counters were deliberately opened, so widths differ. |
| Berlin Sans FB | `BerlinSansFBDemi-Bold`, `BerlinSansFB-Reg` | 10 | Jost | close | 100–900 + italic | Heavy geometric display; Jost is the nearest geometric with a 600 weight. |
| CMG Sans | `CMGSans-ExtraBold`, `CMGSans-BoldCn` | 10 | none → `sans-serif` | generic | — | Unidentified proprietary sans; the browser default is as good a guess as any specific face. |
| Garamond | `Garamond`, `Garamond-Italic`, `Garamond-Bold` | 9 | EB Garamond | close | 400–800 + italic | EB Garamond is a revival of the same Garamont source, not a metric clone — URW Garamond No.8 is 8pt-only and not on Google Fonts. |
| Comic Sans MS | `ComicSansMS`, `ComicSansMS-Bold` | 9 | Comic Relief | metric-clone | 400/700, no italic | Metric clone: Comic Relief was drawn to be a drop-in for Comic Sans MS. Regular and Bold only — it has no italic, so Comic Sans italic is synthesized. |
| Consolas | `Consolas`, `Consolas-Bold` | 8 | Inconsolata | close | 200–900, no italic | Consolas’s metric clone (DMCA Sans Serif) is not on Google Fonts. Inconsolata was influenced by Consolas but is 0.5em wide against Consolas’s 0.55em, so lines run short. |
| Bradley Hand | `BradleyHandITCTT-Bold` | 8 | Caveat | close | 400–700, no italic | Casual hand lettering; Caveat matches the informal upright script and has 400–700. |
| Monaco | `Monaco` | 8 | Roboto Mono | close | 100–700 + italic | Same reasoning as Menlo. |
| Graphik | `Graphik-Regular`, `Graphik-Light`, `Graphik-Medium`, `Graphik-Semibold` | 8 | Inter | close | 100–900 + italic | Contemporary neo-grotesque; Inter matches the skeleton and covers Light/Medium/Semibold. |
| Bodoni 72 | `BodoniSvtyTwoITCTT-Book` | 7 | Bodoni Moda | close | 400–900 + italic | Didone. Bodoni Moda is a Bodoni revival with an optical-size axis. |
| Constantia | `Constantia` | 7 | Source Serif 4 | close | 200–900 + italic | ClearType humanist serif with a large x-height; Source Serif 4 is the nearest on Google Fonts. |
| Noteworthy | `Noteworthy-Light`, `Noteworthy-Bold` | 6 | Patrick Hand | close | 400, no italic | Casual hand-printed face; Patrick Hand is the closest, single weight only. |
| Apple Color Emoji | `AppleColorEmoji` | 5 | none → `sans-serif` | generic | — | Every browser ships a colour emoji font. Noto Color Emoji is on Google Fonts but is 24 MB, so substituting would be a large download for no gain. |
| Poppins | `Poppins-Regular`, `Poppins-Bold`, `Poppins-SemiBold`, `Poppins-Medium` +2 | 5 | Poppins | metric-clone | 100–900 + italic | The document’s own family, published on Google Fonts. |
| CMU Serif | `CMUSerif-Roman`, `CMUSerif-Italic`, `CMUSerif-BoldItalic`, `CMUSerif-Bold` | 5 | none → `serif` | generic | — | Computer Modern has no Google Fonts equivalent; the browser serif is closer than any available face. |
| Corbel | `Corbel` | 5 | Lato | close | 100–900 + italic | ClearType humanist sans with a modest x-height; Lato is the nearest on Google Fonts. |
| Didot | `Didot`, `Didot-Bold` | 5 | Playfair Display | close | 400–900 + italic | Didone with extreme contrast; Playfair Display is the closest display serif. |
| Inconsolata | `Inconsolata-Regular`, `Inconsolata-Bold`, `Inconsolata`, `InconsolataForPowerline` | 5 | Inconsolata | metric-clone | 200–900, no italic | The document’s own family, published on Google Fonts. |
| Montserrat | `Montserrat-Medium`, `Montserrat-Bold`, `Montserrat-Regular`, `Montserrat-ExtraBold` +1 | 5 | Montserrat | metric-clone | 100–900 + italic | The document’s own family, published on Google Fonts. |
| Arial Unicode MS | `ArialUnicodeMS` | 5 | Arimo | metric-clone | 400–700 + italic | Arial Unicode MS carries Arial’s Latin metrics, so Arimo is metrically right for Latin; its non-Latin coverage has no equivalent here. |
| OpenDyslexic | `OpenDyslexic-Regular`, `OpenDyslexic-Bold` | 5 | none → `sans-serif` | generic | — | The whole point of the face is its own letterforms, so any substitute defeats it. It is OFL-licensed and could be self-hosted directly. |
| Bebas Neue | `BebasNeue` | 5 | Bebas Neue | metric-clone | 400, no italic | The document’s own family, published on Google Fonts. |
| Marker Felt | `MarkerFelt-Thin`, `MarkerFelt-Wide` | 4 | Permanent Marker | close | 400, no italic | Felt-tip marker face; Permanent Marker is the closest on Google Fonts. |
| Hiragino Mincho | `HiraMinProN-W3` | 4 | Noto Serif JP | close | 200–900, no italic | Japanese serif (Mincho); Noto Serif JP covers the script. |
| Perpetua Titling | `PerpetuaTitlingMT-Light`, `PerpetuaTitlingMT-Bold` | 4 | Cinzel | close | 400–900, no italic | Engraved Roman titling capitals; Cinzel is the nearest. |
| Boulder | `Boulder-Regular` | 4 | none → `sans-serif` | generic | — |  |
| Vivaldi | `Vivaldii` | 4 | Tangerine | close | 400/700, no italic | Ornate calligraphic script; Tangerine is the closest formal calligraphic on Google Fonts, at lighter weight. |
| Arial Rounded MT | `ArialRoundedMTBold` | 4 | Nunito | close | 200–1000 + italic | Rounded geometric sans; Nunito has rounded terminals and a real 700. |
| Produkt | `Produkt-Light`, `Produkt-Extralight`, `Produkt-Medium` | 4 | Zilla Slab | close | 300–700 + italic | Slab companion to Graphik; Zilla Slab is the nearest contemporary slab. Produkt’s Extralight lands on Zilla Slab’s 300. |
| Optima | `Optima-Regular`, `Optima-Bold`, `Optima-Italic` | 4 | Marcellus | close | 400, no italic | Optima’s metric clone (URW Classico) is AFPL-licensed and not on Google Fonts. Marcellus shares the flared inscriptional terminals and classical proportions; single weight only. |
| Proxima Nova | `ProximaNova-Regular`, `ProximaNova-Bold`, `ProximaNova-Medium`, `ProximaNova-Semibold` +1 | 4 | Figtree | close | 300–900 + italic | Geometric humanist with a large x-height; Figtree is the closest on Google Fonts and covers 300–900. |
| Zapfino | `Zapfino` | 4 | none → `cursive` | generic | — | Nothing on Google Fonts approaches Zapfino’s swash extremes; the `cursive` generic is the honest answer. |
| Canela | `Canela-Regular`, `CanelaDeck-Regular`, `Canela-Bold`, `CanelaText-Regular` +1 | 4 | Cormorant | close | 300–700 + italic | High-contrast serif with flared stems; Cormorant is the nearest display serif. |
| Charter | `Charter-Roman`, `Charter-Italic`, `Charter-BoldItalic`, `Charter-Bold` | 4 | Charis SIL | close | 400/700 + italic | Charis SIL is built on Bitstream Charter, which is what Apple ships as Charter — the same outlines, extended. No metric claim is published, so this is filed as close. |
| Chalkboard SE | `ChalkboardSE-Regular` | 4 | Comic Neue | close | 300/400/700 + italic | Same reasoning as Chalkboard. |

## The long tail — 179 families, 1 to 3 documents each

Grouped by fallback. A dagger (†) marks a family that IS the Google Fonts
family, so the substitute is the same font. Everything else in this table is
`close` unless the fallback column says `none`, in which case it is
`generic`. The reasoning is the shared one for the group: the same
classification and roughly the same proportions, or — for the `none` rows —
an unidentified or one-off face where a specific guess would be worse than
the browser default.

| fallback | families (docs) |
|---|---|
| none → `fantasy` | Jazz LET (2), Luminari (2), Party LET (1), Arnprior (1), Hanzel Extended (1), Abject Failure (1), University Roman (1), Mariah (1), Good Times (1), Denmark (1), Pegasus (1), Blue Highway Linocut (1), Grunge Face (1), Surfboard (1), Berlin Fashion (1), Ring of Kerry (1), TOSCA ZERO (1), Spongeboy Me Bob (1), Bauhaus 93 (1), Titi (1), Hobo (1), KG Geronimo Blocks (1), Mind Boggle (1), Watermelon (1) |
| Source Sans 3 | Lucida Sans (3), Adobe Clean (2), Skia (2), Myriad Pro (2), Scala Sans (1), Seravek (1), Lucida Sans Unicode (1), Freight Sans (1), Source Sans Pro† (1) |
| none → `serif` | Adelon Light (3), Calligraphic 421 (1), Credit Valley (1), RT Gaia Serif (1), Euclid Symbol (1), Computer Modern Math Italic (1), Symbol Pi (1), Computer Modern Symbols (1), Apple Symbols (1) |
| Dancing Script | Sonoma Script (2), Rage Italic (1), The Brooklyn (1), Kate Celebration (1), Cursive Standard (1), Script Ecole 2 (1), Quentin (1), School House Cursive B (1) |
| none → `sans-serif` | Pythagoras (2), Geo (1), Webdings (1), Univerza Sans (1), Biancoenero (1), Wingdings 3 (1), Wingdings 2 (1) |
| Cinzel | Herculanum (3), Copperplate Gothic (1), Engravers MT (1), Capitals (1), Academy Engraved LET (1), Germania One (1) |
| Caveat | Caveat† (3), KB Stick to It (2), Lucida Handwriting (1), Handwriting Mutlu (1), Santa Fe LET (1) |
| Noto Sans JP | Yu Gothic (3), Hiragino Sans (2), MS PGothic (1), Osaka (1), Keifont (1) |
| Archivo | Abadi MT (2), Founders Grotesk (1), Founders Grotesk Text (1), Roc Grotesk (1) |
| none → `monospace` | Iosevka (3), Latin Modern Mono (1), CMU Typewriter (1), APL385 (1) |
| Bitter | Superclarendon (3), Chaparral Pro (2), Bookman Old Style (1) |
| EB Garamond | Adobe Garamond (2), Garamond Premier (2), Minion Pro (1) |
| Great Vibes | Amazone BT (1), Snell Roundhand (1), Savoye LET (1) |
| Italianno | Apple Chancery (3), Lucida Calligraphy (3), Black Chancery (1) |
| Noto Serif JP | Yu Mincho (1), IPAmj Mincho (1), IPAmj Shiyou Mincho (1) |
| PT Serif | Century Schoolbook (2), Swift (1), Lucida Fax (1) |
| Poppins | Muller (1), All Round Gothic (1), Blair MD (1) |
| Anton | Druk (2), Phosphate (1) |
| Archivo Narrow | Arial Narrow (3), Founders Grotesk Cond (2) |
| Arimo | Geneva (3), Microsoft Sans Serif (2) |
| Barlow Semi Condensed | Segoe Condensed (1), Barlow Semi Condensed† (1) |
| Cabin | ITC Stone Sans (2), Humanist 521 (2) |
| Eczar | Baloo (1), Eczar† (1) |
| Encode Sans Semi Condensed | Avenir Next Condensed (2), Encode Sans Semi Condensed† (1) |
| Gentium Plus | Gentium (1), Galatia SIL (1) |
| IBM Plex Mono | Favorit Mono (2), IBM Plex Mono† (1) |
| Inter | Inter† (3), SF NS Text (1) |
| JetBrains Mono | JetBrains Mono† (3), JuliaMono (1) |
| Jost | Tw Cen MT (2), Kabel (1) |
| Libre Franklin | Bell Gothic (2), Franklin Gothic (1) |
| Mulish | Mulish† (3), Museo Sans (1) |
| Noto Sans KR | AppleGothic (1), JCHEadA (1) |
| Noto Sans TC | STHeiti TC (1), LiGothic Medium (1) |
| Roboto Mono | Roboto Mono† (3), SF Mono (1) |
| Zen Maru Gothic | Hiragino Maru Gothic (1), Tsukushi Round Gothic (1) |
| none → `cursive` | Amienne (1), Trattatello (1) |
| Alfa Slab One | Cooper Black (1) |
| Anonymous Pro | Anonymous Pro† (1) |
| Bodoni Moda | bodonisvtytwoos (1) |
| Cairo | GE Typo (1) |
| Carlito | Carlito† (1) |
| Comic Neue | Lucida Casual (1) |
| Cousine | Andale Mono (2) |
| Encode Sans Condensed | Encode Sans Condensed† (1) |
| Fira Code | Fira Code† (1) |
| Gudea | Gudea† (1) |
| IBM Plex Sans | IBM Plex Sans† (1) |
| Julius Sans One | Julius Sans One† (1) |
| Lato | Candara (3) |
| League Gothic | League Gothic† (1) |
| Libre Caslon Text | Big Caslon (2) |
| Marcellus | Marcellus† (1) |
| Noto Naskh Arabic | Adobe Arabic (1) |
| Noto Sans | Noto Sans† (1) |
| Noto Sans Canadian Aboriginal | Euphemia UCAS (1) |
| Noto Sans Cherokee | Plantagenet Cherokee (2) |
| Noto Sans Gurmukhi | Monotype Gurmukhi (1) |
| Noto Sans SC | STHeiti SC (1) |
| Noto Sans Thai | Sathu (1) |
| Noto Serif SC | SimSun (1) |
| Open Sans | Frutiger (1) |
| Oswald | Norwester (1) |
| Patrick Hand | Letters for Learners (1) |
| Quicksand | Quicksand† (2) |
| Raleway | Raleway† (2) |
| Roboto | Roboto† (1) |
| Rock Salt | Squeaky Chalk Sound (1) |
| Rokkitt | Rockwell (3) |
| Rubik | Rubik† (1) |
| STIX Two Math | Cambria Math (2) |
| STIX Two Text | STIX General (2) |
| Sorts Mill Goudy | Californian FB (1) |
| Source Code Pro | Source Code Pro† (1) |
| Source Serif 4 | Publico Text (1) |
| Tajawal | Tajawal† (1) |

## Loading: what it costs, and the hosting decision

The viewer requests the substitutes from Google Fonts at render time
(`webfonts.ts`): one `fonts.googleapis.com/css2` stylesheet naming only the
families, weights and slopes the open document actually uses. The two
documents used for the visual check:

| document | families requested | latin woff2 |
|---|---|---:|
| `eb299192….numbers` (Calibri / Arial / Verdana / Helvetica Neue) | `Arimo:400,700` `Carlito:400,700,700i` `Inter:400` `Open Sans:400,700` | 203 KiB |
| `85c3a6f1….key` (Gill Sans / Scala Sans / Helvetica / Courier New) | `Cousine:400` `Cabin:400` `Arimo:400` `Source Sans 3:300,400,700` | 121 KiB |

Google serves each family split by `unicode-range`, so a Latin document
downloads only the `latin` files. A document with Central/Eastern European
accents also pulls `latin-ext`, roughly tripling those figures (631 KiB and
392 KiB respectively). CJK families are split into 100+ ranges and the
browser fetches only the few a page touches — Noto Sans JP is 5.1 MB in
total but a slide of Japanese text costs tens of kilobytes.

If the fonts are ever **self-hosted** instead (all of them are OFL 1.1, which
permits redistribution — see `docs/format/ATTRIBUTION.md`), the sizes to
budget, measured as the latin-subset woff2 the Google API serves for
regular / bold / italic / bold-italic:

| set | families | latin woff2 |
|---|---:|---:|
| the seven metric clones (Arimo, Tinos, Cousine, Carlito, Caladea, Gelasio, Comic Relief) | 7 | **472 KiB** |
| the ten highest-traffic mappings (adds Inter, Cabin, Source Sans 3) | 10 | **1.04 MiB** |
| every Latin family the mapping names | 60 | **5.3 MiB** |
| the four CJK families (all weights, all ranges) | 4 | 20.3 MiB |

So the practical split is: the seven metric clones are small enough to bundle
outright, and the long tail is worth loading lazily whatever the hosting
choice. Nothing is bundled today.

## The setting, and what it means for privacy

`viewer/index.html` carries a strict Content-Security-Policy. It now allows
exactly two remote origins and nothing else:

    style-src 'self' https://fonts.googleapis.com
    font-src https://fonts.gstatic.com

There is still no remote `connect-src`, `img-src` or `form-action`, so the
document cannot leave the page by any route. What does leave, when the
setting is on, is the stylesheet request: Google sees the reader's IP address
and the list of family names asked for. It never sees the document, the file
name, or its text.

The viewer's nav carries a **settings** menu with one checkbox, "Load
substitute fonts from Google Fonts", persisted in `localStorage` under
`pnk.googleFonts` and **on by default**. Turning it off removes the
stylesheet, drops the substitute from every font stack, and re-renders the
open document; with it off the viewer makes no network request of any kind
after its own static assets. The Playwright gate asserts both halves: the
five existing tests run with the setting off and still assert zero runtime
network requests, and one test turns it on and asserts that every external
request goes to `fonts.googleapis.com` or `fonts.gstatic.com`, that the
`<link>` names the expected families, and that the Carlito face reaches
`status === "loaded"`.

## Verified by eye

Two documents rendered with `scripts/visual_diff.py` against Apple's own PDF
export, once with the setting off and once on (viewer on ports 8705 / 8704).

**`eb299192….numbers`, a Calibri worksheet.** With substitutes off, our
Calibri text fell to the browser sans (Helvetica), which is wider: the
"Pumpdown from CU Liquid Outlet…" cell wrapped onto four lines against
Apple's three, pushing the "Oil to Charge" block below its box, and every
label row sat a little wide of Apple's. With substitutes on, Carlito loads
and the same cell wraps onto three lines; the table's rows line up with
Apple's row for row down the sheet. This is the case the mapping is for.

**`85c3a6f1….key`, a Netnod deck (Gill Sans, Scala Sans).** Gill Sans is
installed on this machine, so nothing changed for it — the check confirmed
the "reader has the real font" path leaves the render alone. Scala Sans is
not installed, and with substitutes on it renders in Source Sans 3, visibly
narrower than the Helvetica it fell to before. Here the substitute moves our
render AWAY from Apple's export, because Apple has no Scala Sans either and
falls back to Helvetica itself. That is the honest limit of this ground
truth: the comparison can only validate a substitution when Apple has the
font and the browser does not. Where neither has it, the substitute matches
the document's intent rather than Apple's fallback.

## Open questions

- **Hosting.** Nothing is self-hosted yet; the sizes above are the input to
  that decision.
- **Only fetch what the reader lacks.** The viewer currently requests the
  substitute even for a reader who has the real font. `document.fonts.check()`
  cannot be used to detect this — it answers `true` for a family nobody has,
  because the system font satisfies the query (asserted in the gate). A width
  measurement against a known fallback would work, at the cost of a layout
  probe per family.
- **Variable-width axes.** `Nunito Sans`, `Cabin`, `Archivo` and others carry
  a `wdth` axis that could serve the condensed cuts properly
  (`AvenirNextCondensed`, `HelveticaNeue-CondensedBold`). The loader requests
  static weights only, so those render at normal width today.
