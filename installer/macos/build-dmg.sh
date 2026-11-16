#!/usr/bin/env bash
# Builds a universal (Apple Silicon + Intel) LightScio.app and
# target/installer/LightScio-<version>-macOS.dmg.
# Version: LIGHTSCIO_VERSION env var, else the latest git tag (v1.2.3 -> 1.2.3), else Cargo.toml.
set -euo pipefail
cd "$(dirname "$0")/../.."

VERSION="${LIGHTSCIO_VERSION:-}"
if [ -z "$VERSION" ]; then
  VERSION="$(git describe --tags --abbrev=0 2>/dev/null | sed 's/^[vV]//' || true)"
fi
if [ -z "$VERSION" ]; then
  VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
fi
export LIGHTSCIO_VERSION="$VERSION"
echo "LightScio $VERSION"

rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin

STAGE="target/dmg"
APP="$STAGE/LightScio.app"
rm -rf "$STAGE"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" target/installer
lipo -create -output "$APP/Contents/MacOS/lightscio" \
  target/aarch64-apple-darwin/release/lightscio \
  target/x86_64-apple-darwin/release/lightscio
cp README.md LICENSE "$APP/Contents/Resources/"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>LightSciò</string>
  <key>CFBundleDisplayName</key><string>LightSciò</string>
  <key>CFBundleIdentifier</key><string>io.github.thomasborgogno.lightscio</string>
  <key>CFBundleExecutable</key><string>lightscio</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.music</string>
</dict>
</plist>
EOF

# ad-hoc signature (needed to run on Apple Silicon; not a Developer ID signature)
codesign --force --deep -s - "$APP"

ln -s /Applications "$STAGE/Applications"
DMG="target/installer/LightScio-${VERSION}-macOS.dmg"
rm -f "$DMG"
hdiutil create -volname "LightScio ${VERSION}" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
echo "Created $DMG"
