param(
    [Parameter(Mandatory = $true)][string] $BackupPath,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string] $ExpectedBackupSha256,
    [Parameter(Mandatory = $true)][string] $RunDirectory,
    [Parameter(Mandatory = $true)][ValidatePattern('^[A-Za-z0-9._-]+$')][string] $Label,
    [string] $SourceRoot,
    [double] $AlignmentGap = 100,
    [int] $ReportTimeoutSeconds = 1800
)
# Deliverable chain for a DS-exported .bak, unattended, every step a proven driver; any refusal stops it.
#  A. working session (restore r1):
#     1. fresh native Restore + open (interim restore; -SourceRoot for multi-root backups);
#     2. AutoSag every section through the Section Table (pls-section-table-autosag.ps1; never 40337);
#     3. P&P paging settings (pls-sheet-paging.ps1: new sheet per alignment, -AlignmentGap, page starts not
#        rounded), Save (40003);
#     4. gate: Section Usage in this session (did AutoSag take);
#     5. PLS File > Backup of the saved project -> backup\<Label>.bak; Exit (catalogued prompts).
#  B. deliverables from a FRESH restore of that backup (restore r2) - what a reviewer opening the backup sees.
#     A report made in the working session can differ: after the paging gap change PLS re-evaluated one
#     prohibited-zone flag only on reopen (2026-09-25, v19 cap6: 4 flags in session, 3 after restore).
#     6. the six reports saved as RTF (owner: report PDFs are made from the RTF): 40015 Section Usage,
#        40014 Structure Usage, 40016 Terrain Clearances (all feature codes), 40020 Wind & Weight Span,
#        40019 Summary, 40403 Sag Tension; verdict lines read from each;
#     7. every plan & profile sheet to one PDF (Sheets View; pls-save-sheets-pdf.ps1); Exit without saving;
#     8. the RTFs to A3 landscape PDFs (pls-rtf-to-pdf.ps1).
# Everything under -RunDirectory on the Drive (C: refused). Writes deliver.log and deliver.json.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
$run = [System.IO.Path]::GetFullPath($RunDirectory)
if ($run -match '^[Cc]:') { throw "Refusing a run directory on C: ($run)" }
if (Test-Path -LiteralPath $run) { throw "Run directory already exists: $run" }
if (Get-Process pls_cadd64 -ErrorAction SilentlyContinue) { throw 'PLS-CADD is already running; close it first' }
$dirs = 'gate', 'reports', 'pdf', 'backup', 'ev-autosag', 'ev-backup'
New-Item -ItemType Directory -Path (@($run) + @($dirs | ForEach-Object { Join-Path $run $_ })) | Out-Null
function Log([string]$m) { $line = '{0} {1}' -f [DateTime]::UtcNow.ToString('o'), $m; Add-Content -LiteralPath (Join-Path $run 'deliver.log') -Value $line -Encoding utf8; $line }
function Count([string]$path, [string]$pattern) {
    $m = Select-String -LiteralPath $path -Pattern $pattern | Select-Object -First 1
    if ($m) { [int]$m.Matches[0].Groups[1].Value } else { $null }
}
function Verdict([string]$path) {
    [ordered]@{
        section_violations    = Count $path '(\d+) section violations'
        structure_violations  = Count $path '(\d+) structure violations'
        structure_warnings    = Count $path '(\d+) structure warnings'
        clearance_spans_ng    = Count $path '(\d+) spans with clearance violations'
        clearance_spans_ok    = Count $path '(\d+) spans without clearance violations'
    }
}
function Watch([int]$seconds) { & (Join-Path $here 'pls-dialog-watch.ps1') -ProcessId $script:procId -MainWindowHandle $script:frame -TimeoutSeconds $seconds -JournalPath (Join-Path $run 'watch-journal.jsonl') | ConvertFrom-Json }
function Title { (Get-Process -Id $script:procId).MainWindowTitle }
function Save([string]$what) {
    & (Join-Path $here 'pls-command.ps1') -WindowHandle $script:frame -CommandId 40003 -Post | Out-Null
    Start-Sleep -Seconds 8
    $w = Watch 120
    if ($w.outcome -ne 'ready') { throw "save ($what) did not return to ready: $($w.outcome)" }
    Log "saved ($what)"
}
function Open([string]$bak, [string]$sha, [string]$tag, [string]$root) {
    Log "restore ($tag) $bak"
    $a = @{ CandidateBackupPath = $bak; ExpectedCandidateBackupSha256 = $sha
            RestoreDirectory = (Join-Path $run $tag); EvidenceDirectory = (Join-Path $run "ev-$tag"); Execute = $true }
    if ($root) { $a.SourceRoot = $root }
    & (Join-Path $here 'interim\pls-restore-open-interim.ps1') @a | Out-Null
    $o = Get-Content -LiteralPath (Join-Path $run "ev-$tag\restore-open.json") -Raw | ConvertFrom-Json
    $script:procId = [int] $o.restore.process_id; $script:frame = [long] $o.restore.main_window_handle
    Log "opened ($tag) pid=$($script:procId) hwnd=$($script:frame) files=$($o.pre_open_presence.verified_files)"
    # late open-time boxes (sheet paging Warning/Problem) can arrive after the restore driver returns
    $w0 = Watch 60
    if ($w0.outcome -ne 'ready') { throw "PLS-CADD not ready after open ($tag): $($w0.outcome)" }
    $o
}
# Exit (57665). PLS asks 'Save changes to <project>?' even right after a save (seen 2026-09-24 and
# 2026-09-25); the catalogued watcher answers No (save_changes, id 7): in session A the backup already
# holds the saved state, in session B nothing may be saved. The interim close driver refuses that prompt.
function ExitPls([string]$tag) {
    & (Join-Path $here 'pls-command.ps1') -WindowHandle $script:frame -CommandId 57665 -Post | Out-Null
    $events = @()
    for ($i = 0; $i -lt 60 -and (Get-Process -Id $script:procId -ErrorAction SilentlyContinue); $i++) {
        Start-Sleep -Seconds 1
        if (-not (Get-Process -Id $script:procId -ErrorAction SilentlyContinue)) { break }
        try { $w = & (Join-Path $here 'pls-dialog-watch.ps1') -ProcessId $script:procId -MainWindowHandle $script:frame -TimeoutSeconds 3 -Once -JournalPath (Join-Path $run 'watch-journal.jsonl') | ConvertFrom-Json }
        catch { continue }   # the process can vanish while the watcher reads its windows
        foreach ($e in @($w.events)) { $events += "$($e.event) $($e.dialog)" }
        if ($w.outcome -in @('unknown', 'stop', 'flow')) { throw "dialog at exit ($tag, $($w.outcome)): $($events -join '; ')" }
    }
    if (Get-Process -Id $script:procId -ErrorAction SilentlyContinue) { throw "PLS-CADD did not exit within 60 s ($tag)" }
    Log "closed ($tag) via Exit ($($events -join '; '))"
}

