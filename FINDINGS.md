# Rust Code Review Findings

Date: 2026-08-30

Scope: `crates/iwadump`, `crates/pnk2json`, `crates/pnk2json-wasm`, their Rust
tests, and the immediate TypeScript sink for Rust-produced hyperlinks. This was
a read-only review of the implementation; no fixes are included here.

## Executive summary

The implementation has a strong format-research foundation and broad corpus
coverage, but it is not yet safe to expose as an untrusted-file browser parser.
The highest risks are availability failures: several tiny crafted inputs can
request multi-gigabyte allocations or exhaust the native/WASM stack. There are
also two core product-correctness blockers:

1. incremental-save patches are deliberately discarded, so conversion can
   return plausible but stale pre-edit content; and
2. Numbers JSON is not deterministic across processes because randomized
   `HashMap` iteration controls style-pool indices.

| Severity | Count | Release guidance |
| --- | ---: | --- |
| High | 7 | Block public release until resolved or explicitly rejected with a controlled error |
| Medium | 14 | Fix before treating conversion as faithful and hardened |
| Low | 5 | Address as part of stabilization and CI work |

Severity here is contextual: **High** means a practical browser/process crash,
silent material document corruption, or loss of reproducibility on ordinary
product inputs; **Medium** means a narrower security boundary violation,
incorrect modeled output, or misleading acceptance; **Low** means lifecycle,
CLI, diagnostics, documentation, or quality-gate debt.

## High severity

### H-1 — A five-byte Snappy payload can request a near-4 GiB allocation

