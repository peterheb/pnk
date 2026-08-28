# Fixtures EC2 runbook — full Common Crawl iWork scan

One-time full-scale run of the fixtures pipeline on EC2. The local machine only
did a smoke run (`fixtures/crawl.jsonl` was built from 6 of 300 index parts);
this completes the job. Total index volume is ~246 GB compressed across 300
`cdx-*.gz` parts, so we scan shards in parallel on one instance. Wall-clock
target: ~2–4 h for the scan + ~1 h for downloads.

Principle: **S3 reads from us-east-1 are free and fast; CloudFront (HTTPS) is
neither.** Use `s3://` or `aws s3 cp --no-sign-request`, never
`data.commoncrawl.org`, for bulk reads on the instance. No AWS credentials are
required (Common Crawl's bucket is public); an instance role just makes duckdb
S3 reads smoother, it is not required.

> Copy-paste each block in order. `$HOST` is the instance hostname/IP Main
> provides (e.g. `ssh ec2-user@ec2-…compute-1.amazonaws.com`).

## 0. Launch

Any us-east-1 instance works. Recommended: `m7g.xlarge` (4 vCPU graviton,
16 GB) — the bottleneck is network, not CPU. 30 GB root disk is plenty
(nothing large is stored locally: parts are streamed, fixtures are ~2 GB).
Instance role optional (see §2 note).

```bash
HOST=<instance host>
ssh -o ServerAliveInterval=30 ec2-user@$HOST
```

## 1. Tools (skip what's present)

```bash
command -v uv >/dev/null || curl -LsSf https://astral.sh/uv/install.sh | sh
source ~/.cargo/env 2>/dev/null || export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"
uv --version && aws --version && python3 --version
```

## 2. Get the scripts off the Mac

From the **Mac** (not on the instance):

```bash
cd /Users/phebert/pnk
scp scripts/fixtures_queryindex.py scripts/fixtures_downloadall.py ec2-user@$HOST:~/
```

Back **on the instance**:

```bash
mkdir -p ~/fixtures && cd ~
```

> Note on S3 auth for duckdb: with an instance role, duckdb's aws extension
> picks it up automatically (`load_aws_credentials()` — the script does this).
> Without a role the script falls back and anonymous reads of the public
> bucket usually still work. If duckdb S3 reads fail, use the alternative in
> §3b which avoids duckdb+S3 entirely.

## 3. Scan the index (sharded, ~2–4 h)

8 shards, one process each, writing per-shard selections. `nohup` + logs so an
SSH drop doesn't kill the run.

```bash
cd ~
for i in $(seq 0 7); do
  nohup uv run --with duckdb fixtures_queryindex.py \
      --data-base s3://commoncrawl --shard $i/8 \
      --limit 800 --out-dir ~/fixtures \
      > ~/fixtures/scan-$i.log 2>&1 &
done
jobs   # 8 running
```

Monitor:

```bash
tail -n2 ~/fixtures/scan-*.log
ls -la ~/fixtures/crawl-shard-*.jsonl 2>/dev/null
```

Wait until all 8 processes exit and every `scan-*.log` ends with
`wrote N selections`:

```bash
while pgrep -f fixtures_queryindex > /dev/null; do sleep 60; done
grep -H wrote ~/fixtures/scan-*.log
```

Merge the shards (dedup by digest across shards) into the final selections:

```bash
python3 - <<'EOF'
import json, glob
seen, out = set(), []
for f in sorted(glob.glob('/home/ec2-user/fixtures/crawl-shard-*.jsonl')):
    for line in open(f):
        r = json.loads(line)
        k = r['content_digest'] or r['local_id']
        if k not in seen:
            seen.add(k); out.append(line)
with open('/home/ec2-user/fixtures/crawl.jsonl', 'w') as f:
    f.writelines(out)
print('merged:', len(out), 'unique selections')
EOF
```

## 3b. Alternative scan without duckdb+S3

If the instance has no role and duckdb cannot read `s3://` anonymously,
prefetch the parts with the anonymous AWS CLI instead and point the scanner at
the local copies (same result, one extra ~246 GB of disk writes — use ≥350 GB
disk for this path):

```bash
mkdir -p ~/parts && cd ~/parts
seq -f 'cdx-%05g.gz' 0 299 | xargs -P 8 -I{} \
  aws s3 cp --no-sign-request s3://commoncrawl/cc-index/collections/CC-MAIN-2026-34/indexes/{} . &
# then, once all 300 files are present:
cd ~
for i in $(seq 0 7); do
  nohup uv run --with duckdb fixtures_queryindex.py \
      --parts-dir ~/parts --shard $i/8 --limit 800 --out-dir ~/fixtures \
      > ~/fixtures/scan-$i.log 2>&1 &
done
```

## 4. Download the fixtures (~1 h)

```bash
cd ~
nohup uv run fixtures_downloadall.py --fixtures-dir ~/fixtures \
    --cc-concurrency 12 > ~/fixtures/download.log 2>&1 &
```

Monitor (`crawl/` fills with `<sha256>.<ext>` files, `success.tsv` grows,
`ccrawl_gone.txt` collects dead origins):

```bash
tail -f ~/fixtures/download.log        # Ctrl-C to stop watching (download continues)
ls ~/fixtures/crawl | wc -l
wc -l ~/fixtures/success.tsv ~/fixtures/ccrawl_gone.txt 2>/dev/null
```

Restartable by design: if it dies, just rerun the same command.

## 5. Sanity-check results before paying for more instance time

```bash
awk -F'\t' 'NR>1 {print $4}' ~/fixtures/success.tsv | sort | uniq -c
ls ~/fixtures/crawl_old 2>/dev/null | wc -l   # legacy pre-iWork-13 bundles
```

Gate: ≥5 modern files per format (pages / numbers / keynote) after losses.
The full run should net hundreds per format.

## 6. Ship it back to the Mac

From the **Mac**:

```bash
rsync -av --exclude 'crawl-shard-*.jsonl' --exclude '*.tmp' \
    ec2-user@$HOST:fixtures/ ~/Development/pnk-fixtures/
```

Then verify locally and tear the instance down **immediately**:

```bash
ssh ec2-user@$HOST 'sudo shutdown -h now'
```

## Cost sketch

m7g.xlarge ≈ $0.19/h on-demand. Scan ~2–4 h + download ~1 h ⇒ **under $2**,
provided §6 happens promptly. S3 GETs for 300 objects: negligible.
