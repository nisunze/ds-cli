param(
    [Parameter(Mandatory = $true)][string] $ResultPath,
    [Parameter(Mandatory = $true)][string] $ProjectPath,
    [Parameter(Mandatory = $true)][string] $RunDirectory
)
# ds pls desktop sheets-pdf - every plan & profile sheet of a saved project to one PDF with
# PLS-CADD's own exporter, as the deliver chain does it: open, the Sheets View,
# pls-save-sheets-pdf.ps1, Exit without saving.
. (Join-Path $PSScriptRoot 'ds-desktop-lib.ps1')
Invoke-DsEntry $ResultPath 'sheets-pdf' {
    $script:run = New-DsRunDirectory $RunDirectory @('pdf')
    Assert-DsPlsNotRunning
    $launch = Open-DsProject $ProjectPath
    Enter-DsSheetsView
    $sheets = & (Join-Path $here 'pls-save-sheets-pdf.ps1') -ProcessId $script:procId -MainWindowHandle $script:frame `
        -OutputPdf (Join-Path $run 'pdf\Plan and Profile.pdf') -JournalPath (Join-Path $run 'watch-journal.jsonl') | ConvertFrom-Json
    Log "sheets $($sheets.bytes) bytes in $($sheets.seconds) s"
    ExitPls 'project'
    $receipt = Join-Path $run 'sheets.json'
    Write-DsDocument $receipt ([ordered]@{
        schema = 'ds.pls.desktop_sheets_pdf.v1'
        project = [ordered]@{ path = $launch.project_path; sha256 = $launch.project_sha256 }
        sheets = [ordered]@{ pdf = $sheets.pdf; bytes = $sheets.bytes; seconds = $sheets.seconds }
    })
    $script:DsResult = [ordered]@{ receipt = $receipt }
}
