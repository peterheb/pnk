// Google Fonts substitutes for the fonts iWork documents ask for.
//
// A document stores a PostScript FACE name ("HelveticaNeue-Light",
// "Calibri-Bold", "TimesNewRomanPSMT"). When the reader's machine has that
// font the browser uses it and nothing here matters. When it does not — the
// usual case off macOS — the choice is between the browser's generic default
// (everything becomes Times or Helvetica) and a face of the same class and
// roughly the same width. This module is the table of those choices, derived
// from a census of 960 corpus documents (491 distinct PostScript names, 253
// normalized families) recorded in `docs/fonts.md`.
//
// Data only: nothing here touches the DOM or the network. `webfonts.ts`
// turns these entries into a stylesheet request; `text.ts` turns them into a
// font-family stack.

/** How much of the original a substitute preserves. */
export type FallbackKind =
  /** Same advance widths as the original, so line breaks land in the same
   *  places: the document's layout survives. Also used when the requested
   *  family IS the Google Fonts family (Open Sans, Inter, Roboto…). */
  | "metric-clone"
  /** Same classification and similar proportions, but its own metrics: text
   *  will run shorter or longer than Apple's layout. */
  | "close"
  /** No substitute is better than the browser's own default for this class. */
  | "generic";

export type GenericFamily = "sans-serif" | "serif" | "monospace" | "cursive" | "fantasy";

interface GoogleFamily {
  /** Static weights the family publishes (variable families are listed by
   *  the instances Google's API serves). From fonts.google.com/metadata/fonts,
   *  fetched 2026-09-05. */
  readonly weights: readonly number[];
  /** The family ships real italics (as opposed to none, or synthesized). */
  readonly italic: boolean;
  /** What to fall through to if even the Google font fails to load. */
  readonly generic: GenericFamily;
}

const W_100_900 = [100, 200, 300, 400, 500, 600, 700, 800, 900] as const;

/**
 * Every Google Fonts family this table points at, with the weights it
 * actually ships — a fallback that lacks the weight a face asks for is a
 * real fidelity loss, so the loader needs the list to pick the nearest.
 */
