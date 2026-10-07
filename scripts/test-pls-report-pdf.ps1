[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string] $ConverterPath,
    [Parameter(Mandatory = $true)][string] $OutputDirectory,
    [string[]] $NativeRtf = @()
)
$ErrorActionPreference = 'Stop'
if ($PSVersionTable.PSVersion.Major -ne 5 -or $PSVersionTable.PSVersion.Minor -ne 1) { throw 'Use Windows PowerShell 5.1 for this native adapter proof.' }
$converter = (Resolve-Path -LiteralPath $ConverterPath).Path
$root = [System.IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $root) { throw 'Choose an absent test output folder.' }
if (-not (Test-Path -LiteralPath (Split-Path $root -Parent) -PathType Container)) { throw 'Test output parent is absent.' }
New-Item -ItemType Directory -Path $root | Out-Null
$beforeConverter = (Get-FileHash -LiteralPath $converter).Hash.ToLowerInvariant()
$tests = @()
$sources = @()
$files = @()
foreach ($paper in @('A4', 'A3')) {
    $rtf = Join-Path $root ("synthetic-$paper.rtf")
    [System.IO.File]::WriteAllText($rtf, '{\rtf1\ansi\deff0{\fonttbl{\f0 Courier New;}}\paperw12240\paperh15840\margl720\margr720\f0\fs20 PLS report converter qualification\par Native source content preserved\par}', [System.Text.Encoding]::ASCII)
    $before = (Get-FileHash -LiteralPath $rtf).Hash.ToLowerInvariant()
    $args = @{ RtfPath = @($rtf); A4Landscape = ($paper -eq 'A4'); A3Landscape = ($paper -eq 'A3') }
    $result = & $converter @args | ConvertFrom-Json
    if ((Get-FileHash -LiteralPath $rtf).Hash.ToLowerInvariant() -cne $before) { throw 'Synthetic RTF bytes changed.' }
    $pdfText = [System.Text.Encoding]::GetEncoding(28591).GetString([System.IO.File]::ReadAllBytes($result.pdf))
    $boxes = [regex]::Matches($pdfText, '/MediaBox\s*\[\s*0(?:\.0+)?\s+0(?:\.0+)?\s+([0-9.]+)\s+([0-9.]+)\s*\]')
    if ($boxes.Count -lt 1) { throw 'PDF has no readable page MediaBox evidence.' }
    $expectedWidth = $(if ($paper -eq 'A4') { 841.89 } else { 1190.55 })
    $expectedHeight = $(if ($paper -eq 'A4') { 595.28 } else { 841.89 })
    foreach ($box in $boxes) {
        if ([math]::Abs([double]$box.Groups[1].Value - $expectedWidth) -gt 1 -or [math]::Abs([double]$box.Groups[2].Value - $expectedHeight) -gt 1) { throw "$paper PDF page dimensions differ." }
    }
    $tests += [ordered]@{ name = "$paper landscape source-preserving conversion"; passed = $true; pages_with_mediabox = $boxes.Count; converter = $result.converter }
    $files += $result
    $refused = $false
    try { & $converter @args | Out-Null } catch { $refused = $_.Exception.Message -like 'PDF already exists:*' }
    if (-not $refused) { throw 'An existing PDF was not refused.' }
    $tests += [ordered]@{ name = "$paper overwrite guard"; passed = $true }
}
foreach ($source in $NativeRtf) {
    $source = (Resolve-Path -LiteralPath $source).Path
    $hash = (Get-FileHash -LiteralPath $source).Hash.ToLowerInvariant()
    $copy = Join-Path $root ('native-' + [System.IO.Path]::GetFileName($source))
    Copy-Item -LiteralPath $source -Destination $copy
    $result = & $converter -RtfPath @($copy) -A4Landscape | ConvertFrom-Json
    if ((Get-FileHash -LiteralPath $source).Hash.ToLowerInvariant() -cne $hash -or (Get-FileHash -LiteralPath $copy).Hash.ToLowerInvariant() -cne $hash) { throw 'Native source RTF or its test copy changed.' }
    $sources += [ordered]@{ path = $source; sha256 = $hash; preserved = $true }
    $files += $result
    $tests += [ordered]@{ name = 'native saved RTF conversion'; source = $source; passed = $true; converter = $result.converter }
}
if ((Get-FileHash -LiteralPath $converter).Hash.ToLowerInvariant() -cne $beforeConverter) { throw 'Converter source changed during test.' }
$receipt = [ordered]@{ schema = 'ds.pls.report_pdf_test.v1'; runtime = $PSVersionTable.PSVersion.ToString(); converter = $converter; converter_sha256 = $beforeConverter; native_models_touched = $false; tests = $tests; preserved_sources = $sources; pdfs = $files }
$receiptPath = Join-Path $root 'test-receipt.json'
[System.IO.File]::WriteAllText($receiptPath, ($receipt | ConvertTo-Json -Depth 8), [System.Text.UTF8Encoding]::new($false))
$receipt | ConvertTo-Json -Depth 8
