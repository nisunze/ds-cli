param(
 [Parameter(Mandatory=$true)][int]$ProcessId,
 [Parameter(Mandatory=$true)][long]$DialogHandle,
 [Parameter(Mandatory=$true)][string]$ExpectedTitle,
 [Parameter(Mandatory=$true)][string]$ExpectedProjectPath,
 [Parameter(Mandatory=$true)][int]$ExpectedRestoredFiles
)
$ErrorActionPreference='Stop'
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsPlsKnownOpen {
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr h,int id);
 [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h,uint m,IntPtr w,StringBuilder l);
 [DllImport("user32.dll",SetLastError=true)] public static extern IntPtr SendMessageTimeout(IntPtr h,uint m,IntPtr w,IntPtr l,uint f,uint timeout,out IntPtr result);
}
"@
$p=Get-Process -Id $ProcessId -ErrorAction Stop
if ($p.Path -cne 'C:\Program Files\PLS\pls_cadd\pls_cadd64.exe') { throw 'Unexpected executable' }
$h=[IntPtr]$DialogHandle
$owner=0
[DsPlsKnownOpen]::GetWindowThreadProcessId($h,[ref]$owner)|Out-Null
if ($owner -ne $ProcessId) { throw 'Dialog PID mismatch' }
$title=New-Object System.Text.StringBuilder 1024
[DsPlsKnownOpen]::GetWindowText($h,$title,$title.Capacity)|Out-Null
if ($title.ToString() -cne $ExpectedTitle) { throw "Dialog title mismatch: $title" }
$bodyRows=& (Join-Path $PSScriptRoot 'pls-windows.ps1') -ProcessId $ProcessId -Children $DialogHandle
$body=($bodyRows -join ' ')
$required="$ExpectedRestoredFiles files restored, 0 files skipped"
if (-not $body.Contains($required) -or
 -not $body.Contains("Would you like to open project '$ExpectedProjectPath'?")) {
 throw "Dialog body mismatch: $body"
}
$yes=[DsPlsKnownOpen]::GetDlgItem($h,6)
if ($yes -eq [IntPtr]::Zero -or [DsPlsKnownOpen]::GetDlgCtrlID($yes) -ne 6) {
 throw 'Exact Yes control absent'
}
$yesText=New-Object System.Text.StringBuilder 32
[DsPlsKnownOpen]::SendMessage($yes,0x000D,[IntPtr]$yesText.Capacity,$yesText)|Out-Null
if ($yesText.ToString() -cne '&Yes') { throw "Yes control text mismatch: $yesText" }
$result=[IntPtr]::Zero
$return=[DsPlsKnownOpen]::SendMessageTimeout($h,0x0111,[IntPtr]6,$yes,0x0002,5000,[ref]$result)
if ($return -eq [IntPtr]::Zero) {
 throw "Exact dialog IDYES command failed; Win32 error $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
}
$deadline=[DateTime]::UtcNow.AddSeconds(15)
do {
 Start-Sleep -Milliseconds 200
 $p.Refresh()
 if ($p.HasExited) { throw 'PLS-CADD exited after Yes' }
 if ($p.MainWindowTitle -like '*nyamagabe.xyz*') {
  [ordered]@{status='opened';process_id=$ProcessId;project=$ExpectedProjectPath;title=$p.MainWindowTitle}|ConvertTo-Json
  exit 0
 }
} while ([DateTime]::UtcNow -lt $deadline)
throw "Yes command sent, project did not open within 15 seconds: $($p.MainWindowTitle)"
