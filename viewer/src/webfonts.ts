// Substitute faces from Google Fonts.
//
// A document names PostScript faces the reader may not have. `fontmap.ts`
// says which Google Fonts family stands in for each; this module turns the
// document's font list into ONE stylesheet request for exactly the families,
// weights and slopes that document uses, and hands `text.ts` the family name
// to put in its font-family stack.
//
// This is the viewer's only outbound request, and it is a setting: when
// "Load substitute fonts from Google Fonts" is off, nothing is requested and
// the stacks fall through to the fonts the reader has plus the CSS generic.
// The privacy cost when it is on is the usual one for a web font: Google
// sees the reader's IP address and which families were asked for. It never
// sees the document — the file is parsed in-page and only family NAMES leave
// the browser.

import { fallbackFor, nearestWeight, parseFace } from "./fontmap";

const SETTING_KEY = "pnk.googleFonts";
const LINK_ID = "pnk-webfonts";
/** A document that names more than this many substitutable families is
 *  asking for more bytes than the render is worth; the rest fall through to
 *  the generic. No corpus document comes close. */
const MAX_FAMILIES = 16;

function readSetting(): boolean {
  try {
    return window.localStorage.getItem(SETTING_KEY) !== "0";
  } catch {
    // Storage can throw (private mode, blocked site data). Default is on.
    return true;
  }
}

// Read once: `text.ts` asks per run, and a localStorage hit per run on a
// 200-page document is a measurable cost.
let enabled = readSetting();

export function googleFontsEnabled(): boolean {
  return enabled;
}

export function setGoogleFontsEnabled(on: boolean): void {
  enabled = on;
  try {
    window.localStorage.setItem(SETTING_KEY, on ? "1" : "0");
  } catch {
    /* the setting is then per-session only */
  }
}

/**
 * The Google Fonts family that stands in for a PostScript name, or null when
 * the setting is off, the family is unknown, or no substitute beats the
 * generic. Goes into the font-family stack AFTER the document's own face, so
 * a reader who has the real font never sees the substitute.
 */
export function substituteFamily(psName: string): string | null {
  if (!enabled) return null;
  return fallbackFor(psName)?.family ?? null;
}

/** `family=Inter:ital,wght@0,300;0,400;1,400` for one family. */
function familyParam(family: string, faces: ReadonlySet<string>): string {
  const specs = [...faces].sort((a, b) => {
    const [ai, aw] = a.split(",").map(Number) as [number, number];
    const [bi, bw] = b.split(",").map(Number) as [number, number];
    return ai - bi || aw - bw;
  });
  const name = family.replace(/ /g, "+");
  if (specs.every((s) => s.startsWith("0,"))) {
    return `family=${name}:wght@${specs.map((s) => s.slice(2)).join(";")}`;
  }
  return `family=${name}:ital,wght@${specs.join(";")}`;
}

/** One substitute face a document needs: a Google Fonts family plus the
 *  weight and slope actually requested. */
interface Face {
  family: string;
  weight: number;
  italic: boolean;
}

/**
 * The substitute faces a document's font list resolves to, grouped by
 * family. Only the weights the fallback actually ships are asked for —
 * requesting a weight a family does not have makes the whole request 400.
 */
function wantedFaces(fontNames: readonly string[]): Face[] {
  if (!enabled) return [];
  const wanted = new Map<string, Set<string>>();
  for (const name of fontNames) {
    const fb = fallbackFor(name);
    if (!fb?.family) continue;
    const face = parseFace(name);
    const weight = nearestWeight(fb.weights, face.weight);
    const ital = face.italic && fb.italic ? 1 : 0;
    let faces = wanted.get(fb.family);
    if (!faces) {
      if (wanted.size >= MAX_FAMILIES) continue;
      faces = new Set<string>();
      wanted.set(fb.family, faces);
    }
    faces.add(`${ital},${weight}`);
  }
  const out: Face[] = [];
  for (const [family, faces] of wanted) {
    for (const f of faces) {
      const [ital, weight] = f.split(",").map(Number) as [number, number];
      out.push({ family, weight, italic: ital === 1 });
    }
  }
  return out;
}

/** The stylesheet URL for the faces a document uses, or null when it needs none. */
export function googleFontsHref(fontNames: readonly string[]): string | null {
  const faces = wantedFaces(fontNames);
  if (faces.length === 0) return null;
  const byFamily = new Map<string, Set<string>>();
  for (const f of faces) {
    let set = byFamily.get(f.family);
    if (!set) byFamily.set(f.family, (set = new Set<string>()));
    set.add(`${f.italic ? 1 : 0},${f.weight}`);
  }
  const params = [...byFamily].map(([family, set]) => familyParam(family, set));
  // display=swap: paint the document immediately in whatever is available
  // and restyle when the substitute arrives, rather than blocking on it.
  return `https://fonts.googleapis.com/css2?${params.join("&")}&display=swap`;
}

/** How long a render waits for the substitute faces before going ahead with
 *  what it has. Google Fonts answers in well under a second on a normal
 *  connection; an offline reader should not stare at a blank page. */
const FONT_WAIT_MS = 4000;

/**
 * Resolve once every face is usable for layout, the wait expires, or the
 * stylesheet fails. A paginated document must be MEASURED with the faces
 * it will be painted in: with display=swap the fallback face lays the pages
 * out and the substitute then restyles them, so lines re-wrap and the text
 * runs past the page bottom (4047e81b0665 page 6, the body over the footer).
 * `document.fonts.load` fetches a face even before any text uses it.
 */
function fontsSettled(faces: readonly Face[], link: HTMLLinkElement, fresh: boolean): Promise<void> {
  if (faces.length === 0 || typeof document.fonts?.load !== "function") return Promise.resolve();
  const sheet = fresh
    ? new Promise<void>((resolve) => {
        link.addEventListener("load", () => resolve(), { once: true });
        link.addEventListener("error", () => resolve(), { once: true });
      })
    : Promise.resolve();
  const loads = sheet.then(() =>
    Promise.all(
      faces.map((f) =>
        document.fonts.load(`${f.italic ? "italic " : ""}${f.weight} 16px "${f.family}"`).catch(() => []),
      ),
    ),
  );
  const expiry = new Promise<void>((resolve) => setTimeout(resolve, FONT_WAIT_MS));
  return Promise.race([loads.then(() => undefined), expiry]);
}

/**
 * Point the document's substitute-font stylesheet at what THIS document
 * needs, or remove it. One <link>, replaced per document: the browser's HTTP
 * cache makes a repeat open free. Resolves when the faces are usable for
 * layout (or the wait expires), so a caller can measure with them.
 */
export function loadSubstituteFonts(fontNames: readonly string[]): Promise<void> {
  const href = googleFontsHref(fontNames);
  const existing = document.getElementById(LINK_ID) as HTMLLinkElement | null;
  if (!href) {
    existing?.remove();
    return Promise.resolve();
  }
  const faces = wantedFaces(fontNames);
  if (existing) {
    const fresh = existing.href !== href;
    if (fresh) existing.href = href;
    return fontsSettled(faces, existing, fresh);
  }
  const link = document.createElement("link");
  link.id = LINK_ID;
  link.rel = "stylesheet";
  link.href = href;
  document.head.appendChild(link);
  return fontsSettled(faces, link, true);
}
