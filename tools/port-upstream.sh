#!/bin/sh
# Port upstream Deno commits into this repo, renamed to the simien_* names.
#
#   tools/port-upstream.sh <deno-checkout> <commit|range>
#       commits from denoland/deno, limited to tools/upstream-paths.txt
#   tools/port-upstream.sh <checkout> <commit|range> libs/<name>
#       commits from one of the standalone repos (denoland/deno_error, …),
#       applied under libs/<name>
#
# Each commit keeps its author and message, is renamed by tools/rename.py, and
# is applied with a three-way merge (git am -3). A conflict stops the port the
# way git am does: fix it, then `git am --continue`.
set -eu
here=$(cd "$(dirname "$0")/.." && pwd)
src=$1
range=$2
prefix=${3:-}

case "$range" in
  *..*) ;;
  *) range="$range^!" ;;
esac

if [ -n "$prefix" ]; then
  patches=$(git -C "$src" format-patch --stdout "$range")
  directory="--directory=$prefix"
else
  paths=$(grep -v '^#' "$here/tools/upstream-paths.txt" | grep -v '^$')
  # shellcheck disable=SC2086 # one path per word, on purpose
  patches=$(git -C "$src" format-patch --stdout "$range" -- $paths)
  directory=""
fi

if [ -z "$patches" ]; then
  echo "nothing in $range touches the paths kept here" >&2
  exit 0
fi

printf '%s\n' "$patches" | python3 "$here/tools/rename.py" patch | git -C "$here" -c core.hooksPath=/dev/null am -3 $directory
