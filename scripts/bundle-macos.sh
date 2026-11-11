#!/usr/bin/env bash
# Builds a universal (Apple Silicon + Intel) "Nightscout Glance.app" and zips it.
# Run on macOS from the repository root. Output: dist/NightscoutGlance-macos.zip
#
# Signing (all optional; without them the app gets an ad-hoc signature):
#   MACOS_SIGN_IDENTITY  "Developer ID Application: Name (TEAMID)", must be in the keychain
#   APPLE_API_KEY_PATH, APPLE_API_KEY_ID, APPLE_API_ISSUER_ID
#                        App Store Connect API key; when set, the app is notarized and stapled
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
SRC=assets/brand/icon-macos-1024.png
for size in 16 32 128 256 512; do
  sips -z $size $size "$SRC" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) "$SRC" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
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

ZIP=dist/NightscoutGlance-macos.zip
if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
  # Developer ID signature with the hardened runtime, as notarization requires.
  codesign --force --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$APP"
  codesign --verify --strict --verbose=2 "$APP"
  SIGNED="signed"
else
  # Ad-hoc signature: enough to run locally, not notarized.
  codesign --force --deep --sign - "$APP"
  SIGNED="ad-hoc signed"
fi
ditto -c -k --keepParent "$APP" "$ZIP"

if [ -n "${MACOS_SIGN_IDENTITY:-}" ] && [ -n "${APPLE_API_KEY_PATH:-}" ]; then
  xcrun notarytool submit "$ZIP" --wait     --key "$APPLE_API_KEY_PATH" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER_ID"
  # Attach the ticket so the app opens without a network check, then zip the stapled app.
  xcrun stapler staple "$APP"
  rm "$ZIP"
  ditto -c -k --keepParent "$APP" "$ZIP"
  SIGNED="signed and notarized"
fi
echo "Built $ZIP ($VERSION, $SIGNED)"
