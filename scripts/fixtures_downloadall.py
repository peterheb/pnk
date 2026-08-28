#!/usr/bin/env python3
"""Download iWork captures selected by fixtures_queryindex.py from Common Crawl.

Reads fixtures/crawl.jsonl, fetches each WARC record with a byte-Range request
against https://data.commoncrawl.org/<warc_file>, extracts the HTTP response
body, zip-sanity-checks it, classifies it, and stores accepted files as
fixtures/crawl/<sha256>.<ext> with a crosswalk row in fixtures/success.tsv.

Truncated / digest-mismatched / zip-broken CC copies are refetched from the
origin server (gentle: <=3 concurrent, backoff). If the origin no longer
serves the file, the capture's local_id goes to fixtures/ccrawl_gone.txt and
we move on.

Legacy pre-iWork-13 bundles (ZIP containing index.apxl etc.) are kept under
fixtures/crawl_old/ — reference material, not viewer fixtures.

RESTARTABLE: state is derived from disk. Ids in ccrawl_gone.txt and ids
already crosswalked in success.tsv are skipped; everything else is retried.
Content-addressed filenames (<sha256>.<ext>) make re-downloads idempotent.
Just rerun.

Usage:
  uv run scripts/fixtures_downloadall.py [--fixtures-dir ./fixtures] [--limit N]
"""

from __future__ import annotations

import argparse
import base64
import concurrent.futures
import gzip
import hashlib
import io
import json
import os
import random
import struct
import sys
import threading
import time
import urllib.error
import urllib.request
import zipfile

CC_BASE = "https://data.commoncrawl.org"
UA = "pnk-fixtures/0.1 (+https://github.com/peterheb/pnk)"

# TSP registry type ids of the root document archives (dunhamsteve/iwork
# codegen/{Pages,Numbers,Keynote}.json — .scratch/iwork checkout)
ROOT_TYPE_PAGES = 10000    # TP.DocumentArchive
ROOT_TYPE_NUMBERS = 1      # TN.DocumentArchive
ROOT_TYPE_KEYNOTE = 1      # KN.DocumentArchive (disambiguated by Slide-*.iwa)

TSV_COLUMNS = ["local_id", "sha256", "ext", "format", "origin_url",
               "warc_url", "bytes", "evidence"]


def log(msg: str) -> None:
    print(msg, flush=True)


def http_get(url: str, range_: tuple[int, int] | None = None, tries: int = 6,
             timeout: int = 60) -> bytes:
    """GET with exponential backoff + jitter. Raises urllib.error.HTTPError on
    404/410 immediately (origin genuinely gone). Transient failures retry with
    short backoff; CDN rate-limit signals (403/429/503 — data.commoncrawl.org
    WAF-blocks IPs that burst, verified from EC2) get a long escalating
    cool-down instead."""
    headers = {"User-Agent": UA}
    if range_ is not None:
        headers["Range"] = f"bytes={range_[0]}-{range_[1]}"
    last: Exception | None = None
    for attempt in range(tries):
        try:
            req = urllib.request.Request(url, headers=headers)
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                return resp.read()
        except urllib.error.HTTPError as e:
            if e.code in (404, 410):
                raise
            last = e
            if e.code in (403, 429, 503):
                time.sleep(min(120.0, 10.0 * 2 ** attempt) + random.uniform(1.0, 5.0))
                continue
        except Exception as e:  # noqa: BLE001
            last = e
        if attempt + 1 < tries:
            time.sleep(min(20.0, 2.0**attempt) + random.uniform(0.5, 3.0))
    raise last if last else RuntimeError(f"GET failed: {url}")


def extract_warc_payload(ranged: bytes) -> tuple[bytes, str]:
    """Given the gzipped bytes of one WARC record, return (payload, warc_target_uri).
    For WARC response records the block is a raw HTTP response; strip its
    headers, honor Content-Length/chunked, and undo Content-Encoding: gzip."""
    raw = gzip.decompress(ranged)
    head_end = raw.find(b"\r\n\r\n")
    if head_end < 0:
        raise ValueError("no WARC header terminator")
    headers = {}
    for line in raw[:head_end].split(b"\r\n"):
        if b":" in line:
            k, v = line.split(b":", 1)
            headers[k.decode("latin1").strip().lower()] = v.decode("latin1").strip()
    body = raw[head_end + 4:]
    uri = headers.get("warc-target-uri", "")
    if headers.get("warc-type", "response") == "response":
        http_end = body.find(b"\r\n\r\n")
        if http_end < 0:
            raise ValueError("no HTTP header terminator in WARC response")
        http_head = body[:http_end].decode("latin1", "replace")
        payload = body[http_end + 4:]
        clen = None
        chunked = content_gzip = False
        for line in http_head.split("\r\n")[1:]:
            k, _, v = line.partition(":")
            k = k.strip().lower()
            if k == "content-length":
                clen = int(v.strip())
            elif k == "transfer-encoding" and "chunked" in v.lower():
                chunked = True
            elif k == "content-encoding" and "gzip" in v.lower():
                content_gzip = True
        if chunked:
            payload = dechunk(payload)
        elif clen is not None:
            payload = payload[:clen]
        if content_gzip:
            payload = gzip.decompress(payload)
    else:  # WARC resource record: the block is the file itself
        payload = body
    return payload, uri


