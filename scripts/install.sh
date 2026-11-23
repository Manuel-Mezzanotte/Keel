#!/bin/sh
# Installs Keel on this Mac. From https://keel.mezzalab.eu:
#
#   curl -fsSL https://keel.mezzalab.eu/install.sh | sh
#
# What it does, and nothing else:
#   1. checks that this is a Mac with Apple Silicon and macOS 13 or later;
#   2. downloads Keel's disk image over HTTPS;
#   3. checks its SHA-256 against the one written below, and stops if it
#      differs;
#   4. copies Keel.app into /Applications, or ~/Applications when
#      /Applications cannot be written; a Keel already there is replaced
#      only once the new one is complete;
#   5. removes what it downloaded and opens Keel.
#
# No password, no sudo, nothing left running. Updates come from Keel itself
# afterwards. Everything runs from the last line, so a script cut short by
# the network does nothing.

KEEL_VERSION="@VERSION@"
KEEL_URL="@URL@"
KEEL_SHA256="@SHA256@"

keel_install() {
  set -eu

  fail() {
    printf 'keel: %s\n' "$*" >&2
    exit 1
  }

  [ "$(uname -s)" = Darwin ] || fail "Keel runs only on macOS."
  [ "$(uname -m)" = arm64 ] || fail "Keel needs a Mac with Apple Silicon."
  major="$(sw_vers -productVersion | cut -d. -f1)"
  [ "$major" -ge 13 ] || fail "Keel needs macOS 13 or later; this Mac has $(sw_vers -productVersion)."

  dir="${KEEL_INSTALL_DIR:-/Applications}"
  if [ -z "${KEEL_INSTALL_DIR:-}" ] && [ ! -w /Applications ]; then
    dir="$HOME/Applications"
  fi
  mkdir -p "$dir"
  [ -w "$dir" ] || fail "$dir cannot be written."

  work="$(mktemp -d "${TMPDIR:-/tmp}/keel-install.XXXXXX")"
  mount="$work/disk"
  trap 'hdiutil detach -quiet "$mount" >/dev/null 2>&1 || true; rm -rf "$work"' EXIT INT TERM

  printf 'Downloading Keel %s…\n' "$KEEL_VERSION"
  curl --fail --silent --show-error --location --proto '=https,file' \
    --output "$work/Keel.dmg" "$KEEL_URL"
  sum="$(shasum -a 256 "$work/Keel.dmg" | cut -d' ' -f1)"
  [ "$sum" = "$KEEL_SHA256" ] ||
    fail "the download is not the published Keel (its SHA-256 differs). Nothing was installed."

  mkdir "$mount"
  hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mount" "$work/Keel.dmg"
  [ -d "$mount/Keel.app" ] || fail "the disk image holds no Keel.app. Nothing was installed."
  rm -rf "$dir/.Keel.app.new" "$dir/.Keel.app.old"
  ditto "$mount/Keel.app" "$dir/.Keel.app.new"
  if [ -e "$dir/Keel.app" ]; then
    mv "$dir/Keel.app" "$dir/.Keel.app.old"
  fi
  mv "$dir/.Keel.app.new" "$dir/Keel.app"
  rm -rf "$dir/.Keel.app.old"

  printf 'Keel %s is in %s.\n' "$KEEL_VERSION" "$dir"
  if [ "${KEEL_INSTALL_OPEN:-1}" = 1 ]; then
    open "$dir/Keel.app"
  fi
}

keel_install "$@"
