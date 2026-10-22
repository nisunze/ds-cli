$ErrorActionPreference='Stop'
Set-StrictMode -Version 2.0
Import-Module (Join-Path (Split-Path $PSScriptRoot -Parent) 'pls-stopped-session.psm1') -Force
$failure=@{schema='ds.pls.backup_restore_failure.v1';process_left_for_operator=$true;
    message='Fresh PLS backup changed protected candidate content: pole';active_process_id=12;failed_at_utc='2026-10-11T09:15:00Z'}
$preflight=@{at_utc='2026-10-11T09:14:25Z';data=@{executable='pinned.exe';executable_sha256='abc';save_before_backup_authorized=$false}}
$started=@{at_utc='2026-10-11T09:14:28Z';data=@{process_id=12;main_window_handle=34}}
$opened=@{data=@{project='School.xyz'}}
$stable=@{data=@{completion_prompt_observed=$true}}
$live=@{id=12;handle=34;path='pinned.exe';digest='abc';started_at_utc='2026-10-11T09:14:27Z';title='PLS-CADD - School.xyz'}
$passed=0
Assert-PlsStoppedSession $failure $preflight $started $opened $stable $live 'pinned.exe' 'abc'
$passed++
function Assert-Rejected([scriptblock]$Action) {
    try { & $Action } catch { return }
    throw 'Unsafe stopped-session recovery was admitted'
}
foreach ($case in 'pid','handle','executable','reuse','project','native_stop','uncompleted','save_authorized') {
    $f=$failure.Clone();$l=$live.Clone();$p=$preflight.Clone();$p.data=$preflight.data.Clone();$b=$stable.Clone();$b.data=$stable.data.Clone()
    switch($case) {
        'pid' { $l.id=13 }
        'handle' { $l.handle=99 }
        'executable' { $l.digest='different' }
        'reuse' { $l.started_at_utc='2026-10-11T10:00:00Z' }
        'project' { $l.title='PLS-CADD - another.xyz' }
        'native_stop' { $f.message='Unexpected backup dialog' }
        'uncompleted' { $b.data.completion_prompt_observed=$false }
        'save_authorized' { $p.data.save_before_backup_authorized=$true }
    }
    Assert-Rejected { Assert-PlsStoppedSession $f $p $started $opened $b $l 'pinned.exe' 'abc' }
    $passed++
}
@{status='passed';ownership_cases=$passed}|ConvertTo-Json
