param(
    [Parameter(Mandatory = $true)]
    [string] $ReportBundleDirectory,
    [Parameter(Mandatory = $true)]
    [string] $BackupPath,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9A-Fa-f]{64}$')]
    [string] $ExpectedBackupSha256,
    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[A-Za-z0-9][A-Za-z0-9._ -]{0,120}$')]
    [string] $PackageBaseName,
    [string] $DesktopDirectory = [Environment]::GetFolderPath('Desktop')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

Add-Type -AssemblyName System.IO.Compression

function Resolve-RegularFile([string] $Path, [string] $Label) {
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer) { throw "$Label is a directory: $Path" }
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "$Label may not be a reparse point: $Path"
    }
    return $item.FullName
}

function Resolve-PlainDirectory([string] $Path, [string] $Label) {
    $item = Get-Item -LiteralPath $Path -Force
    if (-not $item.PSIsContainer) { throw "$Label is not a directory: $Path" }
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "$Label may not be a reparse point: $Path"
    }
    return $item.FullName.TrimEnd('\')
}

function Get-Sha256([string] $Path) {
    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Get-RelativePath([string] $Root, [string] $Path) {
    $rootUri = [Uri] (([System.IO.Path]::GetFullPath($Root)).TrimEnd('\') + '\')
    $pathUri = [Uri] ([System.IO.Path]::GetFullPath($Path))
    return [Uri]::UnescapeDataString($rootUri.MakeRelativeUri($pathUri).ToString())
}

function Write-JsonCreateNew([string] $Path, [object] $Value) {
    $json = $Value | ConvertTo-Json -Depth 30
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($json + "`n")
    $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try {
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
}

function Get-TreeFiles([string] $Root) {
    $rootPath = Resolve-PlainDirectory $Root 'tree root'
    foreach ($item in Get-ChildItem -LiteralPath $rootPath -Force -Recurse) {
        if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Package source contains a reparse point: $($item.FullName)"
        }
        if (-not $item.PSIsContainer) { $item.FullName }
    }
}

function Get-FileRecord([string] $Root, [string] $Path, [string] $Role) {
    $item = Get-Item -LiteralPath $Path -Force
    return [ordered]@{
        role = $Role
        path = (Get-RelativePath $Root $item.FullName).Replace('\', '/')
        bytes = [long] $item.Length
        sha256 = Get-Sha256 $item.FullName
    }
}

function Copy-TreeCreateNew([string] $Source, [string] $Destination) {
    $sourceRoot = Resolve-PlainDirectory $Source 'report bundle directory'
    [System.IO.Directory]::CreateDirectory($Destination) | Out-Null
    foreach ($directory in Get-ChildItem -LiteralPath $sourceRoot -Force -Directory -Recurse) {
        if (($directory.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Report bundle contains a reparse point: $($directory.FullName)"
        }
        $relative = Get-RelativePath $sourceRoot $directory.FullName
        [System.IO.Directory]::CreateDirectory((Join-Path $Destination $relative)) | Out-Null
    }
    foreach ($file in @(Get-TreeFiles $sourceRoot)) {
        $relative = Get-RelativePath $sourceRoot $file
        $target = Join-Path $Destination $relative
        $parent = [System.IO.Path]::GetDirectoryName($target)
        [System.IO.Directory]::CreateDirectory($parent) | Out-Null
        [System.IO.File]::Copy($file, $target, $false)
    }
}

function New-ZipCreateNew([string] $SourceRoot, [string] $Destination) {
    $output = [System.IO.File]::Open($Destination, [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::ReadWrite, [System.IO.FileShare]::None)
    $archive = $null
    try {
        $archive = [System.IO.Compression.ZipArchive]::new(
            $output, [System.IO.Compression.ZipArchiveMode]::Create, $true)
        foreach ($file in @(Get-TreeFiles $SourceRoot | Sort-Object)) {
            $relative = (Get-RelativePath $SourceRoot $file).Replace('\', '/')
            $entry = $archive.CreateEntry($relative, [System.IO.Compression.CompressionLevel]::Optimal)
            $input = [System.IO.File]::Open($file, [System.IO.FileMode]::Open,
                [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
            $entryStream = $entry.Open()
            try {
                $input.CopyTo($entryStream)
            } finally {
                $entryStream.Dispose()
                $input.Dispose()
            }
        }
    } finally {
        if ($null -ne $archive) { $archive.Dispose() }
        $output.Dispose()
    }
}

function Get-StreamSha256([System.IO.Stream] $Stream) {
    $sha = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([BitConverter]::ToString($sha.ComputeHash($Stream))).Replace('-', '').ToLowerInvariant()
    } finally {
        $sha.Dispose()
    }
}

function Assert-ZipMatches([string] $ZipPath, [object[]] $Expected) {
    $expectedByPath = @{}
    foreach ($record in $Expected) {
        if ($expectedByPath.ContainsKey([string] $record.path)) {
            throw "Duplicate expected archive entry: $($record.path)"
        }
        $expectedByPath[[string] $record.path] = $record
    }
    $stream = [System.IO.File]::Open($ZipPath, [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
    $archive = $null
    try {
        $archive = [System.IO.Compression.ZipArchive]::new(
            $stream, [System.IO.Compression.ZipArchiveMode]::Read, $true)
        $observed = @{}
        foreach ($entry in $archive.Entries) {
            if ([string]::IsNullOrWhiteSpace($entry.Name)) {
                throw "Archive contains an unexpected directory entry: $($entry.FullName)"
            }
            if ($observed.ContainsKey($entry.FullName)) {
                throw "Archive contains a duplicate entry: $($entry.FullName)"
            }
            if (-not $expectedByPath.ContainsKey($entry.FullName)) {
                throw "Archive contains an unexpected entry: $($entry.FullName)"
            }
            $expectedRecord = $expectedByPath[$entry.FullName]
            $entryStream = $entry.Open()
            try { $digest = Get-StreamSha256 $entryStream } finally { $entryStream.Dispose() }
            if ([long] $entry.Length -ne [long] $expectedRecord.bytes -or
                $digest -cne [string] $expectedRecord.sha256) {
                throw "Archive entry bytes/hash mismatch: $($entry.FullName)"
            }
            $observed[$entry.FullName] = $true
        }
        if ($observed.Count -ne $expectedByPath.Count) {
            $missing = @($expectedByPath.Keys | Where-Object { -not $observed.ContainsKey($_) } | Sort-Object)
            throw "Archive is missing expected entries: $($missing -join ', ')"
        }
    } finally {
        if ($null -ne $archive) { $archive.Dispose() }
        $stream.Dispose()
    }
}

if ($PackageBaseName.EndsWith('.zip', [StringComparison]::OrdinalIgnoreCase)) {
    throw 'PackageBaseName must not include the .zip extension'
}
$desktop = Resolve-PlainDirectory $DesktopDirectory 'Desktop directory'
$bundle = Resolve-PlainDirectory $ReportBundleDirectory 'report bundle directory'
$backup = Resolve-RegularFile $BackupPath 'fresh native backup'
if ([System.IO.Path]::GetExtension($backup) -ine '.bak') {
    throw "BackupPath must end in .bak: $backup"
}
$backupDigest = Get-Sha256 $backup
if ($backupDigest -cne $ExpectedBackupSha256.ToLowerInvariant()) {
    throw "Fresh backup digest mismatch: expected $ExpectedBackupSha256, got $backupDigest"
}
$bundleManifestPath = Resolve-RegularFile (Join-Path $bundle 'manifest.json') 'report bundle manifest'
if (Test-Path -LiteralPath (Join-Path $bundle 'INCOMPLETE.json')) {
    throw 'Report bundle still carries INCOMPLETE.json'
}
$bundleManifest = Get-Content -LiteralPath $bundleManifestPath -Raw | ConvertFrom-Json
if ([string] $bundleManifest.schema -cne 'ds.pls.report_bundle_manifest.v1' -or
    [string] $bundleManifest.status -cne 'complete_with_caveat') {
    throw 'Report bundle manifest is not finalized'
}
if ([string] $bundleManifest.source_backup_sha256 -cne $backupDigest) {
    throw 'Report bundle was not finalized against the supplied fresh backup'
}
foreach ($artifact in @($bundleManifest.artifacts)) {
    $relative = ([string] $artifact.path).Replace('/', '\')
    if ([System.IO.Path]::IsPathRooted($relative) -or $relative -match '(^|\\)\.\.(\\|$)') {
        throw "Unsafe report-bundle artifact path: $($artifact.path)"
    }
    $artifactPath = Resolve-RegularFile (Join-Path $bundle $relative) 'report-bundle artifact'
    if ([long] (Get-Item -LiteralPath $artifactPath).Length -ne [long] $artifact.bytes -or
        (Get-Sha256 $artifactPath) -cne [string] $artifact.sha256) {
        throw "Report-bundle artifact changed after finalization: $($artifact.path)"
    }
}

$staging = Join-Path $desktop $PackageBaseName
$zipPath = Join-Path $desktop ($PackageBaseName + '.zip')
$receiptPath = Join-Path $desktop ($PackageBaseName + '.zip.manifest.json')
foreach ($target in @($staging, $zipPath, $receiptPath)) {
    if (Test-Path -LiteralPath $target) { throw "Package output already exists: $target" }
}

$partialZip = Join-Path $desktop ($PackageBaseName + '.' + [Guid]::NewGuid().ToString('n') + '.partial')
$createdStaging = $false
try {
    [System.IO.Directory]::CreateDirectory($staging) | Out-Null
    $createdStaging = $true
    $backupDirectory = Join-Path $staging 'native-backup'
    $bundleDirectory = Join-Path $staging 'report-bundle'
    [System.IO.Directory]::CreateDirectory($backupDirectory) | Out-Null
    $stagedBackup = Join-Path $backupDirectory ([System.IO.Path]::GetFileName($backup))
    [System.IO.File]::Copy($backup, $stagedBackup, $false)
    Copy-TreeCreateNew $bundle $bundleDirectory
    if ((Get-Sha256 $stagedBackup) -cne $backupDigest) {
        throw 'Staged native backup differs from the qualified source backup'
    }
    $stagedBundleManifest = Resolve-RegularFile (Join-Path $bundleDirectory 'manifest.json') `
        'staged report bundle manifest'
    if ((Get-Sha256 $stagedBundleManifest) -cne (Get-Sha256 $bundleManifestPath)) {
        throw 'Staged report bundle manifest differs from the finalized source manifest'
    }
    foreach ($artifact in @($bundleManifest.artifacts)) {
        $relative = ([string] $artifact.path).Replace('/', '\')
        $stagedArtifact = Resolve-RegularFile (Join-Path $bundleDirectory $relative) `
            'staged report-bundle artifact'
        if ([long] (Get-Item -LiteralPath $stagedArtifact).Length -ne [long] $artifact.bytes -or
            (Get-Sha256 $stagedArtifact) -cne [string] $artifact.sha256) {
            throw "Staged report-bundle artifact differs from finalization: $($artifact.path)"
        }
    }

    $payload = New-Object System.Collections.ArrayList
    $payload.Add((Get-FileRecord $staging $stagedBackup 'fresh_native_backup')) | Out-Null
    foreach ($file in @(Get-TreeFiles $bundleDirectory | Sort-Object)) {
        $payload.Add((Get-FileRecord $staging $file 'report_bundle')) | Out-Null
    }
    $packageManifestPath = Join-Path $staging 'package-manifest.json'
    $packageManifest = [ordered]@{
        schema = 'ds.pls.desktop_package_manifest.v1'
        status = 'complete'
        created_at_utc = [DateTime]::UtcNow.ToString('o')
        package_base_name = $PackageBaseName
        fresh_backup_sha256 = $backupDigest
        report_bundle_manifest_sha256 = Get-Sha256 $bundleManifestPath
        report_bundle_status = [string] $bundleManifest.status
        caveat = $bundleManifest.caveat
        payload = @($payload | Sort-Object path)
    }
    Write-JsonCreateNew $packageManifestPath $packageManifest
    $expectedEntries = @($payload) + @((Get-FileRecord $staging $packageManifestPath 'package_manifest'))

    New-ZipCreateNew $staging $partialZip
    Assert-ZipMatches $partialZip $expectedEntries
    if (Test-Path -LiteralPath $zipPath) { throw "Package ZIP appeared concurrently: $zipPath" }
    [System.IO.File]::Move($partialZip, $zipPath)
    Assert-ZipMatches $zipPath $expectedEntries
    $zipDigest = Get-Sha256 $zipPath
    $receipt = [ordered]@{
        schema = 'ds.pls.desktop_package_receipt.v1'
        status = 'verified'
        verified_at_utc = [DateTime]::UtcNow.ToString('o')
        zip_path = $zipPath
        zip_bytes = [long] (Get-Item -LiteralPath $zipPath).Length
        zip_sha256 = $zipDigest
        archive_entry_count = $expectedEntries.Count
        archive_entries_verified = $true
        package_manifest_sha256 = Get-Sha256 $packageManifestPath
        fresh_backup_sha256 = $backupDigest
        report_bundle_manifest_sha256 = Get-Sha256 $bundleManifestPath
    }
    Write-JsonCreateNew $receiptPath $receipt
    $receipt | ConvertTo-Json -Depth 10
} catch {
    if ($createdStaging) {
        $failurePath = Join-Path $staging 'PACKAGE-INCOMPLETE.json'
        if (-not (Test-Path -LiteralPath $failurePath)) {
            try {
                Write-JsonCreateNew $failurePath ([ordered]@{
                    schema = 'ds.pls.desktop_package_incomplete.v1'
                    failed_at_utc = [DateTime]::UtcNow.ToString('o')
                    message = $_.Exception.Message
                })
            } catch {}
        }
    }
    throw
} finally {
    if (Test-Path -LiteralPath $partialZip -PathType Leaf) {
        [System.IO.File]::Delete($partialZip)
    }
}