def dechunk(data: bytes) -> bytes:
    out = io.BytesIO()
    while True:
        line_end = data.find(b"\r\n")
        if line_end < 0:
            break
        try:
            size = int(data[:line_end].split(b";")[0], 16)
        except ValueError:
            break
        if size == 0:
            break
        out.write(data[line_end + 2: line_end + 2 + size])
        data = data[line_end + 2 + size + 2:]
    return out.getvalue()


def verify_digest(payload: bytes, digest: str) -> bool:
    """CC digests are base32-encoded SHA-1 of the uncompressed payload."""
    if not digest:
        return True
    try:
        return base64.b32encode(hashlib.sha1(payload).digest()).decode().rstrip("=") == digest
    except Exception:  # noqa: BLE001
        return False


def zip_ok(payload: bytes) -> bool:
    try:
        with zipfile.ZipFile(io.BytesIO(payload)):
            return True
    except Exception:  # noqa: BLE001
        return False


# ---------------------------------------------------------------- snappy / iwa

def snappy_decompress_block(data: bytes) -> bytes:
    """Decode one raw Snappy block (NOT the framing format). Pure python; only
    used on the first block(s) of Document.iwa, so speed is irrelevant."""
    def read_varint(buf: bytes, pos: int) -> tuple[int, int]:
        shift = result = 0
        while True:
            b = buf[pos]
            pos += 1
            result |= (b & 0x7F) << shift
            if not b & 0x80:
                return result, pos
            shift += 7

    ulen, pos = read_varint(data, 0)
    out = bytearray()
    n = len(data)
    while pos < n:
        tag = data[pos]
        pos += 1
        kind = tag & 0x03
        if kind == 0:  # literal
            length = (tag >> 2) + 1
            if length > 60:
                extra = length - 60
                length = int.from_bytes(data[pos:pos + extra], "little") + 1
                pos += extra
            out += data[pos:pos + length]
            pos += length
        else:
            if kind == 1:  # copy, 1-byte offset
                length = ((tag >> 2) & 0x07) + 4
                offset = ((tag >> 5) << 8) | data[pos]
                pos += 1
            elif kind == 2:  # copy, 2-byte offset
                length = (tag >> 2) + 1
                offset = int.from_bytes(data[pos:pos + 2], "little")
                pos += 2
            else:  # copy, 4-byte offset
                length = (tag >> 2) + 1
                offset = int.from_bytes(data[pos:pos + 4], "little")
                pos += 4
            for _ in range(length):
                out.append(out[-offset])
    if len(out) != ulen:
        raise ValueError(f"snappy length mismatch: {len(out)} != {ulen}")
    return bytes(out)


def read_varint_stream(buf: bytes, pos: int) -> tuple[int, int]:
    shift = result = 0
    while True:
        b = buf[pos]
        pos += 1
        result |= (b & 0x7F) << shift
        if not b & 0x80:
            return result, pos
        shift += 7


def pb_fields(buf: bytes) -> list[tuple[int, int, bytes]]:
    """Return (field_number, wire_type, value) for every field of a protobuf
    message. Enough for the TSP envelope + ArchiveInfo preamble. Group wire
    types (3/4) DO occur in TSP archives; their content is skipped wholesale
    (we only ever need flat scalar/length-delimited fields near the head)."""
    fields = []
    pos = 0
    n = len(buf)
    while pos < n:
        key, pos = read_varint_stream(buf, pos)
        fn, wt = key >> 3, key & 7
        if wt == 0:
            v, pos = read_varint_stream(buf, pos)
        elif wt == 2:
            ln, pos = read_varint_stream(buf, pos)
            v = buf[pos:pos + ln]
            pos += ln
        elif wt == 5:
            v, pos = buf[pos:pos + 4], pos + 4
        elif wt == 1:
            v, pos = buf[pos:pos + 8], pos + 8
        elif wt == 3:  # start group: skip to the matching end-group
            depth = 1
            v = b""
            while pos < n and depth:
                k2, pos = read_varint_stream(buf, pos)
                w2 = k2 & 7
                if w2 == 0:
                    _, pos = read_varint_stream(buf, pos)
                elif w2 == 2:
                    ln, pos = read_varint_stream(buf, pos)
                    pos += ln
                elif w2 == 5:
                    pos += 4
                elif w2 == 1:
                    pos += 8
                elif w2 == 3:
                    depth += 1
                elif w2 == 4:
                    depth -= 1
        else:  # wt == 4 outside a group: corrupt stream
            break
        fields.append((fn, wt, v))
    return fields


