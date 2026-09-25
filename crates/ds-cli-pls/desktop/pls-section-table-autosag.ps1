param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [Parameter(Mandatory = $true)][string] $EvidenceDirectory,
    [int] $TimeoutSeconds = 600
)
# AutoSag EVERY section with the project's own Automatic Sagging criteria, through
# Sections > Table (40606) - the route characterized 2026-09-24 on Nyamagabe v18 (catalogue
# entry section_table). 40337 (Lite 'Wires > AutoSag') must never be posted: it crashes 16.81.
#  1. open the Section Table (pls-dialog-capture -Leave, PNG evidence);
#  2. click row 1 of 'Command To Apply' with the real cursor parked (pls-click-at -MoveCursor);
#     the in-place combo that opens must carry EXACTLY the 19 characterized commands - that
#     is the proof the click landed in that column - then select 'AutoSag';
#  3. click row 1 'Sec #' (read-only) to end the edit; the combo must hide;
#  4. context menu on row 1 Command cell -> 'Copy && Fill Column' (38079): every row;
#     '&Goto Last Row With Data' (38061) + PrintWindow shows the last rows (evidence);
#  5. OK (WM_COMMAND 1): the commands run; the catalogued watcher must return 'ready';
#  6. reopen the table for an after-PNG and leave it with Cancel (WM_COMMAND 2).
# Any mismatch cancels the table (nothing applied) and throws. The effect is proven
# downstream by Section Usage (40015). The grid geometry is this host's full-screen table
# (1920x1047); the combo identity check refuses anything else.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
if (-not (Test-Path -LiteralPath $EvidenceDirectory -PathType Container)) { throw "Evidence directory does not exist: $EvidenceDirectory" }
$ev = $EvidenceDirectory
$journal = Join-Path $ev 'autosag-journal.jsonl'
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class DsAutoSag {
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L, T, R, B; }
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
}
"@
function Log([string]$event, [hashtable]$data) {
    $data.event = $event; $data.at = [DateTime]::UtcNow.ToString('o')
    ($data | ConvertTo-Json -Compress -Depth 5) | Add-Content -Encoding utf8 $journal
}
function Kids([long]$h) { @(& "$here\pls-windows.ps1" -ProcessId $ProcessId -Children $h) }
function Ctl([string[]]$rows, [string]$pattern) { @($rows | Where-Object { $_ -match $pattern } | ForEach-Object { [long]($_ -replace '^\s*child (\d+).*', '$1') }) }
function Command([long]$dialog, [int]$id, [long]$button) { [DsAutoSag]::PostMessage([IntPtr]$dialog, 0x0111, [IntPtr]$id, [IntPtr]$button) | Out-Null }
function Shot([string]$name) { & "$here\pls-printwindow.ps1" -WindowHandle $script:dialog -OutputPath (Join-Path $ev $name) | Out-Null; $name }
$expectedCommands = @('', 'Clip', 'Unclip', 'Delete', 'Modify', 'AutoSag', 'Reverse Stringing', 'Auto RS', 'Manual RS',
    'Adjust Sag Temp.', 'Find Graph Sag Fit Points...', 'Graph Sag Fit to Points...', 'Create Points Along Wires...',
    'Delete Concentrated Loads', 'Clear length adjust', 'Merge length adjust', 'Set Surveyed Temp', 'Find Surveyed Temp', 'Pretension')

