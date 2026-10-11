Set-StrictMode -Version 2.0
function Assert-PlsStoppedSession {
    param($Failure, $Preflight, $Started, $Opened, $Stable, $Live, [string] $Executable, [string] $Digest)
    $prefix = 'PLS-CADD attach refused: stopped-session recovery: '
    if ($Failure.schema -cne 'ds.pls.backup_restore_failure.v1' -or
        -not $Failure.process_left_for_operator -or
        -not ([string]$Failure.message).StartsWith('Fresh PLS backup changed protected candidate content:')) {
        throw ($prefix + 'the stop was not an offline protected-content mismatch')
    }
    if (-not $Preflight -or -not $Started -or -not $Opened -or -not $Stable -or
        -not $Stable.data.completion_prompt_observed -or $Preflight.data.save_before_backup_authorized) {
        throw ($prefix + 'completed no-save Restore/Backup evidence is missing')
    }
    if ($Preflight.data.executable -ine $Executable -or $Preflight.data.executable_sha256 -ine $Digest -or
        $Live.path -ine $Executable -or $Live.digest -ine $Digest) {
        throw ($prefix + 'the native executable is not the pinned authority')
    }
    if ([int]$Failure.active_process_id -le 0 -or $Failure.active_process_id -ne $Started.data.process_id -or
        $Live.id -ne $Failure.active_process_id -or [long]$Live.handle -eq 0 -or
        $Live.handle -ne $Started.data.main_window_handle) {
        throw ($prefix + 'PID or frame ownership differs from the stopped run')
    }
    $start = [datetime]$Live.started_at_utc
    if ($start -lt [datetime]$Preflight.at_utc -or $start -gt [datetime]$Started.at_utc -or
        [datetime]$Started.at_utc -gt [datetime]$Failure.failed_at_utc) {
        throw ($prefix + 'the PID start time is outside the recorded launch interval')
    }
    $leaf = [IO.Path]::GetFileName([string]$Opened.data.project)
    if ([string]::IsNullOrWhiteSpace($leaf) -or $Live.title.IndexOf($leaf,[StringComparison]::OrdinalIgnoreCase) -lt 0) {
        throw ($prefix + 'the owned frame no longer names the restored project')
    }
}
Export-ModuleMember -Function Assert-PlsStoppedSession
