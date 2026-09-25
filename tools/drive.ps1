param([string]$keys = "", [int]$wait = 600, [string]$out = "", [switch]$noActivate, [switch]$imeToggle)
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class N {
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern IntPtr FindWindowW(string c, string t);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
  [DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);
}
"@
[N]::SetProcessDPIAware() | Out-Null
if (-not $noActivate) {
  $h = [N]::FindWindowW("Window Class", "Anycast")
  if ($h -ne [IntPtr]::Zero) { [N]::ShowWindow($h, 9) | Out-Null; [N]::SetForegroundWindow($h) | Out-Null; Start-Sleep -Milliseconds 300 }
}
if ($imeToggle) { [N]::keybd_event(0x10,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [N]::keybd_event(0x10,0,2,[UIntPtr]::Zero); Start-Sleep -Milliseconds 200 }
if ($keys -ne "") { [System.Windows.Forms.SendKeys]::SendWait($keys) }
Start-Sleep -Milliseconds $wait
if ($out -ne "") {
  $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
  $bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)
  Write-Output "saved $out"
}
