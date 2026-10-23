param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [double] $AlignmentGap = 100,
    [string] $JournalPath = ''
)
# Plan & profile paging settings a multi-alignment DS export needs before its sheets can be cut
# (Nyamagabe 2026-09-24: with the export's 1 m gap and page starts rounded to the 20 m label interval,
# paging stalled at the end of alignment 1 - 'Unable to cut pages' / 'No progress in ...'):
#  1. Terrain > Alignment > Multiple Alignment Options (40576): 'Start new plan and profile sheet for
#     each alignment' (check 2816) on; 'Station gap to insert between alignments' (edit 2817) =
#     -AlignmentGap (default 100 m, the incoming Rev A2 value; owner: default 100, changeable); OK 1.
#  2. Drafting > Plan & Profile Sheet Configuration > Scales (40050): page-start rounding radios
#     2540 'Multiple of station label interval' / 2541 'Multiple of station grid interval' /
#     2542 'Do not round' -> 2542; OK 1.
#  3. Both dialogs are reopened and read back, then left with Cancel (2): the proof the values took.
# Drafting and station display only; the engineering data is untouched. The caller saves (40003).
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
function Kids([long]$h) { @(& "$here\pls-windows.ps1" -ProcessId $ProcessId -Children $h) }
function Ctl([string[]]$kids, [string]$pattern) { ($kids | Where-Object { $_ -match $pattern }) -replace '^\s*child (\d+).*', '$1' | Select-Object -First 1 }
function Id([string[]]$kids, [int]$id) { $c = Ctl $kids "id=$id "; if (-not $c) { throw "control id $id not on the dialog: $($kids -join ' || ')" }; [long]$c }
function OpenDialog([int]$command, [string]$title) {
    & "$here\pls-command.ps1" -WindowHandle $MainWindowHandle -CommandId $command -Post | Out-Null
    for ($i = 0; $i -lt 40; $i++) {
        Start-Sleep -Milliseconds 500
        # a sheet configuration dialog first appears as a 1x1 window with no children (catalogue pp_config)
        $row = & "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match "vis=True.*'$([regex]::Escape($title))'$" } | Select-Object -First 1
        if ($row) { $h = [long](($row.Trim() -split ' ')[0]); $k = Kids $h; if ($k.Count -ge 3) { return @{ handle = $h; kids = $k } } }
    }
    throw "'$title' did not open after command $command"
}
function Close([long]$dialog, [int]$button) {
    & "$here\pls-command.ps1" -WindowHandle $dialog -CommandId $button -Post | Out-Null
    Start-Sleep -Seconds 2
    $w = & "$here\pls-dialog-watch.ps1" -ProcessId $ProcessId -MainWindowHandle $MainWindowHandle -TimeoutSeconds 120 -JournalPath $JournalPath | ConvertFrom-Json
    if ($w.outcome -ne 'ready') { throw "PLS-CADD not ready after closing the dialog with $button ($($w.outcome)): $(($w.events | ConvertTo-Json -Compress -Depth 4))" }
}
function Check([long]$ctl) { [int]((& "$here\pls-control.ps1" -GetCheck $ctl) -replace '^.*= ', '') }
function ReadState {
    $m = OpenDialog 40576 'Multiple Alignment Options'
    $gap = & "$here\pls-control.ps1" -GetText (Id $m.kids 2817)
    $perAlignment = Check (Id $m.kids 2816)
    Close $m.handle 2
    $s = OpenDialog 40050 'Scales'
    $rounding = @{ 2540 = 'label_interval'; 2541 = 'grid_interval'; 2542 = 'do_not_round' }
    $mode = ($rounding.Keys | Where-Object { (Check (Id $s.kids $_)) -eq 1 } | ForEach-Object { $rounding[$_] }) -join ','
    Close $s.handle 2
    [ordered]@{ alignment_gap_m = [double]$gap; new_sheet_per_alignment = $perAlignment; page_start_rounding = $mode }
}

$before = ReadState
$m = OpenDialog 40576 'Multiple Alignment Options'
& "$here\pls-control.ps1" -SetCheck (Id $m.kids 2816) -Checked 1 | Out-Null
$gapText = $AlignmentGap.ToString('0.00', [Globalization.CultureInfo]::InvariantCulture)
& "$here\pls-control.ps1" -SetText (Id $m.kids 2817) -Text $gapText | Out-Null
$echo = & "$here\pls-control.ps1" -GetText (Id $m.kids 2817)
if ($echo -ne $gapText) { throw "gap edit readback '$echo' <> '$gapText'" }
Close $m.handle 1
$s = OpenDialog 40050 'Scales'
foreach ($id in 2540, 2541, 2542) { & "$here\pls-control.ps1" -SetCheck (Id $s.kids $id) -Checked ([int]($id -eq 2542)) | Out-Null }
Close $s.handle 1
$after = ReadState
if ($after.alignment_gap_m -ne $AlignmentGap -or $after.new_sheet_per_alignment -ne 1 -or $after.page_start_rounding -ne 'do_not_round') {
    throw "paging settings did not take: $($after | ConvertTo-Json -Compress)"
}
[ordered]@{ schema = 'ds.pls.sheet_paging.v1'; before = $before; after = $after } | ConvertTo-Json -Compress -Depth 4
