param(
  [string]$title = "Anycast",
  [string]$out   = "D:\Anycast\target\win_shot.png",
  [double]$scale = 1.0,
  [int]$waitMs   = 1200
)

# 恢复并前置指定标题的窗口，然后截取该窗口在屏幕上的合成结果。
# 用途：验证被其他窗口遮挡、或处于最小化/隐藏状态的桌面应用界面。
# 与 screenshot.ps1（全屏）+ crop.ps1（裁剪）的区别：本脚本自己找窗口、自己置前，
# 一次调用完成，中间不会因焦点变化导致目标窗口跑掉。
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text; using System.Drawing;
public class WinShot {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int cmd);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  public static IntPtr Target = IntPtr.Zero;
  public static string Want = "";
  public static bool Cb(IntPtr h, IntPtr l) {
    var sb = new StringBuilder(512); GetWindowTextW(h, sb, 512);
    if (sb.ToString() == Want) { Target = h; return false; }
    return true;
  }
  public static string Go(string path, double sc, int wait) {
    if (Target == IntPtr.Zero) return "ERR window not found: " + Want;
    if (IsIconic(Target)) ShowWindow(Target, 9);   // SW_RESTORE
    else ShowWindow(Target, 5);                    // SW_SHOW
    // HWND_TOPMOST(-1) 再 HWND_NOTOPMOST(-2)，确保真正浮到最前
    SetWindowPos(Target, new IntPtr(-1), 0,0,0,0, 0x0001|0x0002|0x0040);
    SetWindowPos(Target, new IntPtr(-2), 0,0,0,0, 0x0001|0x0002|0x0040);
    SetForegroundWindow(Target);
    System.Threading.Thread.Sleep(wait);
    RECT r; GetWindowRect(Target, out r);
    int w = r.R - r.L, h = r.B - r.T;
    if (w <= 0 || h <= 0) return "ERR bad rect " + r.L + "," + r.T + " " + w + "x" + h;
    int ow = (int)Math.Round(w * sc), oh = (int)Math.Round(h * sc);
    var bmp = new Bitmap(ow, oh);
    var g = Graphics.FromImage(bmp);
    g.InterpolationMode = System.Drawing.Drawing2D.InterpolationMode.HighQualityBicubic;
    g.CopyFromScreen(r.L, r.T, 0, 0, new System.Drawing.Size(w, h));
    if (sc != 1.0) {
      var full = new Bitmap(w, h);
      var g2 = Graphics.FromImage(full);
      g2.CopyFromScreen(r.L, r.T, 0, 0, new System.Drawing.Size(w, h));
      g2.Dispose();
      g.DrawImage(full, new System.Drawing.Rectangle(0, 0, ow, oh));
      full.Dispose();
    }
    bmp.Save(path, System.Drawing.Imaging.ImageFormat.Png);
    g.Dispose(); bmp.Dispose();
    return "ok rect=" + r.L + "," + r.T + " size=" + w + "x" + h + " out=" + ow + "x" + oh;
  }
}
"@ -ReferencedAssemblies System.Drawing
[WinShot]::SetProcessDPIAware() | Out-Null
[WinShot]::Want = $title
[WinShot]::EnumWindows([WinShot+EnumProc]{ param($h,$l) [WinShot]::Cb($h,$l) }, [IntPtr]::Zero) | Out-Null
$msg = [WinShot]::Go($out, $scale, $waitMs)
Set-Content -Path ($out + ".txt") -Value $msg -Encoding UTF8
Write-Output $msg
