$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

Import-Module (Join-Path (Split-Path -Parent $PSScriptRoot) 'pls-backup-restore-lib.psm1') -Force
Import-Module (Join-Path (Split-Path -Parent $PSScriptRoot) 'pls-window-classification.psm1') -Force

$script:passed = 0

function Assert-True([bool] $Condition, [string] $Message) {
    if (-not $Condition) { throw "ASSERTION FAILED: $Message" }
    $script:passed++
}

function Assert-Throws([scriptblock] $Action, [string] $Pattern) {
    try {
        & $Action
    } catch {
        if ($_.Exception.Message -notmatch $Pattern) {
            throw "ASSERTION FAILED: expected error /$Pattern/, got '$($_.Exception.Message)'"
        }
        $script:passed++
        return
    }
    throw "ASSERTION FAILED: expected error /$Pattern/, but action succeeded"
}

function Write-AsciiLine([System.IO.Stream] $Stream, [string] $Value) {
    $bytes = [System.Text.Encoding]::ASCII.GetBytes($Value + "`n")
    $Stream.Write($bytes, 0, $bytes.Length)
}

function Write-NativeBackup([string] $Path, [object[]] $Members) {
    $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try {
        foreach ($member in $Members) {
            $leaf = ($member.path.Replace('/', '\').Split('\'))[-1]
            $content = [byte[]] @($member.content)
            $contentLength = @($content).Count
            Write-AsciiLine $stream "TYPE='***PLSBACKUPFILE***' VERSION='3.1' UNITS='SI' SOURCE='PLS-CADD Version 16.81' USER='contract-test' FILENAME='$leaf'"
            Write-AsciiLine $stream $member.path
            Write-AsciiLine $stream ('{0,12} {1} 1' -f $contentLength, $member.kind)
            Write-AsciiLine $stream '2026 8 12 0 0 0'
            if ($contentLength -gt 0) {
                $stream.Write($content, 0, $contentLength)
            }
        }
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
}

function Bytes([string] $Value) {
    return [System.Text.UTF8Encoding]::new($false).GetBytes($Value)
}

$testRoot = Join-Path ([System.IO.Path]::GetTempPath()) ('ds-pls-backup-contract-' + [Guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($testRoot) | Out-Null
try {
    $bareFrame = "100 vis=True en=True owner=0 [AfxFrame] 'PLS-CADD'"
    $sameTitleModal = "200 vis=True en=True owner=100 [#32770] 'PLS-CADD'"
    $bareSplit = Split-PlsWindowRows @($bareFrame, $sameTitleModal) 100
    Assert-True ($bareSplit.frame -ceq $bareFrame -and
        $bareSplit.others.Count -eq 1 -and $bareSplit.others[0] -ceq $sameTitleModal) `
        'bare PLS-CADD frame is selected by handle while same-title dialog remains a modal'
    $openFrame = "100 vis=True en=True owner=0 [AfxFrame] 'PLS-CADD - model.xyz'"
    Assert-True ((Split-PlsWindowRows @($openFrame, $sameTitleModal) 100).frame -ceq $openFrame) `
        'opened project frame remains selected by the same handle'
    Assert-Throws { Split-PlsWindowRows @($sameTitleModal) 100 | Out-Null } 'Expected one PLS-CADD main frame'

    $native = Join-Path $testRoot 'candidate.bak'
    $members = @(
        @{ path = 'C:\candidate'; kind = 'directory'; content = [byte[]] @() },
        @{ path = 'C:\candidate\structures'; kind = 'directory'; content = [byte[]] @() },
        @{ path = 'C:\candidate\cables'; kind = 'directory'; content = [byte[]] @() },
        @{ path = 'C:\candidate\Asbuilt.xyz'; kind = 'text'; content = Bytes "TYPE='XYZ FILE' VERSION='1'`r`n" },
        @{ path = 'C:\candidate\Asbuilt.don'; kind = 'text'; content = Bytes "TYPE='PLS-CADD DESIGN FILE' VERSION='1681'`r`n" },
        @{ path = 'C:\candidate\Asbuilt.num'; kind = 'text'; content = Bytes "TYPE='NUM FILE' VERSION='1'`r`n" },
        @{ path = 'C:\candidate\Asbuilt.cri'; kind = 'text'; content = Bytes "TYPE='CRI FILE' VERSION='94'`r`n" },
        @{ path = 'C:\candidate\structures\pole.012'; kind = 'text'; content = Bytes "TYPE='PLS-POLE INPUT FILE' VERSION='22'`r`n" },
        @{ path = 'C:\candidate\cables\wire'; kind = 'binary'; content = [byte[]] @(0, 1, 2, 10, 13, 255) },
        @{ path = 'C:\candidate\notes.txt'; kind = 'text'; content = Bytes "ancillary`r`nC:\candidate\notes.txt`r`n" }
    )
    Write-NativeBackup $native $members
    $nativeHash = Get-PlsFileSha256 $native
    $directPayload = Resolve-PlsNativeBackupPayload $native $nativeHash (Join-Path $testRoot 'unused-direct-extract')
    Assert-True ($directPayload.container_format -ceq 'native_plsbackupfile_v3_1') 'direct native format detected'

    $inventory = Get-PlsNativeBackupInventory $native 'Asbuilt.xyz'
    Assert-True ($inventory.project_file -ceq 'Asbuilt.xyz') 'project relative path derived'
    Assert-True (@($inventory.members | Where-Object {
        $_.kind -eq 'directory' -and $_.relative_path -ceq '.'
    }).Count -eq 1) 'explicit native common-root directory is retained without aliasing a file'
    Assert-True ($inventory.counts.project_core -eq 4) 'project core count includes XYZ/DON/NUM/CRI'
    Assert-True ($inventory.counts.engineering_library -eq 1) 'PLS-POLE library recognized'
    Assert-True ($inventory.counts.ancillary -eq 2) 'binary cable without typed header and notes remain ancillary'
    Assert-True ($inventory.digests.protected -match '^[0-9a-f]{64}$') 'protected digest emitted'
    Assert-True (@($inventory.members | Where-Object {
        $_.kind -eq 'text' -and [long] $_.payload_offset -gt [long] $_.record_offset
    }).Count -ge 1) 'inventory pins the original text payload offset for symmetric verification'

    $restore = Join-Path $testRoot 'restore'
    [System.IO.Directory]::CreateDirectory($restore) | Out-Null
    foreach ($member in $inventory.members) {
        $path = Join-Path $restore $member.relative_path
        if ($member.kind -eq 'directory') {
            [System.IO.Directory]::CreateDirectory($path) | Out-Null
            continue
        }
        $parent = [System.IO.Path]::GetDirectoryName($path)
        [System.IO.Directory]::CreateDirectory($parent) | Out-Null
        $source = @($members | Where-Object { $_.path -ceq $member.original_path })[0]
        [System.IO.File]::WriteAllBytes($path, $source.content)
    }
    $tree = Test-PlsRestoredTree $inventory $restore 'Full'
    Assert-True ($tree.verified_files -eq 7) 'full restored tree verified'
    $lockedProject = [System.IO.File]::Open((Join-Path $restore 'Asbuilt.xyz'),
        [System.IO.FileMode]::Open, [System.IO.FileAccess]::ReadWrite,
        [System.IO.FileShare]::None)
    try {
        Assert-Throws { Test-PlsRestoredTree $inventory $restore 'Full' | Out-Null } '(?i)(being used|access|cannot|process)'
    } finally {
        $lockedProject.Dispose()
    }
    Assert-True ((Test-PlsRestoredTree $inventory $restore 'Full').verified_files -eq 7) `
        'a locked project member is verified after the native close releases it'
    $textPath = Join-Path $restore 'notes.txt'
    [System.IO.File]::WriteAllBytes($textPath, (Bytes "ancillary`r`n$restore\notes.txt`r`n"))
    $nativeTextTree = Test-PlsRestoredTree $inventory $restore 'Full'
    Assert-True (@($nativeTextTree.verified_members | Where-Object {
        $_.relative_path -ceq 'notes.txt' -and
        $_.verification -ceq 'native_text_crlf_and_path_rebase'
    }).Count -eq 1) 'a CRLF source and rebased restored text compare after symmetric normalization'
    [System.IO.File]::WriteAllBytes($textPath, (Bytes "changed`r`n"))
    $textPresence = Test-PlsRestoredTree $inventory $restore 'Full' @() -PresenceOnly
    Assert-True ($textPresence.verified_files -eq 7 -and $null -eq $textPresence.verified_digest) `
        'pre-open presence check accepts native text rewrites without claiming content equality'
    Assert-Throws { Test-PlsRestoredTree $inventory $restore 'Full' | Out-Null } 'Restored member'
    [System.IO.File]::WriteAllBytes($textPath, (Bytes "ancillary`nC:\candidate\notes.txt`n"))
    $binaryPath = Join-Path $restore 'cables\wire'
    $binaryOriginal = [System.IO.File]::ReadAllBytes($binaryPath)
    [System.IO.File]::WriteAllBytes($binaryPath, [byte[]] @(0, 1, 2, 13, 10, 13, 255))
    $binaryPresence = Test-PlsRestoredTree $inventory $restore 'Full' @() -PresenceOnly
    Assert-True ($binaryPresence.verified_files -eq 7) `
        'pre-open presence check does not mistake a path count for binary integrity'
    Assert-Throws { Test-PlsRestoredTree $inventory $restore 'Full' | Out-Null } 'Restored member'
    [System.IO.File]::WriteAllBytes($binaryPath, $binaryOriginal)
    [System.IO.File]::WriteAllText((Join-Path $restore 'unexpected.txt'), 'x')
    Assert-Throws { Test-PlsRestoredTree $inventory $restore 'Full' @() -PresenceOnly | Out-Null } 'Unexpected file'
    Assert-Throws { Test-PlsRestoredTree $inventory $restore 'Full' | Out-Null } 'Unexpected file'
    [System.IO.File]::Delete((Join-Path $restore 'unexpected.txt'))

    $multiNative = Join-Path $testRoot 'multi-root.bak'
    $multiMembers = @(
        @{ path = 'C:\common\A\Asbuilt.xyz'; kind = 'text'; content = Bytes "TYPE='XYZ FILE' VERSION='1'`n" },
        @{ path = 'C:\common\A\Asbuilt.don'; kind = 'text'; content = Bytes "TYPE='PLS-CADD DESIGN FILE' VERSION='1681'`n" },
        @{ path = 'C:\common\A\Asbuilt.num'; kind = 'text'; content = Bytes "TYPE='NUM FILE' VERSION='1'`n" },
        @{ path = 'C:\common\A\Asbuilt.cri'; kind = 'text'; content = Bytes "TYPE='CRI FILE' VERSION='94'`n" },
        @{ path = 'C:\common\B\structures\pole.012'; kind = 'text'; content = Bytes "TYPE='PLS-POLE INPUT FILE' VERSION='22'`n" }
    )
    Write-NativeBackup $multiNative $multiMembers
    Assert-Throws { Get-PlsNativeBackupInventory $multiNative 'Asbuilt.xyz' | Out-Null } 'outside project root'
    Assert-Throws { Get-PlsNativeBackupInventory $multiNative 'Asbuilt.xyz' 'C:\other' | Out-Null } 'outside project root'
    $multiInventory = Get-PlsNativeBackupInventory $multiNative 'Asbuilt.xyz' 'C:\common'
    Assert-True ($multiInventory.project_file -ceq 'A\Asbuilt.xyz' -and
        $multiInventory.project_source_root -ceq 'C:\common' -and
        $multiInventory.counts.files -eq 5) `
        'an explicit common native source root maps every selected member without guessing'
    $multiRestore = Join-Path $testRoot 'multi-restore'
    [System.IO.Directory]::CreateDirectory($multiRestore) | Out-Null
    foreach ($member in $multiInventory.members) {
        $path = Join-Path $multiRestore $member.relative_path
        $parent = [System.IO.Path]::GetDirectoryName($path)
        [System.IO.Directory]::CreateDirectory($parent) | Out-Null
        $source = @($multiMembers | Where-Object { $_.path -ceq $member.original_path })[0]
        [System.IO.File]::WriteAllBytes($path, $source.content)
    }
    Assert-True ((Test-PlsRestoredTree $multiInventory $multiRestore 'Full').verified_files -eq 5) `
        'implied intermediate folders in a multi-root restore are accepted without admitting extra files'

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = Join-Path $testRoot 'candidate-wrapped.bak'
    $archive = [System.IO.Compression.ZipFile]::Open($zip, [System.IO.Compression.ZipArchiveMode]::Create)
    try {
        $entry = $archive.CreateEntry('payload.bak', [System.IO.Compression.CompressionLevel]::Optimal)
        $input = [System.IO.File]::OpenRead($native)
        $output = $entry.Open()
        try { $input.CopyTo($output) } finally { $output.Dispose(); $input.Dispose() }
    } finally {
        $archive.Dispose()
    }
    $zipPayload = Resolve-PlsNativeBackupPayload $zip (Get-PlsFileSha256 $zip) (Join-Path $testRoot 'zip-extract')
    Assert-True ($zipPayload.container_format -like 'single_member_zip*') 'single-member ZIP wrapper detected'
    Assert-True ($zipPayload.native_sha256 -ceq $nativeHash) 'ZIP extraction preserves native payload digest'

    $badDigestExtract = Join-Path $testRoot 'bad-digest-extract'
    Assert-Throws { Resolve-PlsNativeBackupPayload $native ('0' * 64) $badDigestExtract | Out-Null } 'digest mismatch'

    $traversal = Join-Path $testRoot 'traversal.bak'
    Write-NativeBackup $traversal @(
        @{ path = 'C:\candidate\Asbuilt.xyz'; kind = 'text'; content = Bytes 'x' },
        @{ path = 'C:\candidate\Asbuilt.don'; kind = 'text'; content = Bytes 'x' },
        @{ path = 'C:\candidate\Asbuilt.num'; kind = 'text'; content = Bytes 'x' },
        @{ path = 'C:\candidate\structures\pole.012'; kind = 'text'; content = Bytes 'x' },
        @{ path = 'C:\candidate\..\escape.txt'; kind = 'text'; content = Bytes 'x' }
    )
    Assert-Throws { Get-PlsNativeBackupInventory $traversal 'Asbuilt.xyz' | Out-Null } 'traversal component'

    $duplicate = Join-Path $testRoot 'duplicate.bak'
    Write-NativeBackup $duplicate @(
        @{ path = 'C:\candidate\Asbuilt.xyz'; kind = 'text'; content = Bytes 'x' },
        @{ path = 'C:\candidate\Asbuilt.don'; kind = 'text'; content = Bytes 'x' },
        @{ path = 'C:\candidate\Asbuilt.num'; kind = 'text'; content = Bytes 'x' },
        @{ path = 'C:\candidate\structures\pole.012'; kind = 'text'; content = Bytes 'x' },
        @{ path = 'C:\candidate\notes.txt'; kind = 'text'; content = Bytes 'one' },
        @{ path = 'C:\candidate\NOTES.TXT'; kind = 'text'; content = Bytes 'two' }
    )
    Assert-Throws { Get-PlsNativeBackupInventory $duplicate 'Asbuilt.xyz' | Out-Null } 'Duplicate case-insensitive'

    $profile = Import-PowerShellDataFile (Join-Path (Split-Path -Parent $PSScriptRoot) 'pls-backup-restore-profile.psd1')
    Assert-True (@($profile.DangerousBackupOptionTextPatterns | Where-Object {
        'Compress backup file' -match $_
    }).Count -eq 1) 'native round-trip profile forces compressed backup output off'
    Assert-True ('PLS-CADD Project Repair Wizard for ''Asbuilt.xyz''' -match $profile.RepairTitlePattern) `
        'anchored repair-wizard title accepts the characterized project suffix'
    Assert-True ('Not PLS-CADD Project Repair Wizard' -notmatch $profile.RepairTitlePattern) `
        'repair-wizard title allowlist remains anchored'
    Assert-True (Test-PlsExecutableVersion 'Version 16.81' '16.81') `
        'Windows file metadata Version prefix is normalized'
    Assert-True (Test-PlsExecutableVersion '16.81.0.0' '16.81') `
        'ordinary numeric file version remains accepted'
    Assert-True (-not (Test-PlsExecutableVersion 'Build 16.81' '16.81')) `
        'arbitrary version prefixes remain refused'
    Assert-True (-not (Test-PlsExecutableVersion 'Version 20.01' '16.81')) `
        'a different product version remains refused'
    $qualifierText = Get-Content -LiteralPath `
        (Join-Path (Split-Path -Parent $PSScriptRoot) 'pls-backup-restore-qualify.ps1') -Raw
    Assert-True (([regex]::Matches($qualifierText, '\[AllowEmptyCollection\(\)\]')).Count -eq 4) `
        'empty prompt and repair evidence lists are valid before the first observed dialog'
    Assert-True (@($profile.AllowedStartupPrompts | Where-Object {
        $_.Title -ceq 'About PLS-CADD' -and
        'PLS-CADD Version 16.81x64 Licensed to: C G' -match $_.BodyPattern -and
        $_.ResponseControlId -eq 1 -and
        $_.ResponseTexts -ccontains 'OK'
    }).Count -eq 1) 'exact 16.81 About dialog is a governed startup prompt'
    Assert-True (@($profile.AllowedStartupPrompts | Where-Object {
        $_.Title -ceq 'Tip of the Day' -and
        'Holding shift allows panning. Did you know?' -match $_.BodyPattern -and
        $_.ResponseControlId -eq 1 -and
        $_.ResponseTexts -ccontains 'Close'
    }).Count -eq 1) 'exact Tip of the Day dialog is a governed startup prompt'
    Assert-True (@($profile.AllowedOpenPrompts | Where-Object {
        $_.Name -ceq 'undefined_feature_codes' -and
        '39 Undefined feature codes found in terrain. Program doesn''t know what these points are or what their required clearances are. 7088 XYZ points with unknown feature codes. 0 PFL points with unknown feature codes. Continue displaying warning messages (click No to redirect this and future messages to a report window for remainder of this operation)?' -match $_.BodyPattern -and
        $_.ResponseControlId -eq 7
    }).Count -eq 1) 'the exact Rutsiro undefined-feature warning is governed without suppressing unknown variants'
    Assert-True ([int] $profile.Controls.CommonFileName -eq 1148) `
        'the characterized PLS restore picker filename Edit retains control id 1148'
    Assert-True ($qualifierText -match '\$editHandle, 0x000C' -and
        $qualifierText -match '\$editHandle, 0x000D') `
        'restore path entry writes and rereads the exact filename Edit control'
    Assert-True ($profile.DialogTitles.RestoreMapping -ceq 'Directory Mapping For Restore') `
        'restore mapping is pinned to the observed 16.81 dialog title'
    Assert-True ($profile.ExactButtons.RestorePickDirectory -ccontains 'Change Common Directory Path') `
        'portable workspace restore remaps the common root without flattening typed subdirectories'
    Assert-True ($qualifierText -match '\$commonDialogRequested = \$true' -and
        $qualifierText -match '\$destinationPickerRequested = \$true' -and
        $qualifierText -match 'Set-RestoreCommonPath') `
        'the nested common-path and destination-picker dialogs have independent one-shot state'
    Assert-True ($qualifierText.Contains("GetFullPath((Join-Path `$RestoreDirectory `$relative))") -and
        $qualifierText.Contains("Where-Object { `$_.kind -ne 'directory' }")) `
        'restore progress is bounded to unique file leaves and excludes directory records'
    Assert-True ($profile.ExactButtons.RestoreSelectDirectory -ccontains 'Select Folder') `
        'destination picker uses the exact observed Select Folder control'
    Assert-True ($qualifierText -match "SendWait\('\^l'\)" -and
        $qualifierText -match 'SendWait\(\$Directory\)' -and
        $qualifierText -match 'Address: \$Directory' -and
        $qualifierText -match "class -ceq 'ToolbarWindow32'" -and
        $qualifierText -notmatch '0x0466' -and
        $qualifierText -notmatch 'RestoreFolderPath') `
        'restore-directory selection navigates the shell address bar and authenticates its actual current folder'
    Assert-True ($qualifierText -match 'PostMessage\(\[IntPtr\] \$Handle, 0x00F5') `
        'validated button clicks are posted so nested modal dialogs remain observable'
    Assert-True ($qualifierText -match '\$body\.Length -eq 0' -and
        $qualifierText -match '\$body -ceq \$expectedScanBody') `
        'empty or exact-filename Restore Backup parsing progress is observed but never clicked'
    Assert-True ($qualifierText.Contains('$body -cmatch ''^Restoring "([^"]+)"$''') -and
        $qualifierText.Contains("`$display = [System.IO.Path]::GetFullPath((Join-Path `$RestoreDirectory `$relative))") -and
        $qualifierText -match 'HashSet\[string\]\]\:\:new\(\s*\[System\.StringComparer\]\:\:OrdinalIgnoreCase\)' -and
        $qualifierText -match '\$restoreMemberDisplays\.Contains\(\$restoringLeaf\)') `
        'restore member progress is no-action only for an exact candidate-inventory display path'
    Assert-True ($qualifierText -match 'Restore Backup of \$nativeBackupLeaf' -and
        $qualifierText.Contains('$expectedCompletionBody = "$expectedRestoredFileCount files restored, 0 files skipped Would you like to open project ''$expectedProjectPath''?"') -and
        $qualifierText -match '\$body -ceq \$expectedCompletionBody' -and
        $qualifierText -notmatch 'OpenRestoredProjectBodyPattern') `
        'restore completion title, counts, skipped count, and project are exact inventory-derived values'
    Assert-True ($qualifierText -match 'Restored project requires repair and is not eligible' -and
        $qualifierText -notmatch 'repair_continue_without_finding') `
        'a final restore requiring Project Repair is refused rather than normalized'
    $presencePosition = $qualifierText.IndexOf("Test-PlsRestoredTree `$Inventory `$RestoreDirectory 'Full' @(`$profile.AllowedFreshRestoreExtras) -PresenceOnly")
    $yesPosition = $qualifierText.IndexOf('Invoke-VerifiedRestoreOpen $dialog $expectedCompletionTitle $expectedCompletionBody')
    $firstClosePosition = $qualifierText.IndexOf('Close-PlsWithoutSaving $activeProcess $first.main_window_handle')
    $firstFullPosition = $qualifierText.IndexOf("`$firstFull = Test-PlsRestoredTree `$candidateInventory `$FirstRestoreDirectory 'Full'")
    $secondClosePosition = $qualifierText.IndexOf('Close-PlsWithoutSaving $activeProcess $second.main_window_handle')
    $secondFullPosition = $qualifierText.IndexOf("`$secondFull = Test-PlsRestoredTree `$freshInventory `$SecondRestoreDirectory 'Full'")
    Assert-True ($presencePosition -ge 0 -and $yesPosition -gt $presencePosition -and
        $firstFullPosition -gt $firstClosePosition -and $firstClosePosition -gt $yesPosition -and
        $secondFullPosition -gt $secondClosePosition -and $secondClosePosition -gt $firstFullPosition -and
        $qualifierText -notmatch '\$postOpenVerification = Test-PlsRestoredTree') `
        'the restore decision checks presence; full content verification follows each native close'
    Assert-True ($qualifierText -match 'SendMessageTimeout\(\[IntPtr\] \$Dialog\.handle' -and
        $qualifierText -match '0x0111, \[IntPtr\] 6, \$yesHandle' -and
        $qualifierText -match '\$yes\.Count -ne 1' -and
        $qualifierText -match 'GetDlgItem\(\[IntPtr\] \$Dialog\.handle, 6\)' -and
        $qualifierText -match 'MainWindowTitle\.Contains\(\$expectedProjectLeaf\)') `
        'the exact Restore Yes control uses synchronous IDYES and waits for the project frame title'
    $closeStart = $qualifierText.IndexOf('function Close-PlsWithoutSaving')
    $closeEnd = $qualifierText.IndexOf('function Assert-FreshAbsolutePath', $closeStart)
    $closeText = $qualifierText.Substring($closeStart, $closeEnd - $closeStart)
    Assert-True ($closeText -match '\$frame\.Count -eq 0' -and
        $closeText -match '\$topLevel \| Where-Object \{' -and
        $closeText -match '\$_\.enabled -or \$_\.class -ceq' -and
        $closeText -match 'Unexpected PLS-CADD window while exiting' -and
        $closeText -match 'if \(-not \$frame\[0\]\.enabled\)' -and
        $closeText -match '\$windows = @\(\$topLevel \| Where-Object \{' -and
        $closeText -notmatch 'Get-ModalWindows \$Process \$MainWindowHandle') `
        'exit checks modals from one frame snapshot and waits through its destruction'

    [ordered]@{
        schema = 'ds.pls.backup_restore_contract_tests.v1'
        passed = $script:passed
        status = 'passed'
    } | ConvertTo-Json
} finally {
    if ($testRoot.StartsWith([System.IO.Path]::GetTempPath(), [System.StringComparison]::OrdinalIgnoreCase) -and
        (Test-Path -LiteralPath $testRoot -PathType Container)) {
        [System.IO.Directory]::Delete($testRoot, $true)
    }
}
