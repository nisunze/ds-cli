# Install exact reviewed artifact bytes; no download, publish or account copying.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Artifact,
    [Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$Sha256,
    [Parameter(Mandatory)][ValidateSet('stable','canary')][string]$Lane,
    [string]$InstallRoot,
    [switch]$Plan
)
. (Join-Path $PSScriptRoot 'windows-server-package.ps1')
if ($env:OS -ne 'Windows_NT' -or -not [Environment]::Is64BitProcess -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw 'Use native 64-bit Windows PowerShell.' }
$artifactPath = Assert-ServerLocalPath $Artifact
if (-not $InstallRoot) { $InstallRoot = Join-Path $env:LOCALAPPDATA $(if ($Lane -eq 'canary') { 'DS GridDesign Canary Server' } else { 'DS GridDesign Server' }) }
$root = Assert-ServerLocalPath $InstallRoot
foreach ($desktop in @('DS GridDesign','DS GridDesign Canary')) {
    $desktopRoot = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA $desktop))
    if ($root -eq $desktopRoot -or $root.StartsWith($desktopRoot + '\', [StringComparison]::OrdinalIgnoreCase) -or
        $desktopRoot.StartsWith($root.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Server installation must not overlap Desktop.' }
}
if ((Get-FileHash -LiteralPath $artifactPath).Hash -ine $Sha256) { throw 'Artifact digest differs from the expected reviewed/public bytes.' }
if ($Plan) { [pscustomobject]@{ artifact=$artifactPath; sha256=$Sha256.ToLowerInvariant(); install_root=$root; lane=$Lane; starts_server=$false } | ConvertTo-Json; exit 0 }
[IO.Directory]::CreateDirectory($root) | Out-Null
$lockPath = Assert-ServerLocalPath (Join-Path $root 'install.lock')
$lock = [IO.File]::Open($lockPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
$stage = Join-Path $root ('.install-' + [guid]::NewGuid().ToString('N'))
try {
    Expand-ServerArchive $artifactPath $stage
    $manifest = Test-ServerPayload $stage $Lane
    $null = Test-ServerNativeIdentity $stage $manifest
    $current = Assert-ServerLocalPath (Join-Path $root 'current.json')
    if (Test-Path -LiteralPath $current) {
        $prior = Get-Content -LiteralPath $current -Raw | ConvertFrom-Json
        if ($prior.contract -ne 'ds.windows-server-install/v1' -or $prior.lane -ne $Lane) { throw 'Installation root belongs to another product/lane.' }
        if ([version]$manifest.version -lt [version]$prior.version) { throw 'A Windows server downgrade requires a separately reviewed installation.' }
    }
    $versions = Assert-ServerLocalPath (Join-Path $root 'versions')
    [IO.Directory]::CreateDirectory($versions) | Out-Null
    $destination = Assert-ServerLocalPath (Join-Path $versions ($manifest.version + '-' + $Sha256.ToLowerInvariant()))
    if (Test-Path -LiteralPath $destination) {
        $priorManifest = Test-ServerPayload $destination $Lane
        $null = Test-ServerNativeIdentity $destination $priorManifest
        if ($priorManifest.version -ne $manifest.version -or
            (Get-FileHash -LiteralPath (Join-Path $destination 'release.json')).Hash -cne
            (Get-FileHash -LiteralPath (Join-Path $stage 'release.json')).Hash) { throw 'Existing version differs from the exact archive payload.' }
        Remove-Item -LiteralPath (Assert-ServerChildPath $root $stage) -Recurse -Force
    } else { [IO.Directory]::Move((Assert-ServerChildPath $root $stage), (Assert-ServerChildPath $versions $destination)) }
    $receipt = [ordered]@{ contract='ds.windows-server-install/v1'; lane=$Lane; version=$manifest.version;
        artifact_sha256=$Sha256.ToLowerInvariant(); directory=$destination; installed_at=[DateTime]::UtcNow.ToString('o') }
    $pending = Join-Path $root ('current-' + [guid]::NewGuid().ToString('N') + '.json')
    [IO.File]::WriteAllText($pending, ($receipt | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
    # Atomic activation. A running older server remains pinned to its own exe;
    # restart explicitly to consume the new version. No user state is deleted.
    if (Test-Path -LiteralPath $current) { [IO.File]::Replace($pending, $current, $null) }
    else { [IO.File]::Move($pending, $current) }
    Write-Output "Installed Windows server $($manifest.version) ($Lane)."
    Write-Output "Launch: & '$destination\start-windows-server.ps1'"
} finally {
    if (Test-Path -LiteralPath $stage) { Remove-Item -LiteralPath (Assert-ServerChildPath $root $stage) -Recurse -Force }
    $lock.Dispose()
    Remove-Item -LiteralPath $lockPath -Force
}
