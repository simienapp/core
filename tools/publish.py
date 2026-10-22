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
environment's CARGO_REGISTRY_TOKEN secret. So is a crate that has no trusted
publisher on crates.io yet: when crates.io says the trusted-publishing token
isn't valid for it, the publish is retried with NEW_CRATE_TOKEN.

A trusted-publishing token lives about 30 minutes, and waiting out the
new-crate rate limit takes longer, so in CI this mints a fresh one (as
crates-io-auth-action does) before publishing with one older than
TOKEN_REFRESH seconds, and revokes the ones it minted at the end.
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
TOKENS_URL = "https://crates.io/api/v1/trusted_publishing/tokens"
TOKEN_REFRESH = 20 * 60


class TrustedToken:
    """The trusted-publishing token, fresh enough to publish with."""

    def __init__(self):
        self.token = os.environ.get("CARGO_REGISTRY_TOKEN", "")
        self.since = time.monotonic()
        self.minted = []

    def can_mint(self):
        return bool(os.environ.get("ACTIONS_ID_TOKEN_REQUEST_URL") and os.environ.get("ACTIONS_ID_TOKEN_REQUEST_TOKEN"))

    def get(self):
        if self.can_mint() and time.monotonic() - self.since > TOKEN_REFRESH:
            self.token = self.mint()
            self.since = time.monotonic()
            self.minted.append(self.token)
            print("token   minted a fresh trusted-publishing token", flush=True)
        return self.token

    def mint(self):
        # GitHub's OIDC token for audience crates.io, exchanged for a crates.io token.
        oidc = urllib.request.Request(
            os.environ["ACTIONS_ID_TOKEN_REQUEST_URL"] + "&audience=crates.io",
            headers={"Authorization": "Bearer " + os.environ["ACTIONS_ID_TOKEN_REQUEST_TOKEN"], "User-Agent": AGENT},
        )
        with urllib.request.urlopen(oidc) as r:
            jwt = json.load(r)["value"]
        exchange = urllib.request.Request(
            TOKENS_URL,
            data=json.dumps({"jwt": jwt}).encode(),
            headers={"Content-Type": "application/json", "User-Agent": AGENT},
            method="POST",
        )
        with urllib.request.urlopen(exchange) as r:
            return json.load(r)["token"]

    def revoke_minted(self):
        for token in self.minted:
            req = urllib.request.Request(TOKENS_URL, headers={"Authorization": "Bearer " + token, "User-Agent": AGENT}, method="DELETE")
            try:
                urllib.request.urlopen(req).close()
            except urllib.error.HTTPError as e:
                print(f"token   revoking a minted token failed: HTTP {e.code}", flush=True)


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


def deno_dependencies():
    """Every crate here that depends on a deno_* crate, of any kind. APS 2.1:
    Simien depends on no Deno crate; what it uses is forked here, renamed."""
    meta = json.loads(subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT, capture_output=True, text=True, check=True).stdout)
    return sorted(
        f"{p['name']} depends on {d['name']}"
        for p in meta["packages"]
        for d in p["dependencies"]
        if d["name"].startswith("deno_") or (d.get("rename") or "").startswith("deno_")
    )


def check():
    """Fail when a crate depends on a deno_* crate (APS 2.1), or changed its
    dependencies but kept a version that's already on crates.io: a publish
    run skips it, and the published set no longer builds together
    (simien_path_util 0.1.1, #18)."""
    stale = [f"{line}: fork it here, renamed simien_* (APS 2.1)" for line in deno_dependencies()]
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
    trusted = TrustedToken()
    try:
        publish_all(trusted)
    finally:
        trusted.revoke_minted()


def publish_all(trusted):
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
            if not new:
                env["CARGO_REGISTRY_TOKEN"] = trusted.get()
            r = subprocess.run(["cargo", "publish", "--no-verify", *args], cwd=ROOT, capture_output=True, text=True, env=env)
            out = r.stdout + r.stderr
            if not new and "not valid for crate" in out and os.environ.get("NEW_CRATE_TOKEN"):
                print(f"publish {name} {version}: no trusted publisher on crates.io yet, using NEW_CRATE_TOKEN", flush=True)
                new = True
                env["CARGO_REGISTRY_TOKEN"] = os.environ["NEW_CRATE_TOKEN"]
                continue
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
