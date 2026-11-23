#!/bin/bash
# Publishes a new version of Keel, in one go:
#
#   scripts/release.sh VERSION NOTES [--to DEST] [--base URL]
#
# VERSION is the version people read, such as 0.2.0; the build number goes
# up by one by itself. NOTES is a file with what is new, in two sections:
#
#   ## it
#   Le novità, in italiano.
#   ## en
#   What is new, in English.
#
# DEST is where the site's downloads are: by default the mini PC,
# minipc:/srv/keel-site/downloads, through the VPN. A plain folder, such as
# --to /tmp/keel-site, publishes there instead, to try a release without
# the site; then nothing is committed and the version files stay changed.
#
# In order: the release key (made the first time, kept in the keychain)
# goes into the app; version and build go up; Keel is built with its disk
# image and its .zip; the .zip is signed; updates.json and install.sh are
# written; the new files are uploaded first, the list and the script next,
# and only then the files of older versions are removed, keeping the
# previous one. Last, the published release is downloaded and checked as
# Keel would, and the new version is committed and tagged.
set -euo pipefail
cd "$(dirname "$0")/.."

die() {
  echo "release: $*" >&2
  exit 1
}

version="${1:-}"
notes="${2:-}"
[[ -n "$version" && -n "$notes" ]] || die "usage: scripts/release.sh VERSION NOTES [--to DEST] [--base URL]"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "the version is three numbers, such as 0.2.0"
[[ -f "$notes" ]] || die "no notes file at $notes"
shift 2
dest="minipc:/srv/keel-site/downloads"
base="https://keel.mezzalab.eu"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --to) dest="${2:?}"; shift 2 ;;
    --base) base="${2:?}"; shift 2 ;;
    *) die "unknown option: $1" ;;
  esac
done
if [[ "$dest" == *:* ]]; then
  remote=1
  host="${dest%%:*}"
  path="${dest#*:}"
else
  remote=0
  mkdir -p "$dest"
  dest="$(cd "$dest" && pwd)"
  # A folder of one's own is reached as a file, not through the site.
  [[ "$base" == https://keel.mezzalab.eu ]] && base="file://$dest"
fi
if [[ $remote == 1 && -n "$(git status --porcelain --untracked-files=no)" ]]; then
  die "commit or put aside your changes first: the release is committed by itself"
fi

echo "== The release key"
cargo build --release --quiet -p update --bin keel-release
tool="target/release/keel-release"
public="$("$tool" key)"
perl -pi -e "s/^pub const PUBLIC_KEY: &str = \"[0-9a-f]*\";/pub const PUBLIC_KEY: &str = \"$public\";/" crates/update/src/lib.rs
grep -q "PUBLIC_KEY: &str = \"$public\"" crates/update/src/lib.rs || die "the key did not go into crates/update/src/lib.rs"

echo "== Version and build"
old_build="$(sed -n 's/^pub const BUILD: u64 = \([0-9]*\);/\1/p' crates/keel/src/version.rs)"
build=$((old_build + 1))
perl -0pi -e "s/^version = \"[^\"]*\"/version = \"$version\"/m" Cargo.toml
perl -pi -e "s/^pub const BUILD: u64 = \d+;/pub const BUILD: u64 = $build;/" crates/keel/src/version.rs
echo "Keel $version, build $build"

echo "== Building"
scripts/make-dmg.sh
dmg="target/release/Keel-$version.dmg"
zip="target/release/Keel-$version.zip"
rm -f "$zip"
ditto -c -k --keepParent target/release/Keel.app "$zip"

echo "== The list of releases and the install script"
out="target/publish/$version"
rm -rf "$out"
mkdir -p "$out"
previous=""
if [[ $remote == 1 ]]; then
  if ssh "$host" "test -f '$path/updates.json'"; then
    ssh "$host" "cat '$path/updates.json'" > "$out/previous.json"
    previous="$out/previous.json"
  fi
elif [[ -f "$dest/updates.json" ]]; then
  cp "$dest/updates.json" "$out/previous.json"
  previous="$out/previous.json"
fi
"$tool" stage --out "$out/site" --version "$version" --build "$build" \
  --zip "$zip" --dmg "$dmg" --notes "$notes" --base "$base" \
  --installer scripts/install.sh ${previous:+--previous "$previous"} > "$out/keep"

echo "== Uploading"
if [[ $remote == 1 ]]; then
  ssh "$host" "mkdir -p '$path/releases'"
  rsync -a "$out/site/releases/" "$host:$path/releases/"
  rsync -a "$out/site/updates.json" "$out/site/install.sh" "$host:$path/"
  ssh "$host" "cat > '$path/.keep'" < "$out/keep"
  ssh "$host" "cd '$path/releases' && for file in *; do grep -qxF \"\$file\" ../.keep || rm -f -- \"\$file\"; done; rm -f ../.keep"
else
  mkdir -p "$dest/releases"
  cp "$out/site/releases/"* "$dest/releases/"
  cp "$out/site/updates.json" "$out/site/install.sh" "$dest/"
  for file in "$dest/releases/"*; do
    grep -qxF "$(basename "$file")" "$out/keep" || rm -f -- "$file"
  done
fi

echo "== Checking what was published"
"$tool" check "$base/updates.json"

if [[ $remote == 1 ]]; then
  git add Cargo.toml Cargo.lock crates/keel/src/version.rs crates/update/src/lib.rs
  git commit -q -m "chore(release): $version (build $build)"
  git tag "v$version"
  echo "Keel $version is out. Committed and tagged v$version; push when you want."
else
  echo "Keel $version is published in $dest. Nothing committed: the version files are changed."
fi