[`decode_block`](crates/iwadump/src/snappy.rs#L14-L22) calls
`snap::raw::Decoder::decompress_vec` without checking the raw Snappy decoded
length. `IwaStream::parse` reaches it for every framed block
([`iwa.rs`](crates/iwadump/src/iwa.rs#L70-L93)). In locked `snap` 1.1.2,
`decompress_vec` allocates `vec![0; decompress_len(input)]` before validating
the compressed commands. A payload containing only the varint
`ff ff ff ff 0f` declares 4,294,967,295 decoded bytes.

**Impact:** a tiny dropped file can OOM/abort the native process or trap the
WASM instance and kill the viewer tab.

**Recommendation:** call `snap::raw::decompress_len` first and reject values
over a configurable per-block and cumulative document budget. Use checked
accounting and fallible reservation; test the maximum-length header in a child
process/WASM harness.

### H-2 — ZIP expansion and retained copies have no resource budget

Native extraction trusts entry sizes and inflates every `.iwa` with unbounded
`read_to_end` ([`container.rs`](crates/iwadump/src/container.rs#L134-L145),
[`container.rs`](crates/iwadump/src/container.rs#L203-L217)). Flat input is
also cloned before scanning ([`container.rs`](crates/iwadump/src/container.rs#L257-L280)).
The WASM path repeats unbounded inflation for IWA entries
([`loader.rs`](crates/pnk2json/src/loader.rs#L201-L246)), then walks the ZIP
again and eagerly inflates and retains **every non-IWA member**, including
unused `Data/` movies and arbitrary junk
([`ctx.rs`](crates/pnk2json/src/ctx.rs#L518-L535)). Per-asset existence checks
materialize and clone full files ([`members.rs`](crates/pnk2json/src/members.rs#L27-L51)).

The IWA representation further retains each block, a concatenated decoded
copy, payload copies, and diagnostic copies
([`iwa.rs`](crates/iwadump/src/iwa.rs#L91-L97),
[`envelope.rs`](crates/iwadump/src/envelope.rs#L119-L150),
[`dump.rs`](crates/iwadump/src/dump.rs#L56-L71)). The WASM ZIP reader also
reopens and reparses the central directory once per named IWA entry, making
many-entry archives algorithmically quadratic
([`loader.rs`](crates/pnk2json/src/loader.rs#L201-L246)).

**Impact:** ZIP bombs, many-entry archives, or legitimate media-heavy files can
exhaust browser/native memory and CPU before the parser can return an error.

**Recommendation:** define one shared `ResourceLimits`/budget object covering
input bytes, entry count/name length, nesting, per-entry and total expanded
bytes, compression ratio, IWA streams/blocks, decoded bytes, archives/messages,
and output bytes. Iterate each ZIP entry once by index, retain the original ZIP
for lazy media reads, and add non-materializing existence checks.

### H-3 — Attacker-controlled counts drive enormous allocations and work

Table `row_count` and `column_count` are taken directly from protobuf varints
([`tables.rs`](crates/pnk2json/src/tables.rs#L515-L526)) and used to allocate a
dense Cartesian grid
([`tables.rs`](crates/pnk2json/src/tables.rs#L788-L793)) plus count-sized
row/column arrays ([`tables.rs`](crates/pnk2json/src/tables.rs#L491-L512)). A
small record claiming 65,536 × 65,536 cells attempts billions of slots. Frame
loops also perform attacker-selected row/column work
([`tables.rs`](crates/pnk2json/src/tables.rs#L807-L840)), and format pooling is
linear-search based per cell ([`tables.rs`](crates/pnk2json/src/tables.rs#L842-L865)),
which becomes quadratic with many unique formats.

The plain-text dumper has a second scalar-to-allocation path: outline levels
accept almost any `u32` ([`styles.rs`](crates/pnk2json/src/styles.rs#L299-L302))
and `--text` calls `"#".repeat(level as usize)` without the clamp used by the
Markdown path ([`dumptext.rs`](crates/pnk2json/src/dumptext.rs#L323-L330)). One
paragraph can therefore request roughly 4 GiB.

**Impact:** controlled OOM, WASM trap, or excessive CPU from a small document.

**Recommendation:** validate dimensions against app/spec maxima and a checked
`rows * columns`/output budget before any allocation. Prefer a sparse table
representation for sparse inputs, hash-cons formats, clamp outline levels, and
bound all dump output.

### H-4 — Several recursive walks can overflow the stack

The generic protobuf group parser recursively calls `scan` with no depth limit
([`proto.rs`](crates/iwadump/src/proto.rs#L92-L134)). Nested reference wrappers
recursively parse and clone with no limit
([`pb.rs`](crates/pnk2json/src/pb.rs#L103-L115)). Native nested `Index.zip`
scanning is also unbounded ([`container.rs`](crates/iwadump/src/container.rs#L220-L241)).

Keynote navigator walks have neither depth nor cycle detection
([`keynote.rs`](crates/pnk2json/src/keynote.rs#L241-L271)); a self-reference or
two-node cycle never returns. Footnote-contained storage recursion is similarly
unguarded ([`text.rs`](crates/pnk2json/src/text.rs#L504-L511)). Diagnostic field
rendering adds another unbounded recursive walk
([`main.rs`](crates/iwadump/src/main.rs#L161-L212)).

**Impact:** compact nested tags or cyclic object graphs cause stack overflow;
Rust stack overflow commonly aborts rather than returning the promised layered
`Result` error.

**Recommendation:** use iterative walks where practical and enforce shared
depth/node/edge budgets plus visited sets everywhere. A protobuf depth near 100
and nested-ZIP depth of one are reasonable starting policies; validate against
real fixtures and add cycle/depth subprocess tests.

### H-5 — TIFF limits are deliberately disabled before full decode

[`tiff_to_png`](crates/pnk2json-wasm/src/lib.rs#L40-L63) calls
`reader.no_limits()` and fully decodes the image before downscaling. The comment
documents a real 375 MB decoded asset. `media_bytes` invokes this for any bytes
with TIFF magic and retains transcodes in an unbounded cache
([`lib.rs`](crates/pnk2json-wasm/src/lib.rs#L102-L128)).

**Impact:** crafted dimensions/compression or several large real TIFFs can kill
the WASM instance/tab; downscaling after allocation does not mitigate decode
memory.

**Recommendation:** inspect dimensions before decode, configure explicit image
allocation/dimension limits, and reject over-budget assets with a render
warning. If very large legitimate TIFFs must work, use bounded downsampling or
a decoder that does not allocate the full-resolution image. Bound cache bytes
and count, and avoid raw/output clones.

### H-6 — Incremental-save content patches are silently discarded

[`loader::load`](crates/pnk2json/src/loader.rs#L89-L94) drops every type-0
message whose archive has `should_merge`, retaining only the base object. The
repository's own verified format reference says these payloads replace a field
at `diff_field_path` and may remove fields
([`incremental.md`](docs/format/incremental.md#patch-messages-type--0)).

A review scan of the first 150 modern corpus fixtures found 233 patches in 35
files, including 71 in one Keynote file. The current conformance harness checks
exit status and non-empty output, so stale text, style, geometry, or references
still receive an `ok` verdict.

**Impact:** the viewer can cleanly show plausible pre-edit content rather than
the saved document, with no warning.

**Recommendation:** implement the documented one-field patch merge and
`fields_to_remove`, with hermetic base+patch tests. Until it is supported,
reject affected documents or emit a prominent document-degraded warning; do
not silently claim successful conversion.

### H-7 — Numbers JSON is nondeterministic across processes

After cell conversion, `edge_overrides: HashMap` is drained in randomized order
and synthesized styles are interned in that order
([`tables.rs`](crates/pnk2json/src/tables.rs#L882-L900)). Those indices are
observable as `cellStyleIndex` and determine `cellStyles` pool order.

This was reproduced by converting the same Numbers fixture in ten separate
processes: all ten outputs had different SHA-256 hashes; diffs were confined to
style-pool/index ordering.

**Impact:** byte output is not reproducible, cache keys and golden artifacts
are unstable, and semantically equivalent documents generate noisy diffs.

**Recommendation:** collect and sort overrides by `(row, column)` before
interning, or use an ordered map. Add a child-process test that converts the
same fixture repeatedly and requires identical bytes.

## Medium severity

### M-1 — Native package reads can escape the package root

Directory-backed `Container::read_member` performs
`fs::read(root.join(name))` without component validation or containment checks
([`container.rs`](crates/iwadump/src/container.rs#L380-L406)). An absolute path
replaces the root; `..` traverses it; symlinks are followed. Document-derived
`DataInfo.file_name` reaches this API through
[`members.rs`](crates/pnk2json/src/members.rs#L37-L47).

**Impact:** the public API can disclose arbitrary readable local files. Current
conversion mostly turns this into an existence oracle and a possible special-
file DoS, but downstream library callers can receive the bytes.

**Recommendation:** accept only normalized relative member names, reject root,
prefix, parent and current-directory components, canonicalize and require root
containment, require regular files, and define a no-symlink policy.

### M-2 — Known-app registry lookup can return another app's type

After checking the selected app and Common tables, `Registry::name_for` still
searches all app tables and accepts a single cross-app match
([`registry.rs`](crates/iwadump/src/registry.rs#L74-L106)). For example, id 195
is only present in the Keynote table, so `name_for(App::Pages, 195)` returns a
`KN.*` command name instead of unknown.

**Impact:** new/unknown Pages or Numbers objects may be mislabeled and then
dropped or dispatched using a foreign schema, violating the "never guess"
invariant.

**Recommendation:** for a known app, consult only that app and Common. Reserve
cross-app unambiguous inference, if retained, for `App::Unknown`; add a concrete
id-195 regression test.

### M-3 — Malformed protobuf values are accepted and silently narrowed

The varint reader accepts an over-wide tenth byte
([`proto.rs`](crates/iwadump/src/proto.rs#L52-L70)); tags and lengths are cast
without enforcing protobuf/u32/`usize` ranges
([`proto.rs`](crates/iwadump/src/proto.rs#L92-L123)); envelope lengths, type ids,
and versions are likewise narrowed with `as`
([`envelope.rs`](crates/iwadump/src/envelope.rs#L89-L108),
[`envelope.rs`](crates/iwadump/src/envelope.rs#L165-L197)). Required
`MessageInfo.type` and `length` presence/wire types are not enforced.

**Impact:** values such as type `2^32 + 195` can become known id 195; lengths
can wrap on wasm32; empty or wrong-wire headers are accepted as zero-valued
messages instead of corruption.

**Recommendation:** use checked `try_from` conversions, enforce the protobuf
maximum field number and tenth-byte rule, require proto2-required fields with
the correct wire types, and use a dedicated bounded prefix reader.

### M-4 — Missing/invalid document roots and container variants fail open

`convert_ctx` substitutes `Msg::default()` when object 1 is absent
([`lib.rs`](crates/pnk2json/src/lib.rs#L89-L100)), while unknown app detection
defaults to Pages ([`ctx.rs`](crates/pnk2json/src/ctx.rs#L134-L144),
[`ctx.rs`](crates/pnk2json/src/ctx.rs#L537-L543)). Empty `.iwa` members also
parse successfully. Any `.iwa` beginning with `bvx` is skipped as operation
storage regardless of its name
([`container.rs`](crates/iwadump/src/container.rs#L203-L217)).

The duplicate WASM container implementation does not re-run encrypted/legacy
checks inside nested `Index.zip`, silently drops failed member reads, and only
handles one inner level ([`loader.rs`](crates/pnk2json/src/loader.rs#L152-L266)).
Native opening accepts case-insensitive `INDEX.ZIP`, but later inner-member
lookup searches case-sensitively
([`container.rs`](crates/iwadump/src/container.rs#L389-L399)).

Native member discovery has a separate corrupt-ZIP bug: `filter_map` drops an
unreadable entry and its original index
([`container.rs`](crates/iwadump/src/container.rs#L159-L168)), then later
enumerates the shortened member vector and uses that new index to read the ZIP
([`container.rs`](crates/iwadump/src/container.rs#L203-L207)). A bad entry can
therefore shift a valid `.iwa` onto the wrong body, and an unreadable encryption
or legacy marker can disappear from detection.

**Impact:** corrupt, encrypted nested, or arbitrary ZIP input can be accepted as
an empty/partial Pages document or misclassified instead of cleanly rejected.

**Recommendation:** share one container implementation between native and
WASM; require a nonempty, recognized object-1 root; propagate member errors;
restrict operation-storage skipping to the expected basename and full magic;
and test every physical container variant through both APIs.

### M-5 — Numbers sheet style and form decoding contradict the extracted schema

Sheet style code reads fields 1/2 as direct colors
([`numbers.rs`](crates/pnk2json/src/numbers.rs#L149-L163)), but the local proto
defines `super = 1`, `override_count = 2`, and `sheet_properties = 3`, whose
fill/tab color are `TSD.FillArchive`
[proto: `.scratch/otorp/Numbers/TNArchives.proto:156-166`]. A real reviewed
fixture emitted `{}` for all three sheet styles despite field-3 properties.

Form-based sheets treat the required inline `TN.SheetArchive.super = 1` as a
`TSP.Reference`, then try to read a numeric-word `CFUUIDArchive` as a string
([`numbers.rs`](crates/pnk2json/src/numbers.rs#L21-L35)) [proto: the same
extracted `TNArchives.proto` and `TSPMessages.proto` schemas].

**Impact:** sheet fills/tab colors disappear and form bindings do not resolve.

**Recommendation:** decode field-3 fill properties with inheritance; read the
inline super message; normalize UUID words and map them to table UID/name;
warn on unresolved bindings. Add proto-shaped unit tests and the planned G3
Numbers golden.

### M-6 — Styled empty cells and partial border inheritance are lost

The v5 decoder returns `None` for type 0/1 before resolving style/formula
metadata ([`tables.rs`](crates/pnk2json/src/tables.rs#L1194-L1245)), although
the model supports valueless styled cells and older decoders preserve them.
Cell border inheritance is all-or-nothing: once any child side exists, ancestor
sides are skipped ([`styles.rs`](crates/pnk2json/src/styles.rs#L635-L645)).

**Impact:** empty cells that carry visible fills/rules disappear, and a child
overriding one edge loses inherited edges on the other sides.

**Recommendation:** construct `Empty` first and discard only when style,
formula, and other metadata are all absent. Resolve top/right/bottom/left
independently along the parent chain.

### M-7 — Smart-field ranges expand and styled remainder text loses styling

Text boundaries include character styles, attachments, and footnotes but omit
smart-field offsets ([`text.rs`](crates/pnk2json/src/text.rs#L182-L211)). A
smart field found anywhere in a larger segment is then applied to the whole
segment ([`text.rs`](crates/pnk2json/src/text.rs#L318-L355)), expanding a link or
date into surrounding text. Remainders after attachment/footnote splitting are
pushed as `Plain`, losing the already-computed style and hyperlink
([`text.rs`](crates/pnk2json/src/text.rs#L255-L303)).

**Impact:** links/date fields cover the wrong text and formatting disappears
after inline objects or footnotes.

**Recommendation:** include smart-field starts/ends in the UTF-16 boundary set
and retain the resolved run style/hyperlink for all split fragments. Add
mid-run, surrogate-pair, attachment-remainder, and footnote-remainder tests.

### M-8 — Keynote placeholder and build semantics lose information

Placeholder inheritance is skipped entirely when the child has text and, for
empty text, replaces the whole child `common` block with the master's
([`keynote.rs`](crates/pnk2json/src/keynote.rs#L571-L599)). Inheritance should be
per property. Build processing retains only the first build for a drawable and
silently ignores later build-out/action specs
([`keynote.rs`](crates/pnk2json/src/keynote.rs#L460-L529)).

**Impact:** explicit child geometry/style can be overwritten, missing master
properties do not inherit, and valid multi-build animations disappear.

**Recommendation:** merge placeholder properties field by field. Change the
model to `builds[]`, or explicitly warn that later builds were degraded. Add
the planned G4 semantic Keynote golden.

### M-9 — Warning aggregation merges unrelated identities

Warnings are grouped only by `(code, message-with-all-digit-runs-replaced)` and
exclude stable `detail` ([`lib.rs`](crates/pnk2json/src/lib.rs#L37-L84)). Distinct
unknown type ids, media ids, coordinates, or counts can collapse into the first
row and retain only its detail.

**Impact:** diagnostics no longer identify all affected object/media types and
the aggregate count may count warning rows rather than underlying objects.

**Recommendation:** normalize only explicitly coordinate-shaped warnings, or
include stable category/detail in the key and sum actual counts.

### M-10 — Remote-only media can be selected even though the viewer is offline

`data_available` treats `remote_url` as available
([`ctx.rs`](crates/pnk2json/src/ctx.rs#L388-L404)), but `MediaRef`/`MediaAsset`
does not expose that URL and `media_bytes` can only return packaged bytes. This
can choose an unrenderable original over a packaged thumbnail without warning.

**Impact:** offline images disappear despite a usable local fallback.

**Recommendation:** for this explicitly offline viewer, treat remote-only data
as unavailable and select a materialized alternative, or model it explicitly
as `remote-unavailable` without ever fetching it.

### M-11 — Rounded ISO milliseconds can produce an invalid timestamp

The timestamp formatter rounds fractional seconds independently and can emit
milliseconds `1000` without carrying into the next second
([`colors.rs`](crates/pnk2json/src/colors.rs#L82-L95)).

**Impact:** values near a second boundary can serialize as `...59.1000Z`.

**Recommendation:** round the total timestamp before decomposition, or carry
1,000 ms into seconds/minutes/date; add boundary tests including negative Apple
epoch values.

### M-12 — Rust hyperlink validation admits active and non-web schemes

`valid_url` accepts any printable string containing `://`
([`text.rs`](crates/pnk2json/src/text.rs#L411-L417)), including `javascript:`,
`file:`, and custom schemes. The viewer assigns the value directly to an anchor
`href` ([`viewer/src/text.ts`](viewer/src/text.ts#L390-L397)); no CSP is declared
in [`viewer/index.html`](viewer/index.html).

**Impact:** a malicious local document can create a user-clicked active URL or
unexpected navigation/protocol launch, crossing the untrusted-document
boundary.

**Recommendation:** parse and allowlist `https` (optionally `http`, `mailto`,
and same-document fragments) in Rust, repeat the policy in the viewer, and add
a restrictive CSP as defense in depth.

### M-13 — Generated Markdown is unsafe and sometimes structurally wrong

Most paragraph/run text is appended without escaping Markdown or raw HTML
([`dumptext.rs`](crates/pnk2json/src/dumptext.rs#L59-L106)). A document can
therefore emit raw HTML such as `<script>` into a `.md` consumer. Table dumping
uses only row 0 as the header but starts body output at `header_row_count`, so
header rows 1..N disappear; zero header rows also produce invalid structure
([`dumptext.rs`](crates/pnk2json/src/dumptext.rs#L426-L475)).

**Impact:** downstream Markdown renderers may execute/surface document-supplied
HTML, and valid tables lose rows.

**Recommendation:** document output as untrusted and sanitize at consumption,
or escape raw HTML/Markdown by default with an explicit unsafe/raw mode. Emit
every table row and use a valid delimiter policy for zero/multiple headers.

### M-14 — Inspector output and selection flags do not provide safe bounds

`--list` inflates every IWA, `--archive` decodes every stream before selecting,
and `--limit` builds statuses and clones every message before truncating
([`main.rs`](crates/iwadump/src/main.rs#L63-L112),
[`dump.rs`](crates/iwadump/src/dump.rs#L56-L71),
[`dump.rs`](crates/iwadump/src/dump.rs#L162-L164)). Human output interpolates
untrusted ZIP names without escaping terminal controls
([`main.rs`](crates/iwadump/src/main.rs#L63-L79)). JSON emits parsed `u64` object
ids as numbers ([`dump.rs`](crates/iwadump/src/dump.rs#L217-L230)), losing
precision in JavaScript above 2^53.

**Impact:** diagnostic modes are still vulnerable to full-document work;
hostile names can forge terminal lines/OSC effects; large IDs cannot round-trip.

**Recommendation:** make list metadata-only, select before decode, apply limits
lazily, quote terminal controls, and emit IDs as decimal strings.

## Low severity

### L-1 — Failed conversions retain the previous document and TIFF cache

The WASM globals are replaced/cleared only after a successful conversion
([`lib.rs`](crates/pnk2json-wasm/src/lib.rs#L16-L27),
[`lib.rs`](crates/pnk2json-wasm/src/lib.rs#L66-L95)). Opening a corrupt/encrypted
next document leaves prior media queryable via `media_bytes` and cached PNGs in
memory.

**Recommendation:** clear state at conversion start and export an explicit
`dispose`/`clear_document`; bind media requests to a conversion token.

### L-2 — CLI flag contracts are ambiguous or unenforced

`--archive` is documented as a unique suffix but returns all matches; `--json`
with `--message`/`--list` silently emits text due to precedence; archive JSON
newline behavior differs ([`main.rs`](crates/iwadump/src/main.rs#L84-L118)).

**Recommendation:** use clap conflicts/groups, reject ambiguous suffixes, make
output-mode precedence explicit, and add CLI integration tests.

### L-3 — Legacy and nested-container behavior is inconsistent

`--legacy-ok` does not downgrade package directories or `*-tef` paths because
those paths return before the flag is applied
([`container.rs`](crates/iwadump/src/container.rs#L248-L307)). The accepted
case-insensitive nested `Index.zip` name is not retained for later lookup.

**Recommendation:** route every legacy signal through one policy, scope the
flag to list mode if that is the intent, and retain the exact selected nested
member identity.

### L-4 — Some degradation is silent and documentation has drifted

Drawable cycle/depth fallback returns `Unknown` without adding the promised
warning ([`drawables.rs`](crates/pnk2json/src/drawables.rs#L12-L43)). Missing
media bytes warn only when `materialized_length` is present and positive
([`ctx.rs`](crates/pnk2json/src/ctx.rs#L410-L437)). `docs/model-design.md`
contains stale/contradictory geometry-angle and group-coordinate statements
relative to the format docs and code.

**Recommendation:** warn on every material degradation, make missing-byte
checks independent of optional length metadata, and reconcile the model docs
using the repository's provenance tags.

### L-5 — Test and quality gates are incomplete and currently red

`cargo test --workspace --all-targets` passes all 37 actual tests, and the
workspace checks for `wasm32-unknown-unknown`. However:

- `cargo fmt --all -- --check` fails with broad formatting drift;
- strict workspace Clippy fails, while ordinary compilation emits 12
  `pnk2json` warnings;
- the WASM crate has no tests;
- no fuzz/property/adversarial resource-limit harness exists;
- semantic goldens cover only two Pages documents; planned Numbers G3 and
  Keynote G4 goldens are absent;
- many fixture-backed tests silently return success when fixtures are missing
  ([`golden.rs`](crates/pnk2json/tests/golden.rs#L63-L101)); and
- the 1,248-file conformance harness primarily checks exit code, timeout/panic,
  and nonempty output ([`conformance.py`](scripts/conformance.py#L42-L90)), not
  fidelity or determinism.

**Recommendation:** make formatting/lint/native+WASM checks required in CI;
make required fixtures fail closed in CI; add `cargo audit`/`cargo deny`; and
add fuzz targets plus the focused regressions listed below.

## Prioritized remediation plan

### P0 — Make untrusted parsing bounded

1. Introduce and thread a shared `ResourceLimits` through ZIP, Snappy, IWA,
   envelope/protobuf, graph conversion, media decode, and dump output.
2. Preflight Snappy decoded size; enforce ZIP expanded-byte/ratio/count limits;
   keep media lazy.
3. Validate table dimensions/products, outline levels, image dimensions, and
   total cache/output sizes before allocation.
4. Replace or bound every recursive walk and add cycle detection.
5. Run conversion in a Web Worker so a controlled parser failure cannot freeze
   the UI; resource budgets remain mandatory because worker OOM can still kill
   the page/process.

### P0 — Restore correctness and reproducibility

1. Apply incremental patches, or reject/warn rather than returning stale
   content.
2. Sort `edge_overrides` before style interning and add repeated-process
   determinism tests.
3. Validate the root object/app instead of fabricating empty Pages documents.

### P1 — Unify and harden boundaries

1. Use one native/WASM container scanner with identical encryption, legacy,
   nested-ZIP, duplicate-name, and error semantics.
2. Contain package filesystem reads and strictly allowlist link schemes.
3. Escape terminal/Markdown output and represent cross-JS IDs as strings.
4. Fix the verified Numbers, text, Keynote, warning, media, and timestamp
   fidelity issues above.

### P1 — Add adversarial and semantic tests

Minimum regression set:

- maximum Snappy decoded-length header and high-ratio ZIP member;
- ZIP entry/count/nesting/duplicate-name budgets;
- maximum table dimensions and huge outline level return controlled errors;
- deeply nested protobuf groups, cyclic slide nodes, cyclic footnotes, and
  nested references;
- oversized TIFF rejection and cache budget;
- base+patch archive produces final merged content;
- same fixture in repeated child processes produces byte-identical JSON;
- known-app registry isolation and malformed/over-wide varints;
- package `..`, absolute path, and symlink containment;
- missing/wrong root and encrypted nested container rejection;
- Numbers sheet style/form/styled-empty/partial-border cases;
- smart fields starting mid-run and styled text after attachments;
- multi-build and partial placeholder inheritance;
- Numbers G3 and Keynote G4 Apple-validated semantic goldens.

### P2 — Reduce memory and maintenance cost

1. Avoid simultaneous raw/block/decoded/payload clones; parse borrowed slices
   or stream where ownership permits.
2. Box the very large `Drawable`/`ParagraphItem` enum variants flagged by
   Clippy to reduce stack moves and container size.
3. Make inspector modes lazy and cheap.
4. Clean compiler/Clippy warnings, run rustfmt, and document supported limits
   and degradation behavior.

## Strengths to preserve

- The 00 + u24 little-endian IWA framing, multi-block handling, strict Snappy
  errors, and declared-payload synchronization are well structured and have
  focused tests.
- Bounds checks before slices and table cell placement avoid ordinary
  out-of-bounds panics.
- UTF-16-aware text offset mapping is careful
  ([`text.rs`](crates/pnk2json/src/text.rs#L27-L46)).
- Drawable groups and style parent chains already use cycle/depth protection
  ([`drawables.rs`](crates/pnk2json/src/drawables.rs#L12-L33),
  [`styles.rs`](crates/pnk2json/src/styles.rs#L35-L60)); those patterns should be
  reused elsewhere.
- Unknown/undecodable payload boundaries remain synchronized and are surfaced
  as warnings instead of guessed schemas.
- Fonts and media identifiers are sorted, and JS-facing pnk model IDs are
  decimal strings.
- The v3/v4/v5 table decoding work and 1,248-file corpus harness provide
  valuable breadth, even though they need semantic and adversarial complements.
- No Rust-side network client or upload behavior was found; `remote_url` is
  modeled only. No production input-driven `unsafe` block or obvious
  `unwrap`/`expect` panic was found in the parser path.

## Verification performed

| Check | Result |
| --- | --- |
| `cargo test --workspace --all-targets` | Pass; 37 actual tests |
| `cargo check --workspace --target wasm32-unknown-unknown` | Pass; 12 `pnk2json` compiler warnings |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Fail |
| `cargo fmt --all -- --check` | Fail |
| Repeated Numbers conversion (10 child processes) | Fail determinism; 10 distinct hashes |
| Fuzz/property/resource-limit tests | None found |
| Rust network/upload behavior | None found |
| Automated dependency audit | Not available locally (`cargo-audit`/`cargo-deny` absent) |

Third-party implementation source inspected during this review, recorded per
the repository provenance policy: `BurntSushi/rust-snappy` (`snap` 1.1.2), git
commit `29fcab53647bcd8cd1550c06bde0cd42777e5565`, BSD-3-Clause. The locked crate
checksum is recorded in `Cargo.lock`.

---

# Remediation status (post-yard1 branch, 2026-08-30)

Worked by Claude on the local `post-yard1` branch (repo deliberately has no
remote until voting closes). Every fix gated on: full workspace test suite,
964-file corpus conformance at baseline (2480 ok + 8 encrypted rejects),
Playwright 6/6 + strict tsc, and — where output changed — corpus A/B hashing
plus eyeballed visual composites against Apple's embedded previews.

| Finding | Status | Commit(s) / notes |
| --- | --- | --- |
| H-1 snappy bomb | **Fixed** | decompress_len preflight, 64 MiB/block + 1 GiB/stream caps, adversarial tests |
| H-2 zip budgets | **Fixed** | entry cap, expansion budget, lazy wasm members, bounded member reads, single-pass scan |
| H-3 table dims | **Fixed** | app-maxima + 8M-cell budget with TableDegraded warning; O(1) format pooling; outline clamp |
| H-4 recursion | **Fixed** | depth/cycle guards on proto groups, references, navigator, text storages, nested zips, field renderer |
| H-5 TIFF limits | **Fixed** | explicit 50k/1 GiB decode limits (375 MB real asset still works), 256 MiB cache cap, state cleared at convert start |
| H-6 patches dropped | **Fixed** | wire-level merge, both shapes; 1,806/1,808 corpus patches apply, 2 multi-segment holdouts warn |
| H-7 nondeterminism | **Fixed** | sorted edge-override drain; convert-twice byte test; 25 fixtures x 5 runs verified |
| M-1 path escape | **Fixed** | component sanitizing + canonicalized containment + regular-file check |
| M-2 registry leak | **Fixed** | app-prefixed names never cross apps; exposed a real misdetection (see detect_app) |
| M-3 narrowing | **Fixed** | varint 10th-byte rule, field-number cap, required type/length, checked u32 |
| M-4 fail-open | **Mostly fixed** | root required, stable member indices, nested marker re-checks, case-consistent lookup. Deferred: unifying the native/wasm container implementations |
| M-5 sheet style/forms | **Fixed** | proto-correct sheet_properties decode w/ inheritance; inline form super; canonical UUIDs |
| M-6 styled empty cells | **Fixed** | metadata resolved before dropping; per-side border inheritance; verified vs Apple preview |
| M-7 smart fields | **Fixed** | span boundaries + entry-at semantics; styled remainders fall through |
| M-8 keynote | **Fixed** | keynoteBuilds carries all builds (additive model change); per-property placeholder merge |
| M-9 warning identity | **Fixed** | identity-bearing codes key on detail |
| M-10 remote media | **Fixed** | offline = remote unavailable; materialized alternatives win |
| M-11 timestamps | **Fixed** | total-milliseconds rounding with carry |
| M-12 link schemes | **Fixed** | https/http/mailto/# allowlist both sides + CSP |
| M-13 markdown | **Fixed** | & and < neutralized; all table rows emitted; zero-header tables valid |
| M-14 inspector bounds | **Deferred** | iwadump is a dev-facing CLI, not the browser boundary; H-1..H-4 caps still bound its work |
| L-1 stale wasm state | **Fixed** | with H-5 |
| L-2 CLI contracts | **Deferred** | dev-facing CLI polish |
| L-3 legacy routing | **Partial** | case-consistent nested lookup landed; --legacy-ok routing untouched |
| L-4 silent degradation | **Partial** | media warnings fixed; docs/model-design.md reconciliation pending |
| L-5 quality gates | **Partial** | zero compiler warnings, rustfmt clean, clippy clean; fuzz harness, CI wiring, cargo audit/deny and G3/G4 goldens pending |

Also fixed along the way (found while verifying M-2): `detect_app` ranked the
theme signal above `Tables/`, so two xlsx-derived Numbers documents rendered
as one bogus Keynote slide; and slide detection required a `Slide-` dash that
real decks (`Slide7.iwa`, `MasterSlide.iwa`) don't have.

Deferred with rationale, in priority order for a future pass:
1. G3 (Numbers) / G4 (Keynote) Apple-validated goldens — need Peter to build
   fixtures per checklist (fixtures/golden/G6-*-checklist.md exist).
2. Native/wasm container unification (M-4 residual).
3. Fuzz targets + CI wiring + cargo audit/deny (L-5 residual).
4. Multi-segment diff_field_path patch descent (2 corpus occurrences, warned).
5. Boxing the large Drawable/ParagraphItem enum variants (P2).
