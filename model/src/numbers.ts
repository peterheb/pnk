/**
 * pnk JSON document models — Numbers (.numbers).
 *
 * Maps TN.DocumentArchive [1] (+ TSA/TSK supers) onto a resolved,
 * reference-free model: document → sheets → drawables/tables.
 *
 * NOTE on hierarchy: the spec's "sheets → canvases" has no intermediate
 * object in the format — a Numbers sheet IS the free-form canvas
 * (TN.SheetArchive.drawable_infos holds every table/image/chart/shape
 * directly; there is no separate canvas archive — docs/format/numbers.md).
 * The model therefore goes document → sheets → drawables, one level.
 *
 * Tables are fully resolved into the shared TableModel (tiles, storage
 * buffers and header-bucket indirection are flattened — docs/model-design.md);
 * charts resolve their inline grid, with Numbers' table-bound charts carrying
 * a `dataBinding` placeholder. Formulas remain TSceFormulaRef placeholders.
 *
 * Format facts: docs/format/numbers.md (+ tables.md, drawables.md, charts.md).
 */

import type {
  ChartModel,
  Drawable,
  DocumentEnvelope,
  EdgeInsets,
  IsoDateString,
  PageLayoutOrientation,
  StyledText,
  TableModel,
} from "./shared";

// ---------------------------------------------------------------------------
// Print setup (per-sheet, not per-document) — TN.SheetArchive fields 3-14
// ---------------------------------------------------------------------------

export interface SheetPrintSetup {
  orientation?: PageLayoutOrientation;
  showPageNumbers?: boolean;
  /** Print zoom scale (1 = 100%). [proto: content_scale] */
  contentScale?: number;
  /** Page order for multi-page content. [proto: TN.PageOrder] */
  pageOrder?: "down-then-over" | "over-then-down";
  margins?: EdgeInsets;
  /** Page numbering start. [proto: using_start_page_number/start_page_number] */
  startPageNumber?: number;
  useCustomStartPageNumber?: boolean;
  /** Header/footer insets in points. [proto: fields 13/14] */
  pageHeaderInset?: number;
  pageFooterInset?: number;
}

// ---------------------------------------------------------------------------
// Sheets (TN.SheetArchive)
// ---------------------------------------------------------------------------

export interface Sheet {
  /** Sheet name. [proto: field 1, required] */
  name: string;
  hidden?: boolean;
  /** Every object on the sheet canvas, in paint order (z-order). */
  drawables: Drawable[];
  /** Repeating header/footer text of this sheet. [proto: fields 18/19] */
  headers?: StyledText[];
  footers?: StyledText[];
  /** One shared header/footer for all pages instead of first/rest. [proto: field 20] */
  usesSingleHeaderFooter?: boolean;
  /** Tab color / canvas fill. [proto: TN.SheetStyleArchive] */
  style?: { tabColor?: string; fill?: string };
  print?: SheetPrintSetup;
  /** Right-to-left canvas layout. [proto: layout_direction] */
  layoutDirectionRtl?: boolean;
}

// ---------------------------------------------------------------------------
// Document root
// ---------------------------------------------------------------------------

/**
 * The Numbers document model. Envelope fields (`meta`, `warnings`, `fonts`,
 * `media`) follow the shared DocumentEnvelope contract.
 */
export interface NumbersDocument extends DocumentEnvelope {
  kind: "numbers";

  /** One entry per TN.SheetArchive, in document order. */
  sheets: Sheet[];

  /** Document-level print paper size in points. [proto: page_size/paper_id] */
  pageSize?: { width: number; height: number };

  /**
   * Form-based layout entries (TN.FormBasedSheetArchive): a form view bound
   * to one table. Rare; recorded so nothing is silently dropped.
   */
  forms?: { sheetName?: string; boundTableName?: string }[];
}

// Re-export so converters importing "./numbers" get the whole surface.
export type { ChartModel, Drawable, IsoDateString, TableModel };
