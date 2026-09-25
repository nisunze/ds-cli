param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [long] $Children = 0,
    [long] $Click = 0,
    [long] $CloseWindow = 0,
    [switch] $All
)
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsPw {
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc cb, IntPtr lParam);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc cb, IntPtr lParam);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, StringBuilder l);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetWindow(IntPtr h, uint cmd);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
}
"@
$rows = New-Object System.Collections.ArrayList
function Text([IntPtr]$h) {
    $sb = New-Object System.Text.StringBuilder 4096
    [DsPw]::SendMessage($h, 0x000D, [IntPtr]4096, $sb) | Out-Null   # WM_GETTEXT works across processes
    $sb.ToString()
}
function Cls([IntPtr]$h) { $sb = New-Object System.Text.StringBuilder 256; [DsPw]::GetClassName($h, $sb, 256) | Out-Null; $sb.ToString() }

if ($Click -ne 0) {
    [DsPw]::PostMessage([IntPtr]$Click, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null   # BM_CLICK
    "clicked $Click"
    exit 0
}
if ($CloseWindow -ne 0) {
    [DsPw]::PostMessage([IntPtr]$CloseWindow, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null  # WM_CLOSE
    "closed $CloseWindow"
    exit 0
}
if ($Children -ne 0) {
    $cb2 = [DsPw+EnumWindowsProc]{ param($c, $l)
        $t = (Text $c); if ($t.Length -gt 160) { $t = $t.Substring(0, 160) }
        [void]$rows.Add("  child $([long]$c) id=$([DsPw]::GetDlgCtrlID($c)) [$(Cls $c)] vis=$([DsPw]::IsWindowVisible($c)) en=$([DsPw]::IsWindowEnabled($c)) '$($t -replace "`r`n", " | ")'")
        $true }
    [DsPw]::EnumChildWindows([IntPtr]$Children, $cb2, [IntPtr]::Zero) | Out-Null
    $rows
    exit 0
}
$pid_ = $ProcessId
$cb = [DsPw+EnumWindowsProc]{ param($h, $l)
    $p = [uint32]0; [DsPw]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
    if ($p -eq $pid_) {
        $t = Text $h; $vis = [DsPw]::IsWindowVisible($h)
        if ($All -or $vis -or $t.Length -gt 0) {
            [void]$rows.Add("$([long]$h) vis=$vis en=$([DsPw]::IsWindowEnabled($h)) owner=$([long][DsPw]::GetWindow($h, 4)) [$(Cls $h)] '$t'")
        }
    }
    $true }
[DsPw]::EnumWindows($cb, [IntPtr]::Zero) | Out-Null
$rows
