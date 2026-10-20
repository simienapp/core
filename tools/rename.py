#!/usr/bin/env python3
"""Rename upstream Deno crate names to Simien's, using tools/rename-map.txt.

  rename.py tree [PATH...]   rewrite tracked files in place (default: the repo)
  rename.py patch            rewrite a patch on stdin to stdout

A name is replaced only as a whole word, so `deno_error` doesn't touch
`deno_error_macro` and `serde_v8` doesn't touch `serde_v8_utilities`.
Upstream URLs (github.com/denoland/..., deno.land) are left as they are.
"""

import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def load_map():
    pairs = []
    with open(os.path.join(ROOT, "tools", "rename-map.txt")) as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#"):
                old, new = line.split()
                pairs.append((old, new))
    # Longest first, so a name never rewrites part of a longer one.
    pairs.sort(key=lambda p: -len(p[0]))
    return pairs


def compile_map(pairs):
    names = {old: new for old, new in pairs}
    pattern = re.compile(r"(?<![\w/-])(" + "|".join(re.escape(o) for o, _ in pairs) + r")(?!\w)")
    keep = re.compile(r"github\.com/denoland|deno\.land|denoland/")

    def rename_line(line):
        if keep.search(line):
            return line
        return pattern.sub(lambda m: names[m.group(1)], line)

    return rename_line


def rename_text(text, rename_line):
    return "".join(rename_line(line) for line in text.splitlines(keepends=True))


def tree(paths):
    rename_line = compile_map(load_map())
    files = subprocess.run(["git", "ls-files", *paths], cwd=ROOT, capture_output=True, text=True, check=True).stdout.split()
    changed = 0
    for rel in files:
        if rel.startswith("tools/"):
            continue
        path = os.path.join(ROOT, rel)
        try:
            with open(path, encoding="utf-8") as f:
                text = f.read()
        except UnicodeDecodeError:
            continue  # binary
        if "\0" in text:
            continue
        new = rename_text(text, rename_line)
        if new != text:
            with open(path, "w", encoding="utf-8") as f:
                f.write(new)
            changed += 1
    print(f"renamed in {changed} files")


def patch():
    sys.stdout.write(rename_text(sys.stdin.read(), compile_map(load_map())))


if __name__ == "__main__":
    if len(sys.argv) >= 2 and sys.argv[1] == "tree":
        tree(sys.argv[2:])
    elif len(sys.argv) == 2 and sys.argv[1] == "patch":
        patch()
    else:
        sys.exit(__doc__)