const GOOGLE = {
  // metric-compatible clones (see docs/fonts.md for the sources)
  Arimo: { weights: [400, 500, 600, 700], italic: true, generic: "sans-serif" },
  Tinos: { weights: [400, 700], italic: true, generic: "serif" },
  Cousine: { weights: [400, 700], italic: true, generic: "monospace" },
  Carlito: { weights: [400, 700], italic: true, generic: "sans-serif" },
  Caladea: { weights: [400, 700], italic: true, generic: "serif" },
  Gelasio: { weights: [400, 500, 600, 700], italic: true, generic: "serif" },
  "Comic Relief": { weights: [400, 700], italic: false, generic: "cursive" },

  // sans
  Inter: { weights: W_100_900, italic: true, generic: "sans-serif" },
  "Open Sans": { weights: [300, 400, 500, 600, 700, 800], italic: true, generic: "sans-serif" },
  "Noto Sans": { weights: W_100_900, italic: true, generic: "sans-serif" },
  "Source Sans 3": { weights: [200, 300, 400, 500, 600, 700, 800, 900], italic: true, generic: "sans-serif" },
  Lato: { weights: [100, 300, 400, 700, 900], italic: true, generic: "sans-serif" },
  Cabin: { weights: [400, 500, 600, 700], italic: true, generic: "sans-serif" },
  "Fira Sans": { weights: W_100_900, italic: true, generic: "sans-serif" },
  "Nunito Sans": { weights: [200, 300, 400, 500, 600, 700, 800, 900, 1000], italic: true, generic: "sans-serif" },
  Nunito: { weights: [200, 300, 400, 500, 600, 700, 800, 900, 1000], italic: true, generic: "sans-serif" },
  Montserrat: { weights: W_100_900, italic: true, generic: "sans-serif" },
  Poppins: { weights: W_100_900, italic: true, generic: "sans-serif" },
  Jost: { weights: W_100_900, italic: true, generic: "sans-serif" },
  Figtree: { weights: [300, 400, 500, 600, 700, 800, 900], italic: true, generic: "sans-serif" },
  Mulish: { weights: [200, 300, 400, 500, 600, 700, 800, 900, 1000], italic: true, generic: "sans-serif" },
  Archivo: { weights: W_100_900, italic: true, generic: "sans-serif" },
  "Libre Franklin": { weights: W_100_900, italic: true, generic: "sans-serif" },
  Raleway: { weights: W_100_900, italic: true, generic: "sans-serif" },
  Rubik: { weights: [300, 400, 500, 600, 700, 800, 900], italic: true, generic: "sans-serif" },
  Roboto: { weights: W_100_900, italic: true, generic: "sans-serif" },
  Quicksand: { weights: [300, 400, 500, 600, 700], italic: false, generic: "sans-serif" },
  "IBM Plex Sans": { weights: [100, 200, 300, 400, 500, 600, 700], italic: true, generic: "sans-serif" },
  "Julius Sans One": { weights: [400], italic: false, generic: "sans-serif" },
  Gudea: { weights: [400, 700], italic: true, generic: "sans-serif" },
  Tajawal: { weights: [200, 300, 400, 500, 700, 800, 900], italic: false, generic: "sans-serif" },
  Cairo: { weights: [200, 300, 400, 500, 600, 700, 800, 900, 1000], italic: false, generic: "sans-serif" },

  // condensed sans
  "Barlow Condensed": { weights: W_100_900, italic: true, generic: "sans-serif" },
  "Barlow Semi Condensed": { weights: W_100_900, italic: true, generic: "sans-serif" },
  "Archivo Narrow": { weights: [400, 500, 600, 700], italic: true, generic: "sans-serif" },
  "Encode Sans Condensed": { weights: W_100_900, italic: false, generic: "sans-serif" },
  "Encode Sans Semi Condensed": { weights: W_100_900, italic: false, generic: "sans-serif" },
  Oswald: { weights: [200, 300, 400, 500, 600, 700], italic: false, generic: "sans-serif" },
  Anton: { weights: [400], italic: false, generic: "sans-serif" },
  "Bebas Neue": { weights: [400], italic: false, generic: "sans-serif" },
  "League Gothic": { weights: [400], italic: false, generic: "sans-serif" },

  // serif
  "Libre Baskerville": { weights: [400, 500, 600, 700], italic: true, generic: "serif" },
  "EB Garamond": { weights: [400, 500, 600, 700, 800], italic: true, generic: "serif" },
  Vollkorn: { weights: [400, 500, 600, 700, 800, 900], italic: true, generic: "serif" },
  "Crimson Pro": { weights: [200, 300, 400, 500, 600, 700, 800, 900], italic: true, generic: "serif" },
  "PT Serif": { weights: [400, 700], italic: true, generic: "serif" },
  "Noto Serif": { weights: W_100_900, italic: true, generic: "serif" },
  "Source Serif 4": { weights: [200, 300, 400, 500, 600, 700, 800, 900], italic: true, generic: "serif" },
  "Playfair Display": { weights: [400, 500, 600, 700, 800, 900], italic: true, generic: "serif" },
  "Bodoni Moda": { weights: [400, 500, 600, 700, 800, 900], italic: true, generic: "serif" },
  Cormorant: { weights: [300, 400, 500, 600, 700], italic: true, generic: "serif" },
  "Libre Caslon Text": { weights: [400, 700], italic: true, generic: "serif" },
  "Sorts Mill Goudy": { weights: [400], italic: true, generic: "serif" },
  "Charis SIL": { weights: [400, 700], italic: true, generic: "serif" },
  "Gentium Plus": { weights: [400, 700], italic: true, generic: "serif" },
  "STIX Two Text": { weights: [400, 500, 600, 700], italic: true, generic: "serif" },
  "STIX Two Math": { weights: [400], italic: false, generic: "serif" },
  Cinzel: { weights: [400, 500, 600, 700, 800, 900], italic: false, generic: "serif" },
  Marcellus: { weights: [400], italic: false, generic: "serif" },
  Eczar: { weights: [400, 500, 600, 700, 800], italic: false, generic: "serif" },

  // slab
  Bitter: { weights: W_100_900, italic: true, generic: "serif" },
  Rokkitt: { weights: W_100_900, italic: true, generic: "serif" },
  "Zilla Slab": { weights: [300, 400, 500, 600, 700], italic: true, generic: "serif" },
  Cutive: { weights: [400], italic: false, generic: "serif" },
  "Alfa Slab One": { weights: [400], italic: false, generic: "serif" },

  // monospace
  "Roboto Mono": { weights: [100, 200, 300, 400, 500, 600, 700], italic: true, generic: "monospace" },
  Inconsolata: { weights: [200, 300, 400, 500, 600, 700, 800, 900], italic: false, generic: "monospace" },
  "IBM Plex Mono": { weights: [100, 200, 300, 400, 500, 600, 700], italic: true, generic: "monospace" },
  "JetBrains Mono": { weights: [100, 200, 300, 400, 500, 600, 700, 800], italic: true, generic: "monospace" },
  "Source Code Pro": { weights: [200, 300, 400, 500, 600, 700, 800, 900], italic: true, generic: "monospace" },
  "Anonymous Pro": { weights: [400, 700], italic: true, generic: "monospace" },
  "Fira Code": { weights: [300, 400, 500, 600, 700], italic: false, generic: "monospace" },

  // handwriting / script
  "Comic Neue": { weights: [300, 400, 700], italic: true, generic: "cursive" },
  "Patrick Hand": { weights: [400], italic: false, generic: "cursive" },
  Caveat: { weights: [400, 500, 600, 700], italic: false, generic: "cursive" },
  "Permanent Marker": { weights: [400], italic: false, generic: "cursive" },
  "Rock Salt": { weights: [400], italic: false, generic: "cursive" },
  "Dancing Script": { weights: [400, 500, 600, 700], italic: false, generic: "cursive" },
  "Great Vibes": { weights: [400], italic: false, generic: "cursive" },
  Italianno: { weights: [400], italic: false, generic: "cursive" },
  Tangerine: { weights: [400, 700], italic: false, generic: "cursive" },

  // CJK and other scripts (large files — see the size note in docs/fonts.md)
  "Noto Sans JP": { weights: W_100_900, italic: false, generic: "sans-serif" },
  "Noto Serif JP": { weights: [200, 300, 400, 500, 600, 700, 800, 900], italic: false, generic: "serif" },
  "Zen Maru Gothic": { weights: [300, 400, 500, 700, 900], italic: false, generic: "sans-serif" },
  "Noto Sans SC": { weights: W_100_900, italic: false, generic: "sans-serif" },
  "Noto Serif SC": { weights: [200, 300, 400, 500, 600, 700, 800, 900], italic: false, generic: "serif" },
  "Noto Sans TC": { weights: W_100_900, italic: false, generic: "sans-serif" },
  "Noto Sans KR": { weights: W_100_900, italic: false, generic: "sans-serif" },
  "Noto Naskh Arabic": { weights: [400, 500, 600, 700], italic: false, generic: "serif" },
  "Noto Sans Thai": { weights: W_100_900, italic: false, generic: "sans-serif" },
  "Noto Sans Cherokee": { weights: W_100_900, italic: false, generic: "sans-serif" },
  "Noto Sans Gurmukhi": { weights: W_100_900, italic: false, generic: "sans-serif" },
  "Noto Sans Canadian Aboriginal": { weights: W_100_900, italic: false, generic: "sans-serif" },
} as const satisfies Record<string, GoogleFamily>;

