# Shared package checks; independent of ds-web and Tauri.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem

function Assert-ServerLocalPath([string]$Path) {
    if ($Path -notmatch '^[A-Za-z]:[\\/]') { throw 'Expected an absolute local Windows drive path.' }
    $resolved = [IO.Path]::GetFullPath($Path)
    $cursor = $resolved
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            if ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
                throw "Reparse points are not admitted: $cursor"
            }
        }
        $parent = Split-Path $cursor -Parent
        if ($parent -eq $cursor) { break }
        $cursor = $parent
    }
    return $resolved
}

function Assert-ServerRelativePath([string]$Path) {
    if (-not $Path -or $Path.Contains('\') -or $Path.StartsWith('/')) { throw 'Invalid package member path.' }
    foreach ($part in $Path.Split('/')) {
        if (-not $part -or $part -in @('.', '..') -or $part -match '[<>:"|?*\x00-\x1f]' -or
            $part -match '[. ]$' -or $part -match '^(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)') {
            throw "Invalid package member: $Path"
        }
    }
}

function Get-ServerInventory([string]$Root) {
    $Root = Assert-ServerLocalPath $Root
    $entries = @(Get-ChildItem -LiteralPath $Root -Recurse -Force)
    if ($entries.Count -gt 2048) { throw 'Package inventory exceeds 2048 entries.' }
    $total = [long]0
    foreach ($entry in $entries) {
        if ($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Package contains a reparse point.' }
        if ($entry.PSIsContainer) { continue }
        $relative = $entry.FullName.Substring($Root.TrimEnd('\').Length + 1).Replace('\', '/')
        Assert-ServerRelativePath $relative
        if ($relative -eq 'release.json') { continue }
        $total += $entry.Length
        if ($entry.Length -gt 536870912 -or $total -gt 2147483648) { throw 'Package exceeds its size budget.' }
        [pscustomobject]@{ path=$relative; size_bytes=$entry.Length; sha256=(Get-FileHash -LiteralPath $entry.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
    }
}

function Expand-ServerArchive([string]$Archive, [string]$Destination) {
    $Archive = Assert-ServerLocalPath $Archive
    $Destination = Assert-ServerLocalPath $Destination
    if (Test-Path -LiteralPath $Destination) { throw 'Extraction destination already exists.' }
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    try {
        if ($zip.Entries.Count -gt 2048) { throw 'Archive exceeds 2048 members.' }
        $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
        $total = [long]0
        # Validate the complete directory BEFORE exposing a single file.
        foreach ($entry in $zip.Entries) {
            $name = $entry.FullName.TrimEnd('/')
            Assert-ServerRelativePath $name
            if (-not $names.Add($name)) { throw 'Archive contains duplicate Windows paths.' }
            $attributes = [long]$entry.ExternalAttributes -band 0xffffffffL
            if (($attributes -band 0x400) -or (($attributes -shr 16) -band 0xf000) -eq 0xa000) { throw 'Archive contains a link.' }
            $total += $entry.Length
            if ($entry.Length -gt 536870912 -or $total -gt 2147483648) { throw 'Archive exceeds its size budget.' }
            if ($entry.FullName.EndsWith('/') -and $entry.Length) { throw 'Archive directory carries bytes.' }
        }
        [IO.Directory]::CreateDirectory($Destination) | Out-Null
        foreach ($entry in $zip.Entries) {
            $target = Join-Path $Destination $entry.FullName.Replace('/', '\')
            if ($entry.FullName.EndsWith('/')) { [IO.Directory]::CreateDirectory($target) | Out-Null; continue }
            [IO.Directory]::CreateDirectory((Split-Path $target -Parent)) | Out-Null
            $output = [IO.File]::Open($target, [IO.FileMode]::CreateNew)
            $memberSource = $entry.Open()
            try {
                # Enforce the validated byte budget while writing, even if the
                # ZIP's central directory lies about its decompressed length.
                $buffer = [byte[]]::new(65536)
                $remaining = [long]$entry.Length
                while ($remaining -gt 0) {
                    $read = $memberSource.Read($buffer, 0, [int][Math]::Min($buffer.Length, $remaining))
                    if ($read -eq 0) { throw 'Archive member ended before its declared length.' }
                    $output.Write($buffer, 0, $read)
                    $remaining -= $read
                }
                if ($memberSource.ReadByte() -ne -1) { throw 'Archive member expanded beyond its declared length.' }
            } finally { $memberSource.Dispose(); $output.Dispose() }
            if ((Get-Item -LiteralPath $target).Length -ne $entry.Length) { throw 'Extracted length differs.' }
        }
    } finally { $zip.Dispose() }
}

function Test-ServerPayload([string]$Root, [string]$Lane) {
    $Root = Assert-ServerLocalPath $Root
    $manifestPath = Join-Path $Root 'release.json'
    if ((Get-Item -LiteralPath $manifestPath).Length -gt 1048576) { throw 'Oversized release manifest.' }
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    if ($manifest.contract -ne 'ds.windows-server-payload/v1' -or $manifest.lane -ne $Lane -or
        $manifest.architecture -ne 'x86_64' -or $manifest.version -notmatch '^\d+\.\d+\.\d+$') { throw 'Wrong Windows server package identity.' }
    foreach ($repo in @('ds-cli','ds-network','ds-command-kernel','ds-solar','ds-network-reporter')) {
        if ($manifest.source_revisions.$repo -cnotmatch '^[0-9a-f]{40}$') { throw "Missing exact native source: $repo" }
    }
    $expected = @{}
    foreach ($file in $manifest.files) {
        Assert-ServerRelativePath $file.path
        if ($expected.ContainsKey($file.path) -or $file.path -eq 'release.json' -or
            $file.sha256 -cnotmatch '^[0-9a-f]{64}$') { throw 'Invalid manifest inventory.' }
        $expected[$file.path] = $file
    }
    $actual = @(Get-ServerInventory $Root)
    if ($expected.Count -ne $actual.Count) { throw 'Package inventory differs from its manifest.' }
    foreach ($file in $actual) {
        if (-not $expected.ContainsKey($file.path) -or $expected[$file.path].sha256 -cne $file.sha256 -or
            $expected[$file.path].size_bytes -ne $file.size_bytes) { throw "Package bytes differ: $($file.path)" }
    }
    foreach ($required in @('ds.exe','ds-solar.exe','ds-report.exe','ds-client-profiles/catalog.json',
        'ds-cli-skills/receipt.json','start-windows-server.ps1','windows-server-package.ps1')) {
        if (-not $expected.ContainsKey($required)) { throw "Missing package member: $required" }
    }
    return $manifest
}

function Invoke-ServerJson([string]$Executable, [string[]]$Arguments) {
    $answer = & $Executable @Arguments
    if ($LASTEXITCODE) { throw "Native package probe failed: $([IO.Path]::GetFileName($Executable)) $Arguments" }
    return ($answer | Out-String | ConvertFrom-Json)
}

function Test-ServerNativeIdentity([string]$Root, $Manifest) {
    $names = @('DS_NATIVE_CLIENT_PROFILE_BUNDLE','DS_CLI_SKILLS_BUNDLE','DS_REPORT_BIN','DS_SOLAR_BIN')
    $previous = @{}
    foreach ($name in $names) { $previous[$name] = [Environment]::GetEnvironmentVariable($name, 'Process'); [Environment]::SetEnvironmentVariable($name, $null, 'Process') }
    try {
        $cli = Join-Path $Root 'ds.exe'
        $version = Invoke-ServerJson $cli @('version','--output','json')
        $catalog = (Get-FileHash -LiteralPath (Join-Path $Root 'ds-client-profiles/catalog.json')).Hash.ToLowerInvariant()
        if ($version.status -ne 'ok' -or $version.data.dirty -ne $false -or $version.data.profile -ne 'release' -or
            $version.data.target -ne 'x86_64-pc-windows-msvc' -or $version.data.source_sha -cne $Manifest.source_revisions.'ds-cli' -or
            $version.data.command_kernel_source_sha -cne $Manifest.source_revisions.'ds-command-kernel' -or
            $version.data.ds_network_source_sha -cne $Manifest.source_revisions.'ds-network' -or
            $version.data.ds_network_source_state -ne 'release_pin' -or $version.data.native_client_profile_catalog_sha256 -cne $catalog) {
            throw 'Native CLI identity differs from package provenance.'
        }
        $doctor = Invoke-ServerJson $cli @('doctor','--output','json')
        if ($doctor.status -ne 'ok' -or $doctor.data.skills.source_sha -cne $Manifest.source_revisions.'ds-cli' -or
            $doctor.data.skills.status -ne 'ready') { throw 'Packaged skills do not match the CLI.' }
        $native = Invoke-ServerJson $cli @('server','engine','--output','json')
        if ($native.status -ne 'ok' -or $native.data.source_sha -cne $Manifest.source_revisions.'ds-solar' -or
            $native.data.profile -ne 'release') { throw 'Native Solar identity differs.' }
        $solar = Invoke-ServerJson $cli @('solar','engine','--output','json')
        $report = Invoke-ServerJson $cli @('report','engine','--output','json')
        if ($solar.status -ne 'ok' -or $solar.data.source_sha -cne $Manifest.source_revisions.'ds-solar' -or
            $solar.data.path -ne (Join-Path $Root 'ds-solar.exe') -or $report.status -ne 'ok' -or
            $report.data.path -ne (Join-Path $Root 'ds-report.exe') -or
            $report.data.identity.source_sha -cne $Manifest.source_revisions.'ds-network-reporter') { throw 'CLI did not discover its packaged process engines.' }
        foreach ($name in @('ds-solar.exe','ds-report.exe')) {
            $info = Invoke-ServerJson (Join-Path $Root $name) @('build-info')
            if ($info.schema -ne 'ds.engine-build/v1' -or $info.profile -ne 'release' -or
                $info.target -ne 'x86_64-pc-windows-msvc' -or $info.build_manifest_sha256 -cnotmatch '^[0-9a-f]{64}$') {
                throw 'A packaged process engine is not an exact native release.'
            }
        }
        $capability = Invoke-ServerJson $cli @('capabilities','server.serve','--output','json')
        if ($capability.status -ne 'ok' -or $capability.data.command.id -ne 'server.serve' -or
            $capability.data.command.availability -ne 'available') { throw 'Packaged CLI cannot host a native Windows server.' }
        # Probe discovery without starting a host or reading an account.
        return $version.data
    } finally { foreach ($name in $names) { [Environment]::SetEnvironmentVariable($name, $previous[$name], 'Process') } }
}
