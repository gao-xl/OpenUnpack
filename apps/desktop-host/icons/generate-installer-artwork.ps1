# generate-installer-artwork.ps1
# Generates NSIS installer visual assets (BMP) for OpenUnpack.
# Brand palette (aligned with apps/desktop-host/icons/icon.png):
#   background deep teal-green #173A3A, cube mint #4EC9A4, flat style, no gradients.
# Output: installer-sidebar.bmp (164x314) and installer-header.bmp (150x57) + PNG previews.
# Run with pwsh 7. ASCII-only script body (safe for any host encoding).

param(
    [string]$OutDir = $PSScriptRoot
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

# ---- Brand palette -------------------------------------------------------
$colorBg      = [System.Drawing.Color]::FromArgb(0x17, 0x3A, 0x3A) # deep teal-green
$colorBgSoft  = [System.Drawing.Color]::FromArgb(0x1E, 0x4A, 0x49) # subtle decor on dark bg
$colorCubeTop = [System.Drawing.Color]::FromArgb(0x4E, 0xC9, 0xA4) # mint cube top
$colorCubeL   = [System.Drawing.Color]::FromArgb(0x35, 0x80, 0x6A) # cube left face
$colorCubeR   = [System.Drawing.Color]::FromArgb(0x2A, 0x68, 0x55) # cube right face
$colorWhite   = [System.Drawing.Color]::White
$colorMint    = [System.Drawing.Color]::FromArgb(0x4E, 0xC9, 0xA4)
$colorTextDark= [System.Drawing.Color]::FromArgb(0x17, 0x3A, 0x3A)
$colorDivider = [System.Drawing.Color]::FromArgb(0xE6, 0xEB, 0xE9)

function New-Canvas {
    param([int]$Width, [int]$Height)
    $bmp = New-Object System.Drawing.Bitmap($Width, $Height, [System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
    $g = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::ClearTypeGridFit
    return @{ Bitmap = $bmp; Graphics = $g }
}

function Draw-IsometricCube {
    param($Graphics, [double]$Cx, [double]$Cy, [double]$HalfW, [double]$HalfH, [double]$BodyH)
    # Closed isometric cube; (Cx, Cy) is the center of the top face.
    $topT = @{ X = $Cx; Y = ($Cy - $HalfH) }
    $topR = @{ X = ($Cx + $HalfW); Y = $Cy }
    $topB = @{ X = $Cx; Y = ($Cy + $HalfH) }
    $topL = @{ X = ($Cx - $HalfW); Y = $Cy }
    $botL = @{ X = ($Cx - $HalfW); Y = ($Cy + $BodyH) }
    $botB = @{ X = $Cx; Y = ($Cy + $HalfH + $BodyH) }
    $botR = @{ X = ($Cx + $HalfW); Y = ($Cy + $BodyH) }

    $leftPts  = @(
        (New-Object System.Drawing.PointF([single]$topL.X, [single]$topL.Y)),
        (New-Object System.Drawing.PointF([single]$topB.X, [single]$topB.Y)),
        (New-Object System.Drawing.PointF([single]$botB.X, [single]$botB.Y)),
        (New-Object System.Drawing.PointF([single]$botL.X, [single]$botL.Y))
    )
    $rightPts = @(
        (New-Object System.Drawing.PointF([single]$topB.X, [single]$topB.Y)),
        (New-Object System.Drawing.PointF([single]$topR.X, [single]$topR.Y)),
        (New-Object System.Drawing.PointF([single]$botR.X, [single]$botR.Y)),
        (New-Object System.Drawing.PointF([single]$botB.X, [single]$botB.Y))
    )
    $topPts   = @(
        (New-Object System.Drawing.PointF([single]$topT.X, [single]$topT.Y)),
        (New-Object System.Drawing.PointF([single]$topR.X, [single]$topR.Y)),
        (New-Object System.Drawing.PointF([single]$topB.X, [single]$topB.Y)),
        (New-Object System.Drawing.PointF([single]$topL.X, [single]$topL.Y))
    )

    $Graphics.FillPolygon((New-Object System.Drawing.SolidBrush($colorCubeL)), $leftPts)
    $Graphics.FillPolygon((New-Object System.Drawing.SolidBrush($colorCubeR)), $rightPts)
    $Graphics.FillPolygon((New-Object System.Drawing.SolidBrush($colorCubeTop)), $topPts)
}

# ---- Sidebar 164x314 (MUI2 WELCOMEFINISHPAGE_BITMAP) ----------------------
function New-SidebarBitmap {
    $c = New-Canvas -Width 164 -Height 314
    $g = $c.Graphics
    $bmp = $c.Bitmap

    # Background
    $g.Clear($colorBg)

    # Subtle decorative circle, upper-right (thin outline, low contrast)
    $decorPen = New-Object System.Drawing.Pen($colorBgSoft, 2)
    $g.DrawEllipse($decorPen, 92, -34, 128, 128)

    # White extraction arrow above the cube
    $white = New-Object System.Drawing.SolidBrush($colorWhite)
    $arrowHead = @(
        (New-Object System.Drawing.PointF(82, 52)),
        (New-Object System.Drawing.PointF(70, 70)),
        (New-Object System.Drawing.PointF(94, 70))
    )
    $g.FillPolygon($white, $arrowHead)
    $g.FillRectangle($white, 76, 70, 12, 26)

    # Cube: top face center (82, 140), half width 48, half height 24, body height 48
    Draw-IsometricCube -Graphics $g -Cx 82 -Cy 140 -HalfW 48 -HalfH 24 -BodyH 48

    # Product name
    $nameFont = New-Object System.Drawing.Font('Segoe UI Semibold', 21, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
    $nameBrush = New-Object System.Drawing.SolidBrush($colorWhite)
    $nameFormat = New-Object System.Drawing.StringFormat
    $nameFormat.Alignment = [System.Drawing.StringAlignment]::Center
    $g.DrawString('OpenUnpack', $nameFont, $nameBrush, (New-Object System.Drawing.RectangleF(0, 224, 164, 30)), $nameFormat)

    # Chinese subtitle
    $subFont = New-Object System.Drawing.Font('Microsoft YaHei UI', 13, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
    $subBrush = New-Object System.Drawing.SolidBrush($colorMint)
    $g.DrawString('解压工坊', $subFont, $subBrush, (New-Object System.Drawing.RectangleF(0, 256, 164, 20)), $nameFormat)

    # Bottom accent bar
    $accent = New-Object System.Drawing.SolidBrush($colorMint)
    $g.FillRectangle($accent, 62, 302, 40, 3)

    $g.Dispose()
    return $bmp
}

# ---- Header 150x57 (MUI2 HEADERIMAGE_BITMAP) -------------------------------
function New-HeaderBitmap {
    $c = New-Canvas -Width 150 -Height 57
    $g = $c.Graphics
    $bmp = $c.Bitmap

    # White background blends with the wizard page
    $g.Clear($colorWhite)

    # Mini cube, top face center (24, 24), half width 15, half height 8, body height 15
    Draw-IsometricCube -Graphics $g -Cx 24 -Cy 24 -HalfW 15 -HalfH 8 -BodyH 15

    # Product name
    $nameFont = New-Object System.Drawing.Font('Segoe UI Semibold', 14, [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
    $nameBrush = New-Object System.Drawing.SolidBrush($colorTextDark)
    $g.DrawString('OpenUnpack', $nameFont, $nameBrush, 44, 18)

    # Hairline divider at the bottom
    $dividerPen = New-Object System.Drawing.Pen($colorDivider, 1)
    $g.DrawLine($dividerPen, 0, 56, 150, 56)

    $g.Dispose()
    return $bmp
}

# ---- Emit ------------------------------------------------------------------
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$sidebar = New-SidebarBitmap
$sidebar.Save((Join-Path $OutDir 'installer-sidebar.bmp'), [System.Drawing.Imaging.ImageFormat]::Bmp)
$sidebar.Save((Join-Path $OutDir 'installer-sidebar-preview.png'), [System.Drawing.Imaging.ImageFormat]::Png)
$sidebar.Dispose()

$header = New-HeaderBitmap
$header.Save((Join-Path $OutDir 'installer-header.bmp'), [System.Drawing.Imaging.ImageFormat]::Bmp)
$header.Save((Join-Path $OutDir 'installer-header-preview.png'), [System.Drawing.Imaging.ImageFormat]::Png)
$header.Dispose()

Get-ChildItem -LiteralPath $OutDir -Filter 'installer-*' | Select-Object Name, Length
