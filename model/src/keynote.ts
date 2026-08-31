/**
 * pnk JSON document models — Keynote (.key).
 *
 * Maps KN.DocumentArchive [1] → KN.ShowArchive [2] onto a resolved,
 * reference-free model: show → slides, with masters (theme templates)
 * resolved into the slides that follow them.
 *
 * Placeholder chain resolution: a slide's title/body/object/slide-number
 * placeholders are drawables; when a slide placeholder is empty, its geometry
 * and styling inherit from the master slide's placeholder of the same role —
 * the converter bakes the resolved values in and flags `placeholder.inherited`
 * (docs/model-design.md). Slide order comes from KN.SlideTreeArchive.slides
 * (the authoritative list; rootSlideNode is deprecated, slideList is newer).
 *
 * Builds/transitions are kept minimal and viewer-level: each drawable can
 * carry a `build` (via DrawableCommon.keynoteBuild) and each slide one
 * `transition`. Presenter notes resolve to StyledText.
 *
 * Format facts: docs/format/keynote.md (+ text.md, drawables.md).
 */

import type {
  DocumentEnvelope,
  Drawable,
  DrawableCommon,
  Fill,
  IsoDateString,
  MediaAsset,
  Size,
  StyledText,
} from "./shared";

// ---------------------------------------------------------------------------
// Masters (KN.ThemeArchive.templates — slides that act as page masters)
// ---------------------------------------------------------------------------

/**
 * A master (template) slide. Same content shape as Slide; slides link to it
 * by `masterName`. [proto: KN.ThemeArchive.templates; KN.SlideArchive.template_slide]
 */
export interface MasterSlide {
  name: string;
  drawables: Drawable[];
  notes?: StyledText;
  /** Master background fill [proto: KN.SlideStyleArchive.slide_properties.fill]. */
  background?: Fill;
}

// ---------------------------------------------------------------------------
// Builds & transitions (minimal, viewer-level)
// ---------------------------------------------------------------------------

/**
 * One build (animation) on a drawable.
 * [proto: KN.BuildArchive { drawable, delivery, attributes } + BuildChunks;
 *  delivery is a string in the proto, e.g. "build-in"/"build-out"/"action"]
 */
export interface BuildSpec {
  delivery: "in" | "out" | "action" | "other";
  /** Effect name as stored (e.g. "dissolve", "pop"). [proto: AnimationAttributesArchive.effect] */
  effect?: string;
  /** Animation type string when present (source grouping). */
  animationType?: string;
  durationSec?: number;
  delaySec?: number;
  automatic?: boolean;
  /** Easing. [proto: BuildAttributesAcceleration] */
  acceleration?: "none" | "ease-in" | "ease-out" | "ease-both" | "custom";
  /** Text-level staging. [proto: BuildAttributesTextDelivery] */
  textDelivery?: "by-object" | "by-word" | "by-character" | "by-line";
  /** Staged build chunks. [proto: KN.BuildChunkArchive delay/duration/automatic] */
  chunks?: { delaySec?: number; durationSec?: number; automatic?: boolean }[];
  motionBlur?: { amount: number };
  /** 0-based order within the slide's build sequence, when stored. */
  order?: number;
}

/**
 * Slide transition. [proto: KN.TransitionArchive → TransitionAttributesArchive
 * → AnimationAttributesArchive { animation_type, effect, duration, direction, delay }]
 * Effect/direction names are kept as stored strings — the effect vocabulary is
 * large and app-version dependent; the viewer matches prefixes it knows.
 */
export interface TransitionSpec {
  /** Effect name as stored (e.g. "Magic Move", "Dissolve"). */
  effect?: string;
  animationType?: string;
  durationSec?: number;
  delaySec?: number;
  automatic?: boolean;
  /** Direction as a stored enum number cast to string (app-defined meaning). */
  direction?: string;
  /** Accent color used by some effects. */
  color?: string;
}