export type GoogleFamilyName = keyof typeof GOOGLE;

/** The whole catalogue, for callers that need the weight lists. */
export const GOOGLE_FAMILIES: Readonly<Record<string, GoogleFamily>> = GOOGLE;

/** One family's entry: the Google family to load, or null for "generic only". */
export interface FontFallback {
  readonly family: GoogleFamilyName | null;
  readonly kind: FallbackKind;
  readonly generic: GenericFamily;
  readonly weights: readonly number[];
  readonly italic: boolean;
}

type Entry =
  | readonly [GoogleFamilyName, FallbackKind]
  | readonly [GoogleFamilyName, FallbackKind, GenericFamily]
  | readonly [null, "generic", GenericFamily];

/**
 * Normalized family key -> substitute. Ordered by the number of corpus
 * documents that ask for the family (docs/fonts.md carries the counts and
 * the reasoning); the comments here record only what a reader of the code
 * needs. Keys come from `familyKey()`.
 */
const FAMILIES: Readonly<Record<string, Entry>> = {
  // ---- metric-compatible clones -----------------------------------------
  // Arial and Helvetica share advance widths, and Arimo clones Arial's.
  arial: ["Arimo", "metric-clone"],
  helvetica: ["Arimo", "metric-clone"],
  geneva: ["Arimo", "close"], // bitmap-era Helvetica stand-in
  microsoftsansserif: ["Arimo", "close"],
  // Arial Unicode MS carries Arial's Latin metrics; its non-Latin coverage
  // has no equivalent here and falls to whatever the browser has.
  arialunicode: ["Arimo", "metric-clone"],
  calibri: ["Carlito", "metric-clone"],
  cambria: ["Caladea", "metric-clone"],
  timesnewroman: ["Tinos", "metric-clone"],
  times: ["Tinos", "metric-clone"],
  georgia: ["Gelasio", "metric-clone"],
  couriernew: ["Cousine", "metric-clone"],
  courier: ["Cousine", "metric-clone"],
  andalemono: ["Cousine", "close"],
  comicsans: ["Comic Relief", "metric-clone", "cursive"],

  // ---- the big Apple faces ----------------------------------------------
  // No clone exists for Helvetica Neue's own widths. Inter is the closest
  // grotesque that also covers Thin/UltraLight/Light/Medium, which this
  // family uses in hundreds of documents; Arimo would give Helvetica widths
  // but only 400-700.
  helveticaneue: ["Inter", "close"],
  sfnstext: ["Inter", "close"],
  graphik: ["Inter", "close"],
  gillsans: ["Cabin", "close"],
  humanist521: ["Cabin", "close"], // Bitstream's Gill Sans
  stonesans: ["Cabin", "close"],
  lucidagrande: ["Source Sans 3", "close"],
  lucidasans: ["Source Sans 3", "close"],
  lucidasansunicode: ["Source Sans 3", "close"],
  skia: ["Source Sans 3", "close"],
  seravek: ["Source Sans 3", "close"],
  scalasans: ["Source Sans 3", "close"],
  myriad: ["Source Sans 3", "close"],
  adobeclean: ["Source Sans 3", "close"],
  freightsanslf: ["Source Sans 3", "close"],
  baskerville: ["Libre Baskerville", "close"],
  iowanoldstyle: ["Libre Baskerville", "close"],
  palatino: ["Vollkorn", "close"],
  bookantiqua: ["Vollkorn", "close"],
  trebuchet: ["Fira Sans", "close"],
  fruti: ["Open Sans", "close"], // Frutiger
  dincondensed: ["Barlow Condensed", "close"],
  dinalternate: ["Barlow Semi Condensed", "close"],
  segoecondensed: ["Barlow Semi Condensed", "close"],
  avenir: ["Nunito Sans", "close"],
  avenirnext: ["Nunito Sans", "close"],
  avenirnextcondensed: ["Encode Sans Semi Condensed", "close"],
  americantypewriter: ["Cutive", "close"],
  centurygothic: ["Poppins", "close"],
  allroundgothic: ["Poppins", "close"],
  blairmd: ["Poppins", "close"],
  mullermedium: ["Poppins", "close"],
  cochin: ["EB Garamond", "close"],
  garamond: ["EB Garamond", "close"],
  agaramond: ["EB Garamond", "close"],
  garamondpremr: ["EB Garamond", "close"],
  minion: ["EB Garamond", "close"],
  chalkboard: ["Comic Neue", "close", "cursive"],
  chalkboardse: ["Comic Neue", "close", "cursive"],
  lucidacasual: ["Comic Neue", "close", "cursive"],
  tahoma: ["Open Sans", "close"],
  verdana: ["Open Sans", "close"],
  copperplate: ["Cinzel", "close"],
  copperplategothic: ["Cinzel", "close"],
  perpetuatitling: ["Cinzel", "close"],
  engravers: ["Cinzel", "close"],
  capitalsregular: ["Cinzel", "close"],
  academyengravedletplain: ["Cinzel", "close"],
  herculanum: ["Cinzel", "close"],
  menlo: ["Roboto Mono", "close"],
  monaco: ["Roboto Mono", "close"],
  sfmono: ["Roboto Mono", "close"],
  hoeflertext: ["Crimson Pro", "close"],
  charcoalcy: ["Noto Sans", "close"], // Cyrillic coverage is the point
  futura: ["Jost", "close"],
  twcen: ["Jost", "close"],
  kabelitcby: ["Jost", "close"],
  berlinsansfb: ["Jost", "close"],
  chalkduster: ["Rock Salt", "close", "cursive"],
  squeakychalksound: ["Rock Salt", "close", "cursive"],
  impact: ["Anton", "close"],
  druk: ["Anton", "close"],
  phosphate: ["Anton", "close"],
  norwester: ["Oswald", "close"],
  consolas: ["Inconsolata", "close"],
  bradleyhand: ["Caveat", "close", "cursive"],
  lucidahandwriting: ["Caveat", "close", "cursive"],
  kbsticktoit: ["Caveat", "close", "cursive"],
  handwritingmutlu: ["Caveat", "close", "cursive"],
  santafeletplain: ["Caveat", "close", "cursive"],
  bodonisvtytwo: ["Bodoni Moda", "close"],
  bodonisvtytwoos: ["Bodoni Moda", "close"],
  didot: ["Playfair Display", "close"],
  constantia: ["Source Serif 4", "close"],
  publicotext: ["Source Serif 4", "close"],
  noteworthy: ["Patrick Hand", "close", "cursive"],
  lettersforlearners: ["Patrick Hand", "close", "cursive"],
  corbel: ["Lato", "close"],
  candara: ["Lato", "close"],
  markerfelt: ["Permanent Marker", "close", "cursive"],
  vivaldii: ["Tangerine", "close", "cursive"],
  arialrounded: ["Nunito", "close"],
  produkt: ["Zilla Slab", "close"],
  optima: ["Marcellus", "close"],
  proximanova: ["Figtree", "close"],
  rocgrotesk: ["Archivo", "close"],
  abadi: ["Archivo", "close"],
  foundersgrotesk: ["Archivo", "close"],
  foundersgrotesktext: ["Archivo", "close"],
  foundersgroteskcond: ["Archivo Narrow", "close"],
  arialnarrow: ["Archivo Narrow", "close"],
  canela: ["Cormorant", "close"],
  charter: ["Charis SIL", "close"],
  rockwell: ["Rokkitt", "close"],
  apple: ["Italianno", "close", "cursive"], // Apple-Chancery
  blackchancery: ["Italianno", "close", "cursive"],
  lucidacalligraphy: ["Italianno", "close", "cursive"],
  superclarendon: ["Bitter", "close"],
  chaparral: ["Bitter", "close"],
  bookmanoldstyle: ["Bitter", "close"],
  centuryschoolbook: ["PT Serif", "close"],
  lucidafax: ["PT Serif", "close"],
  swiftef: ["PT Serif", "close"],
  bellgothic: ["Libre Franklin", "close"],
  franklingothic: ["Libre Franklin", "close"],
  bigcaslon: ["Libre Caslon Text", "close"],
  californianfb: ["Sorts Mill Goudy", "close"],
  galatiasil: ["Gentium Plus", "close"],
  stixgeneral: ["STIX Two Text", "close"],
  cambriamath: ["STIX Two Math", "close"],
  favoritmono: ["IBM Plex Mono", "close"],
  juliamono: ["JetBrains Mono", "close"],
  museosans: ["Mulish", "close"],
  sonomascript: ["Dancing Script", "close", "cursive"],
  rageitalic: ["Dancing Script", "close", "cursive"],
  cursivestandard: ["Dancing Script", "close", "cursive"],
  scriptecole2: ["Dancing Script", "close", "cursive"],
  schoolhousecursiveb: ["Dancing Script", "close", "cursive"],
  thebrooklyn: ["Dancing Script", "close", "cursive"],
  katecelebration: ["Dancing Script", "close", "cursive"],
  quentin: ["Dancing Script", "close", "cursive"],
  snellroundhand: ["Great Vibes", "close", "cursive"],
  savoyeletplain: ["Great Vibes", "close", "cursive"],
  amazone: ["Great Vibes", "close", "cursive"],
  cooperblack: ["Alfa Slab One", "close"],

  // ---- the family IS a Google Fonts family ------------------------------
  opensans: ["Open Sans", "metric-clone"],
  poppins: ["Poppins", "metric-clone"],
  montserrat: ["Montserrat", "metric-clone"],
  inconsolata: ["Inconsolata", "metric-clone"],
  bebasneue: ["Bebas Neue", "metric-clone"],
  leaguegothic: ["League Gothic", "metric-clone"],
  jetbrainsmono: ["JetBrains Mono", "metric-clone"],
  robotomono: ["Roboto Mono", "metric-clone"],
  roboto: ["Roboto", "metric-clone"],
  mulish: ["Mulish", "metric-clone"],
  caveat: ["Caveat", "metric-clone", "cursive"],
  inter: ["Inter", "metric-clone"],
  raleway: ["Raleway", "metric-clone"],
  rubik: ["Rubik", "metric-clone"],
  quicksandbook: ["Quicksand", "metric-clone"],
  notosans: ["Noto Sans", "metric-clone"],
  carlito: ["Carlito", "metric-clone"],
  sourcecode: ["Source Code Pro", "metric-clone"],
  sourcesans: ["Source Sans 3", "metric-clone"], // Source Sans Pro, renamed
  ibmplexsans: ["IBM Plex Sans", "metric-clone"],
  ibmplexmono: ["IBM Plex Mono", "metric-clone"],
  anonymous: ["Anonymous Pro", "metric-clone"],
  firacode: ["Fira Code", "metric-clone"],
  eczar: ["Eczar", "metric-clone"],
  gudea: ["Gudea", "metric-clone"],
  juliussansone: ["Julius Sans One", "metric-clone"],
  marcellus: ["Marcellus", "metric-clone"],
  tajawal: ["Tajawal", "metric-clone"],
  encodesanscondensed: ["Encode Sans Condensed", "metric-clone"],
  encodesanssemicondensed: ["Encode Sans Semi Condensed", "metric-clone"],
  barlowsemicondensed: ["Barlow Semi Condensed", "metric-clone"],
  baloo: ["Eczar", "close"], // Baloo v1 was withdrawn; Baloo 2 is Devanagari-first
  gentium: ["Gentium Plus", "close"],
  germaniaone: ["Cinzel", "close"], // Germania One is blackletter-adjacent display

  // ---- non-Latin scripts: match the script, not the style ---------------
  hirakaku: ["Noto Sans JP", "close"],
  hiraginosans: ["Noto Sans JP", "close"],
  yugo: ["Noto Sans JP", "close"],
  ms: ["Noto Sans JP", "close"], // MS-PGothic
  osaka: ["Noto Sans JP", "close"],
  keifont: ["Noto Sans JP", "close"],
  hiramaru: ["Zen Maru Gothic", "close"],
  tsukuardgothic: ["Zen Maru Gothic", "close"],
  hiramin: ["Noto Serif JP", "close"],
  yumin: ["Noto Serif JP", "close"],
  ipamjmincho: ["Noto Serif JP", "close"],
  ipamjshiyoumincho: ["Noto Serif JP", "close"],
  stheitisc: ["Noto Sans SC", "close"],
  simsun: ["Noto Serif SC", "close"],
  stheititc: ["Noto Sans TC", "close"],
  ligothicmed: ["Noto Sans TC", "close"],
  applegothic: ["Noto Sans KR", "close"],
  jcheada: ["Noto Sans KR", "close"],
  adobearabic: ["Noto Naskh Arabic", "close"],
  getypo: ["Cairo", "close"],
  sathu: ["Noto Sans Thai", "close"],
  plantagenetcherokee: ["Noto Sans Cherokee", "close"],
  monotypegurmukhi: ["Noto Sans Gurmukhi", "close"],
  euphemiaucas: ["Noto Sans Canadian Aboriginal", "close"],

  // ---- nothing beats the browser default --------------------------------
  // Symbol and dingbat fonts: substituting a text face shows letters where
  // the document means symbols, which is worse than any default.
  symbol: [null, "generic", "serif"],
  symbolpi: [null, "generic", "serif"],
  euclidsymbol: [null, "generic", "serif"],
  applesymbols: [null, "generic", "serif"],
  wingdings: [null, "generic", "sans-serif"],
  wingdings2: [null, "generic", "sans-serif"],
  wingdings3: [null, "generic", "sans-serif"],
  webdings: [null, "generic", "sans-serif"],
  applecoloremoji: [null, "generic", "sans-serif"], // every browser has one
  // TeX / math faces: Computer Modern has no Google Fonts equivalent.
  cmuserif: [null, "generic", "serif"],
  cmutypewriter: [null, "generic", "monospace"],
  cmmi10: [null, "generic", "serif"],
  cmsy10: [null, "generic", "serif"],
  lmmono10: [null, "generic", "monospace"],
  apl385: [null, "generic", "monospace"],
  // Accessibility faces whose whole point is their own letterforms: a
  // substitute defeats them. Both are freely licensed and self-hostable.
  opendyslexic: [null, "generic", "sans-serif"],
  biancoenero: [null, "generic", "sans-serif"],
  // Monospace with no near neighbour on Google Fonts.
  iosevka: [null, "generic", "monospace"],
  // Unidentified or one-off display faces: a specific substitute would be a
  // guess, and a wrong guess is worse than the default.
  papyrus: [null, "generic", "fantasy"],
  zapfino: [null, "generic", "cursive"],
  trattatello: [null, "generic", "cursive"],
  amienne: [null, "generic", "cursive"],
  ringofkerry: [null, "generic", "fantasy"],
  luminari: [null, "generic", "fantasy"],
  jazzletplain: [null, "generic", "fantasy"],
  partyletplain: [null, "generic", "fantasy"],
  kggeronimoblocks: [null, "generic", "fantasy"],
  mindboggledemo: [null, "generic", "fantasy"],
  grungeface: [null, "generic", "fantasy"],
  abjectfailure: [null, "generic", "fantasy"],
  spongeboymebob: [null, "generic", "fantasy"],
  toscazero: [null, "generic", "fantasy"],
  goodtimes: [null, "generic", "fantasy"],
  denmark: [null, "generic", "fantasy"],
  pegasus: [null, "generic", "fantasy"],
  bluehighwaylinocut: [null, "generic", "fantasy"],
  berlinfashion: [null, "generic", "fantasy"],
  arnprior: [null, "generic", "fantasy"],
  mariah: [null, "generic", "fantasy"],
  hanzelextendednormal: [null, "generic", "fantasy"],
  universityroman: [null, "generic", "fantasy"],
  bauhaus93: [null, "generic", "fantasy"],
  hobo: [null, "generic", "fantasy"],
  titi: [null, "generic", "fantasy"],
  surfboard: [null, "generic", "fantasy"],
  watermelonfamily: [null, "generic", "fantasy"],
  // Unidentified text faces: the browser's own serif/sans is the safer bet.
  thamesnuderoman: [null, "generic", "serif"],
  adelonlight: [null, "generic", "serif"],
  creditvalley: [null, "generic", "serif"],
  calligraphic421: [null, "generic", "serif"],
  rtgaiaserifdemo: [null, "generic", "serif"],
  cmgsans: [null, "generic", "sans-serif"],
  boulder: [null, "generic", "sans-serif"],
  pythagoras: [null, "generic", "sans-serif"],
  univerzasans: [null, "generic", "sans-serif"],
  geo: [null, "generic", "sans-serif"],
};

