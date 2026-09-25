param(
    [Parameter(Mandatory = $true)][string] $ResultPath,
    [Parameter(Mandatory = $true)][string] $ProjectPath,
    [Parameter(Mandatory = $true)][string] $RunDirectory,
    [int] $ReportTimeoutSeconds = 1800
)
# ds pls desktop reports - the deliver chain's six reports from a saved project: open, the six
# RTFs with their verdict lines, Exit without saving, then A3 landscape PDFs from the RTFs with
# Word (pls-rtf-to-pdf.ps1). Nothing is saved to the project.
. (Join-Path $PSScriptRoot 'ds-desktop-lib.ps1')
Invoke-DsEntry $ResultPath 'reports' {
    Assert-DsWord
    $script:run = New-DsRunDirectory $RunDirectory @('reports')
    Assert-DsPlsNotRunning
    $launch = Open-DsProject $ProjectPath
    $reports = Invoke-DsReports (Join-Path $run 'reports') $ReportTimeoutSeconds
    ExitPls 'project'
    $pdfs = & (Join-Path $here 'pls-rtf-to-pdf.ps1') -A3Landscape -RtfPath @($reports.Values | ForEach-Object { $_.rtf }) | ConvertFrom-Json
    foreach ($p in @($pdfs)) { $k = [System.IO.Path]::GetFileNameWithoutExtension($p.pdf); $reports[$k].pdf = $p.pdf; $reports[$k].pdf_bytes = $p.bytes }
    Log "report pdfs $(@($pdfs).Count)"
    $receipt = Join-Path $run 'reports.json'
    Write-DsDocument $receipt ([ordered]@{
        schema = 'ds.pls.desktop_reports.v1'
        project = [ordered]@{ path = $launch.project_path; sha256 = $launch.project_sha256 }
        reports = $reports
    })
    $script:DsResult = [ordered]@{ receipt = $receipt }
}
