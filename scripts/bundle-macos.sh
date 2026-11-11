#!/usr/bin/env bash
# Builds a universal (Apple Silicon + Intel) "Nightscout Glance.app" and zips it.
# Run on macOS from the repository root. Output: dist/NightscoutGlance-macos.zip
set -euo pipefail

VERSION=$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)".*/\1/')
APP="dist/Nightscout Glance.app"
BIN=nightscout-glance

# Fat LTO fails to load bitcode for one of the macOS-only dependencies (muda).
export CARGO_PROFILE_RELEASE_LTO=off

rustup target add aarch64-apple-darwin x86_64-apple-darwin
cargo build --release --locked --target aarch64-apple-darwin
cargo build --release --locked --target x86_64-apple-darwin

rm -rf dist
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/$BIN" \
  "target/aarch64-apple-darwin/release/$BIN" \
  "target/x86_64-apple-darwin/release/$BIN"

# App icon
ICONSET=dist/AppIcon.iconset
mkdir -p "$ICONSET"
for size in 16 32 128; do
  sips -z $size $size assets/icon.png --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) assets/icon.png --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
sips -z 256 256 assets/icon.png --out "$ICONSET/icon_256x256.png" >/dev/null
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$ICONSET"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>Nightscout Glance</string>
    <key>CFBundleDisplayName</key><string>Nightscout Glance</string>
    <key>CFBundleIdentifier</key><string>com.burnedikt.nightscout-glance</string>
    <key>CFBundleExecutable</key><string>$BIN</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$VERSION</string>
    <key>CFBundleVersion</key><string>$VERSION</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <key>LSUIElement</key><true/>
    <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF

# Ad-hoc signature: enough to run locally, not notarized.
codesign --force --deep --sign - "$APP"
ditto -c -k --keepParent "$APP" dist/NightscoutGlance-macos.zip
echo "Built dist/NightscoutGlance-macos.zip ($VERSION)"
