Add-Type -TypeDefinition @"
using System;using System.Text;using System.Runtime.InteropServices;using System.Collections.Generic;
public static class WPhys {
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr p);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  public struct RECT { public int Left, Top, Right, Bottom; }
  public static List<string> Go(uint pid){ SetThreadDpiAwarenessContext((IntPtr)(-4)); var res=new List<string>();
    EnumWindows((h,p)=>{ uint x; GetWindowThreadProcessId(h,out x);
      if(x==pid && IsWindowVisible(h)){ var sb=new StringBuilder(256); GetWindowTextW(h,sb,256); RECT r; GetWindowRect(h,out r);
        if(r.Right-r.Left > 300){ SetForegroundWindow(h); res.Add(string.Format("{0},{1},{2},{3}", r.Left, r.Top, r.Right-r.Left, r.Bottom-r.Top)); } }
      return true; }, IntPtr.Zero);
    return res; }
}
"@
$pid2 = (Get-Process huyanbao | Where-Object { $_.Path -like '*huyanbao-rebuild*' } | Select-Object -First 1).Id
[WPhys]::Go($pid2)
