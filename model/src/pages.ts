/**
 * pnk JSON document models — Pages (.pages).
 *
 * Maps TP.DocumentArchive [10000] (+ TSA/TSK supers) onto a resolved,
 * reference-free model. Both content flavors are first-class:
 *
 *  - "word-processing": text flows in `body` (TP.DocumentArchive.body_storage
 *    → TSWP.StorageArchive), sections are style/print breaks inside that flow
 *    (TP.SectionArchive referenced via the storage's table_section entries),
 *    and floating objects hang off pages (TP.FloatingDrawablesArchive).
 *  - "page-layout": fixed canvases — each TP.SectionArchive is one canvas of
 *    drawables; body is empty.
 *
 * Page masters (TP.PageTemplateArchive) are resolved into `pageTemplates`;
 * sections name their first/even/odd template. Placeholder chain: a slide/
 * page placeholder that carries no geometry/text inherits it from its
 * template's placeholder of the same role — the converter bakes the resolved
 * values in and flags `placeholder.inherited` (docs/model-design.md).
 *
 * Format facts: docs/format/pages.md (+ text.md, drawables.md, styles.md).
 */

import type {
  Drawable,
  DrawableCommon,
  Fill,
  IsoDateString,
  PageLayoutOrientation,
  Paragraph,
  StyledText,
} from "./shared";
import type { DocumentEnvelope } from "./shared";

// ---------------------------------------------------------------------------
// Page masters (TP.PageTemplateArchive)
// ---------------------------------------------------------------------------

/**
 * A page template ("page master"): repeating furniture applied to pages that
 * use it. [proto: .scratch/otorp/Pages/TPArchives.proto → TP.PageTemplateArchive;
 *  legacy TP.PageMasterArchive [10143] is absent from 15.3.1 — treated as legacy]
 */
export interface PageTemplate {
  /** Display/lookup name when the source carries one. */
  name?: string;
  /** Master drawables (background shapes, rules, logo boxes). */
  drawables: Drawable[];
  /**
   * Template placeholders (tagged drawable pairs in the proto) — title/author
   * boxes etc. that user content snaps into. Roles resolved from the
   * placeholder kind.
   */
  placeholders: PagePlaceholder[];
  backgroundFill?: Fill;
  hideHeadersFooters?: boolean;
  headers: StyledText[];
  footers: StyledText[];
  headersFootersMatchPreviousPage?: boolean;
}

/** A placeholder slot on a template. [proto: TagDrawablePair tag/drawable/z_index] */
export interface PagePlaceholder {
  /** Template-local role tag (app-defined string; "title"/"author" common). */
  tag?: string;
  /** The placeholder drawable (usually a textbox) with its geometry/style. */
  drawable: Drawable;
  zIndex?: number;
}

// ---------------------------------------------------------------------------
// Sections (TP.SectionArchive) — print/style breaks in both flavors
// ---------------------------------------------------------------------------

export interface PagesSection {
  name?: string;
  /** Template names for the section's pages. */
  firstPageTemplate?: string;
  evenPageTemplate?: string;
  oddPageTemplate?: string;
  /** Page numbering behavior. [proto: section_start_kind/page_number_kind/start] */
  pageNumbering?: {
    restart?: boolean;
    startAt?: number;
    /** Which number shows on the first page of the section. */
    firstPageNumberKind?: "continue" | "restart-at" | "from-previous";
  };
  /** Headers/footers carried over from the previous section. [proto: field 17] */
  inheritPreviousHeaderFooter?: boolean;
  /** Section background (page-layout canvases). [proto: background_fill] */
  backgroundFill?: Fill;
  /** Word-processing only: index into `body.paragraphs` where the section starts. */
  bodyParagraphStart?: number;
  /**
   * Multi-column text layout for the section's pages; absent = single column.
   * Equal-width columns (Pages' unequal-column variant degrades to `count`
   * with an unsupported-feature warning). [proto: TP.SectionArchive column
   * storage — verify field in docs/format/pages.md at extraction time]
   */
  columns?: { count: number; gutterPt?: number };
}

// ---------------------------------------------------------------------------
// Body content (word-processing flavor)
// ---------------------------------------------------------------------------

/**
 * Footnote attached to a body position. [proto: table_footnote +
 * TSWP.FootnoteReferenceAttachmentArchive.contained_storage]
 */
export interface Footnote {
  /** The footnote mark's character position, expressed as a path to the
   * containing paragraph + item index (converter-assigned). */
  anchorParagraphIndex: number;
  /** The footnote body text. */
  text: StyledText;
}

// ---------------------------------------------------------------------------
// Floating objects (TP.FloatingDrawablesArchive)
// ---------------------------------------------------------------------------

/**
 * Floating (non-inline) objects grouped by the page they anchor to.
 * [proto: FloatingDrawablesArchive.page_groups → PageGroup { page_index, drawables }]
 * `pageIndex` is 0-based; absent when the group had no page index.
 */
export interface FloatingPage {
  pageIndex?: number;
  /**
   * Page-layout flavor: resolved template underlay for this canvas — the
   * page template's drawables that show under the page content, in paint
   * order, filtered the way Pages renders them (superseded placeholders
   * removed). Painted before `drawables`, verbatim; a viewer must not need
   * `pageTemplates` to render a layout canvas (docs/model-review.md §3c).
   * Word-processing flavor: absent — pages are viewer-paginated, so template
   * furniture composes per page from `pageTemplates` + section names.
   */
  templateDrawables?: Drawable[];
  drawables: Drawable[];
}

// ---------------------------------------------------------------------------
// Document root
// ---------------------------------------------------------------------------

/**
 * The Pages document model. Envelope fields (`meta`, `warnings`, `fonts`,
 * `media`) follow the shared DocumentEnvelope contract.
 */
