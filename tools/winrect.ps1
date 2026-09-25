param([string]$out = "D:\Anycast\target\rect.txt")
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text; using System.Collections.Generic;
public class W2 {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  public static List<string> Found = new List<string>();
  public static uint Pid;
  public static bool Cb(IntPtr h, IntPtr l) {
    uint pid; GetWindowThreadProcessId(h, out pid);
    if (pid != Pid) return true;
    RECT r; GetWindowRect(h, out r);
    var sb = new StringBuilder(256); GetWindowTextW(h, sb, 256);
    var cs = new StringBuilder(256); GetClassNameW(h, cs, 256);
    Found.Add(string.Format("hwnd={0} vis={1} title='{2}' class='{3}' rect={4},{5},{6},{7} size={8}x{9} dpi={10}", h, IsWindowVisible(h), sb, cs, r.L, r.T, r.R, r.B, r.R-r.L, r.B-r.T, GetDpiForWindow(h)));
    return true;
  }
}
"@
[W2]::SetProcessDPIAware() | Out-Null
$proc = Get-Process anycast -ErrorAction SilentlyContinue
if (-not $proc) { Set-Content -Path $out -Value "anycast not running" -Encoding UTF8; exit 0 }
[W2]::Pid = $proc.Id
[W2]::EnumWindows([W2+EnumProc]{ param($h,$l) [W2]::Cb($h,$l) }, [IntPtr]::Zero) | Out-Null
Set-Content -Path $out -Value ([W2]::Found -join "`n") -Encoding UTF8
