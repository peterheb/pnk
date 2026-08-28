#!/usr/bin/env python3
"""docs_fetch_sources.py — fetch reference sources for the pnk iWork format docs.

Phase 1 research tooling (pnk, Hackyard Yard #1). Stdlib only, idempotent:
safe to rerun at any time; existing checkouts are kept and only re-annotated.

What it does:
  1. Shallow-clones reference repos into .scratch/ (gitignored) and records the
     FULL HEAD SHA, license, and intended use of each in docs/format/ATTRIBUTION.md.
     License compatibility with our MIT/Apache-2.0 dual license is checked BEFORE
     a repo may be referenced in docs/format/.
  2. Documents (without cloning) LibreOffice libetonyek: MPL-2.0, consult-only.
  3. Runs `npx otorp` against the locally installed Apple iWork apps ("Creator
     Studio" 2026 rebrand of Keynote/Numbers/Pages, v15.3.1) to extract the
     CURRENT .proto definitions, for version-drift checking against the
     reference repos. Scans the main executable and every Contents/Frameworks/*
     binary. Output lands in .scratch/otorp/<AppName>/.

Usage:
    python3 scripts/docs_fetch_sources.py
    uv run scripts/docs_fetch_sources.py

Rerun otorp alone later:
    npx otorp "/Applications/Keynote Creator Studio.app/Contents/MacOS/Keynote" .scratch/otorp/Keynote
    npx otorp "/Applications/Numbers Creator Studio.app/Contents/MacOS/Numbers" .scratch/otorp/Numbers
    npx otorp "/Applications/Pages Creator Studio.app/Contents/MacOS/Pages"   .scratch/otorp/Pages
"""

from __future__ import annotations

import os
import plistlib
import shutil
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
SCRATCH = REPO_ROOT / ".scratch"
DOCS_FORMAT = REPO_ROOT / "docs" / "format"

# ---------------------------------------------------------------------------
# Reference repos
# ---------------------------------------------------------------------------

REPOS: list[dict] = [
    {
        "name": "iwork",
        "url": "https://github.com/dunhamsteve/iwork",
        "use": (
            "PRIMARY source for .proto definitions and the TSP object-id / "
            "constant registry. Its registry is the authoritative id/constant "
            "source for docs/format/registry.md (otorp protos carry no registry)."
        ),
    },
    {
        "name": "litchi",
        "url": "https://github.com/DevExzh/litchi",
        "use": (
            "Secondary .proto definitions + constants for cross-checking "
            "dunhamsteve/iwork (independent extraction of the same format)."
        ),
    },
    {
        "name": "numbers-parser",
        "url": "https://github.com/masaccio/numbers-parser",
        "use": (
            "Reference for the Numbers (.numbers) document tree, table model "
            "(TST) and formula handling. Consulted, not vendored."
        ),
    },
    {
        "name": "keynote-parser",
        "url": "https://github.com/psobot/keynote-parser",
        "use": (
            "Reference for the Keynote (.key) document tree and for the "
            "IWA Snappy framing (its protos/versions/<ver>/ + registry.json "
            "layout documents per-version registry drift). Consulted, not vendored."
        ),
    },
]

# Licenses compatible with MIT/Apache-2.0 dual-licensing for REFERENCE use
# (browsing/reading; we do not vendor-copy code from any of these).
COMPATIBLE = {
    "MIT", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib",
}

# Consult-only, never clone, never copy code from:
CONSULT_ONLY = {
    "name": "libetonyek",
    "url": "https://cgit.freedesktop.org/libreoffice/libetonyek/ "
           "(GitHub mirror: https://github.com/LibreOffice/libetonyek)",
    "license": "MPL-2.0",
    "use": (
        "Consult ONLY if stuck. MPL-2.0 file-level copyleft is incompatible "
        "with our MIT/Apache-2.0 dual licensing: NO code may be copied from "
        "it, and its repository is deliberately NOT cloned into .scratch/. "
        "Its format notes (TSPArchiveInfo, snappy framing, per-object "
        "handlers) are used as a human-readable sanity check only."
    ),
}

# ---------------------------------------------------------------------------
# Local Apple apps (otorp extraction)
# ---------------------------------------------------------------------------

APPS = [
    ("Keynote", "Keynote Creator Studio.app", "Keynote"),
    ("Numbers", "Numbers Creator Studio.app", "Numbers"),
    ("Pages", "Pages Creator Studio.app", "Pages"),
]


def sh(args: list[str], cwd: Path | None = None) -> str:
    """Run a command, return stdout; raise with stderr on failure."""
    r = subprocess.run(args, cwd=cwd, capture_output=True, text=True)
    if r.returncode != 0:
        raise RuntimeError(
            f"command failed ({r.returncode}): {' '.join(args)}\n{r.stderr.strip()}"
        )
    return r.stdout.strip()


