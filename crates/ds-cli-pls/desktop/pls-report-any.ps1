param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [Parameter(Mandatory = $true)][int] $CommandId,
    [Parameter(Mandatory = $true)][string] $OutputPath,
    [Parameter(Mandatory = $true)][string] $ReportTitlePattern,
    [hashtable] $DialogEdits = @{},
    [hashtable] $DialogChecks = @{},
    [switch] $AllFeatureCodes,
    [int] $TimeoutSeconds = 600,
    [string] $JournalPath = '',
    [switch] $SaveOpenDocument,
    [switch] $Rtf
)
# Run any Lines > Reports command by id and save the resulting report document as text.
# -Rtf saves it as RTF instead (answers the 'Convert to rtf?' prompt Yes; give -OutputPath a .rtf name):
# the owner's deliverable form, converted to PDF afterwards (pls-rtf-to-pdf.ps1).
# -SaveOpenDocument skips steps 1-3 and saves an already generated report document whose
# title matches -ReportTitlePattern (the command is not posted again).
#
#  1. post the command (WM_COMMAND 0x0111 to the frame);
#  2. the options dialog (title ' Report$'): apply -DialogEdits (control id -> text,
#     typed with EM_SETSEL + WM_CHAR), -DialogChecks (control id -> 0/1, BM_SETCHECK),
#     and for 40016 -AllFeatureCodes (button 2938 -> 'Feature codes to include' ->
#     Select All id 3 -> OK id 1); then press the VISIBLE+ENABLED OK (id 1) — tabbed
#     option dialogs carry one OK per page and the hidden ones must never be clicked;
#  3. every other modal goes through pls-dialog-watch (catalogued decisions, journal);
#  4. find the report document by cycling MDI children (Ctrl+F6 = command 61504) until
#     the frame title matches -ReportTitlePattern — the document title is NOT the menu
#     name (40016 'Survey Point Clearances' opens 'Terrain Clearances by Span Report');
#  5. Save As (33356): the filename edit is the id-1001 edit whose text is a file name
#     (the Address bar is also id 1001); Save id 1; answer 'Convert to rtf?' with No.
#
# Document titles (16.81): 40014 'Structure Locations and Usage Report'? -> pattern 'Usage',
# 40016 'Terrain Clearances by Span Report', 40402 'Structure Loads Report',
# 32806 'Feature Code Report', 40403 'Section Sag-Tension', 40412 'Staking Table'.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DsRep {
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
}
"@
if (Test-Path -LiteralPath $OutputPath) { throw "Output already exists: $OutputPath" }
function Windows { & "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match 'vis=True' } }
function Kids([long]$h) { & "$here\pls-windows.ps1" -ProcessId $ProcessId -Children $h }
function Handle([string]$row) { [long](($row -split ' ')[0]) }
function Ctl([string[]]$kids, [string]$pattern) { ($kids | Where-Object { $_ -match $pattern }) -replace '^\s*child (\d+).*', '$1' | Select-Object -First 1 }
function VisibleEnabled([string[]]$kids, [int]$id) { Ctl $kids "id=$id .*vis=True en=True" }
function Click([long]$h) { & "$here\pls-windows.ps1" -ProcessId $ProcessId -Click $h | Out-Null }
function TypeInto([long]$edit, [string]$text) {
    [DsRep]::SendMessage([IntPtr]$edit, 0x00B1, [IntPtr]0, [IntPtr](-1)) | Out-Null
    foreach ($ch in [int[]][char[]]$text) { [DsRep]::SendMessage([IntPtr]$edit, 0x0102, [IntPtr]$ch, [IntPtr]1) | Out-Null }
}
function Post([int]$id) { [DsRep]::PostMessage([IntPtr]$MainWindowHandle, 0x0111, [IntPtr]$id, [IntPtr]::Zero) | Out-Null }
function Watch([int]$seconds, [string]$until) {
    $r = & "$here\pls-dialog-watch.ps1" -ProcessId $ProcessId -MainWindowHandle $MainWindowHandle -TimeoutSeconds $seconds -UntilTitle $until -JournalPath $JournalPath | ConvertFrom-Json
    foreach ($e in $r.events) { [void]$journal.Add(($e | ConvertTo-Json -Compress -Depth 4)) }
    $r
}
$journal = [System.Collections.ArrayList]::new()

# ---- 1. the command
if (-not $SaveOpenDocument) {
    Post $CommandId
    Start-Sleep -Seconds 3
}

# ---- 2. the options dialog, if any
$deadline = [DateTime]::UtcNow.AddSeconds(30)
$options = $null
while (-not $SaveOpenDocument -and [DateTime]::UtcNow -lt $deadline -and -not $options) {
    $options = Windows | Where-Object { $_ -match " Report'$" -and $_ -notmatch "'PLS-CADD - " } | Select-Object -First 1
    if (-not $options) { Start-Sleep -Seconds 1 }
}
if ($options) {
    $h = Handle $options
    $kids = Kids $h
    foreach ($id in $DialogChecks.Keys) {
        $ctl = Ctl $kids "id=$id "
        if ($ctl) { & "$here\pls-control.ps1" -SetCheck ([long]$ctl) -Checked ([int]$DialogChecks[$id]) | Out-Null; [void]$journal.Add("check id $id = $($DialogChecks[$id])") }
        else { [void]$journal.Add("check id $id NOT FOUND on the options dialog") }
    }
    foreach ($id in $DialogEdits.Keys) {
        $ctl = Ctl $kids "id=$id "
        if ($ctl) { TypeInto ([long]$ctl) ([string]$DialogEdits[$id]); [void]$journal.Add("edit id $id = $($DialogEdits[$id])") }
        else { [void]$journal.Add("edit id $id NOT FOUND on the options dialog") }
    }
    if ($AllFeatureCodes) {
        $inc = Ctl $kids "id=2938 "
        if (-not $inc) { throw 'AllFeatureCodes: button 2938 (Feature codes to include) not on this dialog' }
        Click ([long]$inc); Start-Sleep -Seconds 2
        $sel = Windows | Where-Object { $_ -match "'Feature codes to include'$" } | Select-Object -First 1
        if (-not $sel) { throw 'Feature codes to include dialog did not open' }
        $sk = Kids (Handle $sel)
        Click ([long](VisibleEnabled $sk 3)); Start-Sleep -Milliseconds 800
        Click ([long](VisibleEnabled $sk 1)); Start-Sleep -Seconds 2
        $kids = Kids $h
        [void]$journal.Add("feature codes to include: $((Ctl $kids 'id=2938 ') -ne $null) -> $(($kids | Where-Object { $_ -match 'id=2938 ' }) -replace '^.*id=2938 \[\] vis=\w+ en=\w+ ', '')")
    }
    # a large project builds the tabbed options dialog slowly: its controls exist but are still hidden
    # when first enumerated (Nyamagabe v19 40014, 2026-09-24); re-read until the visible OK appears
    $ok = VisibleEnabled $kids 1
    $okDeadline = [DateTime]::UtcNow.AddSeconds(15)
    while (-not $ok -and [DateTime]::UtcNow -lt $okDeadline) {
        Start-Sleep -Milliseconds 500
        $kids = Kids $h
        $ok = VisibleEnabled $kids 1
    }
    if (-not $ok) { throw "options dialog '$options' has no visible+enabled OK (id 1); controls: $($kids -join ' || ')" }
    [void]$journal.Add("options accepted: $options via $ok")
    Click ([long]$ok)
}

# ---- 3./4. let the report run; catalogued prompts are answered; find the document
if (-not $SaveOpenDocument) {
    $w = Watch $TimeoutSeconds ''
    if ($w.outcome -notin @('ready')) { throw "report run ended with '$($w.outcome)'; journal: $($journal -join ' ;; ')" }
}
# Some reports (40016 on large projects) keep computing into an 'Untitled' document with no
# progress dialog after the watcher reports ready, so poll until the deadline, not a fixed count.
# Ctrl+F6 is posted ONLY while the frame is enabled and answers WM_NULL: posts made while PLS
# computes queue up and replay afterwards, cycling past the report after the title matched
# (2026-09-24 v17cap4: 40016 matched, queued cycles moved to 'Structure Usage Graph', Save As
# never came). A match must hold across two responsive checks before Save As is posted.
function Title { (Get-Process -Id $ProcessId).MainWindowTitle }
function Responsive {
    $r = [IntPtr]::Zero
    ([DsRep]::SendMessageTimeout([IntPtr]$MainWindowHandle, 0x0000, [IntPtr]::Zero, [IntPtr]::Zero, 0x0002, 2000, [ref]$r) -ne [IntPtr]::Zero) -and [DsRep]::IsWindowEnabled([IntPtr]$MainWindowHandle)
}
function SaveAsWindow { Windows | Where-Object { $_ -match "'Save As'$" } | Select-Object -First 1 }
# A modal can appear AFTER the watcher returned 'ready' (2026-09-24 incoming Rev A2: the 40016
# clearance-voltage Warning came up once the watcher had finished). While the frame is disabled
# or unresponsive, one catalogued watcher pass runs: known dialogs are answered and journaled,
# unknown/stop/flow ones end the run. A short loop (not -Once) so a click that did not take is
# escalated on the next pass (pls-dialog-watch ClickButton).
function FindDocument {
    $docDeadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        if (SaveAsWindow) { return $true }
        if (Responsive) {
            if ((Title) -match $ReportTitlePattern) {
                Start-Sleep -Seconds 2
                if ((Responsive) -and (Title) -match $ReportTitlePattern) { return $true }
            } else { Post 61504 }
        } else {
            $w = & "$here\pls-dialog-watch.ps1" -ProcessId $ProcessId -MainWindowHandle $MainWindowHandle -TimeoutSeconds 6 -JournalPath $JournalPath | ConvertFrom-Json
            foreach ($e in $w.events) { [void]$journal.Add(($e | ConvertTo-Json -Compress -Depth 4)) }
            if ($w.outcome -in @('unknown', 'stop', 'flow')) { throw "dialog while finding the report document ('$($w.outcome)'); journal: $($journal -join ' ;; ')" }
        }
        Start-Sleep -Seconds 2
    } while ([DateTime]::UtcNow -lt $docDeadline)
    return $false
}

