#!/bin/bash
# Copies a newer Material Icon Theme into Keel: its list of which icon goes
# to which file, its SVG icons and its licence, from an installed copy of
# the extension (VS Code's, Cursor's or Keel's own extensions folder).
#
# Usage: scripts/update-material-icons.sh <extension folder>
#   e.g. scripts/update-material-icons.sh ~/.vscode/extensions/pkief.material-icon-theme-5.41.0
#
# Icons the new version no longer has stay in Keel's copy: nothing is
# deleted, and the list never points at them. Afterwards: the version in
# the notices is updated here, then scripts/third-party-licenses.py writes
# them again, and the tests and scripts/snapshots.sh show the result.
set -euo pipefail

source="${1:?usage: scripts/update-material-icons.sh <extension folder>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
target="$root/crates/keel/icons/material"

if [[ ! -f "$source/dist/material-icons.json" || ! -d "$source/icons" ]]; then
  echo "No Material Icon Theme in $source: dist/material-icons.json and icons/ are missing." >&2
  exit 1
fi
version="$(grep -o '"version": *"[^"]*"' "$source/package.json" | head -1 | sed 's/.*"\([^"]*\)"$/\1/')"
if [[ -z "$version" ]]; then
  echo "No version in $source/package.json." >&2
  exit 1
fi
licence=""
for name in LICENSE LICENSE.md LICENSE.txt; do
  if [[ -f "$source/$name" ]]; then
    licence="$source/$name"
    break
  fi
done
if [[ -z "$licence" ]]; then
  echo "No licence in $source." >&2
  exit 1
fi

mkdir -p "$target/dist" "$target/icons"
cp "$source/dist/material-icons.json" "$target/dist/material-icons.json"
cp "$source"/icons/*.svg "$target/icons/"
cp "$licence" "$target/LICENSE.txt"
# The version named in the notices.
sed -i '' "s/Material Icon Theme [0-9][0-9.]* (file icons)/Material Icon Theme $version (file icons)/" \
  "$root/scripts/third-party-licenses.py"

echo "Material Icon Theme $version copied: $(ls "$source"/icons/*.svg | wc -l | tr -d ' ') icons."
echo "Next: python3 scripts/third-party-licenses.py, then the tests."
