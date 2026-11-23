#!/usr/bin/env bash
# macOS 用の配布物 (dist/RSQuake-<version>-macos-<arch>.dmg) を作る。
#   scripts/package-macos.sh              # 実行中のマシンの arch 向け
#   scripts/package-macos.sh universal    # arm64 + x86_64 の universal
# 署名は ad-hoc (Apple Developer ID が無いので初回は右クリック > 開く が必要)。
# Developer ID で署名するときは CODESIGN_IDENTITY="Developer ID Application: ..." を渡す。
set -euo pipefail

cd "$(dirname "$0")/.."

MODE="${1:-native}"
VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n1)"
APP_NAME="RSQuake"
BUNDLE_ID="me.neru.rsquake"
IDENTITY="${CODESIGN_IDENTITY:--}"

mkdir -p dist

if [ "$MODE" = "universal" ]; then
  rustup target add aarch64-apple-darwin x86_64-apple-darwin
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  BIN="dist/rsquake-universal"
  lipo -create -output "$BIN" \
    target/aarch64-apple-darwin/release/rsquake \
    target/x86_64-apple-darwin/release/rsquake
  ARCH="universal"
else
  cargo build --release
  BIN="target/release/rsquake"
  case "$(uname -m)" in
    arm64) ARCH="arm64" ;;
    *) ARCH="x86_64" ;;
  esac
fi

APP="dist/$APP_NAME.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/rsquake"
cp assets/icon/icon.icns "$APP/Contents/Resources/icon.icns"

cat > "$APP/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>$APP_NAME</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>rsquake</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.utilities</string>
</dict>
</plist>
EOF

codesign --force --deep --sign "$IDENTITY" "$APP"

DMG="dist/$APP_NAME-$VERSION-macos-$ARCH.dmg"
STAGE="$(mktemp -d)"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
rm -rf "$STAGE"
echo "created $DMG"
