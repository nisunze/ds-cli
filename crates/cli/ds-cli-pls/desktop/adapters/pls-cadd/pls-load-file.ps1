param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [Parameter(Mandatory = $true)][ValidateSet('str', 'con')][string] $Kind,
    [Parameter(Mandatory = $true)][string] $Path
)
# Load a steering file into the open PLS-CADD project, replacing what it holds:
#   str: Structures > Available Structure List > Load STR File (40030) -> 'Load STR' "Append ...?" -> No (7, replace)
#   con: Structures > Automatic Spotting > Spotting Constraints > Load CON file (32798) -> 'Load CON' "Replace
#        existing constraints?" -> Yes (6)
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$cmd, $prompt, $answer = @{ str = @(40030, 'Load STR', 7); con = @(32798, 'Load CON', 6) }[$Kind]
function Modal([string]$pattern) {
    for ($i = 0; $i -lt 30; $i++) {
        $m = & "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match 'vis=True' -and $_ -match $pattern } | Select-Object -First 1
        if ($m) { return [long](($m -split ' ')[0]) }
        Start-Sleep -Milliseconds 500
    }
    0
}
& "$here\pls-command.ps1" -WindowHandle $MainWindowHandle -CommandId $cmd -Post
$d = Modal "\] 'Open (Available Structure List|Constraint) [Ff]ile'$"
if (-not $d) { throw "no open-file dialog for $Kind" }
$edit = [long]((& "$here\pls-safe-dump.ps1" -Dialog $d | Where-Object { $_ -match ' id=(1148|1001) Edit' } | Select-Object -First 1) -replace ' .*', '')
& "$here\pls-type-path.ps1" -DialogHandle $d -EditHandle $edit -Path $Path | Out-Null
$p = Modal "\] '$prompt'$"
if (-not $p) { throw "no '$prompt' prompt" }
$btn = [long]((& "$here\pls-safe-dump.ps1" -Dialog $p | Where-Object { $_ -match " id=$answer Button" } | Select-Object -First 1) -replace ' .*', '')
& "$here\pls-press.ps1" -Button $btn | Out-Null
Start-Sleep 2
$left = & "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match 'vis=True' -and $_ -notmatch "^$MainWindowHandle " -and $_ -notmatch "'Error Log'$" }
if ($left) { throw "dialog left after loading ${Kind}: $left" }
"loaded $Kind $Path"
