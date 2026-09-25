param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [Parameter(Mandatory = $true)][int] $CommandId,
    [Parameter(Mandatory = $true)][string] $OutputStem,
    [int] $CloseControlId = 2,
    [int] $TimeoutSeconds = 30,
    [switch] $Leave
)
# Characterize one modal dialog: send a menu command, wait for the modal that
# blocks the frame, wait until it has really drawn (a PLS-CADD dialog first
# appears as a 1x1 window with no children — the "looks blocked" moment), then
# PrintWindow it to <stem>.png, dump its UIA names to <stem>.uia.txt and its
# Win32 children to <stem>.children.txt, and close it with -CloseControlId
# (Cancel = 2 by default; -Leave keeps it open and prints its handle).
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class DsCapRect { [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; } [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r); }
"@
& "$here\pls-command.ps1" -WindowHandle $MainWindowHandle -CommandId $CommandId -Post
$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
$dialog = $null
do {
    Start-Sleep -Milliseconds 500
    $wins = @(& "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match 'vis=True' -and $_ -notmatch "'PLS-CADD - " -and $_ -notmatch "'Error Log'" })
    if ($wins.Count -gt 0) {
        $h = [long](($wins[0] -split ' ')[0])
        $r = New-Object DsCapRect+RECT
        [DsCapRect]::GetWindowRect([IntPtr]$h, [ref]$r) | Out-Null
        $kids = @(& "$here\pls-windows.ps1" -ProcessId $ProcessId -Children $h)
        if (($r.R - $r.L) -gt 50 -and $kids.Count -gt 0) { $dialog = $wins[0]; break }
    }
} while ([DateTime]::UtcNow -lt $deadline)
if (-not $dialog) { throw "No drawn modal dialog within $TimeoutSeconds s after command $CommandId" }
$title = if ($dialog -match "\] '(.*)'$") { $Matches[1] } else { '' }
Start-Sleep -Milliseconds 500
& "$here\pls-printwindow.ps1" -WindowHandle $h -OutputPath "$OutputStem.png" | Out-Null
& "$here\pls-dialog-text.ps1" -WindowHandle $h | Out-File -Encoding utf8 "$OutputStem.uia.txt"
$kids = @(& "$here\pls-windows.ps1" -ProcessId $ProcessId -Children $h)
$kids | Out-File -Encoding utf8 "$OutputStem.children.txt"
$closed = $false
if (-not $Leave) {
    $btn = ($kids | Where-Object { $_ -match "id=$CloseControlId .*vis=True en=True" }) -replace '^\s*child (\d+).*', '$1' | Select-Object -First 1
    if ($btn) { & "$here\pls-windows.ps1" -ProcessId $ProcessId -Click ([long]$btn) | Out-Null; $closed = $true }
}
[ordered]@{ schema = 'ds.pls.dialog_capture.v1'; command = $CommandId; handle = $h; title = $title; width = ($r.R - $r.L); height = ($r.B - $r.T); children = $kids.Count; png = "$OutputStem.png"; closed = $closed } | ConvertTo-Json -Compress
