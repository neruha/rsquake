# Generates the app icons (assets/icon/icon.png, icon.ico, icon.icns).
# Only needed when the design changes; the outputs are committed.
#   powershell -File scripts/make-icons.ps1
Add-Type -AssemblyName System.Drawing

$outDir = Join-Path $PSScriptRoot "..\assets\icon"
New-Item -ItemType Directory -Force $outDir | Out-Null
$outDir = (Resolve-Path $outDir).Path

function New-IconBitmap([int]$size) {
    $bmp = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.Clear([System.Drawing.Color]::Transparent)
    $s = [double]$size

    # Rounded background, inset a little to match macOS icon margins
    $m = $s * 0.06
    $w = $s - 2 * $m
    $rad = $w * 0.22
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $path.AddArc($m, $m, $rad * 2, $rad * 2, 180, 90)
    $path.AddArc($m + $w - $rad * 2, $m, $rad * 2, $rad * 2, 270, 90)
    $path.AddArc($m + $w - $rad * 2, $m + $w - $rad * 2, $rad * 2, $rad * 2, 0, 90)
    $path.AddArc($m, $m + $w - $rad * 2, $rad * 2, $rad * 2, 90, 90)
    $path.CloseAllFigures()
    $bg = New-Object System.Drawing.Drawing2D.LinearGradientBrush `
        ([System.Drawing.PointF]::new(0, [single]$m)), ([System.Drawing.PointF]::new(0, [single]($m + $w))), `
        ([System.Drawing.Color]::FromArgb(255, 30, 41, 59)), ([System.Drawing.Color]::FromArgb(255, 15, 23, 42))
    $g.FillPath($bg, $path)

    # Ripples spreading from the epicenter
    $ox = $s / 2
    $oy = $s / 2
    $rings = @(
        @{ r = 0.34; a = 70;  t = 0.035 },
        @{ r = 0.24; a = 140; t = 0.045 },
        @{ r = 0.14; a = 230; t = 0.055 }
    )
    foreach ($ring in $rings) {
        $r = $s * $ring.r
        $pen = New-Object System.Drawing.Pen ([System.Drawing.Color]::FromArgb($ring.a, 251, 146, 60)), ([single]($s * $ring.t))
        $g.DrawEllipse($pen, [single]($ox - $r), [single]($oy - $r), [single]($r * 2), [single]($r * 2))
        $pen.Dispose()
    }
    $dot = $s * 0.075
    $red = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 239, 68, 68))
    $g.FillEllipse($red, [single]($ox - $dot), [single]($oy - $dot), [single]($dot * 2), [single]($dot * 2))

    $red.Dispose(); $bg.Dispose(); $path.Dispose(); $g.Dispose()
    return $bmp
}

function Get-PngBytes([int]$size) {
    $bmp = New-IconBitmap $size
    $ms = New-Object System.IO.MemoryStream
    $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
    return , $ms.ToArray()
}

function Write-BE32($bw, [uint32]$v) {
    $b = [BitConverter]::GetBytes($v)
    [Array]::Reverse($b)
    $bw.Write($b)
}

# PNG
[System.IO.File]::WriteAllBytes((Join-Path $outDir "icon.png"), (Get-PngBytes 1024))

# ICO (embedded PNG)
$icoSizes = 16, 24, 32, 48, 64, 128, 256
$images = @{}
foreach ($sz in $icoSizes) { $images[$sz] = Get-PngBytes $sz }
$fs = [System.IO.File]::Create((Join-Path $outDir "icon.ico"))
$bw = New-Object System.IO.BinaryWriter $fs
$bw.Write([uint16]0); $bw.Write([uint16]1); $bw.Write([uint16]$icoSizes.Count)
$offset = 6 + 16 * $icoSizes.Count
foreach ($sz in $icoSizes) {
    $dim = if ($sz -ge 256) { 0 } else { $sz }
    $bw.Write([byte]$dim); $bw.Write([byte]$dim); $bw.Write([byte]0); $bw.Write([byte]0)
    $bw.Write([uint16]1); $bw.Write([uint16]32)
    $bw.Write([uint32]$images[$sz].Length); $bw.Write([uint32]$offset)
    $offset += $images[$sz].Length
}
foreach ($sz in $icoSizes) { $bw.Write([byte[]]$images[$sz]) }
$bw.Close()

# ICNS (PNG-format chunks)
$icns = @(
    @{ type = "ic07"; size = 128 },
    @{ type = "ic08"; size = 256 },
    @{ type = "ic09"; size = 512 },
    @{ type = "ic10"; size = 1024 }
)
$total = 8
foreach ($e in $icns) { $e.data = Get-PngBytes $e.size; $total += 8 + $e.data.Length }
$fs = [System.IO.File]::Create((Join-Path $outDir "icon.icns"))
$bw = New-Object System.IO.BinaryWriter $fs
$bw.Write([System.Text.Encoding]::ASCII.GetBytes("icns"))
Write-BE32 $bw ([uint32]$total)
foreach ($e in $icns) {
    $bw.Write([System.Text.Encoding]::ASCII.GetBytes($e.type))
    Write-BE32 $bw ([uint32](8 + $e.data.Length))
    $bw.Write([byte[]]$e.data)
}
$bw.Close()

Write-Host "wrote icons to $outDir"
