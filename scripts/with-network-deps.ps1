# Run one command with ds-network and ds-web at Cargo's declared sibling paths.
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
$ExpectedWebCoreSha = (Get-Content -LiteralPath (Join-Path $RepoRoot 'pins\ds-client-core.rev') -Raw).Trim()
if ($ExpectedWebCoreSha -notmatch '^[0-9a-f]{40}$') {
    throw 'with-network-deps: pins/ds-client-core.rev is not one exact Git SHA'
}
$MainCheckout = Split-Path -Parent $CommonGitDir
$NetworkCheckout = Join-Path (Split-Path -Parent $MainCheckout) 'ds-network'
$WebCheckout = Join-Path (Split-Path -Parent $MainCheckout) 'ds-web'
$RequiredNetworkLink = Join-Path (Split-Path -Parent $RepoRoot) 'ds-network'
$RequiredWebLink = Join-Path (Split-Path -Parent $RepoRoot) 'ds-web'
if (Test-Path -LiteralPath $RequiredWebLink) {
    $WebCheckout = (Resolve-Path -LiteralPath $RequiredWebLink).Path
}

foreach ($Crate in @('ds-grid-model', 'ds-grid-engine', 'ds-grid-exchange', 'ds-grid-tasks', 'ds-io')) {
    $Manifest = Join-Path $NetworkCheckout "crates\$Crate\Cargo.toml"
    if (-not (Test-Path -LiteralPath $Manifest -PathType Leaf)) {
        throw "with-network-deps: expected $Manifest"
    }
}
$WebManifest = Join-Path $WebCheckout 'crates\ds-client-core\Cargo.toml'
if (-not (Test-Path -LiteralPath $WebManifest -PathType Leaf)) {
    throw "with-network-deps: expected $WebManifest"
}
$WebOrigin = (& git -C $WebCheckout remote get-url origin).Trim()
if ($LASTEXITCODE -ne 0 -or $WebOrigin -notlike '*nisunze/ds-web.git') {
    throw "with-network-deps: $WebCheckout is not the nisunze/ds-web checkout"
}
$WebSha = (& git -C $WebCheckout rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $WebSha -ne $ExpectedWebCoreSha) {
    throw "with-network-deps: ds-web must be pinned to $ExpectedWebCoreSha"
}
$WebCoreStatus = @(& git -C $WebCheckout status --porcelain -- crates/ds-client-core)
if ($LASTEXITCODE -ne 0 -or $WebCoreStatus.Count -ne 0) {
    throw 'with-network-deps: ds-web client core differs from its pinned commit'
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
            throw "with-network-deps: $RequiredLink exists and is not the main checkout's $Label"
        }
    } else {
        [void](New-Item -ItemType Junction -Path $RequiredLink -Target $Resolved)
        $script:CreatedLinks += ,@($RequiredLink, $Resolved)
    }
}
try {
    Ensure-DependencyLink $RequiredNetworkLink $NetworkCheckout 'ds-network'
    Ensure-DependencyLink $RequiredWebLink $WebCheckout 'ds-web'
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
