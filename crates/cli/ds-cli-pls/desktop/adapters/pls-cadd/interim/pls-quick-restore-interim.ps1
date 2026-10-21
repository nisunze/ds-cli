param(
    [Parameter(Mandatory = $true)][string] $CandidateBackupPath,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string] $ExpectedCandidateBackupSha256,
    [Parameter(Mandatory = $true)][string] $RestoreDirectory,
    [Parameter(Mandatory = $true)][string] $EvidenceDirectory,
    [string] $SourceRoot,
    [string] $ExecutablePath = 'C:\Program Files\PLS\pls_cadd\pls_cadd64.exe',
    [int[]] $IgnoreStuckPid = @(),
    [switch] $Execute
)
# INTERIM (2026-09-29) - native PLS-CADD **Quick Restore** of one backup into one fresh folder (owner direction:
# "use quick restore and choose that target folder"). Quick Restore (mapping-dialog control 1741) flattens every
# member into the chosen folder, so the check is: restored file count = backup file members and every member leaf
# present in the folder. Answers Yes to "open project" and records every prompt the open raises (e.g. "Bad number of
# phases on DE structure" fails the run). PLS windows are found by the NEW process id only. PLS-CADD projects move
# only through a .bak (owner rule): this script never copies project folders.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'pls-interim-loader.ps1')

$script:JournalPath = $null
if (-not $Execute) { throw 'Refusing to launch PLS-CADD without -Execute' }
$executable = Assert-PlsRegularFile $ExecutablePath 'PLS-CADD executable'
$candidate = Assert-PlsRegularFile $CandidateBackupPath 'candidate backup'
$candidateDigest = Get-PlsFileSha256 $candidate
if ($candidateDigest -cne $ExpectedCandidateBackupSha256.ToLowerInvariant()) { throw "Candidate digest mismatch: $candidateDigest" }
Assert-FreshAbsolutePath $RestoreDirectory 'restore directory' $true
Assert-FreshAbsolutePath $EvidenceDirectory 'evidence directory' $true
$running = @(Get-Process -Name 'pls_cadd64' -ErrorAction SilentlyContinue)
foreach ($stuck in $IgnoreStuckPid) {
    $p = $running | Where-Object { $_.Id -eq $stuck }
    if (-not $p) { continue }
    $threads = @($p.Threads)
    if ($threads.Count -ne 1 -or [string] $threads[0].WaitReason -ne 'Executive') {
        throw "PID $stuck is not a stuck, already-killed PLS-CADD; refusing to ignore it"
    }
    $running = @($running | Where-Object { $_.Id -ne $stuck })
}
if ($running.Count -ne 0) { throw "Refusing to run while PLS-CADD is already running (PID(s): $($running.Id -join ', '))" }

[System.IO.Directory]::CreateDirectory($EvidenceDirectory) | Out-Null
$script:JournalPath = Join-Path $EvidenceDirectory 'journal.jsonl'
[System.IO.File]::Open($script:JournalPath, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read).Dispose()
$payload = Resolve-PlsNativeBackupPayload $candidate $candidateDigest (Join-Path $EvidenceDirectory 'candidate-payload')
$inventory = Get-PlsNativeBackupInventory $payload.native_path $null $SourceRoot
Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'candidate-inventory.json') $inventory
$members = @($inventory.members | Where-Object { $_.kind -ne 'directory' })
$expectedFiles = [int] $inventory.counts.files
Write-Journal 'quick_restore_preflight' ([ordered]@{ candidate = $candidate; sha256 = $candidateDigest; files = $expectedFiles; target = $RestoreDirectory })

$prompts = New-Object System.Collections.ArrayList
$process = Start-PlsBare $executable ([int] $profile.StartupTimeoutSeconds)
$main = [long] $process.MainWindowHandle
Dismiss-KnownStartupPrompts $process $main $prompts
Send-PlsCommand $main ([int] $profile.Commands.Restore)
$fileDialog = Wait-ForSingleDialog $process $main @([string] $profile.DialogTitles.RestoreFile) ([int] $profile.DialogTimeoutSeconds)
Set-CommonFileDialogPath $fileDialog ([string] $profile.DialogTitles.RestoreFile) $payload.native_path @('Open', '&Open', 'OK', '&OK')

