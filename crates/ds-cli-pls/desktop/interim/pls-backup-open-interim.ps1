param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $MainWindowHandle,
    [Parameter(Mandatory = $true)][string] $OutputPath,
    [Parameter(Mandatory = $true)][string] $EvidenceDirectory
)
# INTERIM (2026-09-24) - File > Backup (33347) of the project that is OPEN in a running
# PLS-CADD, with the characterized qualify-driver Invoke-PlsBackup (Backup picker ->
# Set-SafeBackupOptions -> completion box). Never authorizes a save: the project must
# already be saved (a save-before-backup prompt is declined, so the on-disk files are
# what gets backed up). Writes backup-result.json with the .bak digest and native
# inventory. Prove the result with a fresh pls-restore-open-interim.ps1 run.
# Project work lives on the Drive: a C: output is refused.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
. (Join-Path $PSScriptRoot 'pls-interim-loader.ps1')

$out = [System.IO.Path]::GetFullPath($OutputPath)
if ($out -match '^[Cc]:') { throw "Refusing a backup on C: ($out): project work lives on the Drive" }
if ([System.IO.Path]::GetExtension($out) -cne '.bak') { throw "Output must end .bak: $out" }
if (Test-Path -LiteralPath $out) { throw "Output already exists: $out" }
if (-not (Test-Path -LiteralPath $EvidenceDirectory -PathType Container)) { throw "Evidence directory does not exist: $EvidenceDirectory" }
$script:JournalPath = Join-Path $EvidenceDirectory 'backup-journal.jsonl'
$process = Get-Process -Id $ProcessId -ErrorAction Stop
if ($process.Path -cne 'C:\Program Files\PLS\pls_cadd\pls_cadd64.exe') { throw "Unexpected executable: $($process.Path)" }
if ([long] $process.MainWindowHandle -ne $MainWindowHandle) { throw "Main window mismatch: $($process.MainWindowHandle)" }
Write-Journal 'interim_backup_start' ([ordered]@{ process_id = $ProcessId; frame_title = $process.MainWindowTitle; output = $out; driver_sha256 = (Get-PlsFileSha256 $driver) })
$run = Invoke-PlsBackup -Process $process -MainWindowHandle $MainWindowHandle -OutputPath $out -AuthorizeSave $false
# Invoke-PlsBackup returns once the .bak is stable; the completion box ('Backup', "N files
# backed up from M project", OK id 2) can come up just after (v18, 2026-09-24: it then closed
# itself ~30 s later). Wait for it, press its OK like the restore Yes (WM_COMMAND), and refuse
# any other window, so the next command never meets a stale modal.
$late = 'not_seen'
$deadline = [DateTime]::UtcNow.AddSeconds(60)
while ([DateTime]::UtcNow -lt $deadline) {
    $process.Refresh()
    $modals = @(Get-ModalWindows $process $MainWindowHandle)
    if ($modals.Count -eq 0) { if ($late -ne 'not_seen') { break }; Start-Sleep -Milliseconds 500; continue }
    if ($modals.Count -gt 1) { throw "Multiple windows after backup: $(@($modals.title) -join ', ')" }
    $dialog = $modals[0]; $body = Get-DialogBody $dialog.handle
    if ($dialog.title -cne 'Backup' -or $body -notmatch '^\d+ files backed up from \d+ projects?$') {
        throw "Unexpected window after backup: title='$($dialog.title)', body='$body'"
    }
    if ($late -eq 'not_seen') {
        $accept = Get-ExactButtonById $dialog.handle 2 @('OK', '&OK')
        $result = [IntPtr]::Zero
        [DsGridBackupRestoreNative]::SendMessageTimeout([IntPtr] $dialog.handle, 0x0111, [IntPtr] 2, [IntPtr] $accept, 0x0002, 5000, [ref] $result) | Out-Null
        Write-Journal 'backup_completion_prompt_after_return' ([ordered]@{ title = $dialog.title; body = $body; response = 'WM_COMMAND id 2 (OK)' })
        $late = $body
    }
    Start-Sleep -Milliseconds 500
}
if ($late -eq 'not_seen') { Write-Journal 'backup_completion_prompt_not_seen' ([ordered]@{ waited_seconds = 60 }) }
$run.completion_prompt_after_return = $late
$digest = Get-PlsFileSha256 $out
$inventory = Get-PlsNativeBackupInventory $out $null $null
Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'backup-inventory.json') $inventory
$result = [ordered]@{
    schema = 'ds.pls.interim_backup.v1'
    at_utc = [DateTime]::UtcNow.ToString('o')
    backup = [ordered]@{ path = $out; bytes = (Get-Item -LiteralPath $out).Length; sha256 = $digest; project_file = $inventory.project_file; counts = $inventory.counts }
    observed = $run
}
Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'backup-result.json') $result
$result | ConvertTo-Json -Depth 6
