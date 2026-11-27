"""Gen Interface JP Bold を JIS X 0208 (第1・第2水準) 中心に絞って assets/fonts に出力する。

必要: python と fontTools (pip install fonttools brotli)

収録するもの:
  - JIS X 0208 の全文字 (第1・第2水準の漢字と、かな・記号・英数・ギリシャ・キリル・罫線)
  - Inter 由来のラテン文字と、一般句読点・矢印・囲み数字・全角形などの記号
  - CP932 拡張 (NEC/IBM 拡張: 髙 﨑 ① ㈱ など、人名・地名で見かける文字)
収録外の文字は theme.rs の with_jp により Gen Interface JP Regular で描かれる。
ヒンティングは残す (元のフォントに hinting テーブルは無い)。
"""
import os
import tempfile
import urllib.request
import zipfile

from fontTools import subset

VERSION = "0.8.0"  # assets/fonts の元ファイルと同一のリリース
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUT = os.path.join(ROOT, "assets", "fonts", "GenInterfaceJP-Bold.ttf")
CACHE = os.path.join(tempfile.gettempdir(), "geninterface")
ZIP_PATH = os.path.join(CACHE, f"GenInterfaceJP-{VERSION}.zip")
MEMBER = f"GenInterfaceJP-{VERSION}/Gen Interface JP/GenInterfaceJP-Bold.ttf"


def fetch_source() -> str:
    src = os.path.join(CACHE, f"GenInterfaceJP-Bold-{VERSION}.full.ttf")
    if os.path.exists(src):
        return src
    os.makedirs(CACHE, exist_ok=True)
    if not os.path.exists(ZIP_PATH):
        url = f"https://github.com/yamatoiizuka/gen-interface-jp/releases/download/v{VERSION}/GenInterfaceJP-{VERSION}.zip"
        urllib.request.urlretrieve(url, ZIP_PATH)
    with zipfile.ZipFile(ZIP_PATH) as z, open(src, "wb") as f:
        f.write(z.read(MEMBER))
    return src


def jis0208() -> set[int]:
    chars = set()
    for r in range(0xA1, 0xFF):
        for c in range(0xA1, 0xFF):
            try:
                t = bytes([r, c]).decode("euc_jp")
            except UnicodeDecodeError:
                continue
            if len(t) == 1:
                chars.add(ord(t))
    return chars


def cp932_double_byte() -> set[int]:
    chars = set()
    for b1 in list(range(0x81, 0xA0)) + list(range(0xE0, 0xFD)):
        for b2 in range(0x40, 0xFD):
            try:
                t = bytes([b1, b2]).decode("cp932")
            except UnicodeDecodeError:
                continue
            if len(t) == 1:
                chars.add(ord(t))
    return chars


BASE = (
    set(range(0x20, 0x250))  # ASCII, Latin-1, ラテン拡張 (Inter)
    | set(range(0x2000, 0x2070))  # 一般句読点
    | set(range(0x2190, 0x2200))  # 矢印・数学記号
    | set(range(0x2460, 0x2500))  # 囲み数字
    | set(range(0x3000, 0x3100))  # CJK 記号・かな
    | set(range(0xFF00, 0xFFF0))  # 全角形
)


def main() -> None:
    unicodes = BASE | jis0208() | cp932_double_byte()
    opt = subset.Options()
    opt.layout_features = ["*"]
    opt.hinting = True
    opt.name_IDs = ["*"]
    opt.notdef_outline = True
    font = subset.load_font(fetch_source(), opt)
    s = subset.Subsetter(opt)
    s.populate(unicodes=sorted(unicodes))
    s.subset(font)
    subset.save_font(font, OUT, opt)
    print(f"{os.path.basename(OUT)}: {os.path.getsize(OUT):,} bytes ({len(font.getBestCmap())} chars)")


if __name__ == "__main__":
    main()