export interface PagesDocument extends DocumentEnvelope {
  kind: "pages";
  /** Which flavor the source document uses. */
  flavor: "word-processing" | "page-layout";

  /** Paper size in points (e.g. US Letter 612×792). [proto: page_width/page_height] */
  pageSize?: { width: number; height: number };
  /** Page margins in points. [proto: margin fields 32-37] */
  pageMargins?: { top?: number; bottom?: number; left?: number; right?: number; header?: number; footer?: number };
  orientation?: PageLayoutOrientation;
  /** Print scale factor (1 = 100%). [proto: page_scale] */
  pageScale?: number;

  /**
   * Word-processing flavor: the flowing document text, fully split into
   * styled paragraphs with inline objects/fields resolved.
   */
  body?: StyledText;
  /**
   * Page-layout flavor only: a live, findable body flow that Pages never
   * renders (same shape as `body`). Present ONLY when the source storage
   * carries a body with at least one non-empty paragraph; omitted when empty
   * (absence = no hidden content). Preserved rather than dropped — viewers
   * ignore it.
   * [fixture-verified: Pages 26.3 layout docs keep the body storage; the
   *  Convert-to-Layout "body discarded" warning is rendering-level only]
   */
  hiddenBody?: StyledText;
  /** Footnotes for the body (word-processing flavor). */
  footnotes?: Footnote[];
  /**
   * Where footnote bodies render: bottom of the anchor's page (the default,
   * omitted), or collected as endnotes per section / per document.
   * [proto: TP.DocumentArchive footnote kind — verify field at extraction]
   */
  footnotePlacement?: "section-endnotes" | "document-endnotes";

  /**
   * Word-processing flavor: floating objects per page. Page-layout flavor:
   * each entry is a canvas (TP.SectionArchive) and `pageIndex` is its number.
   */
  floating: FloatingPage[];

  /** All page masters in the document, resolved. */
  pageTemplates: PageTemplate[];
  /** Section breaks, in document order. */
  sections: PagesSection[];

  /**
   * Table of contents entries as last laid out by Pages. [proto:
   * TSWP.TOCInfoArchive.toc_entry_data → TSWP.TOCEntryInstanceArchive
   * { paragraph_index, page_number, heading, indexed_paragraph_level }]
   * The rendered TOC itself is an inline `textbox` drawable in the body.
   */
  tableOfContents?: TableOfContents;
  /**
   * Reviewer comments anchored in the body, in document order. Comments on
   * text-box and table-cell storages are not collected. [proto:
   * TSWP.StorageArchive table_highlight (23) → TSWP.HighlightArchive →
   * TSD.CommentStorageArchive; fixture 381bbbac]
   */
  comments?: Comment[];
  /**
   * Bookmark anchors in the body. A run `hyperlink` of the form `#<id>`
   * targets the bookmark with that `id`. [proto: TSWP.StorageArchive
   * table_bookmark (15) → TSWP.BookmarkFieldArchive; fixture eb2a7cde]
   */
  bookmarks?: Bookmark[];
  /**
   * Tracked-change markup of the body, in text order. `body` is the
   * ACCEPTED view: an insertion's text is present in it, a deletion's text
   * is not (a deleted paragraph break leaves an empty paragraph behind).
   * [proto: TSWP.StorageArchive table_insertion (21) / table_deletion (22)
   * → TSWP.ChangeArchive { kind, session, date }; session →
   * TSWP.ChangeSessionArchive.author → TSK.AnnotationAuthorArchive.name;
   * fixture 55d37c2b: 3 insertions, 1 deletion, one author]
   */
  changes?: TrackedChange[];
}

/** One tracked change (see PagesDocument.changes). */
export interface TrackedChange {
  kind: "insertion" | "deletion";
  /** Index into `body.paragraphs` where the changed range starts. */
  paragraphIndex: number;
  /** The inserted text (present in `body`) or the deleted text (absent from it). */
  text: string;
  /** Author display name, from the change session. */
  author?: string;
  /** When the change was made. [proto: ChangeArchive.date, else the session's] */
  date?: IsoDateString;
}

/** A comment thread anchored at a body position. */
export interface Comment {
  /** Index into `body.paragraphs` of the paragraph holding the anchor. */
  anchorParagraphIndex: number;
  /** Comment text, plain. [proto: TSD.CommentStorageArchive.text] */
  text: string;
  /** Author display name. [proto: author → TSK.AnnotationAuthorArchive.name] */
  author?: string;
  /** Creation date. [proto: creation_date, TSP.Date seconds since 2001] */
  date?: IsoDateString;
  /** The body text the comment highlights, when its range is non-empty. */
  quotedText?: string;
  replies?: Comment[];
}

/** A bookmark anchor in the body. */
export interface Bookmark {
  /** The bookmark's UUID (the smart field's text_attribute_uuid_string). */
  id: string;
  /** User-visible name when Pages stored one. [proto: BookmarkFieldArchive.name] */
  name?: string;
  /** Index into `body.paragraphs` where the bookmark starts. */
  paragraphIndex: number;
}

/** A rendered TOC entry. [proto: TSWP TOC archives — minimal viewer-level model] */
export interface TableOfContents {
  entries: {
    /** Display text. */
    text: string;
    /** Page number as last rendered, when stored. */
    pageNumber?: number;
    /** Heading level (from the referenced paragraph's outlineLevel). */
    level?: number;
    /** Index into `body.paragraphs` of the heading the entry points at. */
    paragraphIndex?: number;
  }[];
}

// Re-export so converters importing "./pages" get the whole envelope surface.
export type { DocumentEnvelope, Drawable, DrawableCommon, IsoDateString, Paragraph, StyledText };
