[CmdletBinding()]
param([string]$StateDirectory, [int]$Workers = 0,
    [Parameter(ValueFromRemainingArguments=$true)][string[]]$CliArguments)
. (Join-Path $PSScriptRoot 'windows-server-package.ps1')
$release = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'release.json') -Raw | ConvertFrom-Json
$manifest = Test-ServerPayload $PSScriptRoot $release.lane
$null = Test-ServerNativeIdentity $PSScriptRoot $manifest
if (-not $StateDirectory) { $StateDirectory = Join-Path $env:LOCALAPPDATA "ds/server/$($manifest.lane)" }
$state = Assert-ServerLocalPath $StateDirectory
if ($Workers -lt 0) { throw 'Workers must be positive or omitted.' }
$previousHostKind = [Environment]::GetEnvironmentVariable('DS_CLI_HOST_KIND', 'Process')
try {
    $env:DS_CLI_HOST_KIND = 'server'
    if ($CliArguments -and $CliArguments.Count) { & (Join-Path $PSScriptRoot 'ds.exe') @CliArguments }
    else {
        $arguments = @('server','serve','--lane',$manifest.lane,'--state-dir',$state)
        if ($Workers) { $arguments += @('--workers', "$Workers") }
        & (Join-Path $PSScriptRoot 'ds.exe') @arguments
    }
    $serverExitCode = $LASTEXITCODE
} finally {
    [Environment]::SetEnvironmentVariable('DS_CLI_HOST_KIND', $previousHostKind, 'Process')
}
exit $serverExitCode
