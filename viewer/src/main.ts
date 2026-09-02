// pnk viewer entry point: init the wasm converter, wire the drop zone /
// file picker, and dispatch to the per-app renderers. No network calls after
// the static assets load — the file is parsed in-process and never uploaded.

import init, { convert, dump_markdown, dump_text, media_bytes } from "./wasm/pnk2json_wasm.js";
import { ViewerCtx } from "./ctx";
import { hydrate } from "./hydrate";
import { mapError, renderErrorCard } from "./errors";
import { renderKeynote } from "./keynote";
import { renderNumbers } from "./numbers";
import { setTableLocale } from "./tables";
import { renderPages } from "./pages";
import { renderWarnings } from "./warnings";
import type { PnkDocument } from "../../model/src/shared";

let ctx: ViewerCtx | null = null;
let lastJson: { text: string; filename: string } | null = null;

// Source views over the converted document: the JSON model, and the two
// dumps the CLI offers as --text / --markdown, produced by the wasm module
// from the same conversion the render came from. Each is built on first
// use and cached for the document's lifetime.
type SourceMode = "json" | "text" | "markdown";
const SOURCE_BTN_IDS = ["text-btn", "md-btn", "json-btn"];
let sourceMode: SourceMode | null = null;
const sourceCache = new Map<SourceMode, string>();

const $ = (id: string) => document.getElementById(id)!;

function closeSourceView(): void {
  sourceMode = null;
  sourceCache.clear();
  $("json-view").classList.add("hidden");
  $("json-pre").replaceChildren();
  for (const id of SOURCE_BTN_IDS) $(id).classList.remove("active");
}

function showLanding(): void {
  ctx?.dispose();
  ctx = null;
  closeSourceView();
  // the landing card's own CTA is the only "open" on the landing screen
  for (const id of ["doc-filename", "app-badge", "doc-meta", "warnings-dd", ...SOURCE_BTN_IDS, "reset-btn"]) {
    $(id).classList.add("hidden");
  }
  $("view").classList.add("hidden");
  $("view").replaceChildren();
  $("drop-zone").classList.remove("hidden");
}

// Above this many pretty-printed bytes, skip syntax coloring: a dense
// Numbers envelope runs to tens of MB and a span-per-token DOM would hang
// the tab. Plain <pre> text stays fast at any size.
const HIGHLIGHT_LIMIT = 3 * 1024 * 1024;

// Pretty-print with compaction: any object/array whose one-line form fits
// the line budget stays on one line ({"x": 511.5, "y": 728.5} instead of
// four lines), which is how a person would write it. Built bottom-up so
// every node is stringified exactly once — no quadratic restringify on
// multi-MB envelopes.
const LINE_BUDGET = 76;

function prettyJson(v: unknown, indent = ""): { s: string; flat: boolean } {
  if (v === null || typeof v !== "object") {
    const s = JSON.stringify(v) ?? "null";
    return { s, flat: true };
  }
  const isArr = Array.isArray(v);
  const kids: { head: string; r: { s: string; flat: boolean } }[] = isArr
    ? (v as unknown[]).map((x) => ({ head: "", r: prettyJson(x, indent + "  ") }))
    : Object.entries(v as Record<string, unknown>).map(([k, x]) => ({
        head: `${JSON.stringify(k)}: `,
        r: prettyJson(x, indent + "  "),
      }));
  if (kids.length === 0) return { s: isArr ? "[]" : "{}", flat: true };
  if (kids.every(({ r }) => r.flat)) {
    const one = (isArr ? "[" : "{ ") + kids.map(({ head, r }) => head + r.s).join(", ") + (isArr ? "]" : " }");
    if (indent.length + one.length <= LINE_BUDGET) return { s: one, flat: true };
  }
  const inner = kids.map(({ head, r }) => indent + "  " + head + r.s).join(",\n");
  return { s: (isArr ? "[" : "{") + "\n" + inner + "\n" + indent + (isArr ? "]" : "}"), flat: false };
}

