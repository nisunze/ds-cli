param(
    [Parameter(Mandatory = $true)][string[]] $RtfPath,
    [switch] $A3Landscape,
    [switch] $Overwrite
)
# Convert saved PLS-CADD report RTFs to PDF with Microsoft Word (owner 2026-09-24: report deliverables are
# PDFs made from the RTF save). Each <name>.rtf becomes <name>.pdf beside it. -A3Landscape sets every section to
# A3 landscape before export (the incoming Nyamagabe reports are A3 landscape, so wide PLS tables do not wrap).
# An existing PDF is refused unless -Overwrite. Word runs invisibly through COM and is always quit.
$ErrorActionPreference = 'Stop'
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
            if ($A3Landscape) {
                foreach ($s in $doc.Sections) {
                    $s.PageSetup.Orientation = 1                 # wdOrientLandscape
                    $s.PageSetup.PageWidth = 1190.55             # A3 landscape, points
                    $s.PageSetup.PageHeight = 841.89
                }
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
