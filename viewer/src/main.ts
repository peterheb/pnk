// pnk viewer entry point: init the wasm converter, wire the drop zone /
// file picker, and dispatch to the per-app renderers. No network calls after
// the static assets load — the file is parsed in-process and never uploaded.

import init, { convert, media_bytes } from "./wasm/pnk2json_wasm.js";
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
let jsonRendered = false;

const $ = (id: string) => document.getElementById(id)!;

function closeJsonView(): void {
  jsonRendered = false;
  $("json-view").classList.add("hidden");
  $("json-pre").replaceChildren();
  $("json-btn").classList.remove("active");
}

function showLanding(): void {
  ctx?.dispose();
  ctx = null;
  closeJsonView();
  // the landing card's own CTA is the only "open" on the landing screen
  for (const id of ["doc-filename", "app-badge", "doc-meta", "warnings-dd", "json-btn", "reset-btn"]) {
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

function toggleJsonView(): void {
  if (!lastJson) return;
  const panel = $("json-view");
  const showing = !panel.classList.contains("hidden");
  if (showing) {
    panel.classList.add("hidden");
    $("view").classList.remove("hidden");
    $("json-btn").classList.remove("active");
    return;
  }
  if (!jsonRendered) {
    jsonRendered = true;
    const pretty = JSON.stringify(JSON.parse(lastJson.text), null, 2);
    const kb = pretty.length / 1024;
    $("json-size").textContent = `${lastJson.filename} · ${kb >= 1024 ? (kb / 1024).toFixed(1) + " MB" : Math.ceil(kb) + " KB"} pretty-printed`;
    const pre = $("json-pre");
    if (pretty.length <= HIGHLIGHT_LIMIT) pre.innerHTML = highlightJson(pretty);
    else pre.textContent = pretty;
  }
  $("view").classList.add("hidden");
  panel.classList.remove("hidden");
  $("json-btn").classList.add("active");
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
  for (const id of ["doc-filename", "app-badge", "doc-meta", "json-btn", "reset-btn"]) {
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

  closeJsonView();
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
  closeJsonView();
  $("json-btn").classList.add("hidden");
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
  // json = toggle the pretty-printed model view; download lives in its toolbar
  $("json-btn").addEventListener("click", toggleJsonView);
  // Download the converted JSON model (blob URL — still no network, no upload)
  $("json-dl-btn").addEventListener("click", () => {
    if (!lastJson) return;
    const url = URL.createObjectURL(new Blob([lastJson.text], { type: "application/json" }));
    const a = document.createElement("a");
    a.href = url;
    a.download = lastJson.filename;
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
  $("drop-hint").textContent = "encrypted and pre-2013 files are refused.";
}

boot().catch((err) => {
  $("drop-hint").textContent = `Failed to load the local parser: ${err}`;
});