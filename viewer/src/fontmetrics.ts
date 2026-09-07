// Line metrics of the fonts Keynote lays text out with, measured on macOS
// 26.6 with AppKit: NSLayoutManager.defaultLineHeightForFont (the pitch of
// single-spaced lines) and defaultBaselineOffsetForFont (the first baseline
// below the top of the text area) and NSFont.descender, all per em. Keynote's PDF export places
// text exactly there: RIPE 82's Helvetica boxes put the first baseline
// 0.970 em under the padded top and lines 1.2 em apart, where CoreText's
// ascent+descent+leading says 0.770 and 1.0. AppKit inflates the faces with
// no line gap whose ascent+descent is about 1 em (Helvetica, Times, Courier,
// Hoefler, Palatino) by 1.2 and puts the extra above the baseline; the other
// faces keep their hhea metrics. Verified against 37 exported decks (see
// docs/JUDGE.md, Keynote round 4). [inferred: export measurements; the
// numbers are AppKit's]
//
// Keyed by PostScript name, with a family entry (the name up to the first
// dash) for the cuts that share their family's numbers. A face this Mac does
// not have gets Helvetica's numbers: that is the face Keynote substitutes,
// and the export's spans of OpenSans, Rubik and ScalaSansPro decks are all
// drawn in Helvetica at the 0.970 / 1.2 positions.

export interface LineMetrics {
  /** single-spaced line pitch, em */
  height: number;
  /** first baseline below the top of the text area, em */
  baseline: number;
  /** descent below the baseline, em: a bottom-aligned block ends this far under its last baseline */
  descent: number;
  /** false when the face is not in the table (Helvetica's numbers apply) */
  known: boolean;
}

/** Keynote's substitute for a missing face: Helvetica. */
export const FALLBACK_METRICS: [number, number, number] = [1.2, 0.97, 0.23];

const BY_FAMILY: Record<string, [number, number, number]> = {
  "AmericanTypewriter": [1.154, 0.904, 0.25],
  "AndaleMono": [1.125, 0.907, 0.2178],
  "Apple": [1.583, 1.118, 0.4648],
  "AppleColorEmoji": [1.313, 1.0, 0.3125],
  "AppleSDGothicNeo": [1.2, 0.9, 0.3],
  "AppleSymbols": [1.0, 0.667, 0.25],
  "Arial": [1.15, 0.905, 0.2119],
  "ArialHebrew": [1.065, 0.73, 0.335],
  "ArialMT": [1.15, 0.905, 0.2119],
  "ArialNarrow": [1.148, 0.936, 0.2119],
  "ArialRoundedMTBold": [1.157, 0.946, 0.2109],
  "ArialUnicodeMS": [1.34, 1.069, 0.271],
  "Avenir": [1.366, 1.0, 0.366],
  "AvenirNext": [1.366, 1.0, 0.366],
  "AvenirNextCondensed": [1.366, 1.0, 0.366],
  "Baskerville": [1.144, 0.898, 0.2461],
  "BigCaslon": [1.209, 0.934, 0.257],
  "BodoniSvtyTwoITCTT": [1.202, 0.936, 0.266],
  "BradleyHandITCTT": [1.249, 0.85, 0.399],
  "Canela": [1.51, 1.2, 0.3],
  "CanelaDeck": [1.51, 1.2, 0.3],
  "CanelaText": [1.51, 1.2, 0.3],
  "Chalkboard": [1.276, 0.98, 0.2829],
  "ChalkboardSE": [1.427, 1.131, 0.2829],
  "Chalkduster": [1.276, 0.98, 0.2829],
  "Charter": [1.22, 0.98, 0.2402],
  "Cochin": [1.147, 0.897, 0.25],
  "ComicSansMS": [1.394, 1.102, 0.2915],
  "Copperplate": [1.03, 0.763, 0.248],
  "CourierNewPS": [1.133, 0.833, 0.3003],
  "CourierNewPSMT": [1.133, 0.833, 0.3003],
  "DINAlternate": [1.164, 0.938, 0.2261],
  "DINCondensed": [1.2, 0.712, 0.288],
  "Didot": [1.264, 0.941, 0.299],
  "Futura": [1.329, 1.039, 0.2598],
  "GeezaPro": [1.362, 0.895, 0.3309],
  "Geneva": [1.333, 1.0, 0.25],
  "Georgia": [1.136, 0.917, 0.2192],
  "GillSans": [1.193, 0.943, 0.25],
  "Graphik": [1.33, 1.1, 0.22],
  "Helvetica": [1.2, 0.97, 0.23],
  "HelveticaNeue": [1.221, 0.975, 0.217],
  "Herculanum": [1.0, 0.795, 0.205],
  "HiraMinProN": [1.5, 0.88, 0.12],
  "HiraginoSans": [1.5, 0.88, 0.12],
  "HoeflerText": [1.2, 0.921, 0.279],
  "Impact": [1.22, 1.009, 0.2109],
  "KohinoorDevanagari": [1.5, 1.05, 0.35],
  "Krungthep": [1.273, 1.011, 0.2625],
  "LucidaGrande": [1.178, 0.967, 0.2109],
  "Luminari": [1.339, 0.983, 0.356],
  "MarkerFelt": [1.145, 0.908, 0.237],
  "Menlo": [1.164, 0.928, 0.2358],
  "MicrosoftSansSerif": [1.132, 0.922, 0.21],
  "Monaco": [1.333, 1.0, 0.25],
  "MonotypeGurmukhi": [1.303, 0.864, 0.4395],
  "Noteworthy": [1.615, 1.28, 0.32],
  "Optima": [1.212, 0.919, 0.268],
  "Palatino": [1.32, 1.043, 0.2773],
  "Papyrus": [1.543, 0.94, 0.603],
  "PartyLetPlain": [1.4, 0.9, 0.5],
  "Phosphate": [1.25, 0.94, 0.26],
  "PingFangSC": [1.4, 1.06, 0.34],
  "Produkt": [1.31, 1.06, 0.24],
  "ProximaNova": [1.2, 0.99, 0.21],
  "Rockwell": [1.2, 0.679, 0.3208],
  "STHeitiSC": [1.03, 0.86, 0.14],
  "STHeitiTC": [1.03, 0.86, 0.14],
  "SignPainter": [0.968, 0.7, 0.2],
  "Skia": [1.2, 0.977, 0.2231],
  "SnellRoundhand": [1.261, 0.937, 0.324],
  "Symbol": [1.0, 0.701, 0.2988],
  "Tahoma": [1.207, 1.0, 0.2065],
  "Thonburi": [1.377, 1.082, 0.2281],
  "TimesNewRomanPS": [1.149, 0.891, 0.2163],
  "TimesNewRomanPSMT": [1.149, 0.891, 0.2163],
  "Trattatello": [1.812, 1.15, 0.662],
  "Trebuchet": [1.161, 0.939, 0.2222],
  "TrebuchetMS": [1.161, 0.939, 0.2222],
  "Verdana": [1.215, 1.005, 0.21],
  "Wingdings": [1.11, 0.899, 0.2109],
  "Wingdings2": [1.054, 0.843, 0.2109],
  "Wingdings3": [1.139, 0.928, 0.2109],
  "Zapfino": [3.378, 1.875, 1.5025],
};