/**
 * Style tokens stripped from the end of a PostScript name to get the family.
 * Only the technical/foundry tails: weight and slope live after the first
 * hyphen and are cut before this runs.
 */
const TECH_TAIL = /(PSMT|PSStd|PS|MT|Std|ProN|Pro|LTStd|LT|ITCTT|ITC|TT|BT|MS)$/;

/** PostScript names whose face is welded to the family name. */
const KEY_ALIASES: Readonly<Record<string, string>> = {
  arialroundedmtbold: "arialrounded",
  berlinsansfbdemi: "berlinsansfb",
  mulishroman: "mulish",
  mulishitalic: "mulish",
  caveatroman: "caveat",
  scalasanspro: "scalasans",
  biancoenerobold: "biancoenero",
  biancoeneroregular: "biancoenero",
  surfboardbold: "surfboard",
  inconsolataforpowerline: "inconsolata",
  timesnewromanps: "timesnewroman",
  canelatext: "canela",
  caneladeck: "canela",
  agaramondpro: "agaramond",
  garamondpremrpro: "garamondpremr",
};

/**
 * The family key a PostScript name belongs to: lower-cased, punctuation
 * removed, face and foundry tails stripped. ArialMT, Arial-BoldMT and
 * Arial-Black all key as `arial`; TimesNewRomanPSMT and
 * TimesNewRomanPS-BoldMT as `timesnewroman`; HiraKakuProN-W3, HiraKakuPro-W6
 * and HiraKakuStd-W8 as `hirakaku`.
 */
