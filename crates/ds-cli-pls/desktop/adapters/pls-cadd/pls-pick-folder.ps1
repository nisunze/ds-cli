param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $DialogHandle,
    [Parameter(Mandatory = $true)][string] $Directory,
    [int] $TimeoutSeconds = 30
)
# Select a folder in the modern shell folder picker PLS-CADD raises from
# Restore › Change Common Directory Path › New Directory Path (title
# "Select directory (Currently '')") and from similar commands. The Folder
# name field (id 1152) does NOT navigate; the only reliable route is the
# address bar: focus the dialog, Ctrl+L, type the path, Enter, then prove
# the dialog's own address toolbar (id 1001, "Address: <dir>") shows the
# target before pressing Select Folder (id 1). The directory must already
# exist (the picker cannot create it); create it empty beforehand.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DsPick {
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a, uint b, bool attach);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool BringWindowToTop(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr SetActiveWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr SetFocus(IntPtr h);
}
"@
if (-not (Test-Path -LiteralPath $Directory -PathType Container)) { throw "Directory must exist before it can be picked: $Directory" }
$dialog = [IntPtr] $DialogHandle
$pid_ = [uint32] 0
$target = [DsPick]::GetWindowThreadProcessId($dialog, [ref] $pid_)
if ($pid_ -ne $ProcessId) { throw "Dialog $DialogHandle does not belong to PID $ProcessId" }
$current = [DsPick]::GetCurrentThreadId()
$fgPid = [uint32] 0
$fg = [DsPick]::GetWindowThreadProcessId([DsPick]::GetForegroundWindow(), [ref] $fgPid)
try {
    if ($fg -ne 0 -and $fg -ne $current) { [DsPick]::AttachThreadInput($current, $fg, $true) | Out-Null }
    if ($target -ne 0 -and $target -ne $current) { [DsPick]::AttachThreadInput($current, $target, $true) | Out-Null }
    [DsPick]::SetForegroundWindow($dialog) | Out-Null
    [DsPick]::BringWindowToTop($dialog) | Out-Null
    [DsPick]::SetActiveWindow($dialog) | Out-Null
    [DsPick]::SetFocus($dialog) | Out-Null
    Start-Sleep -Milliseconds 250
    [System.Windows.Forms.SendKeys]::SendWait('^l')
    Start-Sleep -Milliseconds 150
    [System.Windows.Forms.SendKeys]::SendWait($Directory.Replace('+', '{+}').Replace('^', '{^}').Replace('%', '{%}').Replace('~', '{~}').Replace('(', '{(}').Replace(')', '{)}'))
    [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
} finally {
    if ($target -ne 0 -and $target -ne $current) { [DsPick]::AttachThreadInput($current, $target, $false) | Out-Null }
    if ($fg -ne 0 -and $fg -ne $current) { [DsPick]::AttachThreadInput($current, $fg, $false) | Out-Null }
}
$expected = "Address: $Directory"
$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
$seen = ''
do {
    Start-Sleep -Milliseconds 300
    $kids = & "$here\pls-windows.ps1" -ProcessId $ProcessId -Children $DialogHandle
    $addr = $kids | Where-Object { $_ -match "id=1001 .*vis=True en=True 'Address: " } | Select-Object -First 1
    if ($addr -match "'(Address: .*)'$") { $seen = $Matches[1] }
} while (-not $seen.Equals($expected, [System.StringComparison]::OrdinalIgnoreCase) -and [DateTime]::UtcNow -lt $deadline)
if (-not $seen.Equals($expected, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Folder picker did not navigate: expected '$expected', saw '$seen'" }
$select = ($kids | Where-Object { $_ -match "id=1 .*vis=True en=True '(Select Folder|&Select Folder|Open|&Open|OK|&OK)'" }) -replace '^\s*child (\d+).*', '$1' | Select-Object -First 1
if (-not $select) { throw 'No visible+enabled Select Folder (id 1) button' }
& "$here\pls-windows.ps1" -ProcessId $ProcessId -Click ([long] $select) | Out-Null
[ordered]@{ schema = 'ds.pls.pick_folder.v1'; dialog = $DialogHandle; directory = $Directory; address_seen = $seen; clicked = [long] $select } | ConvertTo-Json -Compress