function highlightJson(pretty: string): string {
  const esc = pretty.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return esc.replace(
    /("(?:[^"\\]|\\.)*")(\s*:)?|\b(true|false|null)\b|-?\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b/g,
    (m, str: string | undefined, colon: string | undefined, kw: string | undefined) => {
      if (str !== undefined) {
        return colon !== undefined
          ? `<span class="j-key">${str}</span>${colon}`
          : `<span class="j-str">${str}</span>`;
      }
      if (kw !== undefined) return `<span class="j-kw">${kw}</span>`;
      return `<span class="j-num">${m}</span>`;
    },
  );
}

const SOURCE_EXT: Record<SourceMode, string> = { json: ".json", text: ".txt", markdown: ".md" };
const SOURCE_MIME: Record<SourceMode, string> = { json: "application/json", text: "text/plain", markdown: "text/markdown" };

/** The source text for a mode, built once per document. */
function sourceText(mode: SourceMode): string {
  const cached = sourceCache.get(mode);
  if (cached !== undefined) return cached;
  let text: string;
  if (mode === "json") text = prettyJson(JSON.parse(lastJson!.text)).s;
  else if (mode === "text") text = dump_text();
  else text = dump_markdown();
  sourceCache.set(mode, text);
  return text;
}

/** Click on a source button: switch to that mode, or back to the render
 *  when it is the one already showing. */
function toggleSourceView(mode: SourceMode): void {
  if (!lastJson) return;
  const panel = $("json-view");
  if (sourceMode === mode) {
    sourceMode = null;
    panel.classList.add("hidden");
    $("view").classList.remove("hidden");
    for (const id of SOURCE_BTN_IDS) $(id).classList.remove("active");
    return;
  }
  sourceMode = mode;
  const text = sourceText(mode);
  const kb = text.length / 1024;
  const size = kb >= 1024 ? (kb / 1024).toFixed(1) + " MB" : Math.ceil(kb) + " KB";
  const label = mode === "json" ? "pretty-printed" : mode === "text" ? "plain text" : "markdown";
  $("json-size").textContent = `${lastJson.filename.replace(/\.json$/, SOURCE_EXT[mode])} · ${size} ${label}`;
  const pre = $("json-pre");
  if (mode === "json" && text.length <= HIGHLIGHT_LIMIT) pre.innerHTML = highlightJson(text);
  else pre.textContent = text;
  $("view").classList.add("hidden");
  panel.classList.remove("hidden");
  for (const id of SOURCE_BTN_IDS) $(id).classList.toggle("active", ($(id) as HTMLElement).dataset.mode === mode);
}

function renderHeader(doc: PnkDocument, filename: string): void {
  $("doc-filename").textContent = filename;
  const badge = $("app-badge");
  badge.textContent = doc.meta.application ?? doc.meta.app;
  badge.dataset.app = doc.kind;
  const meta: string[] = [];
  if (doc.meta.fileFormatVersion) meta.push(`v${doc.meta.fileFormatVersion}`);
  if (doc.meta.locale) meta.push(doc.meta.locale);
  $("doc-meta").textContent = meta.join(" · ");
  for (const id of ["doc-filename", "app-badge", "doc-meta", ...SOURCE_BTN_IDS, "reset-btn"]) {
    $(id).classList.remove("hidden");
  }
}

function renderDocument(doc: PnkDocument, filename: string): void {
  ctx?.dispose();
  const mediaCtx = new ViewerCtx();
  ctx = mediaCtx;

  // media bytes: per-dataId raw fetch from the wasm side (no base64 in the
  // envelope); missing bytes render as a labeled placeholder instead
  if (typeof media_bytes === "function") {
    for (const asset of doc.media) {
      const bytes = media_bytes(asset.dataId);
      if (bytes) mediaCtx.addMedia(asset.dataId, bytes, asset.fileName ?? asset.preferredFileName);
    }
  }

  closeSourceView();
  renderHeader(doc, filename);
  setTableLocale(doc.meta.locale);
  renderWarnings(doc.warnings);

  const view = $("view");
  view.replaceChildren();
  view.classList.remove("hidden");
  $("drop-zone").classList.add("hidden");

  if (doc.kind === "keynote") renderKeynote(doc, hydrate(doc), mediaCtx, view);
  else if (doc.kind === "numbers") renderNumbers(doc, hydrate(doc), mediaCtx, view);
  else renderPages(doc, hydrate(doc), mediaCtx, view);
}

