param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [Parameter(Mandatory = $true)][int] $CommandId,
    [Parameter(Mandatory = $true)][string] $ExpectedTitle,
    [hashtable] $Texts = @{},
    [hashtable] $Checks = @{},
    [int] $AcceptControlId = 1,
    [string] $CaptureStem = '',
    [int] $TimeoutSeconds = 30
)
# Open one PLS-CADD dialog by menu command, set edit texts (-Texts @{1119='20'})
# and checkbox/radio states (-Checks @{2431=1}) by control id, read every value
# back, optionally PrintWindow the dialog before accepting, then press the
# accept button (OK id 1 by default). Refuses a dialog whose title differs from
# -ExpectedTitle and any control id that is not a visible+enabled child.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class DsSetRect { [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; } [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r); }
"@
& "$here\pls-command.ps1" -WindowHandle $MainWindowHandle -CommandId $CommandId -Post
$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
$h = 0L; $title = ''
do {
    Start-Sleep -Milliseconds 500
    $wins = @(& "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match 'vis=True' -and $_ -notmatch "'PLS-CADD - " -and $_ -notmatch "'Error Log'" })
    if ($wins.Count -gt 0) {
        $cand = [long](($wins[0] -split ' ')[0])
        $r = New-Object DsSetRect+RECT
        [DsSetRect]::GetWindowRect([IntPtr]$cand, [ref]$r) | Out-Null
        $kids = @(& "$here\pls-windows.ps1" -ProcessId $ProcessId -Children $cand)
        if (($r.R - $r.L) -gt 50 -and $kids.Count -gt 0) { $h = $cand; $title = if ($wins[0] -match "\] '(.*)'$") { $Matches[1] } else { '' }; break }
    }
} while ([DateTime]::UtcNow -lt $deadline)
if ($h -eq 0) { throw "No drawn dialog within $TimeoutSeconds s after command $CommandId" }
if ($title -cne $ExpectedTitle) { throw "Dialog title mismatch: expected '$ExpectedTitle', got '$title' (left open)" }
Start-Sleep -Milliseconds 300
function Ctl([int]$id) {
    $row = $kids | Where-Object { $_ -match "^\s*child \d+ id=$id \[\] vis=True en=True" } | Select-Object -First 1
    if (-not $row) { throw "control id $id is not a visible+enabled child of '$title'" }
    [long](($row -replace '^\s*child (\d+).*', '$1'))
}
$applied = [ordered]@{}
foreach ($id in $Texts.Keys) {
    $c = Ctl ([int]$id)
    & "$here\pls-control.ps1" -SetText $c -Text ([string]$Texts[$id]) | Out-Null
    $applied["text_$id"] = & "$here\pls-control.ps1" -GetText $c
}
foreach ($id in $Checks.Keys) {
    $c = Ctl ([int]$id)
    & "$here\pls-control.ps1" -SetCheck $c -Checked ([int]$Checks[$id]) | Out-Null
    $applied["check_$id"] = (& "$here\pls-control.ps1" -GetCheck $c) -replace '^.*= ', ''
}
if ($CaptureStem) { & "$here\pls-printwindow.ps1" -WindowHandle $h -OutputPath "$CaptureStem.png" | Out-Null }
$accept = Ctl $AcceptControlId
& "$here\pls-windows.ps1" -ProcessId $ProcessId -Click $accept | Out-Null
Start-Sleep -Milliseconds 800
$after = @(& "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match 'vis=True' -and $_ -notmatch "'PLS-CADD - " -and $_ -notmatch "'Error Log'" })
[ordered]@{ schema = 'ds.pls.dialog_set.v1'; command = $CommandId; title = $title; applied = $applied; accepted_with = $AcceptControlId; dialogs_after = @($after) } | ConvertTo-Json -Compress -Depth 4
