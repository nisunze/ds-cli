param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [int] $TimeoutSeconds = 120,
    [string] $UntilTitle = '',
    [string] $JournalPath = '',
    [string] $CatalogPath = (Join-Path $PSScriptRoot 'pls-dialog-catalog.psd1'),
    [switch] $Once
)
# The listener. Polls every 2 s for windows owned by the PLS-CADD frame, classifies
# each against pls-dialog-catalog.psd1 (title regex + text regex), acts on the
# catalogued decision (click a visible+enabled button by id, click the only visible
# OK, ignore, stop) and journals EVERY event. An unknown dialog is dumped with its
# whole control tree and reported as 'unknown' — the caller decides; nothing is
# waited out silently. Returns when: the frame is enabled and no modal is up
# (and -UntilTitle, if given, matches the frame title), or on 'stop'/'unknown',
# or at the timeout. -Once does a single pass.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
Import-Module (Join-Path $here 'pls-window-classification.psm1') -Force
$catalog = Import-PowerShellDataFile $CatalogPath
$events = [System.Collections.ArrayList]::new()
function Journal([hashtable]$e) {
    $e.at = [DateTime]::UtcNow.ToString('o')
    [void]$events.Add($e)
    if ($JournalPath) { ($e | ConvertTo-Json -Compress -Depth 5) | Add-Content -Encoding utf8 $JournalPath }
}
function Windows { & "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match 'vis=True' } }
function Kids([long]$h) { & "$here\pls-windows.ps1" -ProcessId $ProcessId -Children $h }
function Handle([string]$row) { [long](($row -split ' ')[0]) }
function Title([string]$row) { if ($row -match "\] '(.*)'$") { $Matches[1] } else { '' } }
function VisibleEnabled([string[]]$kids, [int]$id) {
    ($kids | Where-Object { $_ -match "id=$id .*vis=True en=True" }) -replace '^\s*child (\d+).*', '$1' | Select-Object -First 1
}
function AnyOk([string[]]$kids) {
    ($kids | Where-Object { $_ -match "vis=True en=True '&?(OK|Close|Continue)'" }) -replace '^\s*child (\d+).*', '$1' | Select-Object -First 1
}
function Click([long]$h) { & "$here\pls-windows.ps1" -ProcessId $ProcessId -Click $h | Out-Null }
# A posted BM_CLICK can silently fail when the dialog is not the active window (seen
# 2026-09-24 on the incoming Rev A2 clearance-voltage Warning: clicked, still up minutes later).
# If a catalogued button is still there on a later pass and its thread answers WM_NULL (so it
# is not just busy), post the button's own notification, WM_COMMAND(id, BN_CLICKED), to its
# parent — the message BM_CLICK would have produced. Same decision, never a different button.
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class DsWatch {
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
}
"@
$clickedButtons = @{}
function ClickButton([long]$dialog, [long]$button, [string]$name) {
    $r = [IntPtr]::Zero
    $answers = [DsWatch]::SendMessageTimeout([IntPtr]$dialog, 0x0000, [IntPtr]::Zero, [IntPtr]::Zero, 0x0002, 1000, [ref]$r) -ne [IntPtr]::Zero
    if ($clickedButtons.ContainsKey($button) -and [DsWatch]::IsWindow([IntPtr]$button) -and $answers) {
        $id = [DsWatch]::GetDlgCtrlID([IntPtr]$button)
        [DsWatch]::PostMessage([DsWatch]::GetParent([IntPtr]$button), 0x0111, [IntPtr]($id -band 0xFFFF), [IntPtr]$button) | Out-Null
        Journal @{ event = 'click_escalated'; dialog = $name; handle = $dialog; button = $button; control_id = $id }
    } else { Click $button }
    $clickedButtons[$button] = $true
}

$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
$outcome = 'timeout'
do {
    $wins = Windows
    $classified = Split-PlsWindowRows @($wins) $MainWindowHandle
    $frame = $classified.frame
    $modals = $classified.others
    $blocking = $false
    foreach ($m in $modals) {
        $h = Handle $m; $title = Title $m
        $kids = Kids $h
        $text = (($kids | Where-Object { $_ -match "id=(65535|20|1500|1004|-1) .*vis=True" }) -replace "^.*?'(.*)'$", '$1') -join ' | '
        $entry = $catalog.Entries | Where-Object {
            (($_.Title -eq '') -or ($title -match $_.Title)) -and (($_.Text -eq '') -or ($text -match $_.Text))
        } | Select-Object -First 1
        if (-not $entry) {
            Journal @{ event = 'unknown_dialog'; handle = $h; title = $title; text = $text; controls = @($kids) }
            $outcome = 'unknown'; $blocking = $true
            continue
        }
        switch ($entry.Action) {
            'ignore' { continue }
            'wait'   { Journal @{ event = 'progress'; dialog = $entry.Name; title = $title; status = (($kids | Where-Object { $_ -match 'id=535 ' }) -replace "^.*?'(.*)'$", '$1') }; $blocking = $true; continue }
            'stop'   { Journal @{ event = 'stop'; dialog = $entry.Name; title = $title; text = $text }; $outcome = 'stop'; $blocking = $true; continue }
            'flow'   { Journal @{ event = 'flow_dialog'; dialog = $entry.Name; title = $title }; $outcome = 'flow'; $blocking = $true; continue }
            'options'{
                $btn = VisibleEnabled $kids $entry.ControlId
                if (-not $btn) { Journal @{ event = 'no_visible_ok'; dialog = $entry.Name; title = $title; controls = @($kids) }; $outcome = 'unknown'; $blocking = $true; continue }
                Journal @{ event = 'accept_options'; dialog = $entry.Name; title = $title; button = $btn }
                ClickButton $h ([long]$btn) $entry.Name; $blocking = $true
            }
            'click'  {
                $btn = VisibleEnabled $kids $entry.ControlId
                if (-not $btn) { Journal @{ event = 'button_not_visible'; dialog = $entry.Name; title = $title; wanted = $entry.ControlId; controls = @($kids) }; $outcome = 'unknown'; $blocking = $true; continue }
                Journal @{ event = 'click'; dialog = $entry.Name; title = $title; text = $text; button = $btn; control_id = $entry.ControlId }
                ClickButton $h ([long]$btn) $entry.Name; $blocking = $true
            }
            'click_any_ok' {
                $btn = AnyOk $kids
                if (-not $btn) { Journal @{ event = 'no_ok_button'; dialog = $entry.Name; title = $title; controls = @($kids) }; $outcome = 'unknown'; $blocking = $true; continue }
                Journal @{ event = 'click'; dialog = $entry.Name; title = $title; text = $text; button = $btn }
                ClickButton $h ([long]$btn) $entry.Name; $blocking = $true
            }
        }
    }
    if ($outcome -in @('unknown', 'stop', 'flow')) { break }
    $frameTitle = if ($frame) { Title $frame } else { '' }
    if (-not $blocking -and $frame -match 'en=True' -and (($UntilTitle -eq '') -or ($frameTitle -match $UntilTitle))) { $outcome = 'ready'; break }
    if ($Once) { $outcome = if ($blocking) { 'acted' } else { 'ready' }; break }
    Start-Sleep -Seconds 2
} while ([DateTime]::UtcNow -lt $deadline)

[ordered]@{
    schema = 'ds.pls.dialog_watch.v1'
    outcome = $outcome
    frame_title = (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue).MainWindowTitle
    events = @($events)
} | ConvertTo-Json -Depth 6
