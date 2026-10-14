Add-Type -TypeDefinition @"
using System;using System.Text;using System.Runtime.InteropServices;using System.Collections.Generic;
public static class Frame {
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr p);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int attr, out RECT r, int size);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
  public struct RECT { public int L,T,R,B; }
  public static List<string> Go(uint pid){
    SetThreadDpiAwarenessContext(new IntPtr(-4));
    var res=new List<string>();
    EnumWindows((h,p)=>{ uint t; GetWindowThreadProcessId(h,out t);
      if(t==pid && IsWindowVisible(h)){ var sb=new StringBuilder(512); GetWindowTextW(h,sb,512);
        RECT wr, dr; GetWindowRect(h,out wr);
        DwmGetWindowAttribute(h, 9, out dr, System.Runtime.InteropServices.Marshal.SizeOf(typeof(RECT)));  // DWMWA_EXTENDED_FRAME_BOUNDS
        if(wr.R-wr.L>200) res.Add(string.Format("title=[{0}] winRect={1},{2} {3}x{4}  visible={5},{6} {7}x{8}  inset=L{9} T{10} R{11} B{12}",
          sb.ToString(), wr.L,wr.T,wr.R-wr.L,wr.B-wr.T, dr.L,dr.T,dr.R-dr.L,dr.B-dr.T, dr.L-wr.L, dr.T-wr.T, wr.R-dr.R, wr.B-dr.B)); }
      return true; }, IntPtr.Zero);
    return res; }
}
"@
$p = (Get-Process huyanbao | Where-Object { $_.Path -like '*huyanbao-rebuild*' } | Select-Object -First 1).Id
[Frame]::Go($p)
