param(
    [Parameter(Mandatory = $true)]
    [int] $ProcessId,
    [Parameter(Mandatory = $true)]
    [long] $WindowHandle,
    [Parameter(Mandatory = $true)]
    [string] $ExpectedExactTitle
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsGridCloseWindow {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr window, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
}
"@

$handle = [IntPtr] $WindowHandle
$owner = 0
[DsGridCloseWindow]::GetWindowThreadProcessId($handle, [ref] $owner) | Out-Null
if ($owner -ne $ProcessId) {
    throw "Window does not belong to PID $ProcessId"
}
$text = New-Object System.Text.StringBuilder 1024
[DsGridCloseWindow]::GetWindowText($handle, $text, $text.Capacity) | Out-Null
if ($text.ToString() -cne $ExpectedExactTitle) {
    throw "Window title mismatch: expected '$ExpectedExactTitle', got '$($text.ToString())'"
}
if (-not [DsGridCloseWindow]::PostMessage($handle, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) {
    throw "WM_CLOSE failed for '$ExpectedExactTitle'"
}
