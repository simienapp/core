#!/usr/bin/env python3
"""Publish every crate here that crates.io doesn't have yet, in dependency order.

Safe to rerun: a version already on crates.io is skipped. crates.io limits how
fast brand-new crates can be created (a short burst, then about one every ten
minutes); on a rate limit this waits for the time crates.io gives and retries.

Crates are checked with `cargo package --workspace` before this runs, so
publishing uses --no-verify. The `simien` placeholder in placeholders/simien
goes first.

  CARGO_REGISTRY_TOKEN=... tools/publish.py [--dry-run]
"""

import email.utils
import json
import os
import re
import subprocess
import sys
import time
import urllib.error
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DRY = "--dry-run" in sys.argv
AGENT = "simienapp-core-publish (https://github.com/simienapp/core)"


def on_crates_io(name, version):
    req = urllib.request.Request(f"https://crates.io/api/v1/crates/{name}/{version}", headers={"User-Agent": AGENT})
    try:
        with urllib.request.urlopen(req) as r:
            return r.status == 200
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return False
        raise


def plan():
    meta = json.loads(subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT, capture_output=True, text=True, check=True).stdout)
    crates = {p["name"]: p for p in meta["packages"] if p["publish"] != []}
    deps = {n: {d["name"] for d in p["dependencies"] if d["name"] in crates and d["name"] != n} for n, p in crates.items()}
    order, done = [], set()
    while len(done) < len(crates):
        ready = sorted(n for n in crates if n not in done and deps[n] <= done)
        if not ready:
            sys.exit(f"dependency cycle among {sorted(set(crates) - done)}")
        order += ready
        done |= set(ready)
    steps = [("simien", "0.0.0", ["--manifest-path", "placeholders/simien/Cargo.toml"])]
    steps += [(n, crates[n]["version"], ["-p", n, "--locked"]) for n in order]
    return steps


def wait_from(output):
    # crates.io says e.g. "Please try again after Tue, 20 Oct 2026 19:02:11 GMT".
    m = re.search(r"try again after ([A-Za-z]{3}, \d{2} [A-Za-z]{3} \d{4} [\d:]{8} GMT)", output)
    if m:
        return max(30, email.utils.parsedate_to_datetime(m.group(1)).timestamp() - time.time() + 5)
    return 600


def main():
    for name, version, args in plan():
        if on_crates_io(name, version):
            print(f"skip    {name} {version} (already on crates.io)", flush=True)
            continue
        if DRY:
            print(f"publish {name} {version}", flush=True)
            continue
        while True:
            r = subprocess.run(["cargo", "publish", "--no-verify", *args], cwd=ROOT, capture_output=True, text=True)
            out = r.stdout + r.stderr
            if r.returncode == 0:
                print(f"publish {name} {version}: done", flush=True)
                break
            if "429" in out or "rate limit" in out.lower() or "too many requests" in out.lower():
                seconds = wait_from(out)
                print(f"publish {name} {version}: rate limited, waiting {int(seconds)}s", flush=True)
                time.sleep(seconds)
                continue
            print(out, file=sys.stderr)
            sys.exit(f"publish {name} {version}: failed")


if __name__ == "__main__":
    main()
