param([string]$title = "Anycast", [string]$out = "D:\Anycast\target\win_capture.png")
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text; using System.Drawing;
public class PW2 {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  public static IntPtr Target = IntPtr.Zero;
  public static string Want = "";
  public static bool Cb(IntPtr h, IntPtr l) {
    var sb = new StringBuilder(512); GetWindowTextW(h, sb, 512);
    if (sb.ToString() == Want) { Target = h; return false; }
    return true;
  }
  public static string Capture(string path) {
    if (Target == IntPtr.Zero) return "ERR not found";
    RECT r; GetWindowRect(Target, out r);
    int w = r.R - r.L, h = r.B - r.T;
    var bmp = new Bitmap(w, h);
    var g = Graphics.FromImage(bmp);
    IntPtr hdc = g.GetHdc();
    bool ok = PrintWindow(Target, hdc, 2);
    g.ReleaseHdc(hdc);
    bmp.Save(path, System.Drawing.Imaging.ImageFormat.Png);
    g.Dispose(); bmp.Dispose();
    return "ok=" + ok + " rect=" + r.L + "," + r.T + " size=" + w + "x" + h;
  }
}
"@ -ReferencedAssemblies System.Drawing
[PW2]::SetProcessDPIAware() | Out-Null
[PW2]::Want = $title
[PW2]::EnumWindows([PW2+EnumProc]{ param($h,$l) [PW2]::Cb($h,$l) }, [IntPtr]::Zero) | Out-Null
$msg = [PW2]::Capture($out)
Set-Content -Path ($out + ".txt") -Value $msg -Encoding UTF8
