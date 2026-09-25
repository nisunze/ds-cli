param(
    [Parameter(Mandatory = $true)][string] $CandidateBackupPath,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string] $ExpectedCandidateBackupSha256,
    [Parameter(Mandatory = $true)][string] $RestoreDirectory,
    [Parameter(Mandatory = $true)][string] $EvidenceDirectory,
    [string] $ProjectFileName,
    [string] $SourceRoot,
    [string] $ExecutablePath = 'C:\Program Files\PLS\pls_cadd\pls_cadd64.exe',
    [switch] $CloseAfter,
    [switch] $Execute
)
# INTERIM (2026-09-23) - one fresh native Restore + open with the characterized qualify
# functions (C3a/C3c/C3d fixed upstream in 88c0885) and the patches in
# pls-interim-loader.ps1 (C3e multi-root, C3f post-open lock). Leaves PLS-CADD
# open on the restored project for report driving unless -CloseAfter; close later with
# pls-close-interim.ps1, which also runs the post-close Protected check (C3f: an open
# project locks its .xyz, so hashing it while open fails).
# Proven 2026-09-23 on the approved Gisagara rev 3 control: 145 files, 0 skipped, opened.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'pls-interim-loader.ps1')

$script:JournalPath = $null
if (-not $Execute) { throw 'Refusing to launch PLS-CADD without -Execute' }
$executable = Assert-PlsRegularFile $ExecutablePath 'PLS-CADD executable'
$exeDigest = Get-PlsFileSha256 $executable
if ($exeDigest -cne ([string] $profile.ExecutableSha256).ToLowerInvariant()) { throw "PLS-CADD executable digest does not match profile: $exeDigest" }
$version = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($executable)
if (-not (Test-PlsExecutableVersion $version.FileVersion ([string] $profile.ProductVersion))) { throw "PLS-CADD version mismatch: $($version.FileVersion)" }
$candidate = Assert-PlsRegularFile $CandidateBackupPath 'candidate backup'
$candidateDigest = Get-PlsFileSha256 $candidate
if ($candidateDigest -cne $ExpectedCandidateBackupSha256.ToLowerInvariant()) { throw "Candidate digest mismatch: $candidateDigest" }
Assert-FreshAbsolutePath $RestoreDirectory 'restore directory' $true
Assert-FreshAbsolutePath $EvidenceDirectory 'evidence directory' $true
$running = @(Get-Process -Name 'pls_cadd64' -ErrorAction SilentlyContinue)
if ($running.Count -ne 0) { throw "Refusing to run while PLS-CADD is already running (PID(s): $($running.Id -join ', '))" }

[System.IO.Directory]::CreateDirectory($EvidenceDirectory) | Out-Null
$script:JournalPath = Join-Path $EvidenceDirectory 'journal.jsonl'
[System.IO.File]::Open($script:JournalPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read).Dispose()
Write-Journal 'interim_preflight' ([ordered]@{
    script = $MyInvocation.MyCommand.Path; driver = $driver; driver_sha256 = (Get-PlsFileSha256 $driver)
    executable_sha256 = $exeDigest; executable_version = $version.FileVersion
    candidate = $candidate; candidate_sha256 = $candidateDigest; restore_directory = $RestoreDirectory
    source_root_override = $SourceRoot
})
$payload = Resolve-PlsNativeBackupPayload $candidate $candidateDigest (Join-Path $EvidenceDirectory 'candidate-payload')
$inventory = Get-PlsNativeBackupInventory $payload.native_path $ProjectFileName $SourceRoot
Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'candidate-inventory.json') $inventory
$logStartBytes = if (Test-Path -LiteralPath $PlsLogPath) { (Get-Item -LiteralPath $PlsLogPath).Length } else { 0 }

$prompts = New-Object System.Collections.ArrayList
$repairs = New-Object System.Collections.ArrayList
$opened = Invoke-PlsRestoreInterim $executable $payload.native_path $inventory $RestoreDirectory $prompts $repairs

$logTail = @()
if (Test-Path -LiteralPath $PlsLogPath) {
    $bytes = [System.IO.File]::ReadAllBytes($PlsLogPath)
    if ($bytes.Length -gt $logStartBytes) {
        $logTail = [System.Text.Encoding]::Default.GetString($bytes, [int] $logStartBytes, $bytes.Length - [int] $logStartBytes) -split "`r?`n" | Where-Object { $_ }
    }
}
$result = [ordered]@{
    schema = 'ds.pls.interim_restore_open.v1'
    # Production Invoke-PlsRestore (7e9f6da, C3f) checks presence/count before open and leaves
    # byte verification to after close, so an opened project here is presence-verified only.
    status = 'opened_presence_verified'
    at_utc = [DateTime]::UtcNow.ToString('o')
    executable = [ordered]@{ path = $executable; sha256 = $exeDigest; version = $version.FileVersion }
    candidate = [ordered]@{ path = $candidate; container = $payload.container_format; sha256 = $candidateDigest; native_sha256 = $inventory.native_backup_sha256; project_file = $inventory.project_file; source_root = $inventory.project_source_root; counts = $inventory.counts }
    restore = [ordered]@{ directory = $RestoreDirectory; project = $opened.project_path; process_id = $opened.process.Id; main_window_handle = $opened.main_window_handle; frame_title = (Get-Process -Id $opened.process.Id).MainWindowTitle }
    pre_open_presence = $opened.pre_open_presence_verification
    post_close_protected = 'run pls-close-interim.ps1 (C3f: the open project locks its .xyz)'
    open_prompts = @($prompts)
    repairs = @($repairs)
    log_tail = @($logTail)
    caveat = 'Interim driver: restore/open integrity only. PLS-CADD 16.81 on this host may lack SAPS (see qualify caveat); not engineering acceptance.'
}
Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'restore-open.json') $result
if ($CloseAfter) {
    & (Join-Path $PSScriptRoot 'pls-close-interim.ps1') -ProcessId $opened.process.Id -EvidenceDirectory $EvidenceDirectory -RestoreDirectory $RestoreDirectory
}
$result | ConvertTo-Json -Depth 8
