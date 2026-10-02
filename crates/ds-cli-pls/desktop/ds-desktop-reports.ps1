param(
    [Parameter(Mandatory = $true)][string] $ResultPath,
    [Parameter(Mandatory = $true)][string] $ProjectPath,
    [Parameter(Mandatory = $true)][string] $RunDirectory,
    [int] $ReportTimeoutSeconds = 1800,
    [ValidateSet('A4', 'A3')][string] $PdfPaper = 'A4',
    [switch] $IncludeWindWeightSpan
)
# ds pls desktop reports - the five canonical submission RTFs and A4 landscape PDFs.
# The supplementary wind/weight report and A3 paper require explicit customization.
# Nothing is saved to the project.
. (Join-Path $PSScriptRoot 'ds-desktop-lib.ps1')
Invoke-DsEntry $ResultPath 'reports' {
    Assert-DsWord
    $script:run = New-DsRunDirectory $RunDirectory @('reports')
    Assert-DsPlsNotRunning
    $launch = Open-DsProject $ProjectPath
    $reports = Invoke-DsReports (Join-Path $run 'reports') $ReportTimeoutSeconds ([bool] $IncludeWindWeightSpan)
    ExitPls 'project'
    $paperArgs = @{ A4Landscape = ($PdfPaper -eq 'A4'); A3Landscape = ($PdfPaper -eq 'A3') }
    $pdfs = & (Join-Path $here 'pls-rtf-to-pdf.ps1') @paperArgs -RtfPath @($reports.Values | ForEach-Object { $_.rtf }) | ConvertFrom-Json
    foreach ($p in @($pdfs)) { $k = [System.IO.Path]::GetFileNameWithoutExtension($p.pdf); $reports[$k].pdf = $p.pdf; $reports[$k].pdf_bytes = $p.bytes }
    Log "report pdfs $(@($pdfs).Count)"
    $receipt = Join-Path $run 'reports.json'
    Write-DsDocument $receipt ([ordered]@{
        schema = 'ds.pls.desktop_reports.v1'
        project = [ordered]@{ path = $launch.project_path; sha256 = $launch.project_sha256 }
        pdf_paper = $PdfPaper
        pdf_orientation = 'landscape'
        reports = $reports
    })
    $script:DsResult = [ordered]@{ receipt = $receipt }
}
