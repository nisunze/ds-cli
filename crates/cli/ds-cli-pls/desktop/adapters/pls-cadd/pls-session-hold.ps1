param(
    [Parameter(Mandatory = $true)][string] $BackupPath,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string] $ExpectedBackupSha256,
    [Parameter(Mandatory = $true)][string] $RunDirectory,
    [string] $SourceRoot,
    [int] $HoldHours = 10
)
# Interactive working session: one fresh native Restore + open (the interim driver), then this script
# stays alive until PLS-CADD exits, so a background Bash job that runs it keeps the session's process
# tree intact while other tool calls drive the open project (menu ids, catalogued dialogs, reports).
# Writes <RunDirectory>\session.json (pid, frame handle, project) as soon as the project is open.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
& (Join-Path $here 'pls-units-check.ps1') | Out-Null
$run = [System.IO.Path]::GetFullPath($RunDirectory)
if ($run -match '^[Cc]:') { throw "Refusing a run directory on C: ($run)" }
if (Test-Path -LiteralPath $run) { throw "Run directory already exists: $run" }
New-Item -ItemType Directory -Path $run | Out-Null
$a = @{ CandidateBackupPath = $BackupPath; ExpectedCandidateBackupSha256 = $ExpectedBackupSha256
        RestoreDirectory = (Join-Path $run 'r1'); EvidenceDirectory = (Join-Path $run 'ev-r1'); Execute = $true }
if ($SourceRoot) { $a.SourceRoot = $SourceRoot }
& (Join-Path $here 'interim\pls-restore-open-interim.ps1') @a | Out-Null
$o = Get-Content -LiteralPath (Join-Path $run 'ev-r1\restore-open.json') -Raw | ConvertFrom-Json
$session = [ordered]@{
    process_id = [int] $o.restore.process_id
    main_window_handle = [long] $o.restore.main_window_handle
    project = $o.restore.project
    backup = $BackupPath
    backup_sha256 = $ExpectedBackupSha256
    opened_utc = [DateTime]::UtcNow.ToString('o')
}
$session | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $run 'session.json') -Encoding utf8
$session | ConvertTo-Json -Compress
$deadline = [DateTime]::UtcNow.AddHours($HoldHours)
while ([DateTime]::UtcNow -lt $deadline -and (Get-Process -Id $session.process_id -ErrorAction SilentlyContinue)) {
    Start-Sleep -Seconds 10
}
"session ended $([DateTime]::UtcNow.ToString('o')) (PLS-CADD running: $([bool](Get-Process -Id $session.process_id -ErrorAction SilentlyContinue)))"
