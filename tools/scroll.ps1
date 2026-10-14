param([int]$X, [int]$Y, [int]$Notches = 6)
Add-Type -TypeDefinition @"
using System;using System.Runtime.InteropServices;
public static class Scroller {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
  public const uint WHEEL = 0x0800;
  public static string Go(int x, int y, int n){
    SetThreadDpiAwarenessContext(new IntPtr(-4));
    SetCursorPos(x,y); System.Threading.Thread.Sleep(150);
    for(int i=0;i<n;i++){ mouse_event(WHEEL,0,0,(uint)(0-120),UIntPtr.Zero); System.Threading.Thread.Sleep(90); }
    return "ok";
  }
}
"@
[Scroller]::Go($X, $Y, $Notches)
