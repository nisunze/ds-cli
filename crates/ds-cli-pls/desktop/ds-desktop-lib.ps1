# ds pls desktop - plumbing shared by every ds-desktop-<verb>.ps1 entry. Dot-source only.
#
# `ds` embeds this folder, extracts it into a private temporary folder for one call, checks
# every file against its pinned sha256, and runs exactly one entry with Windows PowerShell 5.1.
# An entry calls the characterized drivers beside it and writes ONE result document to
# -ResultPath: status 'ok' with the receipt the verb reads, or status 'failed' with the
# driver's own message. `ds` maps that document into a typed result or refusal; it never
# parses console text.
#
# Watch, Title, Save, ExitPls, Count and Verdict are pls-deliver-autosag.ps1's own bodies,
# loaded verbatim from its AST (the interim/pls-interim-loader.ps1 pattern), so a project verb
# watches, saves and exits exactly as the proven deliver chain does. They read $here, $run,
# $script:procId and $script:frame, and call Log, which is defined below.
$ErrorActionPreference = 'Stop'
$here = $PSScriptRoot
# The path every vendored driver pins (interim/pls-backup-open-interim.ps1, pls-close-interim.ps1).
$PlsExecutable = 'C:\Program Files\PLS\pls_cadd\pls_cadd64.exe'
$DsProfile = Import-PowerShellDataFile -LiteralPath (Join-Path $here 'pls-backup-restore-profile.psd1')

# The six deliverable reports of pls-deliver-autosag.ps1 part B, in its order. A hand copy of
# that script's report list; ds's bundle test holds every id and title pattern to it.
$DsDeliverableReports = @(
    @{ k = 'Section Usage'; id = 40015; p = 'Section Usage Report'; all = $false },
    @{ k = 'Structure Usage'; id = 40014; p = 'Structure Usage Report'; all = $false },
    @{ k = 'Terrain Clearances'; id = 40016; p = 'Terrain Clearances by Span'; all = $true },
    @{ k = 'Wind & Weight Span'; id = 40020; p = 'Wind & Weight Span'; all = $false },
    @{ k = 'Summary'; id = 40019; p = 'Summary Report'; all = $false },
    @{ k = 'Sag Tension'; id = 40403; p = 'Sag-Tension Report'; all = $false }
)

