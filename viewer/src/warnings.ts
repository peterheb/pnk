// Envelope UI: fonts (font-ready indicator) and warnings (collapsible).
// Warnings never block rendering — unknown-object-type and friends are shown
// as context alongside the rendered document.

import type { Warning } from "../../model/src/shared";

const CODE_LABELS: Record<string, string> = {
  "unknown-object-type": "unknown object type",
  "undecodable-object": "undecodable object",
  "unresolved-reference": "unresolved reference",
  "unsupported-feature": "unsupported feature",
  "media-missing": "media missing",
  "color-degraded": "color degraded",
  "legacy-variant": "legacy variant",
  "table-degraded": "degraded table",
  "formula-unparsed": "formula not parsed",
};

export function renderFonts(fonts: string[]): void {
  const panel = document.getElementById("panel-fonts");
  if (!panel) return;
  panel.classList.remove("hidden");
  document.getElementById("fonts-count")!.textContent = String(fonts.length);
  const list = document.getElementById("fonts-list")!;
  list.replaceChildren();
  for (const f of fonts) {
    const chip = document.createElement("span");
    chip.className = "chip";
    chip.textContent = f;
    list.appendChild(chip);
  }
  if (fonts.length === 0) list.textContent = "none";
}

export function renderWarnings(warnings: Warning[]): void {
  const panel = document.getElementById("panel-warnings");
  if (!panel) return;
  panel.classList.remove("hidden");
  document.getElementById("warnings-count")!.textContent = String(warnings.length);
  const list = document.getElementById("warnings-list")!;
  list.replaceChildren();
  // summary: count per code, so a pile of unknown-type-ids stays one line
  const byCode = new Map<string, number>();
  for (const w of warnings) byCode.set(w.code, (byCode.get(w.code) ?? 0) + 1);
  const summary = document.createElement("div");
  summary.className = "chips";
  for (const [code, count] of byCode) {
    const chip = document.createElement("span");
    chip.className = "chip";
    chip.dataset.warningCode = code;
    chip.textContent = `${CODE_LABELS[code] ?? code} × ${count}`;
    summary.appendChild(chip);
  }
  if (warnings.length > 0) list.appendChild(summary);

  for (const w of warnings) {
    const row = document.createElement("div");
    row.className = "warning-row";
    const code = document.createElement("code");
    code.textContent = CODE_LABELS[w.code] ?? w.code;
    row.appendChild(code);
    const msg = document.createElement("span");
    msg.textContent = w.message;
    row.appendChild(msg);
    if (w.path) {
      const path = document.createElement("span");
      path.className = "path";
      path.textContent = w.path;
      row.appendChild(path);
    }
    list.appendChild(row);
  }
  if (warnings.length === 0) list.textContent = "No warnings — clean decode.";
}