// ---------------------------------------------------------------------------
// Slides (KN.SlideArchive)
// ---------------------------------------------------------------------------

export interface Slide {
  /** Slide name when set. [proto: KN.SlideArchive.name] */
  name?: string;
  /** Navigator "skip" flag. [proto: KN.SlideNodeArchive.isSkipped] */
  skipped?: boolean;
  /** Master this slide follows (by MasterSlide.name). */
  masterName?: string;
  /**
   * Resolved master underlay: the master's drawables that actually show
   * under this slide (furniture kept; placeholder prompts superseded or
   * emptied by the slide are filtered out), in paint order. A viewer paints
   * these before `drawables`, verbatim — it must never need to consult
   * `masters[]` to render correctly (docs/model-review.md §3b).
   */
  masterDrawables?: Drawable[];
  /** All drawables in paint order (z-order), placeholders included. */
  drawables: Drawable[];
  /** Presenter notes. [proto: KN.NoteArchive.containedStorage → TSWP.StorageArchive] */
  notes?: StyledText;
  /** The one slide transition. [proto: KN.SlideArchive.transition (required)] */
  transition?: TransitionSpec;
  /** Show slide number on this slide. [proto: KN.SlideNodeArchive.isSlideNumberVisible] */
  slideNumberVisible?: boolean;
  /**
   * Slide background fill, RESOLVED by the converter (slide value, else the
   * master chain's) [proto: KN.SlideStyleArchive.slide_properties.fill].
   * Absent = no effective fill — never "go look up the master"
   * (docs/model-review.md §3b).
   */
  background?: Fill;
}

// ---------------------------------------------------------------------------
// Show / document root
// ---------------------------------------------------------------------------

/**
 * The Keynote document model. Envelope fields (`meta`, `warnings`, `fonts`,
 * `media`) follow the shared DocumentEnvelope contract.
 */
export interface KeynoteDocument extends DocumentEnvelope {
  kind: "keynote";

  /** Slide size in points. [proto: KN.ShowArchive.size (required)] */
  slideSize: Size;

  /** Slides in presentation order (KN.SlideTreeArchive.slides). */
  slides: Slide[];

  /** Master/template slides from the theme, resolved. [proto: KN.ThemeArchive.templates] */
  masters: MasterSlide[];

  /** Theme identifier when present. [proto: TSS.ThemeArchive.theme_identifier] */
  themeName?: string;

  /** Playback settings. [proto: KN.ShowArchive mode/loop/autoplay fields] */
  playback?: {
    mode?: "normal" | "auto-play" | "hyperlinks-only";
    loop?: boolean;
    autoplayTransitionDelaySec?: number;
    autoplayBuildDelaySec?: number;
    slideNumbersVisible?: boolean;
  };

  /** Self-playing soundtrack. [proto: KN.Soundtrack] */
  soundtrack?: {
    /** Audio asset(s), in order. */
    tracks: MediaAsset[];
    repeat?: "none" | "one" | "all";
  };

  /** Audio narration recording attached to the show. [proto: KN.RecordingArchive] */
  recording?: { durationSec?: number };
}

// Extend DrawableCommon with Keynote-only hooks, declared here to
// keep shared.ts app-neutral.
declare module "./shared" {
  interface DrawableCommon {
    /** Keynote build/animation attached to this drawable (first in slide order). */
    keynoteBuild?: BuildSpec;
    /**
     * All builds on this drawable in slide order (build-in + build-out +
     * actions). Present only when there are two or more; a single build
     * lives in keynoteBuild alone.
     */
    keynoteBuilds?: BuildSpec[];
    /**
     * Placeholder identity: role from the converter plus the inherited flag
     * (master-derived geometry/style). [proto: KN.PlaceholderArchive.Kind]
     */
    placeholder?: { role: string; inherited?: boolean };
  }
}

// Re-export so converters importing "./keynote" get the whole surface.
export type { Drawable, DrawableCommon, IsoDateString };
