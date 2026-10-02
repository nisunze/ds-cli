param(
    [Parameter(Mandatory = $true)][string] $Path,   # a native PLS-CADD report saved as RTF, or a folder of them
    [string[]] $Structures = @(),                    # rows whose first cell is one of these structure numbers
    [switch] $NgOnly,                                # rows carrying an NG verdict (e.g. Terrain Clearances by Span)
    [switch] $Summary,                               # Structure Usage: structure count and its NG rows
    [int] $Width = 260
)
$ErrorActionPreference = 'Stop'
# Reads native report text out of the RTF PLS-CADD saves: paragraph/row breaks become lines, cells become " | ",
# control words and groups are dropped. Replaces the 2026-09-29 Python readers (rtf rows, usage NG, NG lines).
$latin1 = [System.Text.Encoding]::GetEncoding(28591)

function Get-ReportLines([string] $File) {
    $t = $latin1.GetString([System.IO.File]::ReadAllBytes($File))
    $t = [regex]::Replace($t, '\\(par|row)d?\b', "`n")
    $t = [regex]::Replace($t, '\\cell\b', ' | ')
    $t = [regex]::Replace($t, '\\[a-zA-Z]+-?\d* ?', '')
    $t.Replace('{', '').Replace('}', '') -split "`n"
}

function Split-Cells([string] $Line) {
    if ($Line.Contains('|')) { @($Line.Split('|') | ForEach-Object { $_.Trim() } | Where-Object { $_ }) }
    else { @($Line.Split([char[]]" `t", [System.StringSplitOptions]::RemoveEmptyEntries)) }
}

$files = if (Test-Path -LiteralPath $Path -PathType Container) {
    Get-ChildItem -LiteralPath $Path -Filter *.rtf | Sort-Object Name | ForEach-Object FullName
} else { @($Path) }
$wanted = [System.Collections.Generic.HashSet[string]]::new([string[]]$Structures)

foreach ($file in $files) {
    Write-Output "##### $(Split-Path -Leaf $file)"
    $lines = Get-ReportLines $file
    if ($Summary) {
        $seen = [System.Collections.Generic.HashSet[string]]::new()
        $rows = foreach ($line in $lines) {
            $c = Split-Cells $line
            if ($c.Count -ge 6 -and $c[0] -match '^\d+$' -and $c[1] -match '^[a-z].*\.(012|014)$' -and $seen.Add($c[0])) { , $c }
        }
        $ng = @($rows | Where-Object { $_ -contains 'NG' })
        Write-Output "structures $(@($rows).Count) | with NG $($ng.Count)"
        foreach ($r in $ng) { $s = $r -join ' | '; Write-Output ('   ' + $s.Substring(0, [Math]::Min($Width, $s.Length))) }
        if (($lines -join "`n").Contains('analytical scaffold')) { Write-Output 'scaffold label present: True' }
        continue
    }
    if ($NgOnly) {
        $ng = [System.Collections.Generic.List[string]]::new()
        foreach ($line in $lines) {
            if ($line -match '(^|\s|\|)NG(\s|\||$)') {
                $s = ($line -split '\s+' | Where-Object { $_ }) -join ' '
                if (-not $ng.Contains($s)) { $ng.Add($s) }
            }
        }
        Write-Output "NG rows $($ng.Count)"
        foreach ($s in $ng) { Write-Output ('   ' + $s.Substring(0, [Math]::Min($Width, $s.Length))) }
        continue
    }
    foreach ($line in $lines) {
        $c = Split-Cells $line
        if ($c.Count -gt 0 -and $wanted.Contains($c[0])) {
            $s = $c -join ' | '
            Write-Output ('   ' + $s.Substring(0, [Math]::Min($Width, $s.Length)))
        }
    }
}
