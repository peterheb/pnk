// Envelope UI: the nav warnings indicator + dropdown. Warnings never block
// rendering — unknown-object-type and friends are context alongside the
// rendered document, and a clean decode shows no indicator at all.

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

export function renderWarnings(warnings: Warning[]): void {
  const panel = document.getElementById("warnings-dd") as HTMLDetailsElement | null;
  if (!panel) return;
  const total = warnings.reduce((n, w) => n + (w.count ?? 1), 0);
  panel.classList.toggle("hidden", total === 0);
  panel.open = false;
  document.getElementById("warnings-count")!.textContent = String(total);
  const list = document.getElementById("warnings-list")!;
  list.replaceChildren();
  // summary: count per code, so a pile of unknown-type-ids stays one line
  const byCode = new Map<string, number>();
  for (const w of warnings) byCode.set(w.code, (byCode.get(w.code) ?? 0) + (w.count ?? 1));
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
    msg.textContent = w.count && w.count > 1 ? `${w.message} (×${w.count})` : w.message;
    row.appendChild(msg);
    const pathText = w.path ?? (w.paths ? w.paths.join(", ") + (w.count && w.count > w.paths.length ? ", …" : "") : undefined);
    if (pathText) {
      const path = document.createElement("span");
      path.className = "path";
      path.textContent = pathText;
      row.appendChild(path);
    }
    list.appendChild(row);
  }
}