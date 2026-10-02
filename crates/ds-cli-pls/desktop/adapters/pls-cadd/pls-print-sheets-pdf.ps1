param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [Parameter(Mandatory = $true)][string] $OutputPdf,
    [int] $FromPage = 1,
    [int] $ToPage = 1,
    [switch] $AllPages,
    [ValidateSet('scaled', 'unscaled', 'screen')][string] $PlotType = 'scaled',
    [string] $Printer = 'Microsoft Print to PDF',
    [int] $TimeoutSeconds = 600
)
# Print a range of P&P sheets from the ACTIVE Sheets View to a PDF through the
# Windows PDF printer. Chain (PLS-CADD 16.81): File › Print (57607) →
# 'Sheet Print Setup' (radios 2430 unscaled / 2431 scaled / 2432 screen dump,
# OK 1) → standard 'Print' dialog (printer combo 1139, All 1056 / Pages 1058,
# from 1152, to 1153, copies 1154, OK 1) → 'Save Print Output As' (the id-1001
# edit whose text is empty or a file name, NOT the Address bar; Save 1). The
# frame is disabled while the driver spools; completion = the PDF exists, is
# stable, and no dialog is left. Anything else is reported, never waited out.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
if (Test-Path -LiteralPath $OutputPdf) { throw "Output already exists: $OutputPdf" }
$title = (Get-Process -Id $ProcessId).MainWindowTitle
if ($title -notmatch '\[Sheets View\]$') { throw "Active view must be the Sheets View, frame title is '$title'" }
function Wins { @(& "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match 'vis=True' -and $_ -notmatch "'PLS-CADD - " -and $_ -notmatch "'Error Log'" }) }
function Kids([long]$h) { @(& "$here\pls-windows.ps1" -ProcessId $ProcessId -Children $h) }
function Handle([string]$row) { [long](($row -split ' ')[0]) }
function Title([string]$row) { if ($row -match "\] '(.*)'$") { $Matches[1] } else { '' } }
function Ctl([string[]]$kids, [string]$pattern) { ($kids | Where-Object { $_ -match $pattern }) -replace '^\s*child (\d+).*', '$1' | Select-Object -First 1 }
function WaitDialog([string]$expected, [int]$seconds) {
    $deadline = [DateTime]::UtcNow.AddSeconds($seconds)
    do {
        Start-Sleep -Milliseconds 500
        # PLS-CADD's own 'Printing' progress box (title 'PLS-CADD', Cancel only) is up while the job spools;
        # it is expected, not an unknown dialog (Nyamagabe 2026-09-24)
        $w = @(Wins | Where-Object { -not ((Title $_) -ceq 'PLS-CADD' -and ((Kids (Handle $_)) -join ' ') -match "'Printing'") })
        $hit = $w | Where-Object { (Title $_) -ceq $expected } | Select-Object -First 1
        if ($hit) { $h = Handle $hit; if ((Kids $h).Count -gt 0) { return $h } }
        elseif ($w.Count -gt 0 -and (Title $w[0]) -ne '') { throw "Unexpected dialog '$(Title $w[0])' while waiting for '$expected'" }
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "'$expected' did not appear within $seconds s"
}
$journal = [System.Collections.ArrayList]::new()
& "$here\pls-command.ps1" -WindowHandle $MainWindowHandle -CommandId 57607 -Post
$setup = WaitDialog 'Sheet Print Setup' 30
$k = Kids $setup
$radio = @{ unscaled = 2430; scaled = 2431; screen = 2432 }[$PlotType]
foreach ($id in 2430, 2431, 2432) { & "$here\pls-control.ps1" -SetCheck ([long](Ctl $k "id=$id ")) -Checked ([int]($id -eq $radio)) | Out-Null }
[void]$journal.Add("sheet_print_setup plot_type=$PlotType")
& "$here\pls-windows.ps1" -ProcessId $ProcessId -Click ([long](Ctl $k "id=1 .*'OK'")) | Out-Null
$print = WaitDialog 'Print' 30
$k = Kids $print
$combo = [long](Ctl $k "id=1139 ")
$sel = & "$here\pls-combo.ps1" -ComboHandle $combo -Select $Printer | ConvertFrom-Json
if ($sel.selected -cne $Printer) { throw "Printer '$Printer' not selectable; combo shows '$($sel.selected)'" }
& "$here\pls-control.ps1" -SetCheck ([long](Ctl $k "id=1058 ")) -Checked 1 | Out-Null
& "$here\pls-control.ps1" -SetCheck ([long](Ctl $k "id=1056 ")) -Checked 0 | Out-Null
$maxTo = & "$here\pls-control.ps1" -GetText ([long](Ctl $k "id=1153 "))   # pre-filled with the sheet count
if ($AllPages) { $FromPage = 1; $ToPage = [int]$maxTo }
if ($ToPage -gt [int]$maxTo) { throw "ToPage $ToPage exceeds the project's $maxTo sheets" }
& "$here\pls-control.ps1" -SetText ([long](Ctl $k "id=1152 ")) -Text ([string]$FromPage) | Out-Null
& "$here\pls-control.ps1" -SetText ([long](Ctl $k "id=1153 ")) -Text ([string]$ToPage) | Out-Null
[void]$journal.Add("print_dialog printer='$($sel.selected)' pages=$FromPage-$ToPage")
& "$here\pls-windows.ps1" -ProcessId $ProcessId -Click ([long](Ctl $k "id=1 .*'OK'")) | Out-Null
$save = WaitDialog 'Save Print Output As' 60
$k = Kids $save
$edit = Ctl $k "id=1001 \[\] vis=True en=True '(?!Address: ).*'$"
if (-not $edit) { throw 'Save Print Output As: no filename edit (id 1001 that is not the Address bar)' }
& "$here\pls-control.ps1" -SetText ([long]$edit) -Text $OutputPdf | Out-Null
$echo = & "$here\pls-control.ps1" -GetText ([long]$edit)
if ($echo -cne $OutputPdf) { throw "filename readback mismatch: '$echo'" }
& "$here\pls-windows.ps1" -ProcessId $ProcessId -Click ([long](Ctl $k "id=1 .*'&Save'")) | Out-Null
[void]$journal.Add("save_as '$OutputPdf'")
$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
$stable = 0; $last = -1
do {
    Start-Sleep -Seconds 2
    $w = @(Wins)
    if ($w.Count -gt 0) { $t = Title $w[0]; if ($t -ne '') { throw "Unexpected dialog during spooling: '$t'" } }
    if (Test-Path -LiteralPath $OutputPdf) {
        $len = (Get-Item -LiteralPath $OutputPdf).Length
        if ($len -gt 0 -and $len -eq $last) { $stable++ } else { $stable = 0 }
        $last = $len
    }
    $frameEnabled = (& "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match "'PLS-CADD - " }) -match 'en=True'
} while (-not ($stable -ge 2 -and $frameEnabled) -and [DateTime]::UtcNow -lt $deadline)
if (-not (Test-Path -LiteralPath $OutputPdf)) { throw "PDF never appeared: $OutputPdf" }
[ordered]@{ schema = 'ds.pls.print_sheets_pdf.v1'; pdf = $OutputPdf; bytes = (Get-Item -LiteralPath $OutputPdf).Length; from = $FromPage; to = $ToPage; sheets_in_project = [int]$maxTo; plot_type = $PlotType; printer = $sel.selected; journal = @($journal) } | ConvertTo-Json -Compress -Depth 3
