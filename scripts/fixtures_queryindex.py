#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = ["duckdb"]
# ///
"""Query the Common Crawl URL index for iWork captures.

Scans the CDX parts of one crawl vintage (default CC-MAIN-2026-34) and selects
every capture whose MIME (declared or detected) is an iWork document MIME:

    application/x-iwork-keynote-sffkey
    application/x-iwork-pages-sffpages
    application/x-iwork-numbers-sffnumbers
    (plus any other application/x-iwork* variant the index contains)
    application/vnd.apple.keynote / .pages / .numbers  (legacy MIME variants)

Selection is by MIME, not by file extension -- many .key URLs are not Keynote
documents and vice versa.

Index layout (verified against CC-MAIN-2026-34):
  https://data.commoncrawl.org/cc-index/collections/<vintage>/indexes/cluster.idx
      one line per surt-prefix run: "<surt><ts>\\tcdx-NNNNN.gz\\toffset\\tlength\\tn"
  https://data.commoncrawl.org/cc-index/collections/<vintage>/indexes/cdx-NNNNN.gz
      one line per capture: "<surt> <timestamp> {json}" (SPACE-separated; the
      JSON blob itself contains spaces)
      json keys: url, mime, mime-detected, status, digest, length, offset,
                 filename, redirect, recordid [, truncated]

Each line of the output fixtures/crawl.jsonl:
  {"local_id": "<vintage>-cdx-NNNNN-<rownum>", "origin_url": ..., 
   "warc_file": ..., "warc_offset": ..., "warc_length": ..., "warc_url": ...,
   "content_type": ..., "content_mime_detected": ..., "status": ...,
   "content_truncated": ..., "content_digest": ..., "recordid": ...}

The run is ATOMIC: crawl.jsonl is written to a temp file and renamed only when
every scanned part succeeded. No resume support: rerun from scratch.

Usage:
  uv run scripts/fixtures_queryindex.py                # full scan, limit 3000
  uv run scripts/fixtures_queryindex.py --limit 300    # local smoke scale
"""

from __future__ import annotations

import argparse
import json
import os
import random
import sys
import time
import urllib.request

DEFAULT_VINTAGE = "CC-MAIN-2026-34"
DEFAULT_BASE = "https://data.commoncrawl.org"
MIME_PREFIX = "application/x-iwork"
# crawl metadata WARC segments never carry page payloads; skip them entirely
SKIP_SEGMENTS = ("robotstxt", "crawldiagnostics")


def fetch_with_retry(url: str, tries: int = 5, timeout: int = 120) -> bytes:
    """GET with exponential backoff + jitter; CC is squirrelly, be patient."""
    last_err: Exception | None = None
    for attempt in range(tries):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "pnk-fixtures/0.1"})
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                return resp.read()
        except Exception as e:  # noqa: BLE001 - any transport error is retryable
            last_err = e
            if attempt + 1 < tries:
                delay = min(30.0, 2.0**attempt) + random.uniform(0.5, 4.0)
                print(f"  retry {attempt + 1}/{tries - 1} for {url}: {e}; sleep {delay:.1f}s",
                      file=sys.stderr, flush=True)
                time.sleep(delay)
    raise RuntimeError(f"GET failed after {tries} tries: {url}: {last_err}") from last_err


def list_parts(vintage: str, base: str) -> list[str]:
    """Ordered list of cdx part filenames from cluster.idx (order of appearance)."""
    url = f"{base}/cc-index/collections/{vintage}/indexes/cluster.idx"
    print(f"fetching cluster index: {url}", flush=True)
    blob = fetch_with_retry(url)
    parts: list[str] = []
    seen: set[str] = set()
    for line in blob.split(b"\n"):
        fields = line.split(b"\t")
        if len(fields) < 2:
            continue
        part = fields[1].decode("ascii", "replace")
        if part not in seen:
            seen.add(part)
            parts.append(part)
    if not parts:
        raise RuntimeError(f"no index parts found in cluster.idx for {vintage}")
    print(f"  {len(parts)} index parts", flush=True)
    return parts


