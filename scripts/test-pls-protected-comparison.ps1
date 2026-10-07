param(
    [string] $LibraryPath = (Join-Path $PSScriptRoot '../crates/ds-cli-pls/desktop/adapters/pls-cadd/pls-backup-restore-lib.psm1'),
    [string] $ScratchRoot = (Join-Path $PSScriptRoot '../out/protected-comparison-tests')
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
Import-Module $LibraryPath -Force
$dir = Join-Path $ScratchRoot ([guid]::NewGuid().ToString('N'))
[void][IO.Directory]::CreateDirectory($dir)
$cases = New-Object System.Collections.Generic.List[string]
function Make-Inventory([string] $Name, [string] $Root, [string] $Text, [byte] $Binary) {
    $bytes = [Text.Encoding]::GetEncoding(28591).GetBytes($Text)
    $path = Join-Path $dir $Name
    [IO.File]::WriteAllBytes($path, $bytes + @($Binary))
    $hash = [Security.Cryptography.SHA256]::Create()
    try {
        $textDigest = [BitConverter]::ToString($hash.ComputeHash($bytes)).Replace('-','').ToLowerInvariant()
        $binaryDigest = [BitConverter]::ToString($hash.ComputeHash([byte[]]@($Binary))).Replace('-','').ToLowerInvariant()
    } finally { $hash.Dispose() }
    return [ordered]@{
        native_backup_path = $path
        project_source_root = $Root
        digests = @{protected = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash}
        members = @(
            [pscustomobject]@{relative_path='test.don';role='project_core';kind='text';payload_offset=0;bytes=$bytes.Length;sha256=$textDigest},
            [pscustomobject]@{relative_path='structures/test.pol';role='engineering_library';kind='binary';payload_offset=$bytes.Length;bytes=1;sha256=$binaryDigest}
        )
    }
}
function Check([object] $Left, [object] $Right, [bool] $Expected, [string] $Name) {
    $result = Compare-PlsProtectedInventories $Left $Right
    if ($result.equal -ne $Expected) { throw "${Name}: unexpected comparison" }
    $cases.Add($Name)
    return $result
}
$old = Make-Inventory 'old' 'C:\source' "FILENAME='C:\source\test.don'`r`nvalue=17`r`n" 9
$exact = Check $old $old $true 'exact_bytes'
$new = Make-Inventory 'new' 'G:\run\r1' "FILENAME='G:\run\r1\test.don'`nvalue=17`n" 9
$rebase = Check $old $new $true 'only_crlf_and_workspace_relocation'
if ($rebase.raw_digest_equal -or $rebase.members[0].verification -ne 'native_text_crlf_and_path_rebase') { throw 'raw evidence lost' }
$changed = Make-Inventory 'changed' 'G:\run\r1' "FILENAME='G:\run\r1\test.don'`nvalue=18`n" 9
Check $old $changed $false 'engineering_value_change_refused' | Out-Null
$binary = Make-Inventory 'binary' 'G:\run\r1' "FILENAME='G:\run\r1\test.don'`nvalue=17`n" 10
Check $old $binary $false 'binary_change_refused' | Out-Null
$wrong = Make-Inventory 'wrong' 'G:\run\r1' "FILENAME='H:\other\test.don'`nvalue=17`n" 9
Check $old $wrong $false 'unrelated_root_change_refused' | Out-Null
$new.members = @($new.members[0])
Check $old $new $false 'missing_member_refused' | Out-Null
$new.members = @($old.members) + @($old.members[0])
Check $old $new $false 'duplicate_member_refused' | Out-Null
[ordered]@{status='passed';cases=@($cases);scratch=$dir} | ConvertTo-Json -Depth 5