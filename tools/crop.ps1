param(
  [string]$src = "D:\Anycast\target\shot_real.png",
  [int]$x = 635, [int]$y = 304, [int]$w = 1290, [int]$h = 840,
  [double]$scale = 1.0,
  [string]$out = "D:\Anycast\target\crop.png"
)
Add-Type -AssemblyName System.Drawing
$img = [System.Drawing.Image]::FromFile($src)
$rect = New-Object System.Drawing.Rectangle $x, $y, $w, $h
$dst = New-Object System.Drawing.Bitmap ([int]($w*$scale)), ([int]($h*$scale))
$g = [System.Drawing.Graphics]::FromImage($dst)
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
$g.DrawImage($img, (New-Object System.Drawing.Rectangle 0,0,([int]($w*$scale)),([int]($h*$scale))), $rect, [System.Drawing.GraphicsUnit]::Pixel)
$dst.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
$g.Dispose(); $dst.Dispose(); $img.Dispose()
