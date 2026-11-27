"""LINE Seed JP Bold を、震央地名と UI の文言で使う文字だけに絞って assets/fonts に出力する。

必要: python と fontTools (pip install fonttools brotli)

収録するもの:
  - 気象庁「地震情報で用いる震央地名」全 331 件の文字
    (0Quake/JMA_Region の震央地名.geojson、CC0。気象庁は一覧にない震央地名は使わない)
  - UI の文言 (info_panel.rs / widgets.rs / report.rs に書かれた日本語)
  - ASCII、かな・カナ、全角記号 (数値や補足の回り込み用)
収録外の文字は theme.rs の with_jp により Gen Interface JP Regular で描かれる。
ヒンティングは残す。
震央地名が改定されたら、このスクリプトを再実行する。
"""
import json
import os
import tempfile
import urllib.request

from fontTools import subset

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SRC = os.path.join(ROOT, "assets", "fonts", "LINESeedJP-Bold.ttf")
CACHE = os.path.join(tempfile.gettempdir(), "lineseed-subset")
REGION_URL = "https://raw.githubusercontent.com/0Quake/JMA_Region/main/%E9%9C%87%E5%A4%AE%E5%9C%B0%E5%90%8D.geojson"
UI_FILES = ["src/ui/info_panel.rs", "src/ui/widgets.rs", "src/eew/report.rs"]

BASE = (
    set(range(0x20, 0x7F))  # ASCII
    | {0xB7}
    | set(range(0x2010, 0x2016))
    | set(range(0x2018, 0x2020))
    | {0x2026}
    | set(range(0x3000, 0x3100))  # CJK 記号・かな
    | set(range(0xFF01, 0xFF5F))  # 全角形
)


def region_chars() -> set[int]:
    path = os.path.join(CACHE, "region.geojson")
    if not os.path.exists(path):
        os.makedirs(CACHE, exist_ok=True)
        urllib.request.urlretrieve(REGION_URL, path)
    with open(path, encoding="utf-8") as f:
        features = json.load(f)["features"]
    return {ord(c) for ft in features for c in ft["properties"]["name"]}


def ui_chars() -> set[int]:
    chars = set()
    for rel in UI_FILES:
        with open(os.path.join(ROOT, rel), encoding="utf-8") as f:
            chars |= {ord(c) for c in f.read() if ord(c) > 0x7F and ord(c) != 0xFEFF}
    return chars


def main() -> None:
    opt = subset.Options()
    opt.layout_features = ["*"]
    opt.hinting = True
    opt.name_IDs = ["*"]
    opt.notdef_outline = True
    font = subset.load_font(SRC, opt)
    s = subset.Subsetter(opt)
    s.populate(unicodes=sorted(BASE | region_chars() | ui_chars()))
    s.subset(font)
    subset.save_font(font, SRC, opt)
    print(f"{os.path.basename(SRC)}: {os.path.getsize(SRC):,} bytes ({len(font.getBestCmap())} chars)")


if __name__ == "__main__":
    main()
