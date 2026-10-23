param(
    [Parameter(Mandatory = $true)][string] $ResultPath,
    [Parameter(Mandatory = $true)][string] $ProjectPath,
    [Parameter(Mandatory = $true)][string] $RunDirectory,
    [int] $ReportTimeoutSeconds = 1800,
    [ValidateSet('A4', 'A3')][string] $PdfPaper = 'A4',
    [switch] $IncludeWindWeightSpan,
    [switch] $RtfOnly,
    [int] $AttachProcessId = 0
)
# ds pls desktop reports - the five canonical submission RTFs and A4 landscape PDFs.
# The supplementary wind/weight report and A3 paper require explicit customization.
# -RtfOnly skips Word and PDFs; -AttachProcessId reports from a proven open session
# and leaves it open. Nothing is saved to the project.
. (Join-Path $PSScriptRoot 'ds-desktop-lib.ps1')
Invoke-DsEntry $ResultPath 'reports' {
    if (-not $RtfOnly) { Assert-DsReportConverter }
    $script:run = New-DsRunDirectory $RunDirectory @('reports')
    $launch = Connect-DsProject $ProjectPath $AttachProcessId
    $reports = Invoke-DsReports (Join-Path $run 'reports') $ReportTimeoutSeconds ([bool] $IncludeWindWeightSpan)
    if ($AttachProcessId -eq 0) { ExitPls 'project' } else { Assert-DsAttachedProject }
    if (-not $RtfOnly) {
        $paperArgs = @{ A4Landscape = ($PdfPaper -eq 'A4'); A3Landscape = ($PdfPaper -eq 'A3') }
        $pdfs = & (Join-Path $here '..\word\pls-rtf-to-pdf.ps1') @paperArgs -RtfPath @($reports.Values | ForEach-Object { $_.rtf }) | ConvertFrom-Json
        foreach ($p in @($pdfs)) { $k = [System.IO.Path]::GetFileNameWithoutExtension($p.pdf); $reports[$k].pdf = $p.pdf; $reports[$k].pdf_bytes = $p.bytes; $reports[$k].pdf_converter = $p.converter }
        Log "report pdfs $(@($pdfs).Count)"
    }
    $receipt = Join-Path $run 'reports.json'
    $document = [ordered]@{
        schema = 'ds.pls.desktop_reports.v1'
        project = [ordered]@{ path = $launch.project_path; sha256 = $launch.project_sha256 }
    }
    # Paper exists only when PDFs were made; an RTF-only receipt carries no PDF fields.
    if (-not $RtfOnly) { $document.pdf_paper = $PdfPaper; $document.pdf_orientation = 'landscape' }
    $document.reports = $reports
    $document.report_format = $(if ($RtfOnly) { 'rtf_only' } else { 'rtf_pdf' })
    $document.session = Get-DsSession $AttachProcessId
    Write-DsDocument $receipt $document
    $script:DsResult = [ordered]@{ receipt = $receipt }
}