export function familyKey(psName: string): string {
  // "Inter-Regular_Black" and "Rubik-LightItalic_Medium-Italic" come from
  // web-font subsetters: the part before the underscore names the file.
  let base = psName.split("_")[0] ?? "";
  base = base.split("-")[0] ?? "";
  for (let i = 0; i < 3; i++) {
    const cut = base.replace(TECH_TAIL, "");
    if (cut === base || cut.length < 3) break;
    base = cut;
  }
  const key = base.toLowerCase().replace(/[^a-z0-9]/g, "");
  return KEY_ALIASES[key] ?? key;
}

/** Weight and slope a PostScript face name declares. */
export interface Face {
  /** CSS font-weight, 100-900. */
  readonly weight: number;
  readonly italic: boolean;
  /** CSS font-stretch percentage, or null when the face is normal width. */
  readonly stretch: number | null;
}

/**
 * Weight tokens, most specific first. `-Thin` is 100 and `-UltraLight` /
 * `-ExtraLight` 200, which is the CSS naming — note that Apple orders its
 * own families the other way (Helvetica Neue UltraLight is LIGHTER than
 * Thin), so the two are swapped for the Apple families that ship both.
 */
const WEIGHT_TOKENS: readonly (readonly [RegExp, number])[] = [
  [/^(ultra|extra)black$/, 900],
  [/^(black|heavy|ultra|ultrabold)$/, 900],
  [/^(extrabold|extrabld|ultrabld)$/, 800],
  [/^(semibold|demibold|demi|semibld)$/, 600],
  [/^(bold|bd|bld)$/, 700],
  [/^(medium|med|md)$/, 500],
  [/^(book|regular|reg|roman|normal|text)$/, 400],
  [/^(light|lt)$/, 300],
  [/^(extralight|ultralight|extralt)$/, 200],
  [/^(thin|hairline)$/, 100],
];

