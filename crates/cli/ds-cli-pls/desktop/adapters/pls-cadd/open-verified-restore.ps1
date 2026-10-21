param(
    [Parameter(Mandatory=$true)][int] $ProcessId,
    [Parameter(Mandatory=$true)][long] $DialogHandle,
    [Parameter(Mandatory=$true)][string] $ExpectedBackupLeaf,
    [Parameter(Mandatory=$true)][string] $ExpectedProjectPath,
    [Parameter(Mandatory=$true)][int] $ExpectedRestoredFiles,
    [string] $ExpectedExecutablePath = 'C:\Program Files\PLS\pls_cadd\pls_cadd64.exe'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsPlsVerifiedRestoreOpen {
    public delegate bool EnumWindowsProc(IntPtr h, IntPtr data);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr h, EnumWindowsProc cb, IntPtr data);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint p);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h, int id);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, StringBuilder l);
    [DllImport("user32.dll", SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h, uint m, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
}
"@

function Get-ControlText([IntPtr] $Handle) {
    $buffer = New-Object System.Text.StringBuilder 4096
    [DsPlsVerifiedRestoreOpen]::SendMessage($Handle, 0x000D,
        [IntPtr] $buffer.Capacity, $buffer) | Out-Null
    return $buffer.ToString()
}
function Get-ControlClass([IntPtr] $Handle) {
    $buffer = New-Object System.Text.StringBuilder 256
    [DsPlsVerifiedRestoreOpen]::GetClassName($Handle, $buffer,
        $buffer.Capacity) | Out-Null
    return $buffer.ToString()
}

$process = Get-Process -Id $ProcessId -ErrorAction Stop
if ($process.Path -cne $ExpectedExecutablePath) {
    throw "Unexpected PLS executable: $($process.Path)"
}
$dialog = [IntPtr] $DialogHandle
$owner = [uint32] 0
[DsPlsVerifiedRestoreOpen]::GetWindowThreadProcessId($dialog,
    [ref] $owner) | Out-Null
if ($owner -ne $ProcessId -or
    (Get-ControlClass $dialog) -cne '#32770' -or
    -not [DsPlsVerifiedRestoreOpen]::IsWindowVisible($dialog)) {
    throw 'Restore dialog identity mismatch'
}
$title = Get-ControlText $dialog
$expectedTitle = "Restore Backup of $ExpectedBackupLeaf"
if ($title -cne $expectedTitle) {
    throw "Restore dialog title mismatch: $title"
}

$children = New-Object System.Collections.ArrayList
$callback = [DsPlsVerifiedRestoreOpen+EnumWindowsProc] {
    param($child, $data)
    $children.Add([ordered]@{
        handle = [long] $child
        id = [DsPlsVerifiedRestoreOpen]::GetDlgCtrlID($child)
        class = Get-ControlClass $child
        text = Get-ControlText $child
        visible = [DsPlsVerifiedRestoreOpen]::IsWindowVisible($child)
        enabled = [DsPlsVerifiedRestoreOpen]::IsWindowEnabled($child)
    }) | Out-Null
    return $true
}
[DsPlsVerifiedRestoreOpen]::EnumChildWindows($dialog, $callback,
    [IntPtr]::Zero) | Out-Null
$body = ((@($children | Where-Object {
    $_.visible -and $_.class -eq 'Static' -and $_.text
} | ForEach-Object { $_.text }) -join ' ') -replace '\s+', ' ').Trim()
$expectedBody = "$ExpectedRestoredFiles files restored, 0 files skipped Would you like to open project '$ExpectedProjectPath'?"
if ($body -cne $expectedBody) {
    throw "Restore dialog body mismatch: $body"
}
$yes = @($children | Where-Object {
    $_.id -eq 6 -and $_.class -ceq 'Button' -and
    $_.visible -and $_.enabled -and $_.text -ceq '&Yes'
})
if ($yes.Count -ne 1) {
    throw "Expected one enabled ID 6 Yes control; found $($yes.Count)"
}
$yesHandle = [IntPtr] ([long] $yes[0].handle)
if ([DsPlsVerifiedRestoreOpen]::GetDlgItem($dialog, 6) -ne $yesHandle) {
    throw 'Yes handle does not match dialog control ID 6'
}
$result = [IntPtr]::Zero
$sent = [DsPlsVerifiedRestoreOpen]::SendMessageTimeout($dialog, 0x0111,
    [IntPtr] 6, $yesHandle, 0x0002, 5000, [ref] $result)
if ($sent -eq [IntPtr]::Zero) {
    throw "IDYES dialog command failed: Win32 $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
}

$deadline = [DateTime]::UtcNow.AddSeconds(15)
do {
    Start-Sleep -Milliseconds 200
    $process.Refresh()
    if ($process.HasExited) { throw 'PLS-CADD exited after opening Restore' }
    if ($process.MainWindowTitle.Contains([System.IO.Path]::GetFileName($ExpectedProjectPath))) {
        [ordered]@{
            schema = 'ds.pls.verified_restore_open.v1'
            status = 'opened'
            process_id = $ProcessId
            project = $ExpectedProjectPath
            window_title = $process.MainWindowTitle
        } | ConvertTo-Json
        exit 0
    }
} while ([DateTime]::UtcNow -lt $deadline)
throw "Yes sent but project did not open: $($process.MainWindowTitle)"
