#!/usr/bin/env bash
# RSQuake をユーザー単位 (管理者権限なし) で入れ、アプリ一覧 (GNOME / KDE など) に出す。
#   ./install.sh                 # ~/.local に入れる
#   PREFIX=/opt/rsquake ./install.sh   # 別の場所に入れる
# 展開したフォルダの中で実行する。消すときは uninstall.sh。
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
PREFIX="${PREFIX:-$HOME/.local}"

BIN_DIR="$PREFIX/bin"
DATA_DIR="$PREFIX/share/rsquake"
APPS_DIR="$PREFIX/share/applications"

for f in rsquake rsquake.png; do
  if [ ! -f "$HERE/$f" ]; then
    echo "$HERE/$f が見つかりません。展開したフォルダの中で実行してください。" >&2
    exit 1
  fi
done

mkdir -p "$BIN_DIR" "$DATA_DIR" "$APPS_DIR"
install -m 755 "$HERE/rsquake" "$BIN_DIR/rsquake"
install -m 644 "$HERE/rsquake.png" "$DATA_DIR/rsquake.png"
install -m 644 "$HERE/LICENSE" "$DATA_DIR/LICENSE"

# アイコンは 1024px の PNG で、標準のアイコンテーマの置き場所には合わないので、絶対パスで指す。
# ファイル名は、ウィンドウの app_id (rsquake) に合わせる。GNOME はこれでウィンドウとアイコンを結びつける。
cat > "$APPS_DIR/rsquake.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=RSQuake
Comment=Earthquake early warning viewer
Exec="$BIN_DIR/rsquake"
Icon=$DATA_DIR/rsquake.png
Terminal=false
Categories=Utility;
StartupWMClass=rsquake
EOF
chmod 644 "$APPS_DIR/rsquake.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$APPS_DIR" >/dev/null 2>&1 || true
fi

echo "インストールしました: $BIN_DIR/rsquake"
echo "アプリ一覧から「RSQuake」を起動できます (出ないときは、いちどログインし直してください)。"
case ":$PATH:" in
  *":$BIN_DIR:"*) ;;
  *) echo "注意: $BIN_DIR が PATH に入っていません。ターミナルから rsquake と打つときは PATH に足してください。" ;;
esac
