param(
    [Parameter(Mandatory = $true)][string] $ResultPath,
    [Parameter(Mandatory = $true)][string] $BackupPath,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{64}$')][string] $ExpectedBackupSha256,
    [Parameter(Mandatory = $true)][string] $RunDirectory,
    [string] $SourceRoot,
    [string] $ProjectFileName
)
# ds pls desktop qualify - the two-restore native qualification of a .bak with
# pls-backup-restore-qualify.ps1: Restore r1 and open, PLS File > Backup, close, Full and
# Protected tree checks, Restore that fresh backup as r2, close, checks again. Its targets
# are four siblings in one new run folder; its manifest.json is the receipt.
. (Join-Path $PSScriptRoot 'ds-desktop-lib.ps1')
Invoke-DsEntry $ResultPath 'qualify' {
    $script:run = New-DsRunDirectory $RunDirectory @()
    $a = @{ ExecutablePath = $PlsExecutable; ExpectedExecutableSha256 = ([string] $DsProfile.ExecutableSha256)
            CandidateBackupPath = $BackupPath; ExpectedCandidateBackupSha256 = $ExpectedBackupSha256
            FirstRestoreDirectory = (Join-Path $run 'r1'); FreshBackupPath = (Join-Path $run 'fresh-pls-backup.bak')
            SecondRestoreDirectory = (Join-Path $run 'r2'); EvidenceDirectory = (Join-Path $run 'evidence'); Execute = $true }
    if ($SourceRoot) { $a.SourceRoot = $SourceRoot }
    if ($ProjectFileName) { $a.ProjectFileName = $ProjectFileName }
    & (Join-Path $here 'pls-backup-restore-qualify.ps1') @a | Out-Null
    $script:DsResult = [ordered]@{ receipt = (Join-Path $run 'evidence\manifest.json') }
}
