# Native Windows development host. Builds and runs ds; no Tauri or web sources.
[CmdletBinding()]
param(
    [ValidateSet('stable', 'canary')][string]$Lane = 'canary',
    [string]$DevelopmentRoot = (Join-Path $env:LOCALAPPDATA 'ds\server-dev'),
    [string]$StateDirectory,
    [string]$TargetDirectory,
    [string]$ProfileCatalog,
    [int]$Workers = 0,
    [switch]$PrepareOnly,
    [switch]$Offline,
    [switch]$Plan,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$CliArguments
)
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT') { throw 'This launcher requires native Windows.' }
$repository = Split-Path $PSScriptRoot -Parent
if (-not $TargetDirectory) {
    $TargetDirectory = if ($env:DS_SERVER_CARGO_TARGET_DIR) { $env:DS_SERVER_CARGO_TARGET_DIR }
        elseif ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repository 'target' }
}
$target = [IO.Path]::GetFullPath($TargetDirectory)
$devRoot = [IO.Path]::GetFullPath($DevelopmentRoot)
$installedRoot = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'ds\server'))
# A loaded Windows image must not lock Cargo's next link output.
function Get-ServerDevelopmentRuntime([string]$Executable, [string]$Target) {
    $digest = (Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash.ToLowerInvariant()
    $runtimeRoot = Join-Path $Target 'windows-server-runtime'
    $runtimeDirectory = Join-Path $runtimeRoot $digest
    [IO.Directory]::CreateDirectory($runtimeDirectory) | Out-Null
    $runtimeExecutable = Join-Path $runtimeDirectory 'ds.exe'
    if (-not (Test-Path -LiteralPath $runtimeExecutable -PathType Leaf)) {
        $pending = Join-Path $runtimeDirectory ('ds-' + [guid]::NewGuid().ToString('N') + '.pending')
        try {
            [IO.File]::Copy($Executable, $pending, $false)
            if ((Get-FileHash -LiteralPath $pending).Hash -ine $digest) { throw 'Source executable changed while staging the runtime.' }
            try { [IO.File]::Move($pending, $runtimeExecutable) }
            catch [IO.IOException] { if (-not (Test-Path -LiteralPath $runtimeExecutable -PathType Leaf)) { throw } }
        } finally {
            if (Test-Path -LiteralPath $pending) { Remove-Item -LiteralPath $pending -Force }
        }
    }
    if ((Get-FileHash -LiteralPath $runtimeExecutable).Hash -ine $digest) { throw 'Runtime executable differs from its admitted source bytes.' }
    return $runtimeExecutable
}

function Contains-Path([string]$Root, [string]$Path) {
    $Path.Equals($Root, [StringComparison]::OrdinalIgnoreCase) -or
        $Path.StartsWith($Root.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)
}
$installedRoots = @($installedRoot)
if ($env:XDG_STATE_HOME) { $installedRoots += [IO.Path]::GetFullPath((Join-Path $env:XDG_STATE_HOME 'ds/server')) }
if ($env:HOME) { $installedRoots += [IO.Path]::GetFullPath((Join-Path $env:HOME '.local/state/ds/server')) }
foreach ($installed in $installedRoots) {
    if ((Contains-Path $installed $devRoot) -or (Contains-Path $devRoot $installed)) {
        throw 'Development state must not overlap installed server state.'
    }
}
$developmentStateHome = Join-Path $devRoot 'state'
if (-not $StateDirectory) { $StateDirectory = Join-Path $developmentStateHome "ds/server/$Lane" }
$state = [IO.Path]::GetFullPath($StateDirectory)
if ($state.Equals($devRoot, [StringComparison]::OrdinalIgnoreCase) -or -not (Contains-Path $devRoot $state)) {
    throw 'Development state must be a child of the dedicated development root.'
}
# A custom CLI state flag must satisfy the same boundary as server startup.
for ($index = 0; $index -lt $CliArguments.Count; $index++) {
    $argument = $CliArguments[$index]
    if ($argument -in @('--state-dir', '--server-state-dir')) {
        if ($index + 1 -ge $CliArguments.Count) { throw 'Missing CLI state directory.' }
        $candidate = [IO.Path]::GetFullPath($CliArguments[$index + 1])
        if (-not (Contains-Path $devRoot $candidate) -or $candidate -eq $devRoot) { throw 'CLI state must stay within development state.' }
    } elseif ($argument -match '^--(?:server-)?state-dir=(.*)$') {
        $candidate = [IO.Path]::GetFullPath($Matches[1])
        if (-not (Contains-Path $devRoot $candidate) -or $candidate -eq $devRoot) { throw 'CLI state must stay within development state.' }
    }
}
if ($Workers -lt 0) { throw 'Workers must be positive or omitted.' }
$buildArguments = @('build', '--manifest-path', (Join-Path $repository 'Cargo.toml'), '-p', 'ds', '--bin', 'ds', '--locked')
if ($Offline) { $buildArguments += '--offline' }
$executable = Join-Path $target 'debug\ds.exe'
$serverArguments = @('server', 'serve', '--lane', $Lane, '--state-dir', $state)
if ($Workers) { $serverArguments += @('--workers', "$Workers") }
if ($Plan) {
    [pscustomobject]@{ lane=$Lane; state=$state; cargo_target=$target; build_jobs=$(if ($env:CARGO_BUILD_JOBS) { [int]$env:CARGO_BUILD_JOBS } else { 6 }); builds=@('ds'); tauri=$false; standalone_sidecars=@(); prepare_only=[bool]$PrepareOnly } | ConvertTo-Json
    exit 0
}
$names = @('XDG_STATE_HOME','CARGO_TARGET_DIR','CARGO_BUILD_JOBS','CARGO_INCREMENTAL','RUSTUP_TOOLCHAIN','DS_NATIVE_CLIENT_PROFILE_BUNDLE','DS_NATIVE_CLIENT_PROFILE_SHA256','DS_NATIVE_CLIENT_PRODUCT_ROOT','DS_CLI_SOURCE_SHA','DS_CLI_SOURCE_DIRTY')
$names += @(Get-ChildItem Env: | Where-Object Name -Like 'DS_RELEASE_PIN_*' | ForEach-Object Name)
$previous = @{}
foreach ($name in $names) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process') }
try {
    # Commands omitting a server state flag resolve within this source lane,
    # never to the installed server namespace. Native credentials are unchanged.
    $env:XDG_STATE_HOME = $developmentStateHome
    $env:DS_NATIVE_CLIENT_PROFILE_SHA256 = $null
    $env:DS_NATIVE_CLIENT_PRODUCT_ROOT = $null
    foreach ($name in $names | Where-Object { $_ -like 'DS_RELEASE_PIN_*' }) { [Environment]::SetEnvironmentVariable($name, $null, 'Process') }
    $env:CARGO_TARGET_DIR = $target
    if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = '6' }
    if (-not $env:CARGO_INCREMENTAL) { $env:CARGO_INCREMENTAL = '1' }
    $env:RUSTUP_TOOLCHAIN = if ($env:DS_DEV_RUST_TOOLCHAIN) { $env:DS_DEV_RUST_TOOLCHAIN } elseif ($env:RUSTUP_TOOLCHAIN) { $env:RUSTUP_TOOLCHAIN } else { 'stable' }
    $env:DS_CLI_SOURCE_SHA = (& git -C $repository rev-parse HEAD).Trim()
    if ($LASTEXITCODE) { throw 'Cannot identify the source revision.' }
    $env:DS_CLI_SOURCE_DIRTY = if (& git -C $repository status --porcelain --untracked-files=normal) { '1' } else { '0' }
    if ($ProfileCatalog) { $env:DS_NATIVE_CLIENT_PROFILE_BUNDLE = [IO.Path]::GetFullPath($ProfileCatalog) }
    elseif (-not $env:DS_NATIVE_CLIENT_PROFILE_BUNDLE) {
        $products = if ($Lane -eq 'canary') { @('DS GridDesign Canary','DS GridDesign') } else { @('DS GridDesign','DS GridDesign Canary') }
        $installed = $null
        foreach ($product in $products) {
            $candidate = Join-Path (Join-Path $env:LOCALAPPDATA $product) 'ds-client-profiles\catalog.json'
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { $installed = $candidate; break }
        }
        if (-not $installed) { throw 'Install a native public profile catalog or provide -ProfileCatalog. No deployment configuration is inferred.' }
        $metadata = Get-Item -LiteralPath $installed
        if ($metadata.Length -gt 1048576 -or ($metadata.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Invalid installed profile catalog.' }
        $catalog = Get-Content -LiteralPath $installed -Raw | ConvertFrom-Json
        if ($catalog.development -ne $false) { throw 'Expected an installed release catalog.' }
        # Retain installed route/provenance bytes. The current Rust owner
        # validates this development catalog; a route mismatch is a refusal.
        # No user credential is read or copied by the launcher.
        $catalog.development = $true
        $profileRoot = Join-Path $target 'windows-server-profile'
        [IO.Directory]::CreateDirectory($profileRoot) | Out-Null
        $profilePath = Join-Path $profileRoot 'catalog.json'
        [IO.File]::WriteAllText($profilePath, ($catalog | ConvertTo-Json -Depth 32), [Text.UTF8Encoding]::new($false))
        $env:DS_NATIVE_CLIENT_PROFILE_BUNDLE = $profilePath
    }
    Write-Host "Native Windows server: $Lane; compiler cache: $target; jobs: $env:CARGO_BUILD_JOBS"
    & cargo @buildArguments
    if ($LASTEXITCODE) { throw "Native server build failed ($LASTEXITCODE). Cache retained." }
    if ($PrepareOnly) { Write-Host 'Native server prepared; no service started.'; exit 0 }
    $runtimeExecutable = Get-ServerDevelopmentRuntime $executable $target
    Write-Host "Native runtime: $runtimeExecutable"
    # The state lock refuses a second host; jobs are never killed implicitly.
    if ($CliArguments.Count) { & $runtimeExecutable @CliArguments }
    else { & $runtimeExecutable @serverArguments }
    exit $LASTEXITCODE
} finally {
    foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') }
}
