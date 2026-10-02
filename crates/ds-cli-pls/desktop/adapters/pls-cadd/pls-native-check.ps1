param(
    [Parameter(Mandatory = $true)][string] $BackupPath,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string] $ExpectedBackupSha256,
    [Parameter(Mandatory = $true)][string] $RunDirectory,
    [Parameter(Mandatory = $true)][ValidatePattern('^[A-Za-z0-9._-]+$')][string] $Label,
    [string] $SourceRoot,
    [int] $ReportTimeoutSeconds = 1800,
    [ValidateSet('structure_usage', 'terrain_clearances', 'wind_weight_span', 'section_usage')]
    [string[]] $Reports = @('structure_usage', 'terrain_clearances', 'wind_weight_span'),
    [switch] $SkipWindWeight
)
# One native PLS-CADD check of a .bak (DS-exported or incoming), end to end:
#   1. fresh Restore + open (interim\pls-restore-open-interim.ps1: exact dialogs, 0 skipped,
#      catalogued open prompts only, no Repair Wizard);
#   2. native reports saved as text, chosen by -Reports: 40014 Structure Usage, 40016 Survey
#      Point Clearances (all feature codes; it computes for minutes on large models), 40020
#      Wind & Weight Spans, 40015 Section Usage;
#   3. exit without saving + post-close Protected tree check (interim\pls-close-interim.ps1);
#   4. one receipt, native-check.json, with the backup, executable and report digests.
# Everything goes under -RunDirectory: r1\ (restored workspace), ev\ (journals), reports\.
# Owner rule 2026-09-24: project work lives on the project Google Drive, never on C:.
# Replaces the laptop-only automation lost on 2026-09-24. Proven on the Nyamagabe v17 cap2 .bak.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$here = $PSScriptRoot

$run = [System.IO.Path]::GetFullPath($RunDirectory)
if ($run -match '^[Cc]:') { throw "Refusing a run directory on C: ($run): project work lives on the Drive" }
if (Test-Path -LiteralPath $run) { throw "Run directory already exists: $run (every check starts fresh)" }
$backup = [System.IO.Path]::GetFullPath($BackupPath)
$digest = (Get-FileHash -LiteralPath $backup -Algorithm SHA256).Hash.ToLowerInvariant()
if ($digest -cne $ExpectedBackupSha256.ToLowerInvariant()) { throw "Backup digest mismatch: $digest" }
if (Get-Process pls_cadd64 -ErrorAction SilentlyContinue) { throw 'PLS-CADD is already running; close it first' }

$restoreDir = Join-Path $run 'r1'; $evDir = Join-Path $run 'ev'; $repDir = Join-Path $run 'reports'
New-Item -ItemType Directory -Path $run, $repDir | Out-Null

$restoreArgs = @{
    CandidateBackupPath = $backup; ExpectedCandidateBackupSha256 = $digest
    RestoreDirectory = $restoreDir; EvidenceDirectory = $evDir; Execute = $true
}
if ($SourceRoot) { $restoreArgs.SourceRoot = $SourceRoot }
& (Join-Path $here 'interim\pls-restore-open-interim.ps1') @restoreArgs | Out-Null
$opened = Get-Content -LiteralPath (Join-Path $evDir 'restore-open.json') -Raw | ConvertFrom-Json
$procId = [int] $opened.restore.process_id
$frame = [long] $opened.restore.main_window_handle

$saved = [ordered]@{}
$catalog = @{
    structure_usage = @{ id = 40014; pattern = 'Structure Usage Report'; all = $false }
    terrain_clearances = @{ id = 40016; pattern = 'Terrain Clearances by Span'; all = $true }
    wind_weight_span = @{ id = 40020; pattern = 'Wind & Weight Span'; all = $false }
    section_usage = @{ id = 40015; pattern = 'Section Usage Report'; all = $false }
}
$plan = @(foreach ($k in $Reports) {
    if ($SkipWindWeight -and $k -eq 'wind_weight_span') { continue }
    $catalog[$k] + @{ key = $k }
})
try {
    foreach ($r in $plan) {
        $out = Join-Path $repDir "$Label-$($r.key).txt"
        $reportArgs = @{
            ProcessId = $procId; MainWindowHandle = $frame; CommandId = $r.id; OutputPath = $out
            ReportTitlePattern = $r.pattern; JournalPath = (Join-Path $repDir "journal-$($r.id).jsonl")
            TimeoutSeconds = $ReportTimeoutSeconds
        }
        if ($r.all) { $reportArgs.AllFeatureCodes = $true }
        $saved[$r.key] = & (Join-Path $here 'pls-report-any.ps1') @reportArgs | ConvertFrom-Json
    }
} finally {
    $close = & (Join-Path $here 'interim\pls-close-interim.ps1') -ProcessId $procId -EvidenceDirectory $evDir -RestoreDirectory $restoreDir 2>$null
}
$closed = ($close | Out-String) | ConvertFrom-Json

$receipt = [ordered]@{
    schema = 'ds.pls.native_check.v1'
    label = $Label
    at_utc = [DateTime]::UtcNow.ToString('o')
    host = $env:COMPUTERNAME
    backup = [ordered]@{ path = $backup; sha256 = $digest }
    executable = $opened.executable
    restore = [ordered]@{ directory = $restoreDir; restored_files = $opened.pre_open_presence.verified_files; open_prompts = $opened.open_prompts; repairs = $opened.repairs }
    reports = [ordered]@{}
    close = [ordered]@{ status = $closed.status; post_close_protected = $closed.post_close_protected }
}
foreach ($k in $saved.Keys) { $receipt.reports[$k] = [ordered]@{ path = $saved[$k].output; bytes = $saved[$k].bytes; sha256 = $saved[$k].sha256 } }
$json = $receipt | ConvertTo-Json -Depth 8
[System.IO.File]::WriteAllText((Join-Path $run 'native-check.json'), $json, [System.Text.UTF8Encoding]::new($false))
$json