function showError(err: unknown, filename: string): void {
  lastJson = null;
  closeSourceView();
  for (const id of SOURCE_BTN_IDS) $(id).classList.add("hidden");
  $("warnings-dd").classList.add("hidden");
  $("drop-zone").classList.add("hidden");
  $("doc-filename").textContent = filename;
  $("doc-filename").classList.remove("hidden");
  const badge = $("app-badge");
  badge.textContent = "rejected";
  delete badge.dataset.app;
  badge.classList.remove("hidden");
  $("reset-btn").classList.remove("hidden");
  $("doc-meta").textContent = "";
  $("doc-meta").classList.add("hidden");
  const view = $("view");
  view.replaceChildren();
  view.classList.remove("hidden");
  renderErrorCard(mapError(err, filename), view);
}

async function handleFile(file: File): Promise<void> {
  // The current document (or landing card) stays on screen while we parse;
  // the swap happens only once the new document is ready (or errors out).
  const status = $("parse-status");
  status.textContent = `Parsing ${file.name}…`;
  status.classList.remove("hidden");
  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    const json = convert(bytes);
    const doc = JSON.parse(json) as PnkDocument;
    lastJson = { text: json, filename: file.name.replace(/\.[^.]+$/, "") + ".json" };
    renderDocument(doc, file.name);
  } catch (err) {
    showError(err, file.name);
  } finally {
    status.classList.add("hidden");
  }
}

// Whole-window drag & drop: any file drag anywhere over the app raises a
// full-viewport overlay; dropping loads the file, whatever view is showing.
function wireDragAndDrop(): void {
  const overlay = $("drag-overlay");
  const target = $("drop-target");
  let depth = 0; // dragenter/leave fire per descendant element — count them

  const isFileDrag = (e: DragEvent) =>
    Array.from(e.dataTransfer?.types ?? []).includes("Files");
  const hideOverlay = () => {
    depth = 0;
    overlay.classList.add("hidden");
    target.classList.remove("dragover");
  };

  window.addEventListener("dragenter", (e) => {
    if (!isFileDrag(e)) return;
    e.preventDefault();
    depth++;
    overlay.classList.remove("hidden");
    target.classList.add("dragover");
  });
  window.addEventListener("dragover", (e) => {
    if (isFileDrag(e)) e.preventDefault();
  });
  window.addEventListener("dragleave", (e) => {
    if (!isFileDrag(e)) return;
    if (--depth <= 0) hideOverlay();
  });
  window.addEventListener("drop", (e) => {
    e.preventDefault();
    hideOverlay();
    const file = e.dataTransfer?.files?.[0];
    if (file) handleFile(file);
  });
}

function wireEvents(): void {
  const input = $("file-input") as HTMLInputElement;

  $("pick-btn").addEventListener("click", () => input.click());
  // nav "open…" goes straight to the picker; the brand is the way home
  $("reset-btn").addEventListener("click", () => {
    input.value = "";
    input.click();
  });
  $("brand").addEventListener("click", (e) => {
    e.preventDefault();
    showLanding();
  });
  // text / md / json = toggle a source view; download lives in its toolbar
  for (const id of SOURCE_BTN_IDS) {
    const btn = $(id);
    btn.addEventListener("click", () => toggleSourceView(btn.dataset.mode as SourceMode));
  }
  // Download the showing source (blob URL — still no network, no upload);
  // the JSON download keeps the compact model the converter emitted
  $("json-dl-btn").addEventListener("click", () => {
    if (!lastJson) return;
    const mode = sourceMode ?? "json";
    const body = mode === "json" ? lastJson.text : sourceText(mode);
    const url = URL.createObjectURL(new Blob([body], { type: SOURCE_MIME[mode] }));
    const a = document.createElement("a");
    a.href = url;
    a.download = lastJson.filename.replace(/\.json$/, SOURCE_EXT[mode]);
    a.click();
    URL.revokeObjectURL(url);
  });
  input.addEventListener("change", () => {
    if (input.files?.[0]) handleFile(input.files[0]);
  });
  wireDragAndDrop();
}

async function boot(): Promise<void> {
  await init("wasm/pnk2json_wasm_bg.wasm");
  wireEvents();
}

boot().catch((err) => {
  const hint = $("drop-hint");
  hint.textContent = `Failed to load the local parser: ${err}`;
  hint.classList.remove("hidden");
});