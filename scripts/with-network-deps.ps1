# Run one command with ds-network available at Cargo's declared relative path.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string] $Executable,
    [Parameter(Position = 1, ValueFromRemainingArguments = $true)]
    [string[]] $Arguments
)

$ErrorActionPreference = 'Stop'

$RepoRoot = (& git rev-parse --show-toplevel).Trim()
if ($LASTEXITCODE -ne 0 -or -not $RepoRoot) {
    throw 'with-network-deps: run from a ds-cli checkout'
}
$CommonGitDir = (& git -C $RepoRoot rev-parse --path-format=absolute --git-common-dir).Trim()
if ($LASTEXITCODE -ne 0 -or -not $CommonGitDir) {
    throw 'with-network-deps: could not resolve the common Git directory'
}
$MainCheckout = Split-Path -Parent $CommonGitDir
$NetworkCheckout = Join-Path (Split-Path -Parent $MainCheckout) 'ds-network'
$RequiredLink = Join-Path (Split-Path -Parent $RepoRoot) 'ds-network'

foreach ($Crate in @('ds-grid-model', 'ds-grid-engine', 'ds-grid-exchange', 'ds-grid-tasks', 'ds-io')) {
    $Manifest = Join-Path $NetworkCheckout "crates\$Crate\Cargo.toml"
    if (-not (Test-Path -LiteralPath $Manifest -PathType Leaf)) {
        throw "with-network-deps: expected $Manifest"
    }
}

$NetworkResolved = (Resolve-Path -LiteralPath $NetworkCheckout).Path
$CreatedLink = $false
if (Test-Path -LiteralPath $RequiredLink) {
    $Existing = Get-Item -LiteralPath $RequiredLink -Force
    $ExistingTarget = if ($Existing.LinkType -eq 'Junction' -and $Existing.Target) {
        [System.IO.Path]::GetFullPath([string]$Existing.Target[0])
    } else {
        $Existing.FullName
    }
    if ($ExistingTarget -ne $NetworkResolved) {
        throw "with-network-deps: $RequiredLink exists and is not the main checkout's ds-network"
    }
} else {
    [void](New-Item -ItemType Junction -Path $RequiredLink -Target $NetworkResolved)
    $CreatedLink = $true
}

try {
    & $Executable @Arguments
    $ExitCode = $LASTEXITCODE
    if ($null -eq $ExitCode) { $ExitCode = 0 }
    exit $ExitCode
} finally {
    if ($CreatedLink -and (Test-Path -LiteralPath $RequiredLink)) {
        $Current = Get-Item -LiteralPath $RequiredLink -Force
        $CurrentTarget = if ($Current.LinkType -eq 'Junction' -and $Current.Target) {
            [System.IO.Path]::GetFullPath([string]$Current.Target[0])
        } else {
            $Current.FullName
        }
        if ($CurrentTarget -eq $NetworkResolved) {
            Remove-Item -LiteralPath $RequiredLink -Force
        }
    }
}
