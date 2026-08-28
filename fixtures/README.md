# fixtures/

Real `.pages` / `.numbers` / `.key` documents harvested from one Common Crawl
vintage, for phase-1b (parser fixtures) and later phases (viewer screenshots,
cross-validation).

## Layout

| path | what |
| --- | --- |
| `crawl.jsonl` | selection log: one JSON per capture picked from the CC index (committed) |
| `success.tsv` | crosswalk of accepted files: `local_id, sha256, ext, format, origin_url, warc_url, bytes, evidence` (committed) |
| `ccrawl_gone.txt` | `local_id`s whose content was truncated in CC **and** no longer fetchable at the origin (committed) |
| `crawl/` | accepted modern iWork bundles as `<sha256>.<ext>` (gitignored binaries) |
| `crawl_old/` | legacy pre-iWork-13 bundles (`index.apxl`-era) — reference only, not viewer fixtures (gitignored binaries) |
| `provenance.json` | per-file provenance: sha256, format, origin URL, capture id (committed) |

## Where the files came from

Selection ran `scripts/fixtures_queryindex.py` against the **CC-MAIN-2026-34**
URL index (the latest vintage at the time). Selection is by **MIME** —
`application/x-iwork-*` in the index's `mime` / `mime-detected` fields — never
by URL extension. Each row of `crawl.jsonl` records the origin URL, the WARC
file + byte range that holds the capture, the declared/detected MIME, the CC
content digest, and a stable `local_id` of the form
`CC-MAIN-2026-34-cdx-NNNNN-<rownum>`.

Downloads ran `scripts/fixtures_downloadall.py`: each record is fetched from
Common Crawl with a byte-Range request, the HTTP payload is extracted and
checked against the CC digest, and — if CC truncated it — refetched from the
origin server. Files that survived are stored content-addressed by sha256.

## Classification

- ZIP whose members include `*.iwa` streams → modern iWork (iWork '13+).
  - Any `Slide-*.iwa` / `MasterSlide*.iwa` member → **keynote**.
  - Otherwise the first archive type in `Index/Document.iwa` decides:
    `10000` (TP.DocumentArchive) → **pages**, `1` (TN.DocumentArchive) →
    **numbers**. (Type id 1 is shared with KN.DocumentArchive; Keynote is
    already excluded by the slide-member check above.)
  - When neither marker is present, the index MIME decides and the
    `success.tsv` evidence column says `LOW CONFIDENCE`.
- ZIP containing `index.apxl` / `*.apxl.gz` → legacy pre-iWork-13, kept in
  `crawl_old/`, not used by the parser.
- Some captures are a zip inside a zip; the wrapper is unwrapped once.

## Numbers (smoke run)

See `provenance.json` and `success.tsv` for the authoritative list; counts are
summarized at the end of this file after each run.

## Refetch procedure

Everything is restartable and content-addressed:

```bash
# 1. re-select (or reuse the committed crawl.jsonl)
uv run scripts/fixtures_queryindex.py --vintage CC-MAIN-2026-34 --limit 3000

# 2. download what's missing; rerun freely, it skips done work
uv run scripts/fixtures_downloadall.py
```

`downloadall` derives state from disk: ids in `success.tsv` and
`ccrawl_gone.txt` are skipped, and files are named `<sha256>.<ext>` so
re-downloads are idempotent. If an origin 404s after CC served a truncated
copy, its `local_id` lands in `ccrawl_gone.txt` — that content is considered
lost. To recover a specific file, find its `origin_url` in `crawl.jsonl` /
`success.tsv` and try the origin (or the Wayback Machine) by hand.

For the full-scale EC2 run, see `scripts/fixtures_ec2_runbook.md`.
