Set-StrictMode -Version 2.0
$ErrorActionPreference = 'Stop'

function Get-PlsFileSha256 {
    param([Parameter(Mandatory = $true)][string] $Path)

    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Test-PlsExecutableVersion {
    param(
        [Parameter(Mandatory = $true)][string] $Observed,
        [Parameter(Mandatory = $true)][string] $Expected
    )

    $normalized = $Observed.Trim()
    if ($normalized.StartsWith('Version ', [System.StringComparison]::OrdinalIgnoreCase)) {
        $normalized = $normalized.Substring('Version '.Length).TrimStart()
    }
    return $normalized.StartsWith($Expected, [System.StringComparison]::Ordinal)
}

function Assert-PlsRegularFile {
    param(
        [Parameter(Mandatory = $true)][string] $Path,
        [Parameter(Mandatory = $true)][string] $Label
    )

    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer) {
        throw "$Label is a directory: $Path"
    }
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "$Label is a reparse point: $Path"
    }
    return $item.FullName
}

function Write-PlsJsonCreateNew {
    param(
        [Parameter(Mandatory = $true)][string] $Path,
        [Parameter(Mandatory = $true)][object] $Value
    )

    $json = $Value | ConvertTo-Json -Depth 30
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($json + "`n")
    $stream = [System.IO.File]::Open(
        $Path,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::None)
    try {
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
}

function Read-PlsAsciiLine {
    param(
        [Parameter(Mandatory = $true)][System.IO.Stream] $Stream,
        [int] $MaximumBytes = 65536
    )

    $bytes = New-Object System.Collections.Generic.List[byte]
    while ($true) {
        $value = $Stream.ReadByte()
        if ($value -lt 0) {
            if ($bytes.Count -eq 0) { return $null }
            throw 'Truncated PLS backup metadata line (missing LF terminator)'
        }
        if ($value -eq 10) { break }
        if ($bytes.Count -ge $MaximumBytes) {
            throw "PLS backup metadata line exceeds $MaximumBytes bytes"
        }
        $bytes.Add([byte] $value)
    }
    if ($bytes.Count -gt 0 -and $bytes[$bytes.Count - 1] -eq 13) {
        $bytes.RemoveAt($bytes.Count - 1)
    }
    return [System.Text.Encoding]::ASCII.GetString($bytes.ToArray())
}

function ConvertTo-PlsNormalizedMemberPath {
    param([Parameter(Mandatory = $true)][string] $Path)

    if ([string]::IsNullOrWhiteSpace($Path) -or $Path.IndexOf([char] 0) -ge 0) {
        throw 'PLS backup member path is empty or contains NUL'
    }
    $normalized = $Path.Replace('/', '\').TrimEnd('\')
    if ([string]::IsNullOrWhiteSpace($normalized)) {
        throw "PLS backup member path is empty after normalization: '$Path'"
    }
    foreach ($component in $normalized.Split('\')) {
        if ($component -eq '.' -or $component -eq '..') {
            throw "PLS backup member path contains a traversal component: '$Path'"
        }
    }
    return $normalized
}

function Get-PlsPathParent {
    param([Parameter(Mandatory = $true)][string] $Path)

    $separator = $Path.LastIndexOf('\')
    if ($separator -lt 0) { return '' }
    if ($separator -eq 2 -and $Path.Length -ge 3 -and $Path[1] -eq ':') {
        return $Path.Substring(0, 3)
    }
    return $Path.Substring(0, $separator).TrimEnd('\')
}

function Get-PlsPathLeaf {
    param([Parameter(Mandatory = $true)][string] $Path)

    $separator = $Path.LastIndexOf('\')
    if ($separator -lt 0) { return $Path }
    return $Path.Substring($separator + 1)
}

function Get-PlsRelativeMemberPath {
    param(
        [Parameter(Mandatory = $true)][string] $Path,
        [AllowEmptyString()][Parameter(Mandatory = $true)][string] $Root
    )

    if ($Root.Length -eq 0) {
        if ($Path.StartsWith('\') -or $Path -match '^[A-Za-z]:\\') {
            throw "Absolute member '$Path' cannot be mapped below an empty project root"
        }
        return $Path
    }
    if ($Path.Equals($Root, [System.StringComparison]::OrdinalIgnoreCase)) {
        return ''
    }
    $prefix = $Root.TrimEnd('\') + '\'
    if (-not $Path.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Backup member '$Path' is outside project root '$Root'"
    }
    return $Path.Substring($prefix.Length)
}

function Get-PlsGroupDigest {
    param([Parameter(Mandatory = $true)][object[]] $Members)

    $lines = @($Members | Sort-Object { $_.relative_path.ToLowerInvariant() } | ForEach-Object {
        "{0}`t{1}`t{2}" -f $_.relative_path.ToLowerInvariant(), $_.bytes, $_.sha256
    })
    $payload = [System.Text.UTF8Encoding]::new($false).GetBytes(($lines -join "`n") + "`n")
    $hasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        return ([System.BitConverter]::ToString($hasher.ComputeHash($payload))).Replace('-', '').ToLowerInvariant()
    } finally {
        $hasher.Dispose()
    }
}

function Get-PlsMemberRole {
    param(
        [Parameter(Mandatory = $true)][string] $RelativePath,
        [AllowNull()][string] $NativeType,
        [Parameter(Mandatory = $true)][string] $ProjectStem
    )

    $leaf = Get-PlsPathLeaf $RelativePath
    $extension = [System.IO.Path]::GetExtension($leaf).ToLowerInvariant()
    $stem = [System.IO.Path]::GetFileNameWithoutExtension($leaf)
    $coreExtensions = @('.xyz', '.don', '.num', '.cri', '.str', '.brk', '.con', '.fea', '.pps')
    if ($stem.Equals($ProjectStem, [System.StringComparison]::OrdinalIgnoreCase) -and
        $coreExtensions -contains $extension) {
        return 'project_core'
    }

    $libraryExtensions = @('.012', '.014', '.str', '.tow', '.tower', '.pole', '.sps')
    $libraryTypePattern = '(?i)(PLS[- ]POLE|TOWER|STRUCT|SAPS|CABLE|WIRE|MATERIAL|PART)'
    $libraryPathPattern = '(?i)(^|\\)(structures?|[^\\]+\.cables)(\\|$)'
    if (($libraryExtensions -contains $extension) -or
        ($null -ne $NativeType -and $NativeType -match $libraryTypePattern) -or
        ($RelativePath -match $libraryPathPattern)) {
        return 'engineering_library'
    }
    return 'ancillary'
}

function Resolve-PlsNativeBackupPayload {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string] $BackupPath,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string] $ExpectedSha256,
        [Parameter(Mandatory = $true)][string] $ExtractionDirectory,
        [long] $MaximumUncompressedBytes = 8589934592,
        [double] $MaximumCompressionRatio = 500.0
    )

    $source = Assert-PlsRegularFile $BackupPath 'candidate backup'
    $actual = Get-PlsFileSha256 $source
    if ($actual -cne $ExpectedSha256.ToLowerInvariant()) {
        throw "Candidate backup digest mismatch: expected $ExpectedSha256, got $actual"
    }

    $stream = [System.IO.File]::Open($source, [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
    try {
        # Read enough bytes for the full 26-byte native magic.  A shorter
        # probe silently classified valid PLSBACKUPFILE streams as unknown.
        $magic = New-Object byte[] 64
        $count = $stream.Read($magic, 0, $magic.Length)
    } finally {
        $stream.Dispose()
    }
    $prefix = [System.Text.Encoding]::ASCII.GetString($magic, 0, $count)
    if ($prefix.StartsWith("TYPE='***PLSBACKUPFILE***'", [System.StringComparison]::Ordinal)) {
        return [ordered]@{
            container_format = 'native_plsbackupfile_v3_1'
            source_path = $source
            source_sha256 = $actual
            native_path = $source
            native_sha256 = $actual
            native_bytes = (Get-Item -LiteralPath $source).Length
        }
    }
    if ($count -lt 4 -or $magic[0] -ne 0x50 -or $magic[1] -ne 0x4b -or
        $magic[2] -ne 0x03 -or $magic[3] -ne 0x04) {
        throw 'Candidate is neither a native PLSBACKUPFILE nor a ZIP local-file container'
    }

    if (Test-Path -LiteralPath $ExtractionDirectory) {
        throw "Backup extraction directory already exists: $ExtractionDirectory"
    }
    [System.IO.Directory]::CreateDirectory($ExtractionDirectory) | Out-Null
    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($source)
    try {
        $entries = @($archive.Entries)
        if ($entries.Count -ne 1) {
            throw "ZIP-wrapped PLS backup must contain exactly one entry; found $($entries.Count)"
        }
        $entry = $entries[0]
        if ([string]::IsNullOrEmpty($entry.Name) -or $entry.FullName -cne $entry.Name -or
            [System.IO.Path]::GetExtension($entry.Name) -ine '.bak') {
            throw "ZIP backup entry must be one root-level .bak file; got '$($entry.FullName)'"
        }
        if ($entry.Length -le 0 -or $entry.Length -gt $MaximumUncompressedBytes) {
            throw "ZIP backup payload length is outside bounds: $($entry.Length)"
        }
        if ($entry.CompressedLength -le 0 -or
            ([double] $entry.Length / [double] $entry.CompressedLength) -gt $MaximumCompressionRatio) {
            throw 'ZIP backup payload compression ratio is outside bounds'
        }
        $native = Join-Path $ExtractionDirectory 'candidate-native.bak'
        $input = $entry.Open()
        $output = [System.IO.File]::Open($native, [System.IO.FileMode]::CreateNew,
            [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
        try {
            $input.CopyTo($output)
            $output.Flush($true)
        } finally {
            $output.Dispose()
            $input.Dispose()
        }
    } finally {
        $archive.Dispose()
    }

    $nativePrefixBytes = New-Object byte[] 64
    $nativeStream = [System.IO.File]::OpenRead($native)
    try {
        $nativePrefixCount = $nativeStream.Read($nativePrefixBytes, 0, $nativePrefixBytes.Length)
    } finally {
        $nativeStream.Dispose()
    }
    $nativePrefix = [System.Text.Encoding]::ASCII.GetString($nativePrefixBytes, 0, $nativePrefixCount)
    if (-not $nativePrefix.StartsWith("TYPE='***PLSBACKUPFILE***'", [System.StringComparison]::Ordinal)) {
        throw 'The single ZIP member is not a native PLSBACKUPFILE'
    }
    return [ordered]@{
        container_format = 'single_member_zip_wrapping_native_plsbackupfile_v3_1'
        source_path = $source
        source_sha256 = $actual
        native_path = $native
        native_sha256 = Get-PlsFileSha256 $native
        native_bytes = (Get-Item -LiteralPath $native).Length
    }
}

function Get-PlsNativeBackupInventory {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][string] $NativeBackupPath,
        [string] $ProjectFileName,
        [string] $SourceRoot
    )

    $native = Assert-PlsRegularFile $NativeBackupPath 'native PLS backup payload'
    $members = New-Object System.Collections.ArrayList
    $stream = [System.IO.File]::Open($native, [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
    try {
        while ($stream.Position -lt $stream.Length) {
            $recordOffset = $stream.Position
            $header = Read-PlsAsciiLine $stream
            if ($null -eq $header) { break }
            $headerMatch = [regex]::Match($header,
                "^TYPE='\*\*\*PLSBACKUPFILE\*\*\*' VERSION='3\.1' UNITS='SI' SOURCE='([^']+)' USER='([^']*)' FILENAME='([^']+)'$")
            if (-not $headerMatch.Success) {
                throw "Invalid PLSBACKUPFILE v3.1 header at byte $recordOffset"
            }
            $declaredPath = Read-PlsAsciiLine $stream
            $sizeLine = Read-PlsAsciiLine $stream
            $timestamp = Read-PlsAsciiLine $stream
            if ($null -eq $declaredPath -or $null -eq $sizeLine -or $null -eq $timestamp) {
                throw "Truncated PLS backup record metadata at byte $recordOffset"
            }
            $sizeMatch = [regex]::Match($sizeLine, '^\s*([0-9]+)\s+(directory|text|binary)\s+1$')
            if (-not $sizeMatch.Success) {
                throw "Invalid PLS backup member size/kind line at byte $recordOffset"
            }
            if ($timestamp -notmatch '^[0-9]{4}\s+[0-9]{1,2}\s+[0-9]{1,2}\s+[0-9]{1,2}\s+[0-9]{1,2}\s+[0-9]{1,2}$') {
                throw "Invalid PLS backup member timestamp at byte $recordOffset"
            }
            $length = [long] $sizeMatch.Groups[1].Value
            $kind = $sizeMatch.Groups[2].Value
            if ($kind -eq 'directory' -and $length -ne 0) {
                throw "Directory member has nonzero length at byte $recordOffset"
            }
            if ($length -gt ($stream.Length - $stream.Position)) {
                throw "PLS backup member payload exceeds remaining stream at byte $recordOffset"
            }
            $payloadOffset = $stream.Position

            $hasher = [System.Security.Cryptography.SHA256]::Create()
            $buffer = New-Object byte[] 1048576
            $remaining = $length
            $prefixBytes = New-Object System.Collections.Generic.List[byte]
            try {
                while ($remaining -gt 0) {
                    $requested = [int] [Math]::Min([long] $buffer.Length, $remaining)
                    $read = $stream.Read($buffer, 0, $requested)
                    if ($read -le 0) { throw "Truncated member payload at byte $recordOffset" }
                    if ($prefixBytes.Count -lt 1024) {
                        $prefixCount = [Math]::Min(1024 - $prefixBytes.Count, $read)
                        for ($index = 0; $index -lt $prefixCount; $index++) {
                            $prefixBytes.Add($buffer[$index])
                        }
                    }
                    $hasher.TransformBlock($buffer, 0, $read, $null, 0) | Out-Null
                    $remaining -= $read
                }
                $hasher.TransformFinalBlock((New-Object byte[] 0), 0, 0) | Out-Null
                $digest = ([System.BitConverter]::ToString($hasher.Hash)).Replace('-', '').ToLowerInvariant()
            } finally {
                $hasher.Dispose()
            }
            $normalizedPath = ConvertTo-PlsNormalizedMemberPath $declaredPath
            $headerPath = ConvertTo-PlsNormalizedMemberPath $headerMatch.Groups[3].Value
            if (-not (Get-PlsPathLeaf $normalizedPath).Equals(
                    (Get-PlsPathLeaf $headerPath), [System.StringComparison]::OrdinalIgnoreCase)) {
                throw "PLS backup member header/path leaf mismatch at byte $recordOffset"
            }
            $prefixText = [System.Text.Encoding]::ASCII.GetString($prefixBytes.ToArray())
            $nativeTypeMatch = [regex]::Match($prefixText, "(?:^|[\r\n])TYPE='([^']+)'", 'IgnoreCase')
            $nativeType = $null
            if ($nativeTypeMatch.Success) { $nativeType = $nativeTypeMatch.Groups[1].Value }
            $members.Add([ordered]@{
                record_offset = $recordOffset
                payload_offset = $payloadOffset
                source = $headerMatch.Groups[1].Value
                original_path = $normalizedPath
                kind = $kind
                bytes = $length
                sha256 = $digest
                native_type = $nativeType
                timestamp = $timestamp
            }) | Out-Null
        }
    } finally {
        $stream.Dispose()
    }
    if ($members.Count -eq 0) { throw 'Native PLS backup contains no records' }

    $projects = @($members | Where-Object {
        $_.kind -ne 'directory' -and [System.IO.Path]::GetExtension($_.original_path) -ieq '.xyz'
    })
    if (-not [string]::IsNullOrWhiteSpace($ProjectFileName)) {
        if ($ProjectFileName.IndexOfAny([char[]] @('\', '/')) -ge 0) {
            throw 'ProjectFileName must be a leaf name, not a path'
        }
        $projects = @($projects | Where-Object {
            (Get-PlsPathLeaf $_.original_path).Equals($ProjectFileName, [System.StringComparison]::OrdinalIgnoreCase)
        })
    }
    if ($projects.Count -ne 1) {
        throw "Expected exactly one selected .xyz project in backup; found $($projects.Count)"
    }
    $projectPath = $projects[0].original_path
    $projectRoot = if ([string]::IsNullOrWhiteSpace($SourceRoot)) {
        Get-PlsPathParent $projectPath
    } else {
        $selectedRoot = if ($SourceRoot -match '^[A-Za-z]:[\\/]$') {
            $SourceRoot.Replace('/', '\')
        } else {
            ConvertTo-PlsNormalizedMemberPath $SourceRoot
        }
        if ($selectedRoot -notmatch '^[A-Za-z]:\\' -and -not $selectedRoot.StartsWith('\\')) {
            throw "SourceRoot must be one absolute native Windows path: '$SourceRoot'"
        }
        $selectedRoot
    }
    $projectLeaf = Get-PlsPathLeaf $projectPath
    $projectStem = [System.IO.Path]::GetFileNameWithoutExtension($projectLeaf)
    $seen = @{}
    foreach ($member in $members) {
        $relative = Get-PlsRelativeMemberPath $member.original_path $projectRoot
        if ([string]::IsNullOrEmpty($relative)) {
            if ($member.kind -ne 'directory') {
                throw "Backup file maps to the project root rather than a child: '$($member.original_path)'"
            }
            # Native PLS backups may carry one explicit record for the common
            # project root itself. Keep it as a typed directory invariant; a
            # file at this alias remains forbidden.
            $relative = '.'
        }
        $key = $relative.ToLowerInvariant()
        if ($seen.ContainsKey($key)) {
            throw "Duplicate case-insensitive restore path in backup: '$relative'"
        }
        $seen[$key] = $true
        $member.relative_path = $relative
        if ($member.kind -eq 'directory') {
            $member.role = 'directory'
        } else {
            $member.role = Get-PlsMemberRole $relative $member.native_type $projectStem
        }
    }

    $requiredCore = @('.xyz', '.don', '.num')
    foreach ($extension in $requiredCore) {
        $count = @($members | Where-Object {
            $_.role -eq 'project_core' -and
            [System.IO.Path]::GetExtension($_.relative_path) -ieq $extension
        }).Count
        if ($count -ne 1) {
            throw "Candidate backup must contain exactly one project $extension file; found $count"
        }
    }
    $files = @($members | Where-Object { $_.kind -ne 'directory' })
    $core = @($files | Where-Object { $_.role -eq 'project_core' })
    $libraries = @($files | Where-Object { $_.role -eq 'engineering_library' })
    if ($libraries.Count -eq 0) {
        throw 'Candidate backup contains no recognized engineering-library members'
    }
    $protected = @($files | Where-Object { $_.role -in @('project_core', 'engineering_library') })
    $sources = @($members | ForEach-Object { $_.source } | Sort-Object -Unique)
    return [ordered]@{
        schema = 'ds.pls.native_backup_inventory.v1'
        native_backup_path = $native
        native_backup_sha256 = Get-PlsFileSha256 $native
        native_backup_bytes = (Get-Item -LiteralPath $native).Length
        backup_sources = $sources
        project_source_root = $projectRoot
        project_file = (Get-PlsRelativeMemberPath $projectPath $projectRoot)
        project_stem = $projectStem
        counts = [ordered]@{
            records = $members.Count
            directories = @($members | Where-Object { $_.kind -eq 'directory' }).Count
            files = $files.Count
            project_core = $core.Count
            engineering_library = $libraries.Count
            ancillary = @($files | Where-Object { $_.role -eq 'ancillary' }).Count
        }
        digests = [ordered]@{
            all_files = Get-PlsGroupDigest $files
            project_core = Get-PlsGroupDigest $core
            engineering_library = Get-PlsGroupDigest $libraries
            protected = Get-PlsGroupDigest $protected
        }
        members = @($members)
    }
}

function Compare-PlsProtectedInventories {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object] $Expected,
        [Parameter(Mandatory = $true)][object] $Actual
    )

    $differences = New-Object System.Collections.ArrayList
    foreach ($group in @('project_core', 'engineering_library', 'protected')) {
        if ([string] $Expected.digests.$group -cne [string] $Actual.digests.$group) {
            $differences.Add("$group digest differs") | Out-Null
        }
    }
    foreach ($count in @('project_core', 'engineering_library')) {
        if ([int] $Expected.counts.$count -ne [int] $Actual.counts.$count) {
            $differences.Add("$count count differs") | Out-Null
        }
    }
    [ordered]@{
        equal = ($differences.Count -eq 0)
        differences = @($differences)
        expected_protected_sha256 = $Expected.digests.protected
        actual_protected_sha256 = $Actual.digests.protected
    }
}

function Get-PlsNativeMemberBytes([object] $Inventory, [object] $Member) {
    $native = Assert-PlsRegularFile ([string] $Inventory.native_backup_path) 'native PLS backup payload'
    $offset = [long] $Member.payload_offset
    $length = [long] $Member.bytes
    if ($length -lt 0 -or $length -gt [int]::MaxValue) {
        throw "Native member length is not readable: $($Member.relative_path)"
    }
    $stream = [System.IO.File]::Open($native, [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
    try {
        if ($offset -lt 0 -or $offset -gt $stream.Length - $length) {
            throw "Native member offset is outside payload: $($Member.relative_path)"
        }
        [void] $stream.Seek($offset, [System.IO.SeekOrigin]::Begin)
        $bytes = New-Object byte[] ([int] $length)
        $read = 0
        while ($read -lt $bytes.Length) {
            $count = $stream.Read($bytes, $read, $bytes.Length - $read)
            if ($count -le 0) { throw "Truncated native member: $($Member.relative_path)" }
            $read += $count
        }
    } finally {
        $stream.Dispose()
    }
    $hasher = [System.Security.Cryptography.SHA256]::Create()
    try {
        $digest = ([System.BitConverter]::ToString($hasher.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    } finally {
        $hasher.Dispose()
    }
    if ($digest -cne [string] $Member.sha256) {
        throw "Native member payload digest mismatch: $($Member.relative_path)"
    }
    return ,$bytes
}

function Test-PlsRestoredTree {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory = $true)][object] $Inventory,
        [Parameter(Mandatory = $true)][string] $Root,
        [ValidateSet('Full', 'Protected')][string] $Scope = 'Full',
        [string[]] $AllowedExtraRelativePaths = @(),
        [switch] $PresenceOnly
    )

    $rootItem = Get-Item -LiteralPath $Root -Force
    if (-not $rootItem.PSIsContainer) { throw "Restore root is not a directory: $Root" }
    if (($rootItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Restore root is a reparse point: $Root"
    }
    $rootFull = $rootItem.FullName.TrimEnd('\')
    $members = @($Inventory.members | Where-Object {
        if ($Scope -eq 'Full') { return $true }
        return $_.role -in @('project_core', 'engineering_library')
    })
    $expectedFiles = @{}
    $expectedDirectories = @{ '.' = $true }
    $verified = New-Object System.Collections.ArrayList
    foreach ($member in $members) {
        $relative = [string] $member.relative_path
        $path = Join-Path $rootFull $relative
        if ($member.kind -eq 'directory') {
            if (-not (Test-Path -LiteralPath $path -PathType Container)) {
                throw "Restored directory missing: $relative"
            }
            $expectedDirectories[$relative.ToLowerInvariant()] = $true
            continue
        }
        $file = Assert-PlsRegularFile $path "restored member '$relative'"
        # Native backups do not have to record every ancestor directory.
        # Those exact ancestors are implied by the recorded member path;
        # unrelated extra directories still fail the Full check below.
        $parent = [System.IO.Path]::GetDirectoryName($relative)
        while (-not [string]::IsNullOrEmpty($parent) -and $parent -ne '.') {
            $expectedDirectories[$parent.ToLowerInvariant()] = $true
            $parent = [System.IO.Path]::GetDirectoryName($parent)
        }
        $item = Get-Item -LiteralPath $file -Force
        $digest = $null
        $exact = $true
        $verification = 'present'
        if (-not $PresenceOnly) {
            $digest = Get-PlsFileSha256 $file
            $exact = ([long] $item.Length -eq [long] $member.bytes -and
                $digest -ceq [string] $member.sha256)
            $verification = 'exact'
        }
        if (-not $PresenceOnly -and -not $exact -and $member.kind -eq 'text') {
            # Native PLS Restore may change line endings in either direction.
            # Compare both text sides after the same normalization; binary
            # records remain exact. The original is extracted from the exact
            # native payload and checked against the inventory digest first.
            $latin1 = [System.Text.Encoding]::GetEncoding(28591)
            $raw = [System.IO.File]::ReadAllBytes($file)
            $original = Get-PlsNativeMemberBytes $Inventory $member
            $originalText = $latin1.GetString($original).Replace("`r`n", "`n")
            $normalizedText = $latin1.GetString($raw).Replace("`r`n", "`n")
            # PLS also rewrites embedded absolute workspace FILENAME bindings
            # to the fresh Restore root before offering to open the project.
            $rebased = $normalizedText.Replace($rootFull,
                [string] $Inventory.project_source_root)
            if ($rebased -ceq $originalText) {
                $verification = if ($rebased -cne $normalizedText) {
                    'native_text_crlf_and_path_rebase'
                } else {
                    'native_text_crlf'
                }
                $exact = $true
            }
        }
        if (-not $exact) {
            if ([long] $item.Length -ne [long] $member.bytes) {
                throw "Restored member length mismatch: $relative"
            }
            throw "Restored member digest mismatch: $relative"
        }
        $expectedFiles[$relative.ToLowerInvariant()] = $true
        $verified.Add([ordered]@{
            relative_path = $relative
            bytes = $item.Length
            sha256 = $digest
            verification = $verification
        }) | Out-Null
    }

    $extras = New-Object System.Collections.ArrayList
    if ($Scope -eq 'Full') {
        $allowed = @{}
        foreach ($relative in $AllowedExtraRelativePaths) { $allowed[$relative.ToLowerInvariant()] = $true }
        foreach ($item in Get-ChildItem -LiteralPath $rootFull -Force -Recurse) {
            if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Reparse point found below restored tree: $($item.FullName)"
            }
            $relative = $item.FullName.Substring($rootFull.Length).TrimStart('\').Replace('/', '\')
            $key = $relative.ToLowerInvariant()
            if ($item.PSIsContainer) {
                if (-not $expectedDirectories.ContainsKey($key)) {
                    $extras.Add($relative + '\') | Out-Null
                }
                continue
            }
            if (-not $expectedFiles.ContainsKey($key) -and -not $allowed.ContainsKey($key)) {
                $extras.Add($relative) | Out-Null
            }
        }
        if ($extras.Count -ne 0) {
            throw "Unexpected file(s) in fresh restore: $(@($extras) -join ', ')"
        }
    }
    $verifiedDigest = $null
    if (-not $PresenceOnly) {
        $verifiedDigest = Get-PlsGroupDigest @($members | Where-Object { $_.kind -ne 'directory' })
    }
    return [ordered]@{
        schema = 'ds.pls.restored_tree_verification.v1'
        scope = $Scope.ToLowerInvariant()
        root = $rootFull
        verified_files = $verified.Count
        verified_members = @($verified)
        verified_digest = $verifiedDigest
        unexpected_files = @($extras)
    }
}

Export-ModuleMember -Function @(
    'Assert-PlsRegularFile',
    'Compare-PlsProtectedInventories',
    'Get-PlsFileSha256',
    'Get-PlsNativeBackupInventory',
    'Resolve-PlsNativeBackupPayload',
    'Test-PlsExecutableVersion',
    'Test-PlsRestoredTree',
    'Write-PlsJsonCreateNew'
)