def first_archive_type(iwa_bytes: bytes) -> int | None:
    """Return the TSP registry type id of the first message in an .iwa stream
    (its first ArchiveInfo's first MessageInfo type). This identifies the app:
    10000=TP.DocumentArchive (Pages), 1=TN.DocumentArchive (Numbers),
    1=KN.DocumentArchive (Keynote)."""
    pos = 0
    decoded = bytearray()
    while pos + 4 <= len(iwa_bytes) and len(decoded) < 65536:
        # 4-byte block header: zero chunk-type byte + u24 LE compressed
        # length (bytes 1..3) — docs/format/INDEX.md. A struct "<HH" read
        # here swaps byte order and only works on single-block files.
        csize = (iwa_bytes[pos + 1] | (iwa_bytes[pos + 2] << 8)
                 | (iwa_bytes[pos + 3] << 16))
        block = iwa_bytes[pos + 4: pos + 4 + csize]
        pos += 4 + csize
        try:
            decoded += snappy_decompress_block(block)
        except Exception:  # noqa: BLE001 - we only need the leading blocks
            break
    data = bytes(decoded)
    # Decompressed stream: repeated [varint len][TSP.ArchiveInfo envelope] +
    # declared payloads. Envelope schema (docs/format/objects.md, verified in
    # dunhamsteve/iwork + keynote-parser): ArchiveInfo{identifier=1,
    # message_infos=2 (repeated)}, MessageInfo{type=1, length=3}. The first
    # MessageInfo.type of the first envelope is the root document archive.
    p = 0
    while p < len(data):
        try:
            msg_len, p = read_varint_stream(data, p)
            envelope = data[p:p + msg_len]
            p += msg_len
            for fn, wt, v in pb_fields(envelope):
                if fn == 2 and wt == 2:  # ArchiveInfo.message_infos
                    for fn2, wt2, v2 in pb_fields(v):
                        if fn2 == 1 and wt2 == 0:  # MessageInfo.type
                            return v2
        except (IndexError, ValueError, struct.error):
            return None  # corrupt/truncated head: caller falls back to mime
    return None


# ------------------------------------------------------------- classification

