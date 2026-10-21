param(
    [Parameter(Mandatory = $true)]
    [long] $DialogHandle,
    [Parameter(Mandatory = $true)]
    [long] $EditHandle,
    [Parameter(Mandatory = $true)]
    [string] $Path
)

Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DsGridDialogTyping {
    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);

    [DllImport("kernel32.dll")]
    public static extern uint GetCurrentThreadId();

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern bool AttachThreadInput(uint sourceThread, uint targetThread, bool attach);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern bool BringWindowToTop(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern IntPtr SetActiveWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern IntPtr SetFocus(IntPtr hwnd);
}
"@

$dialog = [IntPtr] $DialogHandle
$edit = [IntPtr] $EditHandle
$processId = 0
$targetThread = [DsGridDialogTyping]::GetWindowThreadProcessId($edit, [ref] $processId)
$currentThread = [DsGridDialogTyping]::GetCurrentThreadId()
$foregroundProcessId = 0
$foregroundThread = [DsGridDialogTyping]::GetWindowThreadProcessId(
    [DsGridDialogTyping]::GetForegroundWindow(),
    [ref] $foregroundProcessId)

if ($foregroundThread -ne 0 -and $foregroundThread -ne $currentThread) {
    [DsGridDialogTyping]::AttachThreadInput($currentThread, $foregroundThread, $true) | Out-Null
}
if ($targetThread -ne 0 -and $targetThread -ne $currentThread) {
    [DsGridDialogTyping]::AttachThreadInput($currentThread, $targetThread, $true) | Out-Null
}

[DsGridDialogTyping]::SetForegroundWindow($dialog) | Out-Null
[DsGridDialogTyping]::BringWindowToTop($dialog) | Out-Null
[DsGridDialogTyping]::SetActiveWindow($dialog) | Out-Null
[DsGridDialogTyping]::SetFocus($edit) | Out-Null
Start-Sleep -Milliseconds 250
[System.Windows.Forms.SendKeys]::SendWait('^a')
[System.Windows.Forms.SendKeys]::SendWait($Path)
[System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
Start-Sleep -Milliseconds 250

if ($targetThread -ne 0 -and $targetThread -ne $currentThread) {
    [DsGridDialogTyping]::AttachThreadInput($currentThread, $targetThread, $false) | Out-Null
}
if ($foregroundThread -ne 0 -and $foregroundThread -ne $currentThread) {
    [DsGridDialogTyping]::AttachThreadInput($currentThread, $foregroundThread, $false) | Out-Null
}
