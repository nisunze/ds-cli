# Produce a native Windows server archive; compilation never reads ds-web.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('stable','canary')][string]$Lane,
    [Parameter(Mandatory)][ValidatePattern('^\d+\.\d+\.\d+$')][string]$Version,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [Parameter(Mandatory)][string]$ProfileCatalog,
    [Parameter(Mandatory)][string]$SkillsBundle,
    [Parameter(Mandatory)][string]$SolarExecutable,
    [Parameter(Mandatory)][string]$ReporterExecutable,
    [string]$TargetDirectory,
    [switch]$Offline,
    [switch]$Plan
)
. (Join-Path $PSScriptRoot 'windows-server-package.ps1')
if ($env:OS -ne 'Windows_NT' -or -not [Environment]::Is64BitProcess -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'Use native 64-bit Windows PowerShell.' }
$repository = Split-Path $PSScriptRoot -Parent
$workspace = Split-Path $repository -Parent
$output = Assert-ServerLocalPath $OutputDirectory
if (-not (Test-Path -LiteralPath $output -PathType Container)) { throw 'Output directory must already exist.' }
$target = if ($TargetDirectory) { Assert-ServerLocalPath $TargetDirectory }
    elseif ($env:CARGO_TARGET_DIR) { Assert-ServerLocalPath $env:CARGO_TARGET_DIR } else { Join-Path $repository 'target' }
$product = if ($Lane -eq 'canary') { 'DS GridDesign Canary Server' } else { 'DS GridDesign Server' }
$name = "${product}_${Version}_x64.zip"
$artifact = Join-Path $output $name
$receiptPath = Join-Path $output ($name -replace '\.zip$', '.receipt.json')
if ((Test-Path -LiteralPath $artifact) -or (Test-Path -LiteralPath $receiptPath)) { throw 'Exact-version output already exists; it will not be overwritten.' }
if ($Plan) {
    [pscustomobject]@{ artifact=$artifact; lane=$Lane; cargo_target=$target; build_jobs=6; binaries=@('ds','ds-solar','ds-report'); tauri=$false; publishes=$false } | ConvertTo-Json
    exit 0
}
$sources = [ordered]@{}
foreach ($repo in @('ds-cli','ds-network','ds-command-kernel','ds-solar','ds-network-reporter')) {
    $path = Join-Path $workspace $repo
    $sha = (& git -C $path rev-parse --verify HEAD | Out-String).Trim()
    if ($LASTEXITCODE -or $sha -cnotmatch '^[0-9a-f]{40}$') { throw "Cannot resolve source: $repo" }
    if (& git -C $path status --porcelain --untracked-files=normal) { throw "Commit $repo before packaging; caches are retained." }
    $branch = (& git -C $path branch --show-current | Out-String).Trim()
    if ($branch -ne 'run') { throw "Package sources must follow run: $repo" }
    & git -C $path merge-base --is-ancestor HEAD origin/run
    if ($LASTEXITCODE) { throw "Push $repo before packaging." }
    $sources[$repo] = $sha
}
foreach ($pin in @('ds-command-kernel','ds-client-core')) {
    if ((Get-Content -LiteralPath (Join-Path $repository "pins/$pin.rev") -Raw).Trim() -cne $sources['ds-command-kernel']) {
        throw "The CLI's $pin pin differs from the clean linked kernel."
    }
}
$catalogPath = Assert-ServerLocalPath $ProfileCatalog
$catalog = Get-Content -LiteralPath $catalogPath -Raw | ConvertFrom-Json
if ($catalog.development -ne $false) { throw 'A development profile cannot be packaged.' }
$skills = Assert-ServerLocalPath $SkillsBundle
$skillReceipt = Get-Content -LiteralPath (Join-Path $skills 'receipt.json') -Raw | ConvertFrom-Json
if ($skillReceipt.contract -ne 'ds-cli-skills-bundle/v3' -or $skillReceipt.dirty -ne $false -or
    $skillReceipt.source_sha -cne $sources['ds-cli']) { throw 'Skills must be a clean bundle from this exact CLI source.' }
