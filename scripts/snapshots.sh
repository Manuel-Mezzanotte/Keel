#!/bin/sh
# Draws Keel's window off screen into PNG files, in both interface styles
# and in Mezzanotte and Chiaro, to look at the interface without opening
# anything on screen. The window uses a data folder of its own, with the
# language servers off, and never the real one.
#
#   scripts/snapshots.sh <out-folder> [project] [file]
#
# KEEL_SNAPSHOTS_ONLY=scm,flat-chiaro takes only the pictures whose names
# hold one of the words.
set -eu

out="${1:?usage: scripts/snapshots.sh <out-folder> [project] [file]}"
root="$(cd "$(dirname "$0")/.." && pwd)"
project="${2:-$root}"
file="${3:-$root/crates/keel/src/layout.rs}"

data="$(mktemp -d "${TMPDIR:-/tmp}/keel-snapshots.XXXXXX")"
trap 'rm -rf "$data"' EXIT
cat > "$data/settings.json" <<'JSON'
{
  "keel.language": "it",
  "keel.languageServers.autoInstall": false,
  "keel.languageServers.disabled": [
    "rust-analyzer", "vtsls", "eslint", "tailwindcss", "json", "css", "html",
    "emmet", "yaml", "taplo", "marksman", "bash", "shellcheck", "shfmt",
    "dockerfile", "basedpyright", "ruff", "clangd", "jdtls", "lemminx",
    "csharp-ls", "sqruff", "prisma", "spring-boot"
  ],
  "extensions.autoCheckUpdates": false
}
JSON

cd "$root"
cargo build -q -p keel --features snapshots
if [ -f fixtures/extensions/vsauto-ghost-local-0.6.1-editor-layout.vsix ]; then
  KEEL_DATA_DIR="$data" target/debug/keel \
    --install-extension fixtures/extensions/vsauto-ghost-local-0.6.1-editor-layout.vsix >/dev/null
fi
mkdir -p "$out"
KEEL_DATA_DIR="$data" KEEL_SNAPSHOTS="$out" target/debug/keel "$project" "$file"