# ---- 5. Save As (up to 3 attempts; never post a second 33356 while a Save As is up)
$save = $null
for ($attempt = 1; $attempt -le 3 -and -not $save; $attempt++) {
    $save = SaveAsWindow
    if ($save) { break }
    if (-not (FindDocument)) { throw "no MDI document matching '$ReportTitlePattern' (frame: '$(Title)'); journal: $($journal -join ' ;; ')" }
    $save = SaveAsWindow
    if ($save) { break }
    [void]$journal.Add("report document: $(Title) (attempt $attempt)")
    Post 33356
    for ($i = 0; $i -lt 20 -and -not $save; $i++) { Start-Sleep -Milliseconds 500; $save = Windows | Where-Object { $_ -match "'Save As'$" } | Select-Object -First 1 }
    if (-not $save) { [void]$journal.Add("Save As did not appear on attempt $attempt (frame: '$(Title)')") }
}
if (-not $save) { throw "Save As dialog did not appear after 3 attempts; journal: $($journal -join ' ;; ')" }
$kids = Kids (Handle $save)
$fileEdit = Ctl $kids "id=1001 .*vis=True en=True '[^']*\.[A-Za-z0-9]{1,4}'"
$saveBtn = Ctl $kids "id=1 .*vis=True en=True '&Save'"
if (-not $fileEdit -or -not $saveBtn) { throw "Save As controls not found: $($kids -join ' || ')" }
& "$here\pls-control.ps1" -SetText ([long]$fileEdit) -Text $OutputPath | Out-Null
$readback = & "$here\pls-control.ps1" -GetText ([long]$fileEdit)
if ($readback -ne $OutputPath) { throw "filename did not take: '$readback'" }
Click ([long]$saveBtn)
for ($i = 0; $i -lt 40 -and -not (Test-Path -LiteralPath $OutputPath); $i++) {
    Start-Sleep -Milliseconds 500
    if ($Rtf) {
        $conv = Windows | Where-Object { $_ -match "'PLS-CADD'$" -and $_ -notmatch "owner=0 " } | Select-Object -First 1
        if ($conv) {
            $ck = Kids (Handle $conv)
            if (($ck -join ' ') -match 'Convert to rtf') {
                $yes = VisibleEnabled $ck 6
                if (-not $yes) { throw "Convert-to-rtf prompt without a visible Yes (id 6): $($ck -join ' || ')" }
                Click ([long]$yes)
                [void]$journal.Add('convert to rtf: Yes')
                continue
            }
        }
    }
    $r = & "$here\pls-dialog-watch.ps1" -ProcessId $ProcessId -MainWindowHandle $MainWindowHandle -TimeoutSeconds 2 -Once -JournalPath $JournalPath | ConvertFrom-Json
    foreach ($e in $r.events) { [void]$journal.Add(($e | ConvertTo-Json -Compress -Depth 4)) }
}
if (-not (Test-Path -LiteralPath $OutputPath)) { throw "Report was not written to $OutputPath; journal: $($journal -join ' ;; ')" }
[ordered]@{
    schema = 'ds.pls.report_any.v2'
    command_id = $CommandId
    report_title = (Get-Process -Id $ProcessId).MainWindowTitle
    output = $OutputPath
    bytes = (Get-Item -LiteralPath $OutputPath).Length
    sha256 = (Get-FileHash -LiteralPath $OutputPath -Algorithm SHA256).Hash.ToLowerInvariant()
    journal = @($journal)
} | ConvertTo-Json -Depth 4
