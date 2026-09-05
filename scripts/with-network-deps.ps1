# Run one command with every native path dependency at Cargo's sibling paths.
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
$ExpectedCommandKernelSha = (Get-Content -LiteralPath (Join-Path $RepoRoot 'pins\ds-command-kernel.rev') -Raw).Trim()
if ($ExpectedCommandKernelSha -notmatch '^[0-9a-f]{40}$') {
    throw 'with-network-deps: pins/ds-command-kernel.rev is not one exact Git SHA'
}
$MainCheckout = Split-Path -Parent $CommonGitDir
$NetworkCheckout = Join-Path (Split-Path -Parent $MainCheckout) 'ds-network'
$MainNetworkCheckout = $NetworkCheckout
$WebCheckout = Join-Path (Split-Path -Parent $MainCheckout) 'ds-web'
$CommandKernelCheckout = Join-Path (Split-Path -Parent $MainCheckout) 'ds-command-kernel'
$RequiredNetworkLink = Join-Path (Split-Path -Parent $RepoRoot) 'ds-network'
$RequiredWebLink = Join-Path (Split-Path -Parent $RepoRoot) 'ds-web'
$RequiredCommandKernelLink = Join-Path (Split-Path -Parent $RepoRoot) 'ds-command-kernel'
if (Test-Path -LiteralPath $RequiredNetworkLink) {
    $NetworkCheckout = (Resolve-Path -LiteralPath $RequiredNetworkLink).Path
}
if (Test-Path -LiteralPath $RequiredWebLink) {
    $WebCheckout = (Resolve-Path -LiteralPath $RequiredWebLink).Path
}
if (Test-Path -LiteralPath $RequiredCommandKernelLink) {
    $CommandKernelCheckout = (Resolve-Path -LiteralPath $RequiredCommandKernelLink).Path
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
$MainNetworkSha = (& git -C $MainNetworkCheckout rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $NetworkSha -ne $MainNetworkSha) {
    throw "with-network-deps: ds-network must match the main checkout's exact source revision"
}
$NetworkStatus = @(& git -C $NetworkCheckout status --porcelain --untracked-files=normal -- Cargo.toml Cargo.lock crates)
if ($LASTEXITCODE -ne 0 -or $NetworkStatus.Count -ne 0) {
    throw 'with-network-deps: ds-network Cargo inputs differ from its pinned commit'
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
$CommandKernelManifest = Join-Path $CommandKernelCheckout 'Cargo.toml'
if (-not (Test-Path -LiteralPath $CommandKernelManifest -PathType Leaf)) {
    throw "with-network-deps: expected $CommandKernelManifest"
}
$CommandKernelOrigin = (& git -C $CommandKernelCheckout remote get-url origin).Trim()
if ($LASTEXITCODE -ne 0 -or $CommandKernelOrigin -notlike '*nisunze/ds-command-kernel.git') {
    throw "with-network-deps: $CommandKernelCheckout is not the nisunze/ds-command-kernel checkout"
}
$CommandKernelSha = (& git -C $CommandKernelCheckout rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $CommandKernelSha -ne $ExpectedCommandKernelSha) {
    throw "with-network-deps: ds-command-kernel must be pinned to $ExpectedCommandKernelSha"
}
$CommandKernelStatus = @(& git -C $CommandKernelCheckout status --porcelain --untracked-files=normal -- Cargo.toml Cargo.lock src crates)
if ($LASTEXITCODE -ne 0 -or $CommandKernelStatus.Count -ne 0) {
    throw 'with-network-deps: ds-command-kernel native inputs differ from its pinned commit'
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
    Ensure-DependencyLink $RequiredCommandKernelLink $CommandKernelCheckout 'ds-command-kernel'
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
