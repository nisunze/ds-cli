# Run one command with the independently pinned Network sibling.
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
$ExpectedNetworkSha = (Get-Content -LiteralPath (Join-Path $RepoRoot 'pins\ds-network.rev') -Raw).Trim()
if ($ExpectedNetworkSha -notmatch '^[0-9a-f]{40}$') {
    throw 'with-network-deps: pins/ds-network.rev is not one exact Git SHA'
}
$MainCheckout = Split-Path -Parent $CommonGitDir
$NetworkCheckout = Join-Path (Split-Path -Parent $MainCheckout) 'ds-network'
$RequiredNetworkLink = Join-Path (Split-Path -Parent $RepoRoot) 'ds-network'
if (Test-Path -LiteralPath $RequiredNetworkLink) {
    $NetworkCheckout = (Resolve-Path -LiteralPath $RequiredNetworkLink).Path
}
foreach ($Crate in @('ds-grid-model', 'ds-grid-engine', 'ds-grid-exchange', 'ds-grid-tasks', 'ds-io')) {
    $Manifest = Join-Path $NetworkCheckout "crates\$Crate\Cargo.toml"
    if (-not (Test-Path -LiteralPath $Manifest -PathType Leaf)) {
        throw "with-network-deps: expected $Manifest"
    }
}
$NetworkOrigin = (& git -C $NetworkCheckout remote get-url origin).Trim()
if ($LASTEXITCODE -ne 0 -or $NetworkOrigin -notlike '*nisunze/ds-network.git') {
    throw "with-network-deps: $NetworkCheckout is not the nisunze/ds-network checkout"
}
$NetworkSha = (& git -C $NetworkCheckout rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $NetworkSha -ne $ExpectedNetworkSha) {
    throw "with-network-deps: ds-network must be pinned to $ExpectedNetworkSha"
}
$NetworkStatus = @(& git -C $NetworkCheckout status --porcelain --untracked-files=normal -- Cargo.toml Cargo.lock crates)
if ($LASTEXITCODE -ne 0 -or $NetworkStatus.Count -ne 0) {
    throw 'with-network-deps: ds-network Cargo inputs differ from its pinned commit'
}
$CreatedLinks = @()
function Ensure-DependencyLink([string] $RequiredLink, [string] $Checkout, [string] $Label) {
    $Resolved = (Resolve-Path -LiteralPath $Checkout).Path
    if (Test-Path -LiteralPath $RequiredLink) {
        $Existing = Get-Item -LiteralPath $RequiredLink -Force
        $ExistingTarget = if ($Existing.LinkType -eq 'Junction' -and $Existing.Target) {
            [System.IO.Path]::GetFullPath([string]$Existing.Target[0])
        } else {
            $Existing.FullName
        }
        if ($ExistingTarget -ne $Resolved) {
            throw "with-network-deps: $RequiredLink exists and is not the admitted $Label"
        }
    } else {
        [void](New-Item -ItemType Junction -Path $RequiredLink -Target $Resolved)
        $script:CreatedLinks += ,@($RequiredLink, $Resolved)
    }
}
try {
    Ensure-DependencyLink $RequiredNetworkLink $NetworkCheckout 'ds-network'
    & $Executable @Arguments
    $ExitCode = $LASTEXITCODE
    if ($null -eq $ExitCode) { $ExitCode = 0 }
    exit $ExitCode
} finally {
    foreach ($Pair in $CreatedLinks) {
        $RequiredLink = $Pair[0]
        $Resolved = $Pair[1]
        if (Test-Path -LiteralPath $RequiredLink) {
            $Current = Get-Item -LiteralPath $RequiredLink -Force
            $CurrentTarget = if ($Current.LinkType -eq 'Junction' -and $Current.Target) {
                [System.IO.Path]::GetFullPath([string]$Current.Target[0])
            } else {
                $Current.FullName
            }
            if ($CurrentTarget -eq $Resolved) {
                Remove-Item -LiteralPath $RequiredLink -Force
            }
        }
    }
}
