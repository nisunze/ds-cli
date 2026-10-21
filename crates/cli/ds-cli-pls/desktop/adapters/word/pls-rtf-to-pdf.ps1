param(
    [Parameter(Mandatory = $true)][string[]] $RtfPath,
    [switch] $A3Landscape,
    [switch] $A4Landscape,
    [ValidateRange(0, 72)][int] $FontHalfPoints = 0,
    [switch] $Overwrite
)
# Convert saved PLS-CADD report RTFs to PDF with Microsoft Word or supported LibreOffice (owner 2026-09-24: report deliverables are
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

# This is the existing reviewed Windows bridge conversion, now product-owned.
# A private LibreOffice profile prevents the call from using an open office session.
function Convert-PlsReportWithLibreOffice([string] $Source, [string] $Pdf) {
    $soffice = 'C:\Program Files\LibreOffice\program\soffice.exe'
    if (-not (Test-Path -LiteralPath $soffice -PathType Leaf)) {
        throw 'No report PDF converter: Microsoft Word is not registered and LibreOffice is not installed at its supported path'
    }
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ('pls-rtf-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $tmp | Out-Null
    try {
        $copy = Join-Path $tmp ([System.IO.Path]::GetFileName($Source))
        $text = [System.IO.File]::ReadAllText($Source, [System.Text.Encoding]::GetEncoding(1252))
        if ($text -notmatch '^\{\\rtf') { throw 'Report input is not an RTF document' }
        if ($A3Landscape -or $A4Landscape) {
            $width = $(if ($A4Landscape) { 16838 } else { 23811 })
            $height = $(if ($A4Landscape) { 11906 } else { 16838 })
            $text = [regex]::Replace($text, '\\paperw\d+', "\paperw$width")
            $text = [regex]::Replace($text, '\\paperh\d+', "\paperh$height")
            $text = [regex]::Replace($text, '\\pgwsxn\d+', "\pgwsxn$width")
            $text = [regex]::Replace($text, '\\pghsxn\d+', "\pghsxn$height")
            if ($text -notmatch '\\paperw') {
                $text = [regex]::Replace($text, '^\{\\rtf1', "{\rtf1\paperw$width\paperh$height\landscape")
            } elseif ($text -notmatch '\\landscape') {
                $text = ([regex]"\\paperh$height").Replace($text, "\paperh$height\landscape", 1)
            }
        }
        if ($FontHalfPoints -gt 0) {
            $text = [regex]::Replace($text, '\\fs\d+', "\fs$FontHalfPoints")
            $text = [regex]::Replace($text, '\\marg[lrtb]\d+', '')
            $text = ([regex]'^\{\\rtf1').Replace($text, '{\rtf1\margl567\margr567\margt567\margb567', 1)
        }
        [System.IO.File]::WriteAllText($copy, $text, [System.Text.Encoding]::GetEncoding(1252))
        $profile = ([uri](Join-Path $tmp 'office-profile')).AbsoluteUri
        $process = Start-Process -FilePath $soffice -ArgumentList @("-env:UserInstallation=$profile", '--headless', '--norestore', '--convert-to', 'pdf', '--outdir', "`"$tmp`"", "`"$copy`"") -PassThru -WindowStyle Hidden
        if (-not $process.WaitForExit(120000)) {
            # This process and its private profile were created by this invocation.
            $process.Kill()
            $process.WaitForExit()
            throw 'LibreOffice report PDF conversion timed out after 120 seconds'
        }
        $made = [System.IO.Path]::ChangeExtension($copy, '.pdf')
        if ($process.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $made -PathType Leaf)) {
            throw "LibreOffice did not convert $Source (exit $($process.ExitCode))"
        }
        $pdfBytes = [System.IO.File]::ReadAllBytes($made)
        if ($pdfBytes.Length -lt 5 -or [System.Text.Encoding]::ASCII.GetString($pdfBytes, 0, 5) -cne '%PDF-') {
            throw 'LibreOffice report output is not a PDF document'
        }
        $mode = $(if ($Overwrite) { [System.IO.FileMode]::Create } else { [System.IO.FileMode]::CreateNew })
        $stream = [System.IO.File]::Open($Pdf, $mode, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
        try { $stream.Write($pdfBytes, 0, $pdfBytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
    } finally {
        $cleanupRoot = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath()).TrimEnd('\') + '\'
        $cleanupPath = [System.IO.Path]::GetFullPath($tmp)
        if (-not $cleanupPath.StartsWith($cleanupRoot, [System.StringComparison]::OrdinalIgnoreCase) -or
            [System.IO.Path]::GetFileName($cleanupPath) -notmatch '^pls-rtf-[0-9a-f]{32}$') {
            throw "Refusing cleanup outside this converter's temporary directory: $cleanupPath"
        }
        Remove-Item -LiteralPath $cleanupPath -Recurse -Force -ErrorAction SilentlyContinue
    }
}

if (-not [Type]::GetTypeFromProgID('Word.Application')) {
    $out = @()
    foreach ($p in $RtfPath) {
        $src = (Resolve-Path -LiteralPath $p).Path
        if ([System.IO.Path]::GetExtension($src) -ine '.rtf') { throw 'Report input must be an .rtf file' }
        $before = (Get-FileHash -LiteralPath $src -Algorithm SHA256).Hash.ToLowerInvariant()
        $pdf = [System.IO.Path]::ChangeExtension($src, '.pdf')
        if ((Test-Path -LiteralPath $pdf) -and -not $Overwrite) { throw "PDF already exists: $pdf" }
        Convert-PlsReportWithLibreOffice $src $pdf
        if ((Get-FileHash -LiteralPath $src -Algorithm SHA256).Hash.ToLowerInvariant() -cne $before) { throw 'Source RTF changed during report conversion' }
        $out += [ordered]@{ rtf = $src; rtf_sha256 = $before; pdf = $pdf; bytes = (Get-Item -LiteralPath $pdf).Length
            pdf_sha256 = (Get-FileHash -LiteralPath $pdf -Algorithm SHA256).Hash.ToLowerInvariant(); converter = 'libreoffice' }
    }
    $out | ConvertTo-Json -Depth 3
    return
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
        $out += [ordered]@{ rtf = $src; pdf = $pdf; bytes = (Get-Item -LiteralPath $pdf).Length; converter = 'word' }
    }
} finally {
    $word.Quit()
    [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($word)
}
$out | ConvertTo-Json -Depth 3
