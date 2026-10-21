param([string] $IniPath = (Join-Path $env:APPDATA 'PLS\PLS_CADD.INI'))
# Step 0 of every PLS session (owner 2026-09-26: SI from the beginning). The system-wide unit system lives in
# PLS_CADD.INI [GLOBAL], shared by every PLS program: UNITS=1 (SI) and NEWTON_OVER_DECANEWTON=1 (N force).
# Set it with PLS-CADD File > Preferences (33349) > Unit System > S.I. (N force) (33363) > OK; PLS writes the INI.
# Refuses (throws) unless both keys are 1, so no run starts in US units or daN.
$ErrorActionPreference = 'Stop'
if (-not (Test-Path -LiteralPath $IniPath)) { throw "PLS INI not found: $IniPath" }
$section = ''
$keys = @{}
foreach ($line in [System.IO.File]::ReadAllLines($IniPath)) {
    if ($line -match '^\[(.+)\]\s*$') { $section = $Matches[1]; continue }
    if ($section -eq 'GLOBAL' -and $line -match '^(UNITS|NEWTON_OVER_DECANEWTON)=(.*)$') { $keys[$Matches[1]] = $Matches[2].Trim() }
}
if ($keys['UNITS'] -ne '1' -or $keys['NEWTON_OVER_DECANEWTON'] -ne '1') {
    throw "PLS unit system is not SI (N force): [GLOBAL] UNITS=$($keys['UNITS']) NEWTON_OVER_DECANEWTON=$($keys['NEWTON_OVER_DECANEWTON']) in $IniPath. Set File > Preferences > Unit System > S.I. (N force) first."
}
'units SI (N force)'
