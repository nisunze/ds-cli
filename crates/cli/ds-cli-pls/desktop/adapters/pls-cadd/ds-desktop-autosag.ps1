param(
    [Parameter(Mandatory = $true)][string] $ResultPath,
    [Parameter(Mandatory = $true)][string] $ProjectPath,
    [Parameter(Mandatory = $true)][string] $RunDirectory,
    [int] $ReportTimeoutSeconds = 1800,
    [int] $AttachProcessId = 0
)
# ds pls desktop autosag - AutoSag every section of a saved project in place, as step A of the
# deliver chain does it: open, pls-section-table-autosag.ps1 (never 40337), Save (40003), the
# Section Usage gate (40015) that proves AutoSag took, Exit (the catalogued watcher answers the
# 'Save changes' prompt No: the AutoSag state is already saved). Evidence in the run folder.
. (Join-Path $PSScriptRoot 'ds-desktop-lib.ps1')
Invoke-DsEntry $ResultPath 'autosag' {
    $script:run = New-DsRunDirectory $RunDirectory @('gate', 'ev-autosag')
    $launch = Connect-DsProject $ProjectPath $AttachProcessId
    Assert-DsAttachedProject
    $autosag = (& (Join-Path $here 'pls-section-table-autosag.ps1') -ProcessId $script:procId -MainWindowHandle $script:frame `
        -EvidenceDirectory (Join-Path $run 'ev-autosag') | Out-String) | ConvertFrom-Json
    Log "autosag done: fill=$($autosag.fill.text) watcher=$($autosag.watcher_after_ok.outcome)"
    Assert-DsAttachedProject
    Save 'after autosag'
    Assert-DsAttachedProject
    $g = & (Join-Path $here 'pls-report-any.ps1') -ProcessId $script:procId -MainWindowHandle $script:frame -CommandId 40015 `
        -OutputPath (Join-Path $run 'gate\section-usage-after-autosag.txt') -ReportTitlePattern 'Section Usage Report' `
        -JournalPath (Join-Path $run 'gate\journal-40015.jsonl') -TimeoutSeconds $ReportTimeoutSeconds | ConvertFrom-Json
    $gate = Verdict $g.output
    Log "gate: section violations after AutoSag = $($gate.section_violations)"
    if ($AttachProcessId -eq 0) { ExitPls 'project' } else { Assert-DsAttachedProject }
    $receipt = Join-Path $run 'autosag.json'
    Write-DsDocument $receipt ([ordered]@{
        schema = 'ds.pls.desktop_autosag.v1'
        project = [ordered]@{ path = $launch.project_path; sha256_before = $launch.project_sha256 }
        autosag = [ordered]@{ evidence_directory = $autosag.evidence_directory; fill = $autosag.fill; watcher = $autosag.watcher_after_ok.outcome }
        saved = $true
        session = Get-DsSession $AttachProcessId
        gate_section_usage = [ordered]@{ report = $g.output; verdict = $gate }
    })
    $script:DsResult = [ordered]@{ receipt = $receipt }
}
