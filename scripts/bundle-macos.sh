#!/bin/bash
# Builds Keel and wraps it in Keel.app with its icon, for trying it on this
# Mac like any other app (Dock, Finder, Spotlight). It is not signed for
# others and not notarized: publishing comes at the end of the plan.
#
# Usage: scripts/bundle-macos.sh        (release build)
#        scripts/bundle-macos.sh --skip-build
#
# The app lands in target/release/Keel.app. Running it again replaces the
# files inside the app; nothing is deleted.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if [[ "${1:-}" != "--skip-build" ]]; then
  cargo build --release -p keel
fi

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
app="target/release/Keel.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
# A new file renamed over the old one: a Keel already running from this
# app keeps its own copy until it quits, instead of crashing.
cp target/release/keel "$app/Contents/MacOS/keel.new"
mv -f "$app/Contents/MacOS/keel.new" "$app/Contents/MacOS/keel"
cp assets/brand/Keel.icns "$app/Contents/Resources/Keel.icns"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>Keel</string>
  <key>CFBundleDisplayName</key>
  <string>Keel</string>
  <key>CFBundleIdentifier</key>
  <string>com.manuelmezzanotte.keel</string>
  <key>CFBundleExecutable</key>
  <string>keel</string>
  <key>CFBundleIconFile</key>
  <string>Keel</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${version}</string>
  <key>CFBundleVersion</key>
  <string>${version}</string>
  <key>CFBundleDevelopmentRegion</key>
  <string>it</string>
  <key>CFBundleLocalizations</key>
  <array>
    <string>it</string>
    <string>en</string>
  </array>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.developer-tools</string>
  <key>LSMinimumSystemVersion</key>
  <string>13.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key>
  <true/>
  <!-- Finder's "Open With": text, code and folders. -->
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key>
      <string>Text and code</string>
      <key>CFBundleTypeRole</key>
      <string>Editor</string>
      <key>LSHandlerRank</key>
      <string>Alternate</string>
      <key>LSItemContentTypes</key>
      <array>
        <string>public.text</string>
        <string>public.source-code</string>
        <string>public.data</string>
      </array>
    </dict>
    <dict>
      <key>CFBundleTypeName</key>
      <string>Folder</string>
      <key>CFBundleTypeRole</key>
      <string>Viewer</string>
      <key>LSHandlerRank</key>
      <string>Alternate</string>
      <key>LSItemContentTypes</key>
      <array>
        <string>public.folder</string>
      </array>
    </dict>
  </array>
</dict>
</plist>
PLIST

# An ad-hoc signature, enough to run on this Mac.
codesign --force --sign - "$app" >/dev/null 2>&1
# Tell Finder and the Dock the icon may have changed.
touch "$app"
echo "Keel.app is ready: $root/$app"
