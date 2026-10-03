# Native archive adversarial tests; no DS account, publish, service or build.
. (Join-Path $PSScriptRoot 'windows-server-package.ps1')
$taskRoot = Join-Path ([IO.Path]::GetTempPath()) ('ds-windows-server-test-' + [guid]::NewGuid().ToString('N'))
$taskRoot = Assert-ServerLocalPath $taskRoot
[IO.Directory]::CreateDirectory($taskRoot) | Out-Null
$script:passed = 0
function Assert-Refused([scriptblock]$Operation) {
    $refused = $false
    try { & $Operation | Out-Null } catch { $refused = $true }
    if (-not $refused) { throw 'An unsafe fixture was accepted.' }
    $script:passed++
}
function New-TestZip([string]$Name, [string[]]$Members) {
    $file = Join-Path $taskRoot $Name
    $zip = [IO.Compression.ZipFile]::Open($file, [IO.Compression.ZipArchiveMode]::Create)
    try { foreach ($member in $Members) { $entry = $zip.CreateEntry($member); $stream = $entry.Open(); try { $stream.WriteByte(65) } finally { $stream.Dispose() } } }
    finally { $zip.Dispose() }
    return $file
}
try {
    foreach ($unsafe in @('../escape.txt','/absolute.txt','C:/absolute.txt','nested\bad.txt',
        'safe/../../escape.txt','NUL.txt','folder/a:stream','folder/trailing.','folder/trailing ')) {
        $zip = New-TestZip ([guid]::NewGuid().ToString('N') + '.zip') @($unsafe)
        $destination = Join-Path $taskRoot ([guid]::NewGuid().ToString('N'))
        Assert-Refused { Expand-ServerArchive $zip $destination }
        if (Test-Path -LiteralPath $destination) { throw 'Unsafe archive exposed files before validation.' }
    }
    $duplicates = New-TestZip 'duplicates.zip' @('Core.txt','core.txt')
    Assert-Refused { Expand-ServerArchive $duplicates (Join-Path $taskRoot 'duplicates') }
    $symlinkZip = Join-Path $taskRoot 'symlink.zip'
    $zip = [IO.Compression.ZipFile]::Open($symlinkZip, [IO.Compression.ZipArchiveMode]::Create)
    $entry = $zip.CreateEntry('link'); $entry.ExternalAttributes = -1577058304
    $zip.Dispose()
    Assert-Refused { Expand-ServerArchive $symlinkZip (Join-Path $taskRoot 'symlink') }
    # A corrupted length must not consume disk beyond the admitted budget.
    $forged = Join-Path $taskRoot 'forged-length.zip'
    $zip = [IO.Compression.ZipFile]::Open($forged, [IO.Compression.ZipArchiveMode]::Create)
    $entry = $zip.CreateEntry('bounded.bin', [IO.Compression.CompressionLevel]::NoCompression)
    $stream = $entry.Open(); $bytes = [byte[]]::new(131072)
    try { $stream.Write($bytes, 0, $bytes.Length) } finally { $stream.Dispose(); $zip.Dispose() }
    $bytes = [IO.File]::ReadAllBytes($forged)
    [Array]::Copy([BitConverter]::GetBytes([uint32]1), 0, $bytes, 22, 4)
    $central = -1
    for ($index = 0; $index -lt $bytes.Length - 4; $index++) {
        if ([BitConverter]::ToUInt32($bytes, $index) -eq 0x02014b50) { $central = $index; break }
    }
    if ($central -lt 0) { throw 'Forged fixture has no central directory.' }
    [Array]::Copy([BitConverter]::GetBytes([uint32]1), 0, $bytes, $central + 24, 4)
    [IO.File]::WriteAllBytes($forged, $bytes)
    $destination = Join-Path $taskRoot 'forged-length'
    Assert-Refused { Expand-ServerArchive $forged $destination }
    if ((Get-Item -LiteralPath (Join-Path $destination 'bounded.bin')).Length -gt 1) { throw 'Malformed archive exceeded its admitted write budget.' }
    $script:passed++

    $safe = New-TestZip 'safe.zip' @('normal/bytes.txt')
    $extracted = Join-Path $taskRoot 'safe'
    Expand-ServerArchive $safe $extracted
    if ((Get-Content -LiteralPath (Join-Path $extracted 'normal/bytes.txt') -Raw) -cne 'A') { throw 'Safe archive bytes changed.' }
    $script:passed++
    Assert-Refused { Expand-ServerArchive $safe $extracted }
    Assert-Refused { & (Join-Path $PSScriptRoot 'install-windows-server.ps1') -Artifact $safe -Sha256 ('0' * 64) -Lane canary -Plan }
    Assert-Refused { Assert-ServerLocalPath '\\server\share\state' }
    $payload = Join-Path $taskRoot 'payload'
    [IO.Directory]::CreateDirectory($payload) | Out-Null
    foreach ($name in @('ds.exe','ds-solar.exe','ds-report.exe','ds-client-profiles/catalog.json',
        'ds-cli-skills/receipt.json','start-windows-server.ps1','windows-server-package.ps1')) {
        $target = Join-Path $payload $name
        [IO.Directory]::CreateDirectory((Split-Path $target -Parent)) | Out-Null
        [IO.File]::WriteAllText($target, 'bounded fixture bytes')
    }
    $sources = @{}
    foreach ($repo in @('ds-cli','ds-network','ds-command-kernel','ds-solar','ds-network-reporter')) { $sources[$repo] = 'a' * 40 }
    $manifest = @{ contract='ds.windows-server-payload/v1'; lane='canary'; version='0.1.0'; architecture='x86_64'; source_revisions=$sources; files=@(Get-ServerInventory $payload) }
    [IO.File]::WriteAllText((Join-Path $payload 'release.json'), ($manifest | ConvertTo-Json -Depth 10))
    $null = Test-ServerPayload $payload canary
    $script:passed++
    Assert-Refused { Test-ServerPayload $payload stable }
    [IO.File]::WriteAllText((Join-Path $payload 'ds.exe'), 'tampered fixture bytes')
    Assert-Refused { Test-ServerPayload $payload canary }
    [IO.File]::WriteAllText((Join-Path $payload 'ds.exe'), 'bounded fixture bytes')
    [IO.File]::WriteAllText((Join-Path $payload 'extra.exe'), 'unowned')
    Assert-Refused { Test-ServerPayload $payload canary }
    Write-Output "$script:passed Windows server package checks passed."
} finally {
    $temporaryParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    if (-not $taskRoot.StartsWith($temporaryParent, [StringComparison]::OrdinalIgnoreCase)) { throw 'Test cleanup escaped temporary root.' }
    Remove-Item -LiteralPath $taskRoot -Recurse -Force
}