/** Families where Apple's UltraLight-below-Thin ordering applies. */
const APPLE_LIGHT_ORDER = new Set(["helveticaneue", "avenirnext", "avenir", "sfnstext"]);

/**
 * Names that carry their weight with no separator at all, so the suffix
 * parse finds nothing. Keyed by the whole PostScript name, lower-cased with
 * punctuation removed.
 */
const WELDED_WEIGHTS: Readonly<Record<string, number>> = {
  arialroundedmtbold: 700,
  berlinsansfbdemi: 600,
  biancoenerobold: 700,
  surfboardbold: 700,
  cooperblackms: 900,
};

const STRETCH_TOKENS: readonly (readonly [RegExp, number])[] = [
  [/^(ultracondensed)$/, 50],
  [/^(extracondensed)$/, 62.5],
  [/^(condensed|cond|cn|narrow)$/, 75],
  [/^(semicondensed|semicond)$/, 87.5],
  [/^(semiexpanded)$/, 112.5],
  [/^(expanded|wide)$/, 125],
];

/**
 * The face a PostScript name asks for. iWork stores the face, not a
 * weight — "AvenirNext-DemiBold" and "HiraKakuProN-W3" are the only weight
 * information many runs carry — so this is what decides which cut of the
 * fallback to request.
 */
