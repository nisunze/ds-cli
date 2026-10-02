param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [Parameter(Mandatory = $true)][string] $OutputPdf,
    [string] $JournalPath = '',
    [int] $TimeoutSeconds = 1800
)
# Save every plan & profile sheet of the ACTIVE Sheets View to ONE PDF with PLS-CADD's own
# exporter (Sheets View popup > Save as PDF... > To File..., command 35987). It writes each
# sheet at its page size (A3 1191 x 842 pt on Nyamagabe); the File > Print route through
# Microsoft Print to PDF produced Letter pages, so it is not used for deliverables.
# Chain (16.81): 35987 -> 'Save to Single PDF File' (Yes 6 = one file; No 7 = one per page)
# -> 'Save PDF File As' (filename = the id-1001 edit that is NOT the 'Address: ' bar; Save 1)
# -> PLS pages and writes the sheets with the frame disabled. Paging prompts and any other
# modal go through pls-dialog-watch (catalogued decisions); unknown ones end the run.
# Done = the PDF exists, its size is stable and the frame is enabled again.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
if (Test-Path -LiteralPath $OutputPdf) { throw "Output already exists: $OutputPdf" }
if (-not (Test-Path -LiteralPath (Split-Path -Parent $OutputPdf))) { throw "Output folder missing: $(Split-Path -Parent $OutputPdf)" }
$title = (Get-Process -Id $ProcessId).MainWindowTitle
if ($title -notmatch '\[Sheets View\]$') { throw "Active view must be the Sheets View, frame title is '$title'" }
function Wins { @(& "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match 'vis=True' -and $_ -notmatch 'owner=0 ' -and $_ -notmatch "'Error Log'" }) }
function Kids([long]$h) { @(& "$here\pls-windows.ps1" -ProcessId $ProcessId -Children $h) }
function Handle([string]$row) { [long](($row.Trim() -split ' ')[0]) }
function Title([string]$row) { if ($row -match "\] '(.*)'$") { $Matches[1] } else { '' } }
function Ctl([string[]]$kids, [string]$pattern) { ($kids | Where-Object { $_ -match $pattern }) -replace '^\s*child (\d+).*', '$1' | Select-Object -First 1 }
function Click([long]$h) { & "$here\pls-windows.ps1" -ProcessId $ProcessId -Click $h | Out-Null }
function WaitDialog([string]$expected, [int]$seconds) {
    $deadline = [DateTime]::UtcNow.AddSeconds($seconds)
    do {
        Start-Sleep -Milliseconds 500
        $hit = Wins | Where-Object { (Title $_) -ceq $expected } | Select-Object -First 1
        if ($hit) { $h = Handle $hit; if ((Kids $h).Count -gt 0) { return $h } }
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "'$expected' did not appear within $seconds s; open dialogs: $((Wins | ForEach-Object { Title $_ }) -join ' | ')"
}
$journal = [System.Collections.ArrayList]::new()
& "$here\pls-command.ps1" -WindowHandle $MainWindowHandle -CommandId 35987 -Post | Out-Null
$single = WaitDialog 'Save to Single PDF File' 60
$k = Kids $single
if (($k -join ' ') -notmatch 'Save all pages to a single PDF file') { throw "unexpected 'Save to Single PDF File' body: $($k -join ' || ')" }
Click ([long](Ctl $k "id=6 .*vis=True en=True '&Yes'"))
[void]$journal.Add('single_pdf: Yes')
$save = WaitDialog 'Save PDF File As' 60
$k = Kids $save
$edit = Ctl $k "id=1001 \[\] vis=True en=True '(?!Address: ).*'$"
if (-not $edit) { throw "Save PDF File As: no filename edit (id 1001 that is not the Address bar): $($k -join ' || ')" }
& "$here\pls-control.ps1" -SetText ([long]$edit) -Text $OutputPdf | Out-Null
$echo = & "$here\pls-control.ps1" -GetText ([long]$edit)
if ($echo -cne $OutputPdf) { throw "filename readback mismatch: '$echo'" }
Click ([long](Ctl $k "id=1 .*vis=True en=True '&Save'"))
[void]$journal.Add("save_as '$OutputPdf'")
$started = [DateTime]::UtcNow
$deadline = $started.AddSeconds($TimeoutSeconds)
$stable = 0; $last = -1
do {
    Start-Sleep -Seconds 3
    $frameEnabled = [bool]((& "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match "'PLS-CADD - " }) -match 'en=True')
    if (-not $frameEnabled -and (Wins | Where-Object { (Title $_) -notmatch '^(Save to Single PDF File|Save PDF File As)$' })) {
        $w = & "$here\pls-dialog-watch.ps1" -ProcessId $ProcessId -MainWindowHandle $MainWindowHandle -TimeoutSeconds 6 -JournalPath $JournalPath | ConvertFrom-Json
        foreach ($e in $w.events) { if ($e.event -ne 'progress') { [void]$journal.Add(($e | ConvertTo-Json -Compress -Depth 4)) } }
        if ($w.outcome -in @('unknown', 'stop', 'flow')) { throw "dialog while writing the sheets ('$($w.outcome)'); journal: $($journal -join ' ;; ')" }
    }
    if (Test-Path -LiteralPath $OutputPdf) {
        $len = (Get-Item -LiteralPath $OutputPdf).Length
        if ($len -gt 0 -and $len -eq $last) { $stable++ } else { $stable = 0 }
        $last = $len
    }
} while (-not ($stable -ge 2 -and $frameEnabled) -and [DateTime]::UtcNow -lt $deadline)
if (-not (Test-Path -LiteralPath $OutputPdf)) { throw "PDF never appeared: $OutputPdf; journal: $($journal -join ' ;; ')" }
if (-not ($stable -ge 2 -and $frameEnabled)) { throw "PDF not settled within $TimeoutSeconds s ($last bytes); journal: $($journal -join ' ;; ')" }
[ordered]@{ schema = 'ds.pls.save_sheets_pdf.v1'; pdf = $OutputPdf; bytes = (Get-Item -LiteralPath $OutputPdf).Length; seconds = [int]([DateTime]::UtcNow - $started).TotalSeconds; journal = @($journal) } | ConvertTo-Json -Compress -Depth 3
