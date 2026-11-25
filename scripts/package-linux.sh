#!/usr/bin/env bash
# Linux 用の配布物 (dist/RSQuake-<version>-linux-<arch>.tar.gz) を作る。
#   scripts/package-linux.sh              # 実行中のマシンの arch 向け
#   scripts/package-linux.sh x86_64       # x86_64 のみ
#   scripts/package-linux.sh aarch64      # aarch64 (arm64) のみ
# arch ごとにその arch のマシンでビルドする前提 (GitHub Actions では arch ごとのランナーを使う)。
# クロスビルドするときは、リンカーと、その arch の libasound2 の開発ファイルを自分で用意する。
#
# ビルドに必要なパッケージ (Debian / Ubuntu): build-essential pkg-config libasound2-dev
set -euo pipefail

cd "$(dirname "$0")/.."

ARCH="${1:-native}"
if [ "$ARCH" = "native" ]; then
  ARCH="$(uname -m)"
fi
case "$ARCH" in
  x86_64 | amd64) ARCH="x86_64"; TARGET="x86_64-unknown-linux-gnu" ;;
  aarch64 | arm64) ARCH="aarch64"; TARGET="aarch64-unknown-linux-gnu" ;;
  *)
    echo "usage: $0 [native|x86_64|aarch64]" >&2
    exit 1
    ;;
esac

VERSION="$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -n1)"
NAME="RSQuake-$VERSION-linux-$ARCH"

rustup target add "$TARGET" 2>/dev/null || true
cargo build --release --locked --target "$TARGET"

STAGE="dist/$NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE"
install -m 755 "target/$TARGET/release/rsquake" "$STAGE/rsquake"
install -m 644 assets/icon/icon.png "$STAGE/rsquake.png"
install -m 644 LICENSE "$STAGE/LICENSE"

# メニューに出すためのランチャー。使うときは Exec を rsquake の置き場所に書き換えて、
# ~/.local/share/applications に置く。
cat > "$STAGE/rsquake.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=RSQuake
Comment=Earthquake early warning viewer
Exec=rsquake
Icon=rsquake
Terminal=false
Categories=Utility;
StartupWMClass=rsquake
EOF

TARBALL="dist/$NAME.tar.gz"
rm -f "$TARBALL"
tar -C dist -czf "$TARBALL" "$NAME"
rm -rf "$STAGE"
echo "created $TARBALL"
