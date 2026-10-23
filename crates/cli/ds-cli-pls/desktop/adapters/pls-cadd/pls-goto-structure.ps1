param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [Parameter(Mandatory = $true)][int] $Structure,
    [int] $ZoomInSteps = 0
)
# View > Goto Structure... (command 40570): dialog 'Goto', structure edit id 4326, OK id 1.
# Zooms the ACTIVE view (profile, plan or 3D) on the structure; 'Zoom In +' is command 210.
# Text is typed with EM_SETSEL + WM_CHAR — WM_SETTEXT does not take on this edit.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DsGoto {
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
}
"@
[DsGoto]::PostMessage([IntPtr]$MainWindowHandle, 0x0111, [IntPtr]40570, [IntPtr]::Zero) | Out-Null
$dialog = $null
for ($i = 0; $i -lt 20 -and -not $dialog; $i++) {
    Start-Sleep -Milliseconds 500
    $dialog = (& "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match "vis=True .*'Goto'$" }) -split ' ' | Select-Object -First 1
}
if (-not $dialog) { throw 'Goto dialog did not appear' }
$kids = & "$here\pls-windows.ps1" -ProcessId $ProcessId -Children ([long]$dialog)
$edit = ($kids | Where-Object { $_ -match 'id=4326 ' }) -replace '^\s*child (\d+).*', '$1'
$ok = ($kids | Where-Object { $_ -match "id=1 .*'OK'" }) -replace '^\s*child (\d+).*', '$1'
if (-not $edit -or -not $ok) { throw 'Goto dialog controls not found (edit 4326 / OK 1)' }
[DsGoto]::SendMessage([IntPtr][long]$edit, 0x00B1, [IntPtr]0, [IntPtr](-1)) | Out-Null
foreach ($ch in [int[]][char[]]"$Structure") { [DsGoto]::SendMessage([IntPtr][long]$edit, 0x0102, [IntPtr]$ch, [IntPtr]1) | Out-Null }
[DsGoto]::PostMessage([IntPtr][long]$ok, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
Start-Sleep -Seconds 2
for ($z = 0; $z -lt $ZoomInSteps; $z++) {
    [DsGoto]::PostMessage([IntPtr]$MainWindowHandle, 0x0111, [IntPtr]210, [IntPtr]::Zero) | Out-Null
    Start-Sleep -Milliseconds 1200
}
[ordered]@{ schema = 'ds.pls.goto.v1'; structure = $Structure; zoom_in_steps = $ZoomInSteps; view = (Get-Process -Id $ProcessId).MainWindowTitle } | ConvertTo-Json
