#!/bin/bash
# Builds Keel.app (see bundle-macos.sh) and puts it in a disk image to
# install it like any Mac app: open the DMG, drag Keel onto Applications.
#
# Usage: scripts/make-dmg.sh               (release build)
#        scripts/make-dmg.sh --skip-build
#
# The image lands in target/release/Keel-<version>.dmg, replacing an older
# one of the same version. The app has an ad-hoc signature only: on this Mac
# it opens as is; a copy downloaded from the internet needs right click →
# Open the first time, since it is not notarized by Apple.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

scripts/bundle-macos.sh "$@"

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
dmg="target/release/Keel-${version}.dmg"

# What the window of the disk image shows: the app and Applications.
staging="$(mktemp -d "${TMPDIR:-/tmp}/keel-dmg.XXXXXX")"
ditto target/release/Keel.app "$staging/Keel.app"
ln -s /Applications "$staging/Applications"

hdiutil create -quiet -ov \
  -volname "Keel ${version}" \
  -srcfolder "$staging" \
  -fs HFS+ \
  -format UDZO \
  "$dmg"
codesign --force --sign - "$dmg" >/dev/null 2>&1 || true
echo "The disk image is ready: $root/$dmg ($(du -h "$dmg" | cut -f1))"