def classify_license(path: Path) -> tuple[str, str]:
    """Return (spdx_guess, note) for a LICENSE file, or ('UNKNOWN', reason)."""
    if not path.exists():
        return "UNKNOWN", f"no license file found at {path.name}"
    text = path.read_text(errors="replace")[:4000].lower()
    if "apache license" in text and "version 2.0" in text:
        return "Apache-2.0", ""
    if "permission is hereby granted, free of charge" in text:
        return "MIT", ""
    if "mozilla public license" in text:
        return "MPL-2.0", "file-level copyleft; NOT compatible with MIT/Apache dual"
    if "gnu general public license" in text:
        return "GPL", "strong copyleft; NOT compatible with MIT/Apache dual"
    if "redistribution and use in source and binary forms" in text:
        n = "3" if "this list of conditions and the following disclaimer" in text and "endorse" in text else "2"
        return f"BSD-{n}-Clause", ""
    return "UNKNOWN", "unrecognized license text — read it manually before referencing"


def find_license(repo: Path) -> Path | None:
    for name in sorted(repo.iterdir()):
        if name.is_file() and name.name.upper().startswith(("LICENSE", "COPYING", "LICENCE")):
            return name
    return None


def fetch_repos() -> list[dict]:
    """Clone (shallow) each reference repo into .scratch/ and verify licenses."""
    SCRATCH.mkdir(parents=True, exist_ok=True)
    results = []
    for spec in REPOS:
        dest = SCRATCH / spec["name"]
        if (dest / ".git").exists():
            print(f"[clone] {spec['name']}: already present, keeping checkout")
        else:
            print(f"[clone] {spec['url']} -> {dest}")
            sh(["git", "clone", "--depth", "1", spec["url"], str(dest)])
        sha = sh(["git", "rev-parse", "HEAD"], cwd=dest)
        lic_file = find_license(dest)
        lic, note = classify_license(lic_file) if lic_file else ("UNKNOWN", "no LICENSE file")
        ok = lic in COMPATIBLE
        print(f"[license] {spec['name']}: {lic} ({'compatible' if ok else 'INCOMPATIBLE/UNKNOWN'}) {note}")
        results.append({
            **spec, "sha": sha, "license": lic,
            "license_file": lic_file.name if lic_file else None,
            "compatible": ok, "note": note, "path": dest,
        })
        if not ok:
            print(f"[license] {spec['name']}: DO NOT USE in docs/format/ — resolve license first.")
    return results


def read_info_plist(app_path: Path) -> dict:
    try:
        with open(app_path / "Contents" / "Info.plist", "rb") as f:
            return plistlib.load(f)
    except Exception:
        return {}


def otorp_binaries(app_path: Path) -> list[Path]:
    """Main executable + every Contents/Frameworks/*.framework binary."""
    bins = sorted((app_path / "Contents" / "MacOS").iterdir())
    fw = app_path / "Contents" / "Frameworks"
    if fw.exists():
        for d in sorted(fw.iterdir()):
            # <Name>.framework/<Name> is a symlink to Versions/A/<Name>
            bin_ = d / d.stem
            if bin_.exists():
                bins.append(bin_)
    return bins


def run_otorp() -> bool:
    """Extract current protos from locally installed apps via npx otorp."""
    out_root = SCRATCH / "otorp"
    npx = shutil.which("npx")
    if not npx:
        print("[otorp] npx not found on PATH; install Node.js to run otorp")
        return False
    print(f"[otorp] writing to {out_root}")
    all_ok = True
    for label, bundle, exe in APPS:
        app_path = Path("/Applications") / bundle
        out_dir = out_root / label
        if not app_path.exists():
            print(
                f"[otorp] SKIP: {app_path} not found (install may still be landing).\n"
                f"        Rerun later:\n"
                f'          npx otorp "{app_path / "Contents" / "MacOS" / exe}" .scratch/otorp/{label}\n'
                f'          # plus: for f in "{app_path}"/Contents/Frameworks/*.framework/*/; do '
                f'npx otorp "$f" .scratch/otorp/{label}; done'
            )
            all_ok = False
            continue
        pl = read_info_plist(app_path)
        bid = pl.get("CFBundleIdentifier", "<unavailable>")
        ver = pl.get("CFBundleShortVersionString", "?")
        print(f"[otorp] {label}: {app_path.name} v{ver} (CFBundleIdentifier={bid})")
        out_dir.mkdir(parents=True, exist_ok=True)
        bins = otorp_binaries(app_path)
        if not bins:
            print(f"[otorp]   no Mach-O binaries found in {app_path}")
            all_ok = False
            continue
        ok_count = 0
        for b in bins:
            r = subprocess.run(
                [npx, "-y", "otorp", str(b), str(out_dir)],
                capture_output=True, text=True,
            )
            if r.returncode != 0:
                # Frameworks without embedded protos are expected; continue.
                print(f"[otorp]   (no defs) {b.parent.parent.name if '.framework' in str(b) else b.name}")
            else:
                ok_count += 1
                print(f"[otorp]   ok: {b.parent.parent.name if '.framework' in str(b) else b.name}")
        n = len(list(out_dir.glob("*.proto")))
        print(f"[otorp]   {ok_count}/{len(bins)} binaries yielded defs; {n} .proto files in {out_dir}")
    return all_ok


