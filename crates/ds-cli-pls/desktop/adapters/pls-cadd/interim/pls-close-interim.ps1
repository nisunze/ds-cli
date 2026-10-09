param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][string] $EvidenceDirectory,
    [string] $RestoreDirectory
)
# INTERIM (2026-09-23) - close a PLS-CADD run without saving via the characterized
# Close-PlsWithoutSaving (C3a fixed upstream in 88c0885), then,
# when -RestoreDirectory is given and EvidenceDirectory holds candidate-inventory.json from
# pls-restore-open-interim.ps1, run the Full tree check (every backup member, each missing
# path named) and the Protected check after close (C3f: while the project is open PLS locks
# its .xyz and the hash cannot be read).
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'pls-interim-loader.ps1')
$script:JournalPath = Join-Path $EvidenceDirectory 'journal.jsonl'
if (-not (Test-Path -LiteralPath $script:JournalPath)) {
    [System.IO.Directory]::CreateDirectory($EvidenceDirectory) | Out-Null
    [System.IO.File]::Open($script:JournalPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read).Dispose()
}
$process = Get-Process -Id $ProcessId -ErrorAction Stop
if ($process.Path -cne 'C:\Program Files\PLS\pls_cadd\pls_cadd64.exe') { throw "Unexpected executable: $($process.Path)" }
$title = $process.MainWindowTitle
Write-Journal 'interim_close_start' ([ordered]@{ process_id = $ProcessId; frame_title = $title })
Close-PlsWithoutSavingInterim $process ([long] $process.MainWindowHandle)

$full = $null
$protected = $null
if ($RestoreDirectory) {
    $inventoryPath = Join-Path $EvidenceDirectory 'candidate-inventory.json'
    $inventory = Get-Content -Raw -LiteralPath $inventoryPath | ConvertFrom-Json
    # Every backup member, as the production qualifier checks after each native
    # close: a Restore dialog's file count is not evidence that the files are
    # there (feedback 1086e058). A failure names every missing path.
    try {
        $check = Test-PlsRestoredTree $inventory $RestoreDirectory 'Full' @($profile.AllowedFreshRestoreExtras)
        $full = [ordered]@{ verified_files = $check.verified_files; verified_digest = $check.verified_digest; expected_members = @($inventory.members | Where-Object { $_.kind -ne 'directory' }).Count }
    } catch {
        $full = [ordered]@{ failed = $true; message = $_.Exception.Message }
    }
    Write-Journal 'post_close_full_check' $full
    try {
        $check = Test-PlsRestoredTree $inventory $RestoreDirectory 'Protected'
        $protected = [ordered]@{ verified_files = $check.verified_files; verified_digest = $check.verified_digest; expected_protected = $inventory.digests.protected; members = @($check.verified_members | Group-Object verification | ForEach-Object { "$($_.Name)=$($_.Count)" }) }
    } catch {
        $protected = [ordered]@{ failed = $true; message = $_.Exception.Message }
    }
    Write-Journal 'post_close_protected_check' $protected
}
[ordered]@{ schema = 'ds.pls.interim_close.v1'; status = 'closed_without_saving'; process_id = $ProcessId; frame_title_before = $title; post_close_full = $full; post_close_protected = $protected } | ConvertTo-Json -Depth 6