$quickClicked = $false; $destinationSelected = $false; $restoreReport = $null; $opened = $false
$deadline = [DateTime]::UtcNow.AddSeconds([int] $profile.BackupTimeoutSeconds)
$quietSince = $null
while ([DateTime]::UtcNow -lt $deadline) {
    $process.Refresh()
    if ($process.HasExited) { throw "PLS-CADD exited during quick restore with code $($process.ExitCode)" }
    $windows = @(Get-ModalWindows $process $main)
    if ($windows.Count -eq 0) {
        if ($null -ne $restoreReport) {
            if ($null -eq $quietSince) { $quietSince = [DateTime]::UtcNow }
            if (([DateTime]::UtcNow - $quietSince).TotalSeconds -ge 5) { break }
        }
        Start-Sleep -Milliseconds 200; continue
    }
    $quietSince = $null
    $dialog = $windows[0]
    $body = Get-DialogBody $dialog.handle
    Write-Journal 'quick_restore_dialog' ([ordered]@{ title = $dialog.title; class = $dialog.class; body = $body })
    if ($dialog.handle -eq $fileDialog.handle) { Start-Sleep -Milliseconds 100; continue }
    if ($dialog.title -ceq $profile.DialogTitles.RestoreMapping) {
        if (-not $quickClicked) {
            $quick = Get-ExactButtonById $dialog.handle 1741 @('Quick Restore', '&Quick Restore', 'Quick &Restore')
            Invoke-Button $quick; $quickClicked = $true
            Write-Journal 'quick_restore_clicked' ([ordered]@{ control = 1741 })
            continue
        }
        if ($destinationSelected) {
            # After Quick Restore the mapping dialog can reappear empty while it closes: accept only a live OK.
            $accept = Find-ExactButtonByText $dialog.handle @('OK', '&OK') -AllowMissing
            if ($accept -eq 0 -or $body.Length -eq 0) { Start-Sleep -Milliseconds 150; continue }
            Invoke-Button $accept; Write-Journal 'quick_restore_mapping_accepted' ([ordered]@{}); continue
        }
        Start-Sleep -Milliseconds 100; continue
    }
    if ($dialog.title -ceq 'Select Directory To Restore Files In') {
        # Quick Restore's modern folder picker: the folder must exist to be chosen, so create it empty first.
        # After Select Folder the same picker stays visible briefly while it closes: wait, don't re-select.
        if ($destinationSelected) { Start-Sleep -Milliseconds 150; continue }
        [System.IO.Directory]::CreateDirectory($RestoreDirectory) | Out-Null
        # The folder picker's "Folder:" field is Edit 1152 (Magese 2026-09-29); type as WM_CHAR, read back, Select Folder (1).
        $edit = @((Get-DialogChildren $dialog.handle) | Where-Object { $_.id -eq 1152 -and $_.class -ceq 'Edit' -and $_.visible -and $_.enabled })
        if ($edit.Count -ne 1) { throw "Folder picker must expose one enabled Edit 1152; found $($edit.Count)" }
        $eh = [IntPtr] ([long] $edit[0].handle)
        [DsGridBackupRestoreNative]::SendMessage($eh, 0x00B1, [IntPtr] 0, [IntPtr] (-1)) | Out-Null
        foreach ($ch in [int[]][char[]]$RestoreDirectory) { [DsGridBackupRestoreNative]::SendMessage($eh, 0x0102, [IntPtr] $ch, [IntPtr] 1) | Out-Null }
        Start-Sleep -Milliseconds 250
        $sb = New-Object System.Text.StringBuilder 32768
        [DsGridBackupRestoreNative]::SendMessage($eh, 0x000D, [IntPtr] $sb.Capacity, $sb) | Out-Null
        if (-not $sb.ToString().Equals($RestoreDirectory, [System.StringComparison]::OrdinalIgnoreCase)) { throw "Folder path readback failed: '$($sb.ToString())'" }
        Invoke-Button (Get-ExactButtonById $dialog.handle 1 @('Select Folder', '&Select Folder'))
        $destinationSelected = $true
        Write-Journal 'quick_restore_destination_selected' ([ordered]@{ target = $RestoreDirectory })
        continue
    }
    if ($dialog.title -ceq $profile.DialogTitles.RestoreDirectory) {
        if ($destinationSelected) { throw 'Quick restore asked for its destination twice' }
        Select-FreshRestoreDirectory $dialog $RestoreDirectory
        $destinationSelected = $true
        Write-Journal 'quick_restore_destination_selected' ([ordered]@{ target = $RestoreDirectory })
        continue
    }
    if ($dialog.title -ceq $profile.DialogTitles.RestoreFile -and ($body.Length -eq 0 -or $body -like 'Scanning*' -or $body -like 'Restoring*')) {
        Start-Sleep -Milliseconds 100; continue
    }
    if ($body -match '(\d+) files restored, (\d+) files skipped') {
        $restoreReport = $body
        $yes = @((Get-DialogChildren $dialog.handle) | Where-Object { $_.id -eq 6 -and $_.class -ceq 'Button' -and $_.visible -and $_.enabled -and $_.title -ceq '&Yes' })
        if ($yes.Count -ne 1) { throw "Restore report without one &Yes button: '$body'" }
        $sent = [IntPtr]::Zero
        [DsGridBackupRestoreNative]::SendMessageTimeout([IntPtr] $dialog.handle, 0x0111, [IntPtr] 6,
            [IntPtr] ([long] $yes[0].handle), 0x0002, 5000, [ref] $sent) | Out-Null
        $opened = $true
        Write-Journal 'quick_restore_report' ([ordered]@{ body = $body; open = 'yes' })
        continue
    }
    if ($dialog.title -ceq 'PLS-CADD' -and $body -like 'Created * sub-directories of *to avoid file name conflicts*') {
        # Quick Restore notice (Magese 2026-09-29): same-name members from different backup paths get sub-folders.
        $ok = Find-ExactButtonByText $dialog.handle @('OK', '&OK') -AllowMissing
        if ($ok -eq 0) { throw 'Sub-directory notice without an OK button' }
        Invoke-Button $ok; Write-Journal 'quick_restore_subdirectory_notice' ([ordered]@{ body = $body }); continue
    }
    if ($body -ceq 'Restore complete') {
        # Progress state of the Restore Backup dialog (no button); the report/open prompt follows.
        Start-Sleep -Milliseconds 150; continue
    }
    if (Invoke-KnownOpenPrompt $dialog $prompts) { continue }
    throw "Unexpected dialog during quick restore/open: title='$($dialog.title)', body='$body'"
}
if ($null -eq $restoreReport) { throw 'Quick restore did not report completion before the timeout' }

$files = @(Get-ChildItem -LiteralPath $RestoreDirectory -Recurse -File)
$names = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::OrdinalIgnoreCase)
foreach ($f in $files) { $names.Add($f.Name) | Out-Null }
$missing = @($members | Where-Object { -not $names.Contains([System.IO.Path]::GetFileName([string] $_.relative_path)) } | ForEach-Object { $_.relative_path })
$result = [ordered]@{
    schema = 'ds.pls.interim_quick_restore.v1'
    status = $(if ($files.Count -ge $expectedFiles -and $missing.Count -eq 0) { 'restored_all_members' } else { 'incomplete' })
    candidate_sha256 = $candidateDigest; target = $RestoreDirectory; restore_report = $restoreReport
    expected_files = $expectedFiles; restored_files = $files.Count; missing_members = $missing
    opened = $opened; frame_title = $process.MainWindowTitle; prompts = @($prompts); process_id = $process.Id
}
Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'quick-restore-result.json') $result
$result | ConvertTo-Json -Depth 6
if ($result.status -ne 'restored_all_members') { throw "Quick restore incomplete: $($files.Count)/$expectedFiles files, missing $($missing.Count)" }