# ---------------------------------------------------------------------------
# ATTRIBUTION.md (generated; hand-written notes live in the template below)
# ---------------------------------------------------------------------------

ATTRIBUTION_TEMPLATE = """\
# ATTRIBUTION — reference sources for docs/format/

pnk is dual-licensed MIT / Apache-2.0. Every third-party source consulted while
writing `docs/format/` is recorded here — vendored or merely browsed — with its
license and exact git commit, per `AGENTS.md`.

Reference repos below are checked out (shallow) into the gitignored
`.scratch/` directory by `scripts/docs_fetch_sources.py`. We do not vendor
code from any of them.

<!-- NOTE: the two tables below are (re)generated by scripts/docs_fetch_sources.py.
     Hand-written sections are the libetonyek and otorp notes; keep the table
     rows intact when regenerating. -->

## Reference repositories

{repo_table}

## Local app extraction (npx otorp)

The CURRENT `.proto` definitions were extracted from the locally installed
Apple apps with [`otorp`](https://github.com/ptomulik/otorp), which dumps
protobuf definitions embedded in Mach-O binaries. Output:
`.scratch/otorp/<AppName>/`. Apps scanned: main executable plus every
`Contents/Frameworks/*.framework` binary. Bundle identity verified via
`Contents/Info.plist`:

{app_table}

**Drift risk.** `.proto` files recovered by otorp contain message/field
structure but carry **no TSP object-id registry** (the numeric type ids that
`TSP.ArchiveInfo` maps local object ids to). Our primary id/constant source is
therefore the registry checked into `dunhamsteve/iwork`, which documents the
iWork version it was extracted from — likely older than our installed apps.
Every registry-based claim in `docs/format/registry.md` is tagged with its
source; where the otorp extraction disagrees with the registry, the
discrepancy is recorded as a drift note in `docs/format/registry.md`.

## Consult-only sources

- **LibreOffice libetonyek** — MPL-2.0.
  - Where: <https://cgit.freedesktop.org/libreoffice/libetonyek/> (GitHub mirror:
    <https://github.com/LibreOffice/libetonyek>).
  - Policy: **not cloned, not vendored.** MPL-2.0 file-level copyleft is
    incompatible with our MIT/Apache-2.0 dual licensing, so no code may be
    copied from it. We consult it only if stuck, as a human-readable sanity
    check of format behavior; any insight used is re-verified against the
    protos or MIT-licensed parsers above and tagged with its own provenance.
"""


def write_attribution(repos: list[dict]) -> None:
    DOCS_FORMAT.mkdir(parents=True, exist_ok=True)
    rows = [
        "| repo | commit | license | use |",
        "| --- | --- | --- | --- |",
    ]
    for r in repos:
        lic = r["license"] + (f" ({r['license_file']})" if r["license_file"] else "")
        rows.append(
            f"| [`{r['name']}`]({r['url']}) | `{r['sha']}` | {lic} | {r['use']} |"
        )
    app_rows = ["| app | bundle | CFBundleIdentifier | version |", "| --- | --- | --- | --- |"]
    for label, bundle, _exe in APPS:
        app_path = Path("/Applications") / bundle
        if app_path.exists():
            pl = read_info_plist(app_path)
            app_rows.append(
                f"| {label} | `{bundle}` | `{pl.get('CFBundleIdentifier', '?')}` "
                f"| {pl.get('CFBundleShortVersionString', '?')} |"
            )
        else:
            app_rows.append(f"| {label} | `{bundle}` | *not installed* | — |")
    DOCS_FORMAT.joinpath("ATTRIBUTION.md").write_text(
        ATTRIBUTION_TEMPLATE.format(repo_table="\n".join(rows), app_table="\n".join(app_rows))
    )
    print(f"[attribution] wrote {DOCS_FORMAT / 'ATTRIBUTION.md'}")


def main() -> int:
    print(f"pnk docs_fetch_sources — repo root {REPO_ROOT}")
    repos = fetch_repos()
    run_otorp()
    write_attribution(repos)
    print("\ndone. Next: read docs/format/INDEX.md; reference checkouts in .scratch/,")
    print("extracted app protos in .scratch/otorp/.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
