param(
    [Parameter(Mandatory = $true)][string] $ResultPath,
    [Parameter(Mandatory = $true)][string] $FailurePath,
    [Parameter(Mandatory = $true)][string] $RunDirectory
)
. (Join-Path $PSScriptRoot 'ds-desktop-lib.ps1')
Import-Module (Join-Path $here 'pls-stopped-session.psm1') -Force
Invoke-DsEntry $ResultPath 'close' {
    $evidence = Split-Path $FailurePath -Parent
    $journal = Join-Path $evidence 'journal.jsonl'
    if ((Get-Item -LiteralPath $FailurePath).Length -gt 1048576 -or (Get-Item -LiteralPath $journal).Length -gt 4194304) {
        throw 'PLS-CADD attach refused: recovery evidence exceeds the read bound'
    }
    $failure = Get-Content -LiteralPath $FailurePath -Raw | ConvertFrom-Json
    $rows = @(Get-Content -LiteralPath $journal | ForEach-Object { $_ | ConvertFrom-Json })
    $preflight = @($rows | Where-Object { $_.event -ceq 'preflight_complete' })
    $started = @($rows | Where-Object { $_.event -ceq 'pls_started' -and $_.data.process_id -eq $failure.active_process_id })
    $opened = @($rows | Where-Object { $_.event -ceq 'restore_open_quiet' })
    $stable = @($rows | Where-Object { $_.event -ceq 'fresh_backup_stable' })
    if ($preflight.Count -ne 1 -or $started.Count -ne 1 -or $opened.Count -ne 1 -or $stable.Count -ne 1) {
        throw 'PLS-CADD attach refused: recovery needs one completed native ownership chain'
    }
    $process = Get-Process -Id $failure.active_process_id -ErrorAction Stop
    $live = @{ id=$process.Id; handle=[long]$process.MainWindowHandle; path=$process.Path;
        title=$process.MainWindowTitle; started_at_utc=$process.StartTime.ToUniversalTime().ToString('o');
        digest=(Get-FileHash -LiteralPath $process.Path -Algorithm SHA256).Hash.ToLowerInvariant() }
    Assert-PlsStoppedSession $failure $preflight[0] $started[0] $opened[0] $stable[0] $live $PlsExecutable $DsProfile.ExecutableSha256
    # Reuse the qualifier's pinned native helpers and characterized Exit function.
    $tree = Read-DsScriptAst (Join-Path $here 'pls-backup-restore-qualify.ps1')
    $types = @($tree.FindAll({param($n) $n -is [Management.Automation.Language.CommandAst] -and
        $n.GetCommandName() -ceq 'Add-Type' -and $n.Extent.Text.Contains('public static class DsGridBackupRestoreNative')},$true))
    if ($types.Count -ne 1) { throw 'Qualifier native helper declaration is ambiguous' }
    & ([scriptblock]::Create($types[0].Extent.Text))
    foreach ($fn in @($tree.FindAll({param($n) $n -is [Management.Automation.Language.FunctionDefinitionAst]},$false))) {
        . ([scriptblock]::Create($fn.Extent.Text))
    }
    $windows = @(Get-ProcessWindows $process.Id)
    $frame = @($windows | Where-Object { $_.handle -eq $live.handle })
    if ($frame.Count -ne 1 -or -not $frame[0].enabled -or @($windows | Where-Object { $_.class -ceq '#32770' }).Count -ne 0) {
        throw 'PLS-CADD attach refused: the stopped frame is not ready; preserve its native dialogs'
    }
    $script:run = New-DsRunDirectory $RunDirectory @()
    $script:JournalPath = Join-Path $run 'journal.jsonl'
    $profile = $DsProfile
    Write-Journal 'stopped_session_ownership_verified' $live
    Close-PlsWithoutSaving $process $live.handle
    $receipt = Join-Path $run 'receipt.json'
    Write-DsDocument $receipt ([ordered]@{schema='ds.pls.stopped_session_closed.v1'; status='closed_without_saving';
        process_id=$live.id; source_run=(Split-Path $evidence -Parent); source_failure_sha256=(Get-FileHash -LiteralPath $FailurePath).Hash.ToLowerInvariant()})
    $script:DsResult = @{receipt=$receipt}
}
