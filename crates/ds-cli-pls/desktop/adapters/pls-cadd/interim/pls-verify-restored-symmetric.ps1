param(
    [Parameter(Mandatory = $true)][string] $EvidenceDirectory,
    [Parameter(Mandatory = $true)][string] $RestoreDirectory
)
# INTERIM (2026-09-23) - C3g. Verify every restored member against the exact bytes in the
# candidate's native payload. Binary members must be byte-exact; text members are equal
# when BOTH sides are normalized the same way (CRLF -> LF, restore root -> source root).
# The lib's Test-PlsRestoredTree normalizes only the restored side, so a member that was
# already CRLF in the backup (DS-authored shell members such as <project>.brk) is
# reported as a length mismatch although it is unchanged.
# Inputs come from pls-restore-open-interim.ps1: EvidenceDirectory holds
# candidate-inventory.json and candidate-payload\candidate-native.bak (or the inventory's
# native_backup_path).
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$enc = [System.Text.Encoding]::GetEncoding(28591)
$inventory = Get-Content -Raw -LiteralPath (Join-Path $EvidenceDirectory 'candidate-inventory.json') | ConvertFrom-Json
$nativePath = Join-Path $EvidenceDirectory 'candidate-payload\candidate-native.bak'
if (-not (Test-Path -LiteralPath $nativePath)) { $nativePath = [string] $inventory.native_backup_path }
$native = [System.IO.File]::ReadAllBytes($nativePath)
$root = (Get-Item -LiteralPath $RestoreDirectory).FullName.TrimEnd('\')
$sourceRoot = [string] $inventory.project_source_root
$sha = [System.Security.Cryptography.SHA256]::Create()
function Hex([byte[]] $b) { ([System.BitConverter]::ToString($sha.ComputeHash($b))).Replace('-', '').ToLowerInvariant() }
$counts = [ordered]@{ exact = 0; text_normalized = 0; mismatch = 0; missing = 0 }
$findings = New-Object System.Collections.ArrayList
foreach ($member in @($inventory.members | Where-Object { $_.kind -ne 'directory' })) {
    $path = Join-Path $root ([string] $member.relative_path)
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { $counts.missing++; [void]$findings.Add("missing $($member.relative_path)"); continue }
    $head = $enc.GetString($native, [int] $member.record_offset, [Math]::Min(4096, $native.Length - [int] $member.record_offset))
    $cursor = 0
    for ($line = 0; $line -lt 4; $line++) { $cursor = $head.IndexOf("`n", $cursor) + 1 }
    $original = New-Object byte[] ([int] $member.bytes)
    [System.Array]::Copy($native, [int] $member.record_offset + $cursor, $original, 0, [int] $member.bytes)
    if ((Hex $original) -cne [string] $member.sha256) { throw "payload extraction mismatch for $($member.relative_path)" }
    $restored = [System.IO.File]::ReadAllBytes($path)
    if ((Hex $restored) -ceq [string] $member.sha256) { $counts.exact++; continue }
    if ($member.kind -eq 'text') {
        $a = $enc.GetString($original).Replace("`r`n", "`n")
        $b = $enc.GetString($restored).Replace("`r`n", "`n").Replace($root, $sourceRoot)
        if ($a -ceq $b) { $counts.text_normalized++; continue }
    }
    $counts.mismatch++
    [void]$findings.Add("mismatch $($member.relative_path) ($($member.kind)) expected=$($member.bytes) restored=$($restored.Length)")
}
[ordered]@{
    schema = 'ds.pls.interim_symmetric_tree_verification.v1'
    restore_directory = $root
    source_root = $sourceRoot
    native_payload = $nativePath
    counts = $counts
    findings = @($findings)
    status = if ($counts.mismatch -eq 0 -and $counts.missing -eq 0) { 'verified' } else { 'failed' }
} | ConvertTo-Json -Depth 4
