param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [Parameter(Mandatory = $true)][string] $JournalPath,
    [int] $TimeoutSeconds = 60
)
# Exit PLS-CADD (57665) without saving. 'Save changes to <project>?' is answered No by the catalogued
# watcher (save_changes, id 7); any other prompt that is not catalogued stops here with PLS left open.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
& (Join-Path $here 'pls-command.ps1') -WindowHandle $MainWindowHandle -CommandId 57665 -Post | Out-Null
$events = @()
for ($i = 0; $i -lt $TimeoutSeconds -and (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue); $i++) {
    Start-Sleep -Seconds 1
    if (-not (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue)) { break }
    try { $w = & (Join-Path $here 'pls-dialog-watch.ps1') -ProcessId $ProcessId -MainWindowHandle $MainWindowHandle -TimeoutSeconds 3 -Once -JournalPath $JournalPath | ConvertFrom-Json }
    catch { continue }   # the process can vanish while the watcher reads its windows
    foreach ($e in @($w.events)) { $events += "$($e.event) $($e.dialog)" }
    if ($w.outcome -in @('unknown', 'stop', 'flow')) { throw "dialog at exit ($($w.outcome)): $($events -join '; ')" }
}
if (Get-Process -Id $ProcessId -ErrorAction SilentlyContinue) { throw "PLS-CADD did not exit within $TimeoutSeconds s" }
"exited ($($events -join '; '))"
