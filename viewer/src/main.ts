// pnk viewer entry point: init the wasm converter, wire the drop zone /
// file picker, and dispatch to the per-app renderers. No network calls after
// the static assets load — the file is parsed in-process and never uploaded.

import init, { convert, media_bytes } from "./wasm/pnk2json_wasm.js";
import { ViewerCtx } from "./ctx";
import { mapError, renderErrorCard } from "./errors";
import { renderKeynote } from "./keynote";
import { renderNumbers } from "./numbers";
import { renderPages } from "./pages";
import { renderFonts, renderWarnings } from "./warnings";
import type { PnkDocument } from "../../model/src/shared";

let ctx: ViewerCtx | null = null;

const $ = (id: string) => document.getElementById(id)!;

function showLanding(): void {
  ctx?.dispose();
  ctx = null;
  $("app-header").classList.add("hidden");
  $("panel-fonts").classList.add("hidden");
  $("panel-warnings").classList.add("hidden");
  $("view").classList.add("hidden");
  $("view").replaceChildren();
  $("drop-zone").classList.remove("hidden");
}

function renderHeader(doc: PnkDocument, filename: string): void {
  $("doc-filename").textContent = filename;
  $("app-badge").textContent = doc.meta.application ?? doc.meta.app;
  const meta: string[] = [];
  if (doc.meta.fileFormatVersion) meta.push(`format ${doc.meta.fileFormatVersion}`);
  if (doc.meta.createdAt) meta.push(`created ${doc.meta.createdAt.slice(0, 10)}`);
  if (doc.meta.modifiedAt) meta.push(`modified ${doc.meta.modifiedAt.slice(0, 10)}`);
  if (doc.meta.locale) meta.push(doc.meta.locale);
  if (doc.meta.documentId) meta.push(`id ${doc.meta.documentId.slice(0, 8)}`);
  $("doc-meta").textContent = meta.join("  ·  ");
  $("app-header").classList.remove("hidden");
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

  renderHeader(doc, filename);
  renderFonts(doc.fonts);
  renderWarnings(doc.warnings);

  const view = $("view");
  view.replaceChildren();
  view.classList.remove("hidden");
  $("drop-zone").classList.add("hidden");

  if (doc.kind === "keynote") renderKeynote(doc, mediaCtx, view);
  else if (doc.kind === "numbers") renderNumbers(doc, mediaCtx, view);
  else renderPages(doc, mediaCtx, view);
}

function showError(err: unknown, filename: string): void {
  $("drop-zone").classList.add("hidden");
  $("app-header").classList.remove("hidden");
  $("doc-filename").textContent = filename;
  $("app-badge").textContent = "rejected";
  $("doc-meta").textContent = "";
  const view = $("view");
  view.replaceChildren();
  view.classList.remove("hidden");
  renderErrorCard(mapError(err, filename), view);
}

async function handleFile(file: File): Promise<void> {
  showLanding();
  const hint = $("drop-hint");
  hint.textContent = `Parsing ${file.name}…`;
  $("drop-zone").classList.remove("hidden");
  try {
    const bytes = new Uint8Array(await file.arrayBuffer());
    const doc = JSON.parse(convert(bytes)) as PnkDocument;
    renderDocument(doc, file.name);
  } catch (err) {
    showError(err, file.name);
  }
}

function wireEvents(): void {
  const input = $("file-input") as HTMLInputElement;
  const drop = $("drop-target");

  $("pick-btn").addEventListener("click", () => input.click());
  $("reset-btn").addEventListener("click", () => {
    input.value = "";
    showLanding();
  });
  input.addEventListener("change", () => {
    if (input.files?.[0]) handleFile(input.files[0]);
  });

  for (const zone of [drop, $("drop-zone")]) {
    zone.addEventListener("dragover", (e) => {
      e.preventDefault();
      drop.classList.add("dragover");
    });
    zone.addEventListener("dragleave", () => drop.classList.remove("dragover"));
  }
  window.addEventListener("drop", (e) => {
    e.preventDefault();
    drop.classList.remove("dragover");
    const file = (e as DragEvent).dataTransfer?.files?.[0];
    if (file) handleFile(file);
  });
}

async function boot(): Promise<void> {
  await init("wasm/pnk2json_wasm_bg.wasm");
  wireEvents();
  $("drop-hint").textContent =
    "Encrypted (password-protected) and legacy pre-iWork '13 files are politely refused — nothing about them leaves the browser either.";
}

boot().catch((err) => {
  $("drop-hint").textContent = `Failed to load the local parser: ${err}`;
});