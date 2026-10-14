param([int]$Seconds = 14)
Add-Type -TypeDefinition @"
using System;using System.Text;using System.Runtime.InteropServices;
public static class Poll2 {
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr p);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern int GetWindowTextW(IntPtr h, StringBuilder s, int n);
  public static string Overlay(uint pid){
    string found="";
    EnumWindows((h,p)=>{ uint t; GetWindowThreadProcessId(h,out t);
      if(t==pid && IsWindowVisible(h)){ var sb=new StringBuilder(256); GetWindowTextW(h,sb,256);
        var s=sb.ToString();
        if(s.Contains("overlay")) found=s; }
      return true; }, IntPtr.Zero);
    return found;
  }
}
"@
$p = (Get-Process huyanbao | Where-Object { $_.Path -like '*huyanbao-rebuild*' } | Select-Object -First 1).Id
$first=$null
for ($i=0; $i -lt $Seconds; $i++) {
  $o = [Poll2]::Overlay($p)
  if ($o -and -not $first) { $first = Get-Date -Format "HH:mm:ss" }
  "{0}  {1}" -f (Get-Date -Format "HH:mm:ss"), $(if($o){"[$o]"}else{"-"})
  Start-Sleep -Milliseconds 900
}
