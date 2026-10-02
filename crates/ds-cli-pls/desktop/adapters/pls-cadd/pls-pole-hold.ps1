param([int] $HoldHours = 10)
# Launch one more PLS-POLE instance and stay alive as long as it runs, so a background Bash job that runs this keeps
# the instance's process tree intact while other tool calls drive it with pls-pole-run.ps1 -ProcessId <pid>.
# Step 0 of every PLS session: SI units (pls-units-check.ps1).
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
& (Join-Path $here 'pls-units-check.ps1') | Out-Null
$out = & (Join-Path $here 'pls-pole-run.ps1') -Action Launch -Another
$out
$procId = [int](($out | Where-Object { $_ -match '^pid=' } | Select-Object -First 1) -replace 'pid=', '')
if (-not $procId) { throw "no pid from Launch: $out" }
$deadline = [DateTime]::UtcNow.AddHours($HoldHours)
while ([DateTime]::UtcNow -lt $deadline -and (Get-Process -Id $procId -ErrorAction SilentlyContinue)) { Start-Sleep -Seconds 10 }
"PLS-POLE $procId ended"
