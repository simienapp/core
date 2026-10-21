#!/usr/bin/env python3
"""Publish every crate here that crates.io doesn't have yet, in dependency order.

Safe to rerun: a version already on crates.io is skipped. crates.io limits how
fast brand-new crates can be created (a short burst, then about one every ten
minutes); on a rate limit this waits for the time crates.io gives and retries.

Crates are checked with `cargo package --workspace` before this runs, so
publishing uses --no-verify. The `simien` placeholder in placeholders/simien
goes first.

  CARGO_REGISTRY_TOKEN=... [NEW_CRATE_TOKEN=...] tools/publish.py [--dry-run]

In CI CARGO_REGISTRY_TOKEN comes from crates.io trusted publishing
(publish.yml). Trusted publishing can't create a crate, so a crate crates.io
doesn't have yet is published with NEW_CRATE_TOKEN, the release
environment's CARGO_REGISTRY_TOKEN secret, which is only needed while a new
crate is waiting.
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
CHECK = "--check" in sys.argv
AGENT = "simienapp-core-publish (https://github.com/simienapp/core)"


def on_crates_io(name, version=None):
    path = f"{name}/{version}" if version else name
    req = urllib.request.Request(f"https://crates.io/api/v1/crates/{path}", headers={"User-Agent": AGENT})
    try:
        with urllib.request.urlopen(req) as r:
            return r.status == 200
    except urllib.error.HTTPError as e:
        if e.code == 404:
            return False
        raise


def published_dependencies(name, version):
    """(dependency, kind) pairs crates.io recorded for a published version."""
    req = urllib.request.Request(f"https://crates.io/api/v1/crates/{name}/{version}/dependencies", headers={"User-Agent": AGENT})
    with urllib.request.urlopen(req) as r:
        deps = json.load(r)["dependencies"]
    return {(d["crate_id"], d["kind"]) for d in deps}


def local_dependencies(name):
    """(dependency, kind) pairs this checkout would publish for `name`."""
    meta = json.loads(subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT, capture_output=True, text=True, check=True).stdout)
    package = next(p for p in meta["packages"] if p["name"] == name)
    kinds = {None: "normal", "dev": "dev", "build": "build"}
    # Path-only dev-dependencies aren't published.
    return {(d["name"], kinds[d["kind"]]) for d in package["dependencies"] if not (d["kind"] == "dev" and d.get("req") == "*" )}


def check():
    """Fail when a crate changed its dependencies but kept a version that's
    already on crates.io: a publish run skips it, and the published set no
    longer builds together (simien_path_util 0.1.1, #18)."""
    stale = []
    for name, version, _, _ in plan():
        if name == "simien" or not on_crates_io(name, version):
            continue
        published = {d for d in published_dependencies(name, version) if d[1] != "dev"}
        local = {d for d in local_dependencies(name) if d[1] != "dev"}
        if published != local:
            added = sorted(d[0] for d in local - published)
            removed = sorted(d[0] for d in published - local)
            stale.append(f"{name} {version}: dependencies changed (+{added} -{removed}); bump its version")
    for line in stale:
        print(line, file=sys.stderr)
    sys.exit(1 if stale else 0)


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
    steps = [("simien", "0.0.0", ["--manifest-path", "placeholders/simien/Cargo.toml"], set())]
    steps += [(n, crates[n]["version"], ["-p", n, "--locked"], deps[n]) for n in order]
    return steps


def wait_from(output):
    # crates.io says e.g. "Please try again after Tue, 20 Oct 2026 19:02:11 GMT".
    m = re.search(r"try again after ([A-Za-z]{3}, \d{2} [A-Za-z]{3} \d{4} [\d:]{8} GMT)", output)
    if m:
        return max(30, email.utils.parsedate_to_datetime(m.group(1)).timestamp() - time.time() + 5)
    return 600


def main():
    # A crate that can't be published (no trusted publisher yet, say) doesn't
    # hold up the others: its dependents are skipped, everything else goes
    # up, and the run still fails at the end, naming what didn't.
    failed, skipped = [], []
    for name, version, args, deps in plan():
        if blocked := sorted(deps & set(failed + skipped)):
            print(f"skip    {name} {version} (depends on {', '.join(blocked)}, which didn't publish)", flush=True)
            skipped.append(name)
            continue
        if on_crates_io(name, version):
            print(f"skip    {name} {version} (already on crates.io)", flush=True)
            continue
        # Trusted publishing only covers crates that exist. A brand-new crate's
        # first version goes up with NEW_CRATE_TOKEN; after that the crate
        # trusts publish.yml like the rest.
        env = dict(os.environ)
        new = not on_crates_io(name)
        if new:
            if not os.environ.get("NEW_CRATE_TOKEN"):
                print(f"publish {name} {version}: a new crate, and NEW_CRATE_TOKEN is not set", flush=True)
                failed.append(name)
                continue
            env["CARGO_REGISTRY_TOKEN"] = os.environ["NEW_CRATE_TOKEN"]
        if DRY:
            print(f"publish {name} {version}" + (" (new crate)" if new else ""), flush=True)
            continue
        while True:
            r = subprocess.run(["cargo", "publish", "--no-verify", *args], cwd=ROOT, capture_output=True, text=True, env=env)
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
            print(f"publish {name} {version}: failed", flush=True)
            failed.append(name)
            break
    if failed or skipped:
        sys.exit(f"not published: {', '.join(failed)}" + (f"; skipped because of them: {', '.join(skipped)}" if skipped else ""))


if __name__ == "__main__":
    check() if CHECK else main()
