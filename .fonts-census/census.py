import csv, json, subprocess, sys, collections, os
from concurrent.futures import ThreadPoolExecutor
REPO="/Users/phebert/pnk"; BIN=REPO+"/target/release/pnk2json"
rows=[r for r in csv.DictReader(open(REPO+"/fixtures/success.tsv"), delimiter="\t") if r["format"]!="legacy-unknown"]
def one(r):
    path=f"{REPO}/fixtures/crawl/{r['sha256']}.{r['ext']}"
    if not os.path.exists(path): return None
    try:
        out=subprocess.run([BIN,path],capture_output=True,timeout=120)
        if out.returncode!=0: return None
        d=json.loads(out.stdout)
    except Exception: return None
    return (r["ext"], d.get("fonts",[]))
docs=collections.Counter(); per_app=collections.defaultdict(collections.Counter); n=collections.Counter()
with ThreadPoolExecutor(8) as ex:
    for res in ex.map(one, rows):
        if not res: continue
        app,fonts=res; n[app]+=1
        for f in set(fonts):
            docs[f]+=1; per_app[f][app]+=1
json.dump({"docs_per_app":n,"fonts":{f:{"docs":docs[f],**per_app[f]} for f in docs}}, open(sys.argv[1],"w"), indent=1)
print("documents:",dict(n),"distinct font names:",len(docs))
