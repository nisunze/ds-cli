param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [Parameter(Mandatory = $true)][double] $StartStation,
    [Parameter(Mandatory = $true)][double] $StopStation,
    [string] $Cable = 'acsr 70-12mm2',
    [double] $RulingSpan = 80,
    [double] $MinSpan = 10,
    [double] $MaxSpan = 150,
    [double] $Spacing = 5,
    [switch] $Respot,
    [int] $TimeoutSeconds = 7200,
    [string] $JournalPath = ''
)
# One PLS-CADD 16.81 optimum spotting run over a typed station range (Structures > Automatic Spotting > Optimum
# Spotting > Select Station Range, 40954), driven through the dialogs characterized on host Magese 2026-09-26:
#   'Start Station for Optimization' / 'Stop Station for Optimization' (after any click in the Profile View pane):
#       radio 2876 'Use station to the right' + edit 2755 = the typed station, OK 1;
#   'Open Cable File' (conductor the optimizer strings, edit 1148); 'Available Structure List' (OK 1: the steering
#   table is loaded beforehand from a .str file, never edited here);
#   'Optimum Spotting': Automatic sagging 2304, ruling span 2286, min span 2287, max span 2288, station spacing 2289,
#       respot if ruling span more than 5 % off 2306, merge 2303 (on), existing locations only 2305 (off), OK 1;
#   warnings 'Continue displaying warning messages ...?' are recorded and redirected (No 7) to the Error Log, and the
#   run is reported as failed. Any other dialog stops the driver (PLS left as is).
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
& (Join-Path $here 'pls-units-check.ps1') | Out-Null
function Log([string]$m) { $l = '{0} {1}' -f [DateTime]::UtcNow.ToString('o'), $m; if ($JournalPath) { Add-Content -LiteralPath $JournalPath -Value $l -Encoding utf8 }; $l }
function Modals { & "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match 'vis=True' -and $_ -notmatch "^$MainWindowHandle " } }
function Handle([string]$row) { [long](($row -split ' ')[0]) }
function Text([long]$h) { (& "$here\pls-dialog-text.ps1" -WindowHandle $h 2>$null | Where-Object { $_ -match 'id=65535' }) -replace '^.*id=65535 \| ', '' }
function ButtonId([long]$dlg, [int]$id) { [long]((& "$here\pls-safe-dump.ps1" -Dialog $dlg | Where-Object { $_ -match " id=$id Button" } | Select-Object -First 1) -replace ' .*', '') }
function WaitTitle([string]$pattern, [int]$sec) {
    for ($i = 0; $i -lt $sec * 2; $i++) { $m = Modals | Where-Object { $_ -match $pattern } | Select-Object -First 1; if ($m) { return Handle $m }; Start-Sleep -Milliseconds 500 }
    0
}
# the Profile View pane receives the two picks
for ($i = 0; $i -lt 8 -and (Get-Process -Id $ProcessId).MainWindowTitle -notmatch 'Profile View'; $i++) {
    & "$here\pls-command.ps1" -WindowHandle $MainWindowHandle -CommandId 61504 -Post; Start-Sleep -Milliseconds 800
}
$view = (& "$here\pls-windows.ps1" -ProcessId $ProcessId -Children $MainWindowHandle | Where-Object { $_ -match "'Profile View'$" } | Select-Object -First 1) -replace '^\s*child (\d+).*', '$1'
$pane = (& "$here\pls-windows.ps1" -ProcessId $ProcessId -Children ([long]$view) | Where-Object { $_ -match 'id=59648' } | Select-Object -First 1) -replace '^\s*child (\d+).*', '$1'
if (-not $pane) { throw 'Profile View pane not found' }
$t0 = [DateTime]::UtcNow
& "$here\pls-command.ps1" -WindowHandle $MainWindowHandle -CommandId 40954 -Post; Start-Sleep 2
foreach ($step in @(@{ t = 'Start Station for Optimization'; s = $StartStation; x = 900 }, @{ t = 'Stop Station for Optimization'; s = $StopStation; x = 1100 })) {
    & "$here\pls-click-at.ps1" -WindowHandle ([long]$pane) -X $step.x -Y 500 -MoveCursor | Out-Null
    $d = WaitTitle "\] '$($step.t)'$" 15
    if (-not $d) { throw "no '$($step.t)' dialog: $(Modals)" }
    $st = ('{0:0.000}' -f $step.s)
    Log "$($step.t): $st $(& "$here\pls-dialog-fill.ps1" -Dialog $d -Checks '2876=1,2877=0,2878=0,2879=0' -Texts "2755=$st" -Accept 1)"
    Start-Sleep 2
}
$failed = @(); $ran = $false
while (([DateTime]::UtcNow - $t0).TotalSeconds -lt $TimeoutSeconds) {
    $m = @(Modals)
    if ($m.Count -eq 0) { if ($ran) { break }; Start-Sleep 2; continue }
    $row = $m | Where-Object { $_ -notmatch "\] 'Error Log'$" } | Select-Object -First 1
    if (-not $row) { if ($ran) { break }; Start-Sleep 2; continue }
    $h = Handle $row
    switch -Regex ($row) {
        "\] 'Open Cable File'$" {
            $e = [long]((& "$here\pls-safe-dump.ps1" -Dialog $h | Where-Object { $_ -match ' id=1148 Edit' } | Select-Object -First 1) -replace ' .*', '')
            & "$here\pls-type-path.ps1" -DialogHandle $h -EditHandle $e -Path $Cable | Out-Null; Log "cable $Cable"; Start-Sleep 2; continue
        }
        "\] 'Available Structure List'$" { & "$here\pls-press.ps1" -Button (ButtonId $h 1) | Out-Null; Log 'structure list accepted as loaded'; Start-Sleep 2; continue }
        "\] 'Optimum Spotting'$" {
            $checks = "2304=1,2303=1,2305=0,2307=0,2306=$([int][bool]$Respot)"
            $texts = "2286=$RulingSpan,2287=$MinSpan,2288=$MaxSpan,2289=$Spacing"
            Log "optimum spotting $(& "$here\pls-dialog-fill.ps1" -Dialog $h -Checks $checks -Texts $texts -Accept 1)"
            $ran = $true; Start-Sleep 3; continue
        }
        "\] 'PLS-CADD'$|\] 'Error in " {
            $body = Text $h
            if ($body -match 'Continue displaying warning messages') {
                $failed += $body.Substring(0, [Math]::Min(300, $body.Length)); Log "WARNING: $body"
                & "$here\pls-press.ps1" -Button (ButtonId $h 7) | Out-Null; Start-Sleep 2; continue
            }
            throw "unknown PLS-CADD prompt (left open): $body"
        }
        default { if ($ran -and $row -match "Optimiz|Spotting|Progress|Wait") { Start-Sleep 5; continue }; throw "unknown dialog (left open): $row" }
    }
}
$elapsed = [int]([DateTime]::UtcNow - $t0).TotalSeconds
[ordered]@{ schema = 'ds.pls.optimum_run.v1'; start = $StartStation; stop = $StopStation; spacing_m = $Spacing; respot = [bool]$Respot
            ran = $ran; seconds = $elapsed; warnings = $failed; error_log_open = [bool](Modals | Where-Object { $_ -match "'Error Log'$" }) } | ConvertTo-Json -Depth 4