export function parseFace(psName: string): Face {
  // A subsetter tail ("Inter-Regular_Black", "Rubik-LightItalic_Medium-Italic")
  // names the face the document actually uses after the last underscore, and
  // is all face — no family prefix to cut off.
  const welded = psName.includes("_");
  const tail = welded ? psName.slice(psName.lastIndexOf("_") + 1) : psName;
  const cut = tail.indexOf("-");
  const suffix = welded ? tail : cut >= 0 ? tail.slice(cut + 1) : "";
  // "BoldItalicMT" -> [bold, italic]; "Regular_Bold" already trimmed above;
  // "W3" and "300" are their own tokens.
  const tokens = suffix
    .replace(/(PSMT|PSStd|MT|PS|Std|Pro)$/, "")
    .split(/[-\s]+/)
    .flatMap((t) => t.split(/(?=[A-Z])/))
    .map((t) => t.toLowerCase())
    .filter(Boolean);

  let weight = 0;
  let italic = false;
  let stretch: number | null = null;
  const apple = APPLE_LIGHT_ORDER.has(familyKey(psName));

  // Multi-token weights ("semi"+"bold", "ultra"+"light") rejoin here.
  const joined: string[] = [];
  for (let i = 0; i < tokens.length; i++) {
    const t = tokens[i]!;
    const next = tokens[i + 1];
    if (next && /^(semi|demi|ultra|extra)$/.test(t)) {
      joined.push(t + next);
      i++;
    } else joined.push(t);
  }

  for (const t of joined) {
    if (/^(italic|it|oblique|obl)$/.test(t)) italic = true;
    else if (/^w[1-9]$/.test(t)) weight = Number(t[1]) * 100;
    else if (/^[1-9]00$/.test(t)) weight = Number(t);
    else {
      const s = STRETCH_TOKENS.find(([re]) => re.test(t));
      if (s) stretch = s[1];
      const w = WEIGHT_TOKENS.find(([re]) => re.test(t));
      if (w) {
        let n = w[1];
        if (apple && n === 100) n = 200;
        else if (apple && n === 200) n = 100;
        weight = n;
      }
    }
  }
  // Hiragino W3/W6 land on 300/600 through the W-token rule above.
  if (weight === 0) weight = WELDED_WEIGHTS[psName.toLowerCase().replace(/[^a-z0-9]/g, "")] ?? 0;
  return { weight: weight || 400, italic, stretch };
}

/** The substitute for a PostScript name, or null when the family is unknown. */
export function fallbackFor(psName: string): FontFallback | null {
  const entry = FAMILIES[familyKey(psName)];
  if (!entry) return null;
  const [family, kind, generic] = entry;
  if (family === null) return { family: null, kind, generic: generic!, weights: [], italic: false };
  const g = GOOGLE[family];
  return { family, kind, generic: generic ?? g.generic, weights: g.weights, italic: g.italic };
}

/** The closest weight a fallback actually ships to the one asked for. */
export function nearestWeight(weights: readonly number[], want: number): number {
  if (weights.length === 0) return want;
  let best = weights[0]!;
  for (const w of weights) {
    if (Math.abs(w - want) < Math.abs(best - want)) best = w;
  }
  return best;
}
