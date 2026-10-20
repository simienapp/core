#!/bin/sh
# What upstream has changed since this repo last looked, as Markdown on stdout.
# Prints nothing when there is nothing new. Used by .github/workflows/upstream-watch.yml,
# which files the output as an issue; it also runs locally.
#
#   tools/upstream-watch.sh [workdir]
#
# Covers:
#   - denoland/deno main after the `checked` commit in tools/upstream-sync.txt,
#     limited to tools/upstream-paths.txt
#   - the standalone crate repos after the tag each was taken at
#   - denoland/rusty_v8 releases newer than the v8 version Cargo.lock uses
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
work=${1:-"$here/.tmp/upstream-watch"}
mkdir -p "$work"

fetch() { # repo dir
  if [ -d "$work/$2" ]; then
    git -C "$work/$2" fetch -q --tags origin
  else
    git clone -q --filter=blob:none --no-checkout "https://github.com/denoland/$1.git" "$work/$2"
  fi
}

# Subjects that look like fixes or security work get a marker.
flag() {
  awk '{ s = tolower($0); m = "";
         if (s ~ /secur|cve|vuln|advisory|sandbox|overflow|use-after|uaf|leak/) m = " **(security?)**";
         else if (s ~ / fix(\(|:)/) m = " (fix)";
         print "- " $0 m }'
}

out=""
add() { out="$out$1
"; }

# 1. denoland/deno
checked=$(awk '$1 == "checked" { print $2 }' "$here/tools/upstream-sync.txt")
paths=$(grep -v '^#' "$here/tools/upstream-paths.txt" | grep -v '^$')
fetch deno deno
# shellcheck disable=SC2086 # one path per word, on purpose
log=$(git -C "$work/deno" log --reverse --format='%h %ad %s' --date=short "$checked..origin/main" -- $paths)
if [ -n "$log" ]; then
  add "### denoland/deno, since \`$checked\`"
  add ""
  add "$(printf '%s\n' "$log" | flag)"
  add ""
  add "Port with \`tools/port-upstream.sh <deno checkout> <commit>\`, then record each decision and the new \`checked\` commit in \`tools/upstream-sync.txt\`."
  add ""
fi

# 2. standalone repos: repo, the tag taken
for pair in deno_error:0.7.1 deno_unsync:0.4.4 deno_path_util:0.6.4 boxed_error:0.2.3 wasm_dep_analyzer:0.4.0 deno_native_certs:0.3.1; do
  repo=${pair%%:*}
  tag=${pair#*:}
  # A later review point, if one is recorded: `checked-repo <repo> <commit>`.
  reviewed=$(awk -v r="$repo" '$1 == "checked-repo" && $2 == r { print $3 }' "$here/tools/upstream-sync.txt")
  [ -n "$reviewed" ] && tag=$reviewed
  fetch "$repo" "$repo"
  head=$(git -C "$work/$repo" symbolic-ref -q --short refs/remotes/origin/HEAD || echo origin/main)
  new=$(git -C "$work/$repo" log --reverse --format='%h %ad %s' --date=short "$tag..$head")
  if [ -n "$new" ]; then
    add "### denoland/$repo, since \`$tag\` (libs/$repo)"
    add ""
    add "$(printf '%s\n' "$new" | flag)"
    add ""
  fi
done

# 3. rusty_v8
current=$(awk '/^name = "v8"$/ { getline; gsub(/version = |"/, ""); print; exit }' "$here/Cargo.lock")
# Releases reviewed and not taken yet: `checked-v8 <version>`.
reviewed=$(awk '$1 == "checked-v8" { print $2 }' "$here/tools/upstream-sync.txt")
[ -n "$reviewed" ] && current=$reviewed
fetch rusty_v8 rusty_v8
newer=$(git -C "$work/rusty_v8" tag --sort=-v:refname | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | sed 's/^v//' |
  awk -v cur="$current" 'BEGIN { split(cur, c, ".") }
    { split($0, t, "."); if (t[1] > c[1] || (t[1] == c[1] && (t[2] > c[2] || (t[2] == c[2] && t[3] > c[3])))) print }')
if [ -n "$newer" ]; then
  add "### denoland/rusty_v8: releases newer than \`$current\`"
  add ""
  add "$(printf '%s\n' "$newer" | sed 's/^/- v/')"
  add ""
  add "V8 security fixes ship in these. Sync simienapp/rusty_v8, then bump \`v8\` in \`libs/deno_v8\`."
  add ""
fi

printf '%s' "$out"
