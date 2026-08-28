# Fixtures EC2 runbook — full Common Crawl iWork scan

One-time full-scale run of the fixtures pipeline on EC2. The local machine only
did a smoke run (`fixtures/crawl.jsonl` was built from 40 of 300 index parts);
this completes the job. Total index volume is ~246 GB compressed across 300
`cdx-*.gz` parts, so we scan shards in parallel on one instance. Wall-clock
target: ~0.5–1 h for the scan + ~0.5–1 h for downloads.

Principle: **everything goes over HTTPS to `data.commoncrawl.org` (CloudFront).**
Anonymous S3 API access to the Common Crawl bucket is DENIED (verified
2026-08-28 from a us-east-1 instance: `aws s3 --no-sign-request` → 403 on
HeadObject, duckdb `s3://` → 403 too), so the runbook's old `s3://` shortcut is
dead. No AWS credentials are needed anywhere; an instance role does not help.
CloudFront throttles per-connection harder than S3 would — keep shard counts
sane (16 on a 16-core box) and rely on the scripts' retry/backoff.

> Copy-paste each block in order. `$HOST` is the instance hostname/IP Main
> provides (e.g. `ssh ec2-user@ec2-…compute-1.amazonaws.com`).

## 0. Launch

Any us-east-1 instance works. Recommended: 16 vCPU (e.g. `c7g.4xlarge` /
`m7g.4xlarge`) — the bottleneck is the CloudFront pipe, more parallel streams
win. ≥30 GB root disk is plenty (nothing large is stored locally: parts are
streamed, fixtures are a few GB). Instance role NOT needed (S3 is not used).

```bash
HOST=<instance host>            # or just use the `pnk-ec2` ssh alias
ssh -o ServerAliveInterval=30 -i /tmp/new-access-key ec2-user@$HOST
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

> Note: the index is read over HTTPS via duckdb httpfs (default
> `--base https://data.commoncrawl.org`). Do NOT pass `--data-base s3://` —
> anonymous S3 reads of the Common Crawl bucket are denied (403, verified
> 2026-08-28); the scripts' retry/backoff handles CloudFront throttling.

## 3. Scan the index (sharded, ~0.5–1 h)

16 shards on a 16-core box, one process each, writing per-shard selections
over HTTPS. `nohup` + logs so an SSH drop doesn't kill the run.

```bash
cd ~
for i in $(seq 0 15); do
  nohup uv run --with duckdb fixtures_queryindex.py \
      --shard $i/16 \
      --limit 400 --out-dir ~/fixtures \
      > ~/fixtures/scan-$i.log 2>&1 &
done
jobs   # 16 running
```

Monitor:

```bash
tail -n2 ~/fixtures/scan-*.log
ls -la ~/fixtures/crawl-shard-*.jsonl 2>/dev/null
```

Wait until all 16 processes exit and every `scan-*.log` ends with
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

## 3b. If CloudFront throttles the scan

The only failure mode observed is per-connection throttling. If shards crawl
(a part taking minutes instead of ~30 s), don't kill the blast — the scripts'
backoff + retry ride throttling out; just accept slower per-part times. Shard
output is per-shard, so if a shard process dies entirely, rerunning it alone
is just `--shard i/16` again.

## 4. Download the fixtures (~1 h)

```bash
nohup uv run fixtures_downloadall.py --fixtures-dir ~/fixtures \
    --cc-concurrency 16 > ~/fixtures/download.log 2>&1 &
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
    -i /tmp/new-access-key ec2-user@$HOST:fixtures/ /Users/phebert/Development/pnk-fixtures/
```

Then verify locally and tear the instance down **immediately**:

```bash
ssh -i /tmp/new-access-key ec2-user@$HOST 'sudo shutdown -h now'
```

## Cost sketch

A 16-vCPU instance (e.g. `c7g.4xlarge` ≈ $0.58/h on-demand). Scan ~0.5–1 h +
download ~0.5–1 h ⇒ **a couple of dollars**, provided §6 happens promptly.
All reads are HTTPS/CloudFront GETs; no S3 charges.
