# M PLUS 1p (Light / Regular / Medium) を、UI で使う文字だけに絞って assets/fonts に出力する。
# 必要: python と fontTools (pip install fonttools brotli)
#
# 含まれない文字は theme.rs の with_jp で Gen Interface JP に落ちる。
# info_panel.rs で各フォントを使う文字列を変えたら、$weights の text に足して再実行する。
# ヒンティングは残す。
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot

# ASCII・かな・全角記号は共通で入れる (回り込みや将来の文言変更に備える)
$ranges = "U+0020-007E,U+00B7,U+2010-2015,U+2018-201F,U+2026,U+3000-303F,U+3041-3096,U+30A0-30FF,U+FF01-FF5E"

$weights = @(
    # mplus_light_font: 発生・検知時刻 (「2026/01/01 12:00:00 発生」「不明」)
    @{ name = "Light"; text = "発生検知不明" },
    # mplus_regular_font: 待機表示、震源の補足、値の見出し
    @{ name = "Regular"; text = "緊急地震速報は発表されていません（仮定震源）（精度低）で地震規模深さ" },
    # mplus_medium_font: 情報パネルのタイトル、規模と深さの値 (「M8超」「700km以上」「ごく浅い」)
    @{ name = "Medium"; text = "緊急地震速報（取消）（警報）（予報）規模超深さ以上ごく浅い不明" }
)

foreach ($w in $weights) {
    $file = "MPLUS1p-$($w.name).ttf"
    $src = Join-Path ([IO.Path]::GetTempPath()) "MPLUS1p-$($w.name).full.ttf"
    $out = Join-Path $root "assets/fonts/$file"

    if (-not (Test-Path $src)) {
        Invoke-WebRequest "https://github.com/google/fonts/raw/main/ofl/mplus1p/$file" -OutFile $src
    }

    $textFile = Join-Path ([IO.Path]::GetTempPath()) "mplus-$($w.name)-subset.txt"
    [IO.File]::WriteAllText($textFile, $w.text, [Text.UTF8Encoding]::new($false))

    python -m fontTools.subset $src `
        --unicodes=$ranges `
        --text-file=$textFile `
        --layout-features='*' `
        --hinting `
        --name-IDs='*' `
        --notdef-outline `
        --output-file=$out
    if ($LASTEXITCODE -ne 0) { throw "subset failed: $file" }

    "{0}: {1:N0} bytes" -f $file, (Get-Item $out).Length
}