if (-not [DsAutoSag]::IsWindowEnabled([IntPtr]$MainWindowHandle)) { throw 'PLS-CADD frame is disabled (a modal is up); refusing' }
$cap = & "$here\pls-dialog-capture.ps1" -ProcessId $ProcessId -MainWindowHandle $MainWindowHandle -CommandId 40606 -OutputStem (Join-Path $ev 'autosag-01-open') -Leave -TimeoutSeconds 120 | ConvertFrom-Json
if ($cap.title -cne 'Section Table') { throw "40606 opened '$($cap.title)', not 'Section Table'" }
$script:dialog = [long]$cap.handle
$evidence = [System.Collections.ArrayList]::new(); [void]$evidence.Add('autosag-01-open.png')
$applied = $false
try {
    $kids = Kids $script:dialog
    $grid = Ctl $kids "id=131 .*vis=True en=True 'Section Table\d+_131'$"
    if ($grid.Count -ne 1) { throw "Section Table grid (id 131 'Section Table<n>_131') not found exactly once" }
    $grid = $grid[0]
    $rc = New-Object DsAutoSag+RECT; [DsAutoSag]::GetClientRect([IntPtr]$grid, [ref]$rc) | Out-Null
    $cmdX = $rc.R - 58; $row1Y = 108; $row2Y = 132; $secX = 60
    Log 'section_table_open' @{ dialog = $script:dialog; grid = $grid; grid_client = "$($rc.R)x$($rc.B)"; command_x = $cmdX }

    # 2. the Command To Apply editor on row 1
    & "$here\pls-click-at.ps1" -WindowHandle $grid -X $cmdX -Y $row1Y -MoveCursor | Out-Null
    $combo = $null
    for ($i = 0; $i -lt 20 -and -not $combo; $i++) {
        Start-Sleep -Milliseconds 250
        foreach ($c in (Ctl (Kids $script:dialog) 'vis=True en=True')) {
            if ($c -eq $grid) { continue }
            $list = & "$here\pls-combo.ps1" -ComboHandle $c | ConvertFrom-Json
            if ($list.count -gt 0) { $combo = $list; break }
        }
    }
    if (-not $combo) { throw 'no in-place combo opened on row 1' }
    if ((@($combo.items) -join '|') -cne ($expectedCommands -join '|')) { throw "row-1 editor is not Command To Apply: $(@($combo.items) -join ' | ')" }
    $set = & "$here\pls-combo.ps1" -ComboHandle $combo.handle -Select 'AutoSag' | ConvertFrom-Json
    if ($set.selected -cne 'AutoSag') { throw "combo shows '$($set.selected)' after selecting AutoSag" }
    Log 'row1_command_autosag' @{ combo = $combo.handle; items = $combo.count }

    # 3. end the edit on a read-only cell
    & "$here\pls-click-at.ps1" -WindowHandle $grid -X $secX -Y $row1Y -MoveCursor | Out-Null
    for ($i = 0; $i -lt 12 -and [DsAutoSag]::IsWindowVisible([IntPtr]$combo.handle); $i++) { Start-Sleep -Milliseconds 250 }
    if ([DsAutoSag]::IsWindowVisible([IntPtr]$combo.handle)) { throw 'row-1 editor did not close' }
    [void]$evidence.Add((Shot 'autosag-02-row1.png'))

    # 4. copy row 1 down the whole column; show the last rows. Copy & Fill goes through the Windows
    # clipboard: when another program reads the clipboard between PLS's copy and its paste, PLS shows
    # 'Ttable::clipboard_paste' / 'No clipboard data available' and the column is NOT filled (seen
    # 2026-09-25 on the v19 cap6 proof run; the OK behind it applied AutoSag to row 1 only). That box
    # is dismissed (OK id 2) and the fill redone, up to 3 attempts; it is never left behind an OK.
    function PasteError { & "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match "vis=True .*'Ttable::clipboard_paste'$" } | ForEach-Object { [long](($_.Trim() -split ' ')[0]) } | Select-Object -First 1 }
    for ($attempt = 1; ; $attempt++) {
        $fill = & "$here\pls-context-menu.ps1" -ProcessId $ProcessId -WindowHandle $grid -X $cmdX -Y $row1Y -Choose 'Copy && Fill Column' -ExpectedId 38079 | ConvertFrom-Json
        Start-Sleep -Seconds 2
        $pasteError = PasteError
        if (-not $pasteError) {
            [void]$evidence.Add((Shot 'autosag-03-filled-top.png'))
            $last = & "$here\pls-context-menu.ps1" -ProcessId $ProcessId -WindowHandle $grid -X $cmdX -Y $row2Y -Choose '&Goto Last Row With Data' -ExpectedId 38061 | ConvertFrom-Json
            Start-Sleep -Seconds 1
            $pasteError = PasteError
        }
        if (-not $pasteError) { break }
        $text = ((Kids $pasteError) | Where-Object { $_ -match 'id=65535 ' }) -replace "^.*?'(.*)'$", '$1'
        Log 'fill_paste_failed' @{ attempt = $attempt; box = $pasteError; text = "$text" }
        $okBox = Ctl (Kids $pasteError) "id=2 .*vis=True en=True 'OK'$"
        if ($okBox.Count -ne 1) { throw "clipboard_paste box without a single OK (id 2): $((Kids $pasteError) -join ' || ')" }
        Command $pasteError 2 $okBox[0]
        for ($i = 0; $i -lt 20 -and [DsAutoSag]::IsWindow([IntPtr]$pasteError); $i++) { Start-Sleep -Milliseconds 250 }
        if ([DsAutoSag]::IsWindow([IntPtr]$pasteError)) { throw 'clipboard_paste box did not close' }
        if ($attempt -ge 3) { throw "Copy && Fill Column failed $attempt times (clipboard): the column is not filled" }
        Start-Sleep -Seconds 2
    }
    Log 'copy_fill_column' @{ chosen = $fill.chosen; menu_items = @($fill.items).Count; attempts = $attempt }
    [void]$evidence.Add((Shot 'autosag-04-filled-last.png'))

    # 5. OK runs the commands
    $ok = Ctl (Kids $script:dialog) "id=1 .*vis=True en=True '&OK'$"
    if ($ok.Count -ne 1) { throw 'Section Table OK (id 1) not found exactly once' }
    Log 'section_table_ok' @{ button = $ok[0] }
    Command $script:dialog 1 $ok[0]
    $applied = $true
} catch {
    if (-not $applied -and [DsAutoSag]::IsWindow([IntPtr]$script:dialog)) {
        $cancel = Ctl (Kids $script:dialog) "id=2 .*vis=True en=True '&Cancel'$"
        if ($cancel.Count -eq 1) { Command $script:dialog 2 $cancel[0]; Log 'section_table_cancelled' @{ reason = $_.Exception.Message } }
    }
    throw
}
Start-Sleep -Seconds 2
$w = & "$here\pls-dialog-watch.ps1" -ProcessId $ProcessId -MainWindowHandle $MainWindowHandle -TimeoutSeconds $TimeoutSeconds -JournalPath $journal | ConvertFrom-Json
if ($w.outcome -ne 'ready') { throw "after OK the watcher ended '$($w.outcome)'; see $journal" }
if ([DsAutoSag]::IsWindow([IntPtr]$script:dialog)) { throw 'Section Table still open after OK' }

# 6. after-evidence, left with Cancel
$after = & "$here\pls-dialog-capture.ps1" -ProcessId $ProcessId -MainWindowHandle $MainWindowHandle -CommandId 40606 -OutputStem (Join-Path $ev 'autosag-05-after') -Leave -TimeoutSeconds 120 | ConvertFrom-Json
[void]$evidence.Add('autosag-05-after.png')
$cancel = Ctl (Kids ([long]$after.handle)) "id=2 .*vis=True en=True '&Cancel'$"
if ($cancel.Count -ne 1) { throw 'after-view Cancel not found' }
Command ([long]$after.handle) 2 $cancel[0]
Start-Sleep -Seconds 2
if ([DsAutoSag]::IsWindow([IntPtr]$after.handle)) { throw 'after-view Section Table did not close' }
Log 'autosag_done' @{ watcher = $w.outcome; events = @($w.events).Count }
[ordered]@{
    schema = 'ds.pls.section_table_autosag.v1'
    evidence_directory = $ev
    evidence = @($evidence)
    combo_items = @($combo.items).Count
    fill = $fill.chosen
    goto_last = $last.chosen
    watcher_after_ok = [ordered]@{ outcome = $w.outcome; events = @($w.events) }
} | ConvertTo-Json -Depth 6