def scan_part(con, vintage: str, base: str, part: str, parts_dir: str | None = None) -> list[dict]:
    """Scan one cdx part (local file, https URL, or s3:// URL) and return matching
    captures with their row numbers."""
    url = os.path.join(parts_dir, part) if parts_dir \
        else f"{base}/cc-index/collections/{vintage}/indexes/{part}"
    # One STREAMING query: read_csv streams line by line (constant memory — a
    # cdx part is hundreds of MB compressed and we must not materialize it),
    # a cheap contains() prefilter on the raw line drops non-iWork rows before
    # any JSON parsing, and rownum is the line position in the part.
    #
    # Exact iWork MIME strings, verified against the index: the modern
    # 'application/x-iwork-{keynote,pages,numbers}-sff*' family, plus the
    # legacy 'application/vnd.apple.{keynote,pages,numbers}' variants Apple
    # documents for iWork files. NOT 'application/vnd.apple.%' — that also
    # matches HLS playlists (vnd.apple.mpegurl), pkpass, etc.
    rows = con.execute(f"""
        SELECT rownum, url, mime, mime_detected, status, digest,
               clen, woff, filename, recordid, truncated
        FROM (
            SELECT row_number() OVER ()                       AS rownum,
                   json_extract_string(meta, '$.url')              AS url,
                   json_extract_string(meta, '$.mime')             AS mime,
                   json_extract_string(meta, '$."mime-detected"')  AS mime_detected,
                   json_extract_string(meta, '$.status')           AS status,
                   json_extract_string(meta, '$.digest')           AS digest,
                   json_extract_string(meta, '$.length')           AS clen,
                   json_extract_string(meta, '$.offset')           AS woff,
                   json_extract_string(meta, '$.filename')         AS filename,
                   json_extract_string(meta, '$.recordid')         AS recordid,
                   json_extract_string(meta, '$.truncated')        AS truncated
            FROM (
                SELECT substr(line,
                              length(split_part(line, ' ', 1))
                              + length(split_part(line, ' ', 2)) + 3) AS meta
                FROM read_csv('{url}',
                              delim='\x07', header=false, auto_detect=false,
                              quote='', escape='', compression='gzip',
                              columns={{'line': 'VARCHAR'}})
                WHERE contains(line, 'x-iwork')
                   OR contains(line, 'vnd.apple.keynote')
                   OR contains(line, 'vnd.apple.pages')
                   OR contains(line, 'vnd.apple.numbers')
            )
        )
        WHERE status = '200'
          AND (mime LIKE 'application/x-iwork%'
               OR mime IN ('application/vnd.apple.keynote',
                           'application/vnd.apple.pages',
                           'application/vnd.apple.numbers')
               OR mime_detected LIKE 'application/x-iwork%'
               OR mime_detected IN ('application/vnd.apple.keynote',
                                    'application/vnd.apple.pages',
                                    'application/vnd.apple.numbers'))
        ORDER BY rownum
    """).fetchall()
    out = []
    for rownum, url_, mime, md, status, digest, length, offset, filename, recordid, truncated in rows:
        stem = part.removesuffix(".gz")
        out.append({
            "local_id": f"{vintage}-{stem}-{rownum}",
            "origin_url": url_,
            "warc_file": filename,
            "warc_offset": int(offset),
            "warc_length": int(length),
            "warc_url": f"{DEFAULT_BASE}/{filename}",
            "content_type": mime,
            "content_mime_detected": md,
            "status": status,
            "content_truncated": truncated if truncated not in ("", "false") else None,
            "content_digest": digest,
            "recordid": recordid,
        })
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description="Select iWork captures from a Common Crawl index.")
    ap.add_argument("--vintage", default=DEFAULT_VINTAGE)
    ap.add_argument("--out-dir", default="./fixtures")
    ap.add_argument("--base", default=DEFAULT_BASE,
                    help="index base URL (https://data.commoncrawl.org)")
    ap.add_argument("--data-base", default=None,
                    help="base for cdx part reads (default: --base). "
                         "Set to s3://commoncrawl on EC2 for same-region S3 reads.")
    ap.add_argument("--limit", type=int, default=3000,
                    help="max selections (deduplicated by content digest)")
    ap.add_argument("--max-per-format", type=int, default=0,
                    help="optional post-hoc cap per mime format (0 = no cap)")
    ap.add_argument("--batch-size", dest="batch_size", type=int, default=50,
                    help="parts scanned between progress checkpoints")
    ap.add_argument("--parts-start", type=int, default=0, help="skip the first N parts")
    ap.add_argument("--max-parts", type=int, default=0, help="scan at most N parts (0 = all)")
    ap.add_argument("--parts-dir", default=None,
                    help="read cdx parts from a local dir (e.g. prefetched with "
                         "aws s3 cp --no-sign-request) instead of URLs")
    ap.add_argument("--shard", default=None, metavar="I/N",
                    help="scan only parts I, I+N, I+2N, ... (for parallel EC2 runs); "
                         "output becomes crawl-shard-<I>.jsonl")
    args = ap.parse_args()

    import duckdb

    os.makedirs(args.out_dir, exist_ok=True)
    parts = list_parts(args.vintage, args.base)
    if args.parts_start:
        parts = parts[args.parts_start:]
    if args.max_parts:
        parts = parts[: args.max_parts]
    shard_i = shard_n = 0
    if args.shard:
        shard_i, shard_n = (int(x) for x in args.shard.split("/"))
        parts = parts[shard_i::shard_n]
        print(f"shard {shard_i}/{shard_n}: {len(parts)} parts", flush=True)

    tmp_path = os.path.join(args.out_dir, "crawl.jsonl.tmp")
    final_path = os.path.join(args.out_dir, "crawl.jsonl")
    if args.shard:
        final_path = os.path.join(args.out_dir, f"crawl-shard-{shard_i:03d}.jsonl")
        tmp_path = final_path + ".tmp"
    con = duckdb.connect()
    con.execute("INSTALL httpfs; LOAD httpfs;")
    con.execute("SET http_timeout=120; SET http_retries=2; SET http_retry_backoff=2.0;")
    data_base = args.data_base or args.base
    if data_base.startswith("s3://"):
        con.execute("SET s3_region='us-east-1';")
        try:  # instance profile / env credentials via the aws extension
            con.execute("INSTALL aws; LOAD aws; CALL load_aws_credentials();")
        except Exception as e:  # noqa: BLE001 - anonymous s3 reads may still work
            print(f"note: aws credential chain unavailable ({e}); trying anonymous s3",
                  flush=True)
    selected: dict[str, dict] = {}          # digest -> record (dedup)
    mime_counts: dict[str, int] = {}
    scanned = 0
    hit_limit = False

    try:
        for part in parts:
            try:
                matches = scan_part(con, args.vintage, data_base, part,
                                    parts_dir=args.parts_dir)
            except Exception as e:  # noqa: BLE001 - sporadic index errors: backoff + retry
                for attempt in range(6):
                    delay = min(90.0, 2.0 ** attempt * 3.0) + random.uniform(1.0, 8.0)
                    print(f"  part {part} failed ({e}); backoff {delay:.1f}s", flush=True)
                    time.sleep(delay)
                    try:
                        matches = scan_part(con, args.vintage, data_base, part,
                                            parts_dir=args.parts_dir)
                        break
                    except Exception as e2:  # noqa: BLE001
                        e = e2
                else:
                    raise RuntimeError(f"part {part} failed after retries: {e}") from e

            new_sel = 0
            for rec in matches:
                digest = rec["content_digest"] or f"nodigest:{rec['local_id']}"
                if digest in selected:
                    continue
                selected[digest] = rec
                new_sel += 1
            scanned += 1

            if new_sel or scanned % args.batch_size == 0 or scanned == len(parts):
                print(f"[{scanned}/{len(parts)}] {part}: +{new_sel} "
                      f"(total {len(selected)} unique)", flush=True)
            if len(selected) >= args.limit:
                hit_limit = True
                break
    except Exception as e:  # noqa: BLE001 - atomic: no output on failure
        if os.path.exists(tmp_path):
            os.unlink(tmp_path)
        print(f"ABORTED after {scanned} parts: {e}", file=sys.stderr)
        return 1

    records = list(selected.values())
    if args.max_per_format and args.max_per_format > 0:
        trimmed: list[dict] = []
        per_fmt: dict[str, int] = {}
        for rec in records:
            fmt = (rec["content_mime_detected"] or rec["content_type"] or "unknown")
            if per_fmt.get(fmt, 0) >= args.max_per_format:
                continue
            per_fmt[fmt] = per_fmt.get(fmt, 0) + 1
            trimmed.append(rec)
        records = trimmed

    with open(tmp_path, "w", encoding="utf-8") as f:
        for rec in records:
            f.write(json.dumps(rec, ensure_ascii=False) + "\n")
    os.replace(tmp_path, final_path)

    for rec in records:
        fmt = rec["content_mime_detected"] or rec["content_type"] or "unknown"
        mime_counts[fmt] = mime_counts.get(fmt, 0) + 1
    print(f"\nwrote {len(records)} selections to {final_path} "
          f"({scanned} parts scanned, limit {'hit' if hit_limit else 'not hit'})", flush=True)
    for fmt, n in sorted(mime_counts.items(), key=lambda kv: -kv[1]):
        print(f"  {n:5d}  {fmt}", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
