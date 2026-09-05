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

/**
 * The stylesheet URL for the faces a document uses, or null when it needs
 * none. Only the weights the fallback actually ships are asked for —
 * requesting a weight a family does not have makes the whole request 400.
 */
export function googleFontsHref(fontNames: readonly string[]): string | null {
  if (!enabled) return null;
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
  if (wanted.size === 0) return null;
  const params = [...wanted].map(([family, faces]) => familyParam(family, faces));
  // display=swap: paint the document immediately in whatever is available
  // and restyle when the substitute arrives, rather than blocking on it.
  return `https://fonts.googleapis.com/css2?${params.join("&")}&display=swap`;
}

/**
 * Point the document's substitute-font stylesheet at what THIS document
 * needs, or remove it. One <link>, replaced per document: the browser's HTTP
 * cache makes a repeat open free.
 */
export function loadSubstituteFonts(fontNames: readonly string[]): void {
  const href = googleFontsHref(fontNames);
  const existing = document.getElementById(LINK_ID) as HTMLLinkElement | null;
  if (!href) {
    existing?.remove();
    return;
  }
  if (existing) {
    if (existing.href !== href) existing.href = href;
    return;
  }
  const link = document.createElement("link");
  link.id = LINK_ID;
  link.rel = "stylesheet";
  link.href = href;
  document.head.appendChild(link);
}
