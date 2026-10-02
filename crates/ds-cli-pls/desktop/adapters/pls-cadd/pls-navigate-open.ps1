param(
    [Parameter(Mandatory = $true)]
    [long] $DialogHandle,
    [Parameter(Mandatory = $true)]
    [string] $DirectoryPath
)

Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DsGridOpenNavigation {
    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern bool ShowWindowAsync(IntPtr hwnd, int command);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);

    [DllImport("kernel32.dll")]
    public static extern uint GetCurrentThreadId();

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern bool AttachThreadInput(uint sourceThread, uint targetThread, bool attach);

    [DllImport("user32.dll")]
    public static extern bool BringWindowToTop(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern IntPtr SetActiveWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern IntPtr SetFocus(IntPtr hwnd);
}
"@

$dialog = [IntPtr] $DialogHandle
$processId = 0
$targetThread = [DsGridOpenNavigation]::GetWindowThreadProcessId($dialog, [ref] $processId)
$currentThread = [DsGridOpenNavigation]::GetCurrentThreadId()
$foregroundProcessId = 0
$foregroundThread = [DsGridOpenNavigation]::GetWindowThreadProcessId(
    [DsGridOpenNavigation]::GetForegroundWindow(),
    [ref] $foregroundProcessId)

if ($foregroundThread -ne 0 -and $foregroundThread -ne $currentThread) {
    [DsGridOpenNavigation]::AttachThreadInput($currentThread, $foregroundThread, $true) | Out-Null
}
if ($targetThread -ne 0 -and $targetThread -ne $currentThread) {
    [DsGridOpenNavigation]::AttachThreadInput($currentThread, $targetThread, $true) | Out-Null
}
[DsGridOpenNavigation]::ShowWindowAsync($dialog, 9) | Out-Null
[DsGridOpenNavigation]::SetForegroundWindow($dialog) | Out-Null
[DsGridOpenNavigation]::BringWindowToTop($dialog) | Out-Null
[DsGridOpenNavigation]::SetActiveWindow($dialog) | Out-Null
[DsGridOpenNavigation]::SetFocus($dialog) | Out-Null
Start-Sleep -Milliseconds 250
[System.Windows.Forms.SendKeys]::SendWait('^l')
Start-Sleep -Milliseconds 150
[System.Windows.Forms.SendKeys]::SendWait($DirectoryPath)
[System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
Start-Sleep -Milliseconds 250

if ($targetThread -ne 0 -and $targetThread -ne $currentThread) {
    [DsGridOpenNavigation]::AttachThreadInput($currentThread, $targetThread, $false) | Out-Null
}
if ($foregroundThread -ne 0 -and $foregroundThread -ne $currentThread) {
    [DsGridOpenNavigation]::AttachThreadInput($currentThread, $foregroundThread, $false) | Out-Null
}
