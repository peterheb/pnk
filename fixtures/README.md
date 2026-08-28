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
| `crawl_old/` | legacy pre-iWork-13 bundles (`index.apxl`-era) as `<sha256>.unknown` — reference only, not viewer fixtures (gitignored binaries) |
| `provenance.json` | per-file provenance: sha256, format, origin URL, capture id (committed) |

## Where the files came from

Selection ran `scripts/fixtures_queryindex.py` against the **CC-MAIN-2026-34**
URL index (the latest vintage at the time), scanning all 300 index parts
(sharded on EC2, 4 concurrent processes over HTTPS/CloudFront — anonymous S3
access to the CC bucket is denied). Selection is by **MIME** —
the exact strings `application/x-iwork-*` and the legacy
`application/vnd.apple.{keynote,pages,numbers}` in the index's `mime` /
`mime-detected` fields, never by URL extension. Each row of `crawl.jsonl`
records the origin URL, the WARC file + byte range that holds the capture, the
declared/detected MIME, the CC content digest, and a stable `local_id` of the
form `CC-MAIN-2026-34-cdx-NNNNN-<rownum>`. Selections are deduplicated by CC
content digest.

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
- ZIP containing `index.apxl` / `*.apxl.gz` / `index.xml` (Pages '08/'09 XML
  era) → legacy pre-iWork-13, kept in `crawl_old/`, not used by the parser.
- Some captures are a zip inside a zip; the wrapper is unwrapped once.

## Numbers (full CC-MAIN-2026-34 run, 2026-08-28)

1,297 unique captures selected from the full index (300/300 parts, digest-dedup);
**1,248 accepted (96%)**, 8 gone, 41 errors (truncated-in-CC and origin-lost).

| format | accepted | evidence class |
| --- | --- | --- |
| keynote | 485 | slide/master-slide iwa members (+ Document.iwa root type where decodable) |
| pages | 325 | `Document.iwa` root type 10000 = TP.DocumentArchive |
| numbers | 158 | `Document.iwa` root type 1 = TN.DocumentArchive |
| legacy (crawl_old) | 280 | `index.apxl` / `index.xml` members, pre-iWork-13 |

`success.tsv` is the authoritative crosswalk (1,248 rows, ~18.8 GB stored);
`ccrawl_gone.txt` lists the 8 captures whose content CC truncated **and** whose
origin no longer serves the file. Gate for phase 1b (≥5 files per format) is
exceeded ~30-90×.

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
