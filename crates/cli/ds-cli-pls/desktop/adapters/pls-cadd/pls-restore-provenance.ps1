param(
    [Parameter(Mandatory = $true)][string] $Workspace,     # stick/shaded workspace written by `ds pls shading-variants`
    [Parameter(Mandatory = $true)][string] $SourceProject, # the project folder the native backup was made from
    [Parameter(Mandatory = $true)][string] $Receipt        # JSON receipt to write
)
$ErrorActionPreference = 'Stop'
# Until the installed ds carries the 2026-09-29 provenance fix, `ds pls shading-variants` relabels each structure's
# loads-file line as "DS Grid 16.81 analytical scaffold". Put the source project's own line back (that line only);
# a file whose source also carries the scaffold is left and listed: it has no native source to restore.
$latin1 = [System.Text.Encoding]::GetEncoding(28591)
$scaffold = 'Results generated for loads file: "DS Grid 16.81 analytical scaffold'
$restored = @(); $left = @()
foreach ($file in Get-ChildItem -LiteralPath (Join-Path $Workspace 'structures') -File | Where-Object { $_.Extension -in '.012', '.014' } | Sort-Object Name) {
    $text = $latin1.GetString([System.IO.File]::ReadAllBytes($file.FullName))
    $crlf = $text.Contains("`r`n")
    $lines = $text.Replace("`r`n", "`n").Split("`n")
    $hits = @(for ($i = 0; $i -lt $lines.Count; $i++) { if ($lines[$i].StartsWith($scaffold)) { $i } })
    if ($hits.Count -eq 0) { continue }
    $source = Join-Path (Join-Path $SourceProject 'structures') $file.Name
    if (-not (Test-Path -LiteralPath $source)) { $left += [ordered]@{ file = $file.Name; reason = 'no source file' }; continue }
    $good = @($latin1.GetString([System.IO.File]::ReadAllBytes($source)).Replace("`r`n", "`n").Split("`n") |
        Where-Object { $_.StartsWith('Results generated for loads file:') -and -not $_.Contains('analytical scaffold') })
    if ($good.Count -eq 0) { $left += [ordered]@{ file = $file.Name; reason = 'source also scaffold' }; continue }
    foreach ($i in $hits) { $lines[$i] = $good[0] }
    $out = $lines -join "`n"
    if ($crlf) { $out = $out.Replace("`n", "`r`n") }
    [System.IO.File]::WriteAllBytes($file.FullName, $latin1.GetBytes($out))
    $restored += [ordered]@{ file = $file.Name; lines = $hits.Count; provenance = $good[0].Substring(34) }
}
[ordered]@{ restored = $restored; left = $left } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath $Receipt -Encoding UTF8
"restored $($restored.Count) | left $(($left | ForEach-Object { $_.file + ' (' + $_.reason + ')' }) -join ', ')"
