param(
    [Parameter(Mandatory = $true)][string] $ResultPath,
    [Parameter(Mandatory = $true)][string] $BackupPath,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{64}$')][string] $ExpectedBackupSha256,
    [Parameter(Mandatory = $true)][string] $RestoreDirectory,
    [Parameter(Mandatory = $true)][string] $EvidenceDirectory,
    [string] $SourceRoot,
    [string] $ProjectFileName
)
# ds pls desktop restore - one fresh native PLS-CADD Restore of a .bak into a new folder, open,
# exit without saving, then the post-close Protected tree check. Exactly the proven pair
# interim/pls-restore-open-interim.ps1 then interim/pls-close-interim.ps1 (the order its
# -CloseAfter runs them in); the close result is kept rather than printed.
. (Join-Path $PSScriptRoot 'ds-desktop-lib.ps1')
Invoke-DsEntry $ResultPath 'restore' {
    $a = @{ CandidateBackupPath = $BackupPath; ExpectedCandidateBackupSha256 = $ExpectedBackupSha256
            RestoreDirectory = $RestoreDirectory; EvidenceDirectory = $EvidenceDirectory; Execute = $true }
    if ($SourceRoot) { $a.SourceRoot = $SourceRoot }
    if ($ProjectFileName) { $a.ProjectFileName = $ProjectFileName }
    & (Join-Path $here 'interim\pls-restore-open-interim.ps1') @a | Out-Null
    $receipt = Join-Path $EvidenceDirectory 'restore-open.json'
    $opened = Get-Content -Raw -LiteralPath $receipt | ConvertFrom-Json
    # Consumed as pls-native-check.ps1 (proven on the v17 cap2 .bak) consumes it.
    $closeOutput = & (Join-Path $here 'interim\pls-close-interim.ps1') -ProcessId ([int] $opened.restore.process_id) `
        -EvidenceDirectory $EvidenceDirectory -RestoreDirectory $RestoreDirectory 2>$null
    $close = ($closeOutput | Out-String) | ConvertFrom-Json
    if ($null -eq $close.post_close_protected -or $close.post_close_protected.failed) {
        throw "Restored tree verification failed after close: $($close.post_close_protected.message)"
    }
    $script:DsResult = [ordered]@{ receipt = $receipt; close = $close }
}