$solar = Assert-ServerLocalPath $SolarExecutable
$reporter = Assert-ServerLocalPath $ReporterExecutable
foreach ($pair in @(@($solar,'ds-solar'),@($reporter,'ds-network-reporter'))) {
    $info = Invoke-ServerJson $pair[0] @('build-info')
    if ($info.source_sha -cne $sources[$pair[1]] -or $info.schema -ne 'ds.engine-build/v1' -or
        $info.profile -ne 'release' -or $info.target -ne 'x86_64-pc-windows-msvc' -or
        $info.build_manifest_sha256 -cnotmatch '^[0-9a-f]{64}$') { throw "Process engine is not an exact native release: $($pair[1])" }
}
# Guard the resolved graph, not a guessed package list. No Tauri/web source,
# including build dependencies, may enter native server compilation.
$metadata = (& cargo metadata --manifest-path (Join-Path $repository 'Cargo.toml') --locked --offline --format-version 1 | Out-String | ConvertFrom-Json)
if ($LASTEXITCODE) { throw 'Cannot resolve the locked native source graph.' }
foreach ($package in $metadata.packages) {
    if ($package.name -match '^tauri(?:-|$)' -or $package.manifest_path -match '[\\/]ds-web[\\/]') { throw 'Native server graph contains a desktop/web source input.' }
}
$lockPath = Join-Path $output ($name + '.lock')
$lock = [IO.File]::Open($lockPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
$work = Join-Path $output ('.windows-server-' + [guid]::NewGuid().ToString('N'))
$payload = Join-Path $work 'payload'
$names = @('CARGO_TARGET_DIR','CARGO_BUILD_JOBS','DS_CLI_SOURCE_SHA','DS_CLI_SOURCE_DIRTY','DS_NATIVE_CLIENT_PROFILE_BUNDLE','DS_NATIVE_CLIENT_PROFILE_SHA256',
    'DS_RELEASE_PIN_DS_NETWORK','DS_RELEASE_PIN_DS_COMMAND_KERNEL','DS_RELEASE_PIN_DS_SOLAR','DS_RELEASE_PIN_DS_NETWORK_REPORTER')
$previous = @{}
foreach ($envName in $names) { $previous[$envName] = [Environment]::GetEnvironmentVariable($envName, 'Process') }
try {
    [IO.Directory]::CreateDirectory($payload) | Out-Null
    $env:CARGO_TARGET_DIR = $target
    if (-not $env:CARGO_BUILD_JOBS) { $env:CARGO_BUILD_JOBS = '6' }
    $env:DS_CLI_SOURCE_SHA = $sources['ds-cli']; $env:DS_CLI_SOURCE_DIRTY = '0'
    $env:DS_NATIVE_CLIENT_PROFILE_BUNDLE = $catalogPath
    $env:DS_NATIVE_CLIENT_PROFILE_SHA256 = (Get-FileHash -LiteralPath $catalogPath).Hash.ToLowerInvariant()
    $env:DS_RELEASE_PIN_DS_NETWORK = $sources['ds-network']
    $env:DS_RELEASE_PIN_DS_COMMAND_KERNEL = $sources['ds-command-kernel']
    $env:DS_RELEASE_PIN_DS_SOLAR = $sources['ds-solar']
    $env:DS_RELEASE_PIN_DS_NETWORK_REPORTER = $sources['ds-network-reporter']
    $build = @('build','--manifest-path',(Join-Path $repository 'Cargo.toml'),'--locked','--release','-p','ds','--bin','ds')
    if ($Offline) { $build += '--offline' }
    & cargo @build
    if ($LASTEXITCODE) { throw 'Native release build failed; persistent caches retained.' }
    foreach ($repo in $sources.Keys) {
        $path = Join-Path $workspace $repo
        $after = (& git -C $path rev-parse HEAD | Out-String).Trim()
        if ($LASTEXITCODE -or $after -cne $sources[$repo] -or (& git -C $path status --porcelain --untracked-files=normal)) {
            throw "Source changed during the build: $repo; no artifact was exposed."
        }
    }
    Copy-Item -LiteralPath (Join-Path $target 'release/ds.exe') -Destination (Join-Path $payload 'ds.exe')
    Copy-Item -LiteralPath $solar -Destination (Join-Path $payload 'ds-solar.exe')
    Copy-Item -LiteralPath $reporter -Destination (Join-Path $payload 'ds-report.exe')
    [IO.Directory]::CreateDirectory((Join-Path $payload 'ds-client-profiles')) | Out-Null
    Copy-Item -LiteralPath $catalogPath -Destination (Join-Path $payload 'ds-client-profiles/catalog.json')
    # Check links before recursive copy, and then verify the actual bundle with ds doctor.
    $null = @(Get-ServerInventory $skills)
    Copy-Item -LiteralPath $skills -Destination (Join-Path $payload 'ds-cli-skills') -Recurse
    foreach ($script in @('start-windows-server.ps1','install-windows-server.ps1','windows-server-package.ps1')) {
        Copy-Item -LiteralPath (Join-Path $PSScriptRoot $script) -Destination (Join-Path $payload $script)
    }
    $manifest = [ordered]@{ contract='ds.windows-server-payload/v1'; lane=$Lane; version=$Version; architecture='x86_64';
        built_at=[DateTime]::UtcNow.ToString('yyyy-MM-ddTHH:mm:ss.fffZ'); source_revisions=$sources; files=@(Get-ServerInventory $payload) }
    [IO.File]::WriteAllText((Join-Path $payload 'release.json'), ($manifest | ConvertTo-Json -Depth 10), [Text.UTF8Encoding]::new($false))
    $manifest = Test-ServerPayload $payload $Lane
    $null = Test-ServerNativeIdentity $payload $manifest
    $temporaryZip = Join-Path $work $name
    [IO.Compression.ZipFile]::CreateFromDirectory($payload, $temporaryZip, [IO.Compression.CompressionLevel]::Optimal, $false)
    $extracted = Join-Path $work 'extracted'
    Expand-ServerArchive $temporaryZip $extracted
    $verified = Test-ServerPayload $extracted $Lane
    $null = Test-ServerNativeIdentity $extracted $verified
    $receipt = [ordered]@{ contract='ds.windows-server-artifact/v1'; lane=$Lane; version=$Version; architecture='x86_64'; artifact=$name;
        sha256=(Get-FileHash -LiteralPath $temporaryZip).Hash.ToLowerInvariant(); size_bytes=(Get-Item -LiteralPath $temporaryZip).Length;
        built_at=$manifest.built_at; source_revisions=$sources; native_client_profile_catalog_sha256=$env:DS_NATIVE_CLIENT_PROFILE_SHA256 }
    $temporaryReceipt = Join-Path $work 'receipt.json'
    [IO.File]::WriteAllText($temporaryReceipt, ($receipt | ConvertTo-Json -Depth 10), [Text.UTF8Encoding]::new($false))
    [IO.File]::Move($temporaryZip, $artifact)
    [IO.File]::Move($temporaryReceipt, $receiptPath)
    Write-Output $artifact
    Write-Output $receiptPath
} finally {
    foreach ($envName in $names) { [Environment]::SetEnvironmentVariable($envName, $previous[$envName], 'Process') }
    $lock.Dispose()
    # Both paths were created by this invocation under the checked output root.
    if (Test-Path -LiteralPath $work) { Remove-Item -LiteralPath $work -Recurse -Force }
    Remove-Item -LiteralPath $lockPath -Force
}
