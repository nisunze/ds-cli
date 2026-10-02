param(
    [Parameter(Mandatory = $true)][string[]] $RtfPath,
    [switch] $A3Landscape,
    [switch] $A4Landscape,
    [switch] $Overwrite
)
# Convert saved PLS-CADD report RTFs to PDF with Microsoft Word (owner 2026-09-24: report deliverables are
# PDFs made from the RTF save). Each <name>.rtf becomes <name>.pdf beside it.
# Explicit A4/A3 landscape overrides apply to every section; neither changes the source RTF.
# An existing PDF is refused unless -Overwrite. Word runs invisibly through COM and is always quit.
$ErrorActionPreference = 'Stop'
if ($A3Landscape -and $A4Landscape) { throw 'Select exactly one PDF paper override.' }
function Set-PlsReportPaper([object] $Sections, [ValidateSet('A4', 'A3')][string] $Paper) {
    foreach ($section in $Sections) {
        $section.PageSetup.Orientation = 1                       # wdOrientLandscape
        $section.PageSetup.PageWidth = $(if ($Paper -eq 'A4') { 841.89 } else { 1190.55 })
        $section.PageSetup.PageHeight = $(if ($Paper -eq 'A4') { 595.28 } else { 841.89 })
    }
}
$word = New-Object -ComObject Word.Application
$word.Visible = $false
$word.DisplayAlerts = 0
$out = @()
try {
    foreach ($p in $RtfPath) {
        $src = (Resolve-Path -LiteralPath $p).Path
        $pdf = [System.IO.Path]::ChangeExtension($src, '.pdf')
        if ((Test-Path -LiteralPath $pdf) -and -not $Overwrite) { throw "PDF already exists: $pdf" }
        $doc = $word.Documents.Open($src, $false, $true)   # ConfirmConversions false, ReadOnly true
        try {
            if ($A3Landscape -or $A4Landscape) {
                Set-PlsReportPaper $doc.Sections $(if ($A4Landscape) { 'A4' } else { 'A3' })
            }
            $doc.ExportAsFixedFormat($pdf, 17)                   # wdExportFormatPDF
        } finally {
            $doc.Close(0)
        }
        $out += [ordered]@{ rtf = $src; pdf = $pdf; bytes = (Get-Item -LiteralPath $pdf).Length }
    }
} finally {
    $word.Quit()
    [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($word)
}
$out | ConvertTo-Json -Depth 3