def classify(payload: bytes, rec: dict) -> tuple[str, str, str, bytes]:
    """Return (ext, format, evidence, bundle_bytes). bundle_bytes is the ZIP we
    actually classified (may be an inner zip, see zip-in-zip below)."""
    def mime_hint() -> str:
        m = (rec.get("content_mime_detected") or rec.get("content_type") or "").lower()
        if "sffpages" in m:
            return "pages"
        if "sffnumbers" in m:
            return "numbers"
        if "sffkey" in m:
            return "key"
        return "iwork" if "iwork" in m else ""

    def members_of(zp: bytes) -> list[str]:
        with zipfile.ZipFile(io.BytesIO(zp)) as zf:
            return zf.namelist()

    names = members_of(payload)
    bundle = payload
    if not any(n.endswith(".iwa") for n in names):
        # some CMSs store the iWork bundle inside a wrapper zip: recurse once
        inner = [n for n in names if n.lower().endswith(".zip")]
        if len(inner) == 1:
            with zipfile.ZipFile(io.BytesIO(payload)) as zf:
                inner_bytes = zf.read(inner[0])
            if zip_ok(inner_bytes):
                bundle = inner_bytes
                names = members_of(bundle)

    iwas = [n for n in names if n.endswith(".iwa")]
    hint = mime_hint()

    if not iwas:
        legacy = [n for n in names
                  if n.lower().endswith((".apxl", ".apxl.gz", ".lsdw", ".lswp"))
                  or n.lower() in ("index.xml", "index.xml.gz")]
        if legacy:
            fmt = f"legacy-{hint or 'unknown'}"
            return "legacy", fmt, f"legacy bundle: members={legacy[:3]} (pre-iWork-13); mime={hint}", bundle
        raise ValueError(f"not an iWork bundle: members={names[:5]}")

    key_markers = [n for n in iwas
                   if n.rsplit("/", 1)[-1].startswith(("Slide-", "MasterSlide"))]
    doc_iwa = next((n for n in iwas if n.rsplit("/", 1)[-1] == "Document.iwa"), None)

    root_type = None
    if doc_iwa:
        with zipfile.ZipFile(io.BytesIO(bundle)) as zf:
            root_type = first_archive_type(zf.read(doc_iwa))

    if key_markers:
        return "key", "keynote", f"slide iwa present ({key_markers[0]}); root type {root_type}; mime={hint}", bundle
    if root_type == ROOT_TYPE_PAGES:
        return "pages", "pages", f"Document.iwa root type {root_type}=TP.DocumentArchive; mime={hint}", bundle
    if root_type == ROOT_TYPE_NUMBERS:
        if hint == "key":
            return "key", "keynote", (f"LOW CONFIDENCE: root type {root_type} is shared with "
                                      f"Keynote but no slide iwa; mime={hint}"), bundle
        return "numbers", "numbers", f"Document.iwa root type {root_type}=TN.DocumentArchive; mime={hint}", bundle
    if hint in ("pages", "numbers", "key"):
        fmt = {"key": "keynote"}.get(hint, hint)
        return (hint, fmt,
                f"LOW CONFIDENCE: mime-only (root type {root_type}, mime={hint})", bundle)
    raise ValueError(f"iwa members but unclassifiable: mime={hint}, iwa={iwas[:5]}")


# --------------------------------------------------------------------- worker

def fetch_record(rec: dict, cc_slot: threading.Semaphore,
                 origin_slot: threading.Semaphore,
                 cc_delay: float = 0.0) -> dict:
    """Download + verify + classify one capture; returns the result dict
    including payload bytes. cc_slot gates Common Crawl range fetches
    (moderate), origin_slot gates origin-server refetches (gentle <=3).
    cc_delay adds a random pre-request pause (seconds) to smooth bursts —
    CloudFront rate-blocks AWS IPs that hammer it."""
    local_id = rec["local_id"]
    warc_url = f"{CC_BASE}/{rec['warc_file']}"
    range_ = (rec["warc_offset"], rec["warc_offset"] + rec["warc_length"] - 1)
    result = {"local_id": local_id, "status": "error", "evidence": "", "sha256": "",
              "ext": "", "format": "", "bytes": 0, "origin_url": rec["origin_url"],
              "warc_url": warc_url, "destination": "crawl", "payload": None,
              "filename": ""}

    def origin_refetch(reason: str) -> None:
        try:
            with origin_slot:
                payload = http_get(rec["origin_url"], None, tries=3, timeout=90)
            result["payload"] = payload
            result["evidence"] += f"origin refetch ({reason}); "
        except Exception as e:  # noqa: BLE001
            result["status"] = "gone"
            result["evidence"] += f"origin gone ({reason}): {type(e).__name__}"

    try:
        if cc_delay > 0:
            time.sleep(random.uniform(0, cc_delay))
        with cc_slot:
            ranged = http_get(warc_url, range_)
        payload, _uri = extract_warc_payload(ranged)
        truncated = bool(rec.get("content_truncated"))
        digest_ok = verify_digest(payload, rec.get("content_digest", ""))
        intact = zip_ok(payload)
        if truncated or not digest_ok or not intact:
            reason = f"cc incomplete: truncated={truncated} digest_ok={digest_ok} zip_ok={intact}"
            origin_refetch(reason)
            if result["status"] == "gone":
                return result
            payload = result["payload"]
        else:
            result["evidence"] = "cc payload verified; "

        ext, fmt, ev, _bundle = classify(payload, rec)
        sha = hashlib.sha256(payload).hexdigest()
        result.update(status="ok", sha256=sha, ext=ext, format=fmt,
                      bytes=len(payload), evidence=result["evidence"] + ev,
                      payload=payload)
        result["destination"] = "crawl_old" if ext == "legacy" else "crawl"
        result["filename"] = (f"{sha}.{ext}" if ext != "legacy"
                              else f"{sha}.{fmt.replace('legacy-', '')}")
        return result
    except urllib.error.HTTPError as e:
        if e.code in (404, 410):
            result["status"] = "gone"
            result["evidence"] = f"cc http {e.code}"
        else:
            result["evidence"] = f"cc http {e.code}: {e}"
        return result
    except Exception as e:  # noqa: BLE001
        result["evidence"] = f"{type(e).__name__}: {e}"
        return result