const BY_NAME: Record<string, [number, number, number]> = {
  "AmericanTypewriter-Bold": [1.226, 0.948, 0.278],
  "AmericanTypewriter-Condensed": [1.131, 0.881, 0.25],
  "Arial-Black": [1.411, 1.101, 0.3096],
  "Baskerville-Bold": [1.151, 0.896, 0.2549],
  "Baskerville-Italic": [1.127, 0.881, 0.2461],
  "Baskerville-SemiBold": [1.143, 0.896, 0.2471],
  "Baskerville-SemiBoldItalic": [1.149, 0.903, 0.2461],
  "Cochin-Bold": [1.164, 0.914, 0.25],
  "Cochin-Italic": [1.12, 0.886, 0.234],
  "Copperplate-Bold": [1.035, 0.767, 0.248],
  "Copperplate-Light": [1.028, 0.76, 0.249],
  "Didot-Bold": [1.289, 0.969, 0.294],
  "Futura-Bold": [1.329, 1.039, 0.26],
  "Futura-CondensedMedium": [1.231, 0.983, 0.2188],
  "GillSans": [1.148, 0.918, 0.2305],
  "GillSans-Bold": [1.158, 0.923, 0.2349],
  "GillSans-BoldItalic": [1.156, 0.921, 0.2349],
  "GillSans-Italic": [1.139, 0.909, 0.23],
  "GillSans-Light": [1.136, 0.898, 0.2383],
  "GillSans-UltraBold": [1.246, 0.994, 0.252],
  "HelveticaNeue": [1.193, 0.952, 0.213],
  "HelveticaNeue-CondensedBlack": [1.227, 0.972, 0.227],
  "HelveticaNeue-CondensedBold": [1.21, 0.961, 0.221],
  "HelveticaNeue-Italic": [1.198, 0.957, 0.213],
  "HelveticaNeue-Light": [1.209, 0.967, 0.213],
  "HelveticaNeue-LightItalic": [1.192, 0.951, 0.213],
  "HelveticaNeue-Thin": [1.209, 0.967, 0.213],
  "HelveticaNeue-UltraLight": [1.171, 0.931, 0.213],
  "HoeflerText-Ornaments": [1.015, 0.807, 0.208],
  "MarkerFelt-Thin": [1.086, 0.868, 0.218],
  "Optima-Bold": [1.214, 0.921, 0.268],
  "Optima-Italic": [1.21, 0.923, 0.262],
};

export function lineMetrics(fontName: string | undefined): LineMetrics {
  const hit = (fontName && (BY_NAME[fontName] ?? BY_FAMILY[fontName.split("-")[0]])) || undefined;
  const t = hit ?? FALLBACK_METRICS;
  return { height: t[0], baseline: t[1], descent: t[2], known: !!hit };
}
