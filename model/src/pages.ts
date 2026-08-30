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

  /** Table of contents entries if present. [proto: TOCSmartFieldArchive] */
  tableOfContents?: TableOfContents;
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
  }[];
}

// Re-export so converters importing "./pages" get the whole envelope surface.
export type { DocumentEnvelope, Drawable, DrawableCommon, IsoDateString, Paragraph, StyledText };
