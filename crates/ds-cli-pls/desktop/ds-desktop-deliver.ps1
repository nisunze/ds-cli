param(
    [Parameter(Mandatory = $true)][string] $ResultPath,
    [Parameter(Mandatory = $true)][string] $BackupPath,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{64}$')][string] $ExpectedBackupSha256,
    [Parameter(Mandatory = $true)][string] $RunDirectory,
    [Parameter(Mandatory = $true)][string] $Label,
    [string] $SourceRoot,
    [double] $AlignmentGap = 100,
    [int] $ReportTimeoutSeconds = 1800,
    [switch] $NoSheets
)
# ds pls desktop deliver - pls-deliver-autosag.ps1, the proven end-to-end chain, unchanged in
# every step. Word is checked first: the report PDFs are its last step, hours in.
. (Join-Path $PSScriptRoot 'ds-desktop-lib.ps1')
Invoke-DsEntry $ResultPath 'deliver' {
    Assert-DsWord
    $a = @{ BackupPath = $BackupPath; ExpectedBackupSha256 = $ExpectedBackupSha256; RunDirectory = $RunDirectory
            Label = $Label; AlignmentGap = $AlignmentGap; ReportTimeoutSeconds = $ReportTimeoutSeconds }
    if ($SourceRoot) { $a.SourceRoot = $SourceRoot }
    if ($NoSheets) { $a.NoSheets = $true }
    & (Join-Path $here 'pls-deliver-autosag.ps1') @a | Out-Null
    $script:DsResult = [ordered]@{ receipt = (Join-Path ([System.IO.Path]::GetFullPath($RunDirectory)) 'deliver.json') }
}
