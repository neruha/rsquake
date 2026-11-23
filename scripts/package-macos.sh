#!/usr/bin/env bash
# macOS 用の配布物 (dist/RSQuake-<version>-macos-<arch>.dmg) を作る。
#   scripts/package-macos.sh              # 実行中のマシンの arch 向け
#   scripts/package-macos.sh arm64        # arm64 のみ (クロスビルド可)
#   scripts/package-macos.sh x86_64       # x86_64 (Intel) のみ (クロスビルド可)
#   scripts/package-macos.sh both         # arm64 と x86_64 を別々の dmg で作る
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

# package <binary> <arch ラベル>: .app を組み立てて dmg にする
package() {
  local BIN="$1" ARCH="$2"
  local APP="dist/$APP_NAME.app"
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

  local DMG="dist/$APP_NAME-$VERSION-macos-$ARCH.dmg"
  local STAGE
  STAGE="$(mktemp -d)"
  cp -R "$APP" "$STAGE/"
  ln -s /Applications "$STAGE/Applications"
  rm -f "$DMG"
  hdiutil create -volname "$APP_NAME" -srcfolder "$STAGE" -ov -format UDZO "$DMG"
  rm -rf "$STAGE"
  echo "created $DMG"
}

# build_target <rust target> <arch ラベル>: 1 arch 分をビルドして dmg 化
build_target() {
  local TARGET="$1" ARCH="$2"
  rustup target add "$TARGET"
  cargo build --release --target "$TARGET"
  package "target/$TARGET/release/rsquake" "$ARCH"
}

case "$MODE" in
  universal)
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    cargo build --release --target aarch64-apple-darwin
    cargo build --release --target x86_64-apple-darwin
    lipo -create -output dist/rsquake-universal \
      target/aarch64-apple-darwin/release/rsquake \
      target/x86_64-apple-darwin/release/rsquake
    package dist/rsquake-universal universal
    ;;
  both)
    build_target aarch64-apple-darwin arm64
    build_target x86_64-apple-darwin x86_64
    ;;
  arm64)  build_target aarch64-apple-darwin arm64 ;;
  x86_64) build_target x86_64-apple-darwin x86_64 ;;
  native)
    cargo build --release
    case "$(uname -m)" in
      arm64) ARCH="arm64" ;;
      *) ARCH="x86_64" ;;
    esac
    package target/release/rsquake "$ARCH"
    ;;
  *)
    echo "usage: $0 [native|arm64|x86_64|both|universal]" >&2
    exit 1
    ;;
esac