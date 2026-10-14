param([int]$X=200,[int]$Y=150)
Add-Type -TypeDefinition @"
using System;using System.Text;using System.Runtime.InteropServices;
public static class Place {
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint f);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  public struct RECT { public int L,T,R,B; }
  public static string Go(uint pid, int x, int y){
    SetThreadDpiAwarenessContext(new IntPtr(-4));     // 物理坐标
    string found=null; IntPtr best=IntPtr.Zero; int len=-1;
    EnumWindows((h,p)=>{ uint t; GetWindowThreadProcessId(h,out t);
      if(t==pid && IsWindowVisible(h)){ var sb=new StringBuilder(512); GetWindowTextW(h,sb,512);
        RECT r; GetWindowRect(h,out r);
        if(r.R-r.L>300 && r.B-r.T>300 && sb.Length>len){ len=sb.Length; best=h; found=sb.ToString(); } }
      return true; }, IntPtr.Zero);
    if(best!=IntPtr.Zero) SetWindowPos(best, new IntPtr(-1), x, y, 0,0, 0x0001);
    return found;
  }
}
"@
$p = (Get-Process huyanbao | Where-Object { $_.Path -like '*huyanbao-rebuild*' } | Select-Object -First 1).Id
"placed: " + [Place]::Go($p, $X, $Y)
