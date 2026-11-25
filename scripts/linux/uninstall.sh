#!/usr/bin/env bash
# install.sh で入れた RSQuake を消す。設定ファイル (~/.config/rsquake) は残す。
#   ./uninstall.sh
#   PREFIX=/opt/rsquake ./uninstall.sh   # install.sh と同じ PREFIX を渡す
set -euo pipefail

PREFIX="${PREFIX:-$HOME/.local}"

rm -f "$PREFIX/bin/rsquake"
rm -f "$PREFIX/share/applications/rsquake.desktop"
rm -rf "$PREFIX/share/rsquake"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$PREFIX/share/applications" >/dev/null 2>&1 || true
fi

echo "削除しました。設定ファイルは残しています (消すなら ~/.config/rsquake)。"