# ================= A. working session
$o = Open $BackupPath $ExpectedBackupSha256 'r1' $SourceRoot
$autosag = (& (Join-Path $here 'pls-section-table-autosag.ps1') -ProcessId $procId -MainWindowHandle $frame -EvidenceDirectory (Join-Path $run 'ev-autosag') | Out-String) | ConvertFrom-Json
Log "autosag done: fill=$($autosag.fill.text) attempts-journal=ev-autosag watcher=$($autosag.watcher_after_ok.outcome)"
$paging = & (Join-Path $here 'pls-sheet-paging.ps1') -ProcessId $procId -MainWindowHandle $frame -AlignmentGap $AlignmentGap -JournalPath (Join-Path $run 'watch-journal.jsonl') | ConvertFrom-Json
Log "paging $($paging.before | ConvertTo-Json -Compress) -> $($paging.after | ConvertTo-Json -Compress)"
Save 'after autosag and paging'
$g = & (Join-Path $here 'pls-report-any.ps1') -ProcessId $procId -MainWindowHandle $frame -CommandId 40015 -OutputPath (Join-Path $run 'gate\section-usage-after-autosag.txt') `
    -ReportTitlePattern 'Section Usage Report' -JournalPath (Join-Path $run 'gate\journal-40015.jsonl') -TimeoutSeconds $ReportTimeoutSeconds | ConvertFrom-Json
$gate = Verdict $g.output
Log "gate: section violations after AutoSag = $($gate.section_violations)"
Save 'before backup'
$bakOut = Join-Path $run "backup\$Label.bak"
& (Join-Path $here 'interim\pls-backup-open-interim.ps1') -ProcessId $procId -MainWindowHandle $frame -OutputPath $bakOut -EvidenceDirectory (Join-Path $run 'ev-backup') | Out-Null
$bakSha = (Get-FileHash -LiteralPath $bakOut -Algorithm SHA256).Hash.ToLowerInvariant()
Log "backup $bakSha $((Get-Item -LiteralPath $bakOut).Length) bytes"
ExitPls 'r1'

# ================= B. deliverables from a fresh restore of the delivered backup
$o2 = Open $bakOut $bakSha 'r2' $null
$reports = [ordered]@{}
foreach ($r in @(@{ k = 'Section Usage'; id = 40015; p = 'Section Usage Report'; all = $false },
                 @{ k = 'Structure Usage'; id = 40014; p = 'Structure Usage Report'; all = $false },
                 @{ k = 'Terrain Clearances'; id = 40016; p = 'Terrain Clearances by Span'; all = $true },
                 @{ k = 'Wind & Weight Span'; id = 40020; p = 'Wind & Weight Span'; all = $false },
                 @{ k = 'Summary'; id = 40019; p = 'Summary Report'; all = $false },
                 @{ k = 'Sag Tension'; id = 40403; p = 'Sag-Tension Report'; all = $false })) {
    $ra = @{ ProcessId = $procId; MainWindowHandle = $frame; CommandId = $r.id; OutputPath = (Join-Path $run "reports\$($r.k).rtf")
             ReportTitlePattern = $r.p; JournalPath = (Join-Path $run "reports\journal-$($r.id).jsonl"); TimeoutSeconds = $ReportTimeoutSeconds; Rtf = $true }
    if ($r.all) { $ra.AllFeatureCodes = $true }
    $x = & (Join-Path $here 'pls-report-any.ps1') @ra | ConvertFrom-Json
    $reports[$r.k] = [ordered]@{ rtf = $x.output; bytes = $x.bytes; sha256 = $x.sha256; verdict = (Verdict $x.output) }
    Log "report $($r.k) $($x.bytes) $($reports[$r.k].verdict | ConvertTo-Json -Compress)"
}
for ($i = 0; $i -lt 12 -and (Title) -notmatch '\[Sheets View\]$'; $i++) {
    & (Join-Path $here 'pls-command.ps1') -WindowHandle $frame -CommandId 61504 -Post | Out-Null; Start-Sleep -Milliseconds 1500
}
if ((Title) -notmatch '\[Sheets View\]$') {
    & (Join-Path $here 'pls-command.ps1') -WindowHandle $frame -CommandId 40075 -Post | Out-Null   # Window > New Window > Sheets View
    $w = Watch 300
    if ($w.outcome -ne 'ready' -or (Title) -notmatch '\[Sheets View\]$') { throw "no Sheets View (frame '$(Title)', watch $($w.outcome))" }
}
$sheets = & (Join-Path $here 'pls-save-sheets-pdf.ps1') -ProcessId $procId -MainWindowHandle $frame -OutputPdf (Join-Path $run 'pdf\Plan and Profile.pdf') -JournalPath (Join-Path $run 'watch-journal.jsonl') | ConvertFrom-Json
Log "sheets $($sheets.bytes) bytes in $($sheets.seconds) s"
ExitPls 'r2'

# ---- report PDFs
$pdfs = & (Join-Path $here 'pls-rtf-to-pdf.ps1') -A3Landscape -RtfPath @($reports.Values | ForEach-Object { $_.rtf }) | ConvertFrom-Json
foreach ($p in @($pdfs)) { $k = [System.IO.Path]::GetFileNameWithoutExtension($p.pdf); $reports[$k].pdf = $p.pdf; $reports[$k].pdf_bytes = $p.bytes }
Log "report pdfs $(@($pdfs).Count)"

$result = [ordered]@{
    schema = 'ds.pls.deliver_autosag.v3'; label = $Label; source = [ordered]@{ path = $BackupPath; sha256 = $ExpectedBackupSha256 }
    working_session = [ordered]@{ restored_files = $o.pre_open_presence.verified_files
        autosag = [ordered]@{ evidence_directory = $autosag.evidence_directory; fill = $autosag.fill; watcher = $autosag.watcher_after_ok.outcome }
        paging = $paging.after; gate_section_usage = $gate }
    backup = [ordered]@{ path = $bakOut; sha256 = $bakSha; bytes = (Get-Item -LiteralPath $bakOut).Length }
    deliverables_from_fresh_restore = [ordered]@{ restored_files = $o2.pre_open_presence.verified_files; reports = $reports
        sheets = [ordered]@{ pdf = $sheets.pdf; bytes = $sheets.bytes } }
}
[System.IO.File]::WriteAllText((Join-Path $run 'deliver.json'), ($result | ConvertTo-Json -Depth 8), [System.Text.UTF8Encoding]::new($false))
Log 'DONE'
$result | ConvertTo-Json -Depth 8
