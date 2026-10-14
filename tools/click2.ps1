param([int]$X, [int]$Y)
Add-Type -TypeDefinition @"
using System;using System.Runtime.InteropServices;
public static class Clicker2 {
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, uint d, UIntPtr e);
  [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr c);
  public const uint DOWN=0x0002, UP=0x0004;
  public static string Click(int x, int y){
    var prev = SetThreadDpiAwarenessContext(new IntPtr(-4));   // per-monitor v2 => 物理坐标
    SetCursorPos(x,y); System.Threading.Thread.Sleep(150);
    mouse_event(DOWN,0,0,0,UIntPtr.Zero); System.Threading.Thread.Sleep(60);
    mouse_event(UP,0,0,0,UIntPtr.Zero);
    SetThreadDpiAwarenessContext(prev);
    return "ok";
  }
}
"@
[Clicker2]::Click($X, $Y) + " @ $X,$Y (physical)"