# ----------------------------------------------------------------------- main

def main() -> int:
    ap = argparse.ArgumentParser(
        description="Download iWork fixtures from Common Crawl WARC records.")
    ap.add_argument("--fixtures-dir", default="./fixtures")
    ap.add_argument("--limit", type=int, default=0, help="max downloads this run (0 = all)")
    ap.add_argument("--cc-concurrency", type=int, default=8,
                    help="parallel Common Crawl range fetches")
    ap.add_argument("--cc-delay", type=float, default=0.0,
                    help="random pre-request pause in seconds (0-arg range) to "
                         "smooth bursts; use ~1.0 when CloudFront rate-blocks")
    ap.add_argument("--origin-concurrency", type=int, default=3,
                    help="max parallel origin refetches (be gentle)")
    args = ap.parse_args()

    fd = args.fixtures_dir
    dir_crawl, dir_old = os.path.join(fd, "crawl"), os.path.join(fd, "crawl_old")
    os.makedirs(dir_crawl, exist_ok=True)
    os.makedirs(dir_old, exist_ok=True)
    crawl_jsonl = os.path.join(fd, "crawl.jsonl")
    success_tsv = os.path.join(fd, "success.tsv")
    gone_txt = os.path.join(fd, "ccrawl_gone.txt")

    records = [json.loads(l) for l in open(crawl_jsonl, encoding="utf-8") if l.strip()]
    log(f"crawl.jsonl: {len(records)} selections")

    gone: set[str] = set()
    if os.path.exists(gone_txt):
        gone = {l.strip() for l in open(gone_txt, encoding="utf-8") if l.strip()}
    done_ids = set(gone)
    if os.path.exists(success_tsv):
        with open(success_tsv, encoding="utf-8") as f:
            next(f, None)
            done_ids |= {line.split("\t", 1)[0] for line in f if line.strip()}

    todo = [r for r in records if r["local_id"] not in done_ids]
    log(f"todo: {len(todo)} (skipping {len(done_ids)} done/gone)")
    if args.limit:
        todo = todo[: args.limit]

    def append_gone(local_id: str) -> None:
        with open(gone_txt, "a", encoding="utf-8") as f:
            f.write(local_id + "\n")

    def append_success(row: list[str]) -> None:
        new = not os.path.exists(success_tsv) or os.path.getsize(success_tsv) == 0
        with open(success_tsv, "a", encoding="utf-8") as f:
            if new:
                f.write("\t".join(TSV_COLUMNS) + "\n")
            f.write("\t".join(row) + "\n")

    cc_slot = threading.Semaphore(args.cc_concurrency)
    origin_slot = threading.Semaphore(args.origin_concurrency)

    counts = {"ok": 0, "gone": 0, "error": 0}
    fmt_counts: dict[str, int] = {}
    legacy = 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.cc_concurrency) as pool:
        futures = {pool.submit(fetch_record, rec, cc_slot, origin_slot,
                               args.cc_delay): rec
                   for rec in todo}
        for i, fut in enumerate(concurrent.futures.as_completed(futures)):
            res = fut.result()
            lid = res["local_id"]
            if res["status"] == "gone":
                append_gone(lid)
                counts["gone"] += 1
                log(f"  GONE {lid}: {res['evidence']}")
            elif res["status"] == "ok":
                dest_dir = dir_crawl if res["destination"] == "crawl" else dir_old
                dest = os.path.join(dest_dir, res["filename"])
                if not os.path.exists(dest):
                    tmp = dest + ".tmp"
                    with open(tmp, "wb") as f:
                        f.write(res["payload"])
                    os.replace(tmp, dest)
                append_success([lid, res["sha256"], res["ext"], res["format"],
                                res["origin_url"], res["warc_url"], str(res["bytes"]),
                                res["evidence"]])
                counts["ok"] += 1
                if res["destination"] == "crawl_old":
                    legacy += 1
                else:
                    fmt_counts[res["format"]] = fmt_counts.get(res["format"], 0) + 1
            else:
                counts["error"] += 1
                log(f"  ERROR {lid}: {res['evidence']}")
            if (i + 1) % 25 == 0:
                log(f"  [{i + 1}/{len(todo)}] {counts}")

    log(f"\ndone: {counts}")
    log(f"formats: {fmt_counts}")
    log(f"legacy (crawl_old): {legacy}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