$deliverTree = [System.Management.Automation.Language.Parser]::ParseFile((Join-Path $here 'pls-deliver-autosag.ps1'), [ref] $null, [ref] $null)
$deliverFunctions = @($deliverTree.FindAll({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $false))
foreach ($wanted in 'Count', 'Verdict', 'Watch', 'Title', 'Save', 'ExitPls') {
    $found = @($deliverFunctions | Where-Object { $_.Name -ceq $wanted })
    if ($found.Count -ne 1) { throw "pls-deliver-autosag.ps1 must define $wanted exactly once; found $($found.Count)" }
    . ([scriptblock]::Create($found[0].Extent.Text))
}

function Log([string] $m) {
    $line = '{0} {1}' -f [DateTime]::UtcNow.ToString('o'), $m
    Add-Content -LiteralPath (Join-Path $run 'ds-desktop.log') -Value $line -Encoding utf8
}

function Write-DsDocument([string] $Path, [object] $Value) {
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes(($Value | ConvertTo-Json -Depth 12) + "`n")
    $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::CreateNew, [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
}

# Runs one verb body. The body's pipeline output is discarded: what the verb returns is
# $script:DsResult, so a driver that writes to the pipeline cannot corrupt the document.
function Invoke-DsEntry([string] $ResultPath, [string] $Verb, [scriptblock] $Body) {
    $script:DsResult = $null
    try {
        & $Body | Out-Null
        Write-DsDocument $ResultPath ([ordered]@{ schema = 'ds.pls.desktop_entry.v1'; verb = $Verb; status = 'ok'; result = $script:DsResult })
        exit 0
    } catch {
        $failure = $_   # before any pipeline below rebinds $_
        $running = @(Get-Process -Name 'pls_cadd64' -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
        Write-DsDocument $ResultPath ([ordered]@{
            schema = 'ds.pls.desktop_entry.v1'; verb = $Verb; status = 'failed'
            message = $failure.Exception.Message
            script = [System.IO.Path]::GetFileName([string] $failure.InvocationInfo.ScriptName)
            line = $failure.InvocationInfo.ScriptLineNumber
            pls_cadd_running = $running
        })
        exit 1
    }
}

# A fresh run folder on the project Drive, with the named sub-folders. Same rules as
# pls-deliver-autosag.ps1: never C:, never an existing folder.
function New-DsRunDirectory([string] $RunDirectory, [string[]] $Children) {
    $full = [System.IO.Path]::GetFullPath($RunDirectory)
    if ($full -match '^[Cc]:') { throw "Refusing a run directory on C: ($full)" }
    if (Test-Path -LiteralPath $full) { throw "Run directory already exists: $full" }
    New-Item -ItemType Directory -Path (@($full) + @($Children | ForEach-Object { Join-Path $full $_ })) | Out-Null
    $full
}

function Assert-DsPlsNotRunning {
    if (Get-Process pls_cadd64 -ErrorAction SilentlyContinue) { throw 'PLS-CADD is already running; close it first' }
}

function Assert-DsWord {
    if (-not [Type]::GetTypeFromProgID('Word.Application')) {
        throw 'Microsoft Word is not registered (Word.Application): the report PDFs are made from the RTFs with Word'
    }
}

# Launch PLS-CADD on a project's .xyz entry point (pls-launch-project.ps1: pinned executable
# digest, refuses a running PLS-CADD), then let the catalogued watcher settle the startup and
# open-time prompts. The frame must be the PLS-CADD frame and show the project.
function Open-DsProject([string] $ProjectPath) {
    Log "launch $ProjectPath"
    $launch = (& (Join-Path $here 'pls-launch-project.ps1') -ExecutablePath $PlsExecutable -ProjectPath $ProjectPath `
        -ExpectedExecutableSha256 ([string] $DsProfile.ExecutableSha256) | Out-String) | ConvertFrom-Json
    $script:procId = [int] $launch.process_id; $script:frame = [long] $launch.main_window_handle
    if ((Title) -notmatch '^PLS-CADD(?:\s|$)') { throw "Unexpected PLS-CADD main-window title: '$(Title)'" }
    Log "opened pid=$($script:procId) hwnd=$($script:frame)"
    $w0 = Watch 300
    if ($w0.outcome -ne 'ready') { throw "PLS-CADD not ready after open (project): $($w0.outcome)" }
    $leaf = [System.IO.Path]::GetFileName($ProjectPath)
    if ((Title) -notmatch [regex]::Escape($leaf)) { throw "Project $leaf did not open in the PLS-CADD frame: $(Title)" }
    $launch
}

# pls-deliver-autosag.ps1 part B: the six reports as RTF with their verdict lines.
function Invoke-DsReports([string] $ReportDirectory, [int] $TimeoutSeconds) {
    $reports = [ordered]@{}
    foreach ($r in $DsDeliverableReports) {
        $ra = @{ ProcessId = $script:procId; MainWindowHandle = $script:frame; CommandId = $r.id; OutputPath = (Join-Path $ReportDirectory "$($r.k).rtf")
                 ReportTitlePattern = $r.p; JournalPath = (Join-Path $ReportDirectory "journal-$($r.id).jsonl"); TimeoutSeconds = $TimeoutSeconds; Rtf = $true }
        if ($r.all) { $ra.AllFeatureCodes = $true }
        $x = & (Join-Path $here 'pls-report-any.ps1') @ra | ConvertFrom-Json
        $reports[$r.k] = [ordered]@{ rtf = $x.output; bytes = $x.bytes; sha256 = $x.sha256; verdict = (Verdict $x.output) }
        Log "report $($r.k) $($x.bytes) $($reports[$r.k].verdict | ConvertTo-Json -Compress)"
    }
    $reports
}

# pls-deliver-autosag.ps1 part B: cycle the MDI children to the Sheets View, or open one.
function Enter-DsSheetsView {
    for ($i = 0; $i -lt 12 -and (Title) -notmatch '\[Sheets View\]$'; $i++) {
        & (Join-Path $here 'pls-command.ps1') -WindowHandle $script:frame -CommandId 61504 -Post | Out-Null; Start-Sleep -Milliseconds 1500
    }
    if ((Title) -notmatch '\[Sheets View\]$') {
        & (Join-Path $here 'pls-command.ps1') -WindowHandle $script:frame -CommandId 40075 -Post | Out-Null   # Window > New Window > Sheets View
        $w = Watch 300
        if ($w.outcome -ne 'ready' -or (Title) -notmatch '\[Sheets View\]$') { throw "no Sheets View (frame '$(Title)', watch $($w.outcome))" }
    }
}
