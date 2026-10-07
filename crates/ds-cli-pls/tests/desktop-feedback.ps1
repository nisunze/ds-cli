# Headless source/mock checks. No PLS-CADD, Word or user32 call is executed.
# Run with PowerShell on Linux or Windows; native acceptance remains separate.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
$desktop = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../desktop/adapters/pls-cadd'))
$checks = 0
function Assert([bool] $Condition, [string] $Message) {
    if (-not $Condition) { throw $Message }
    $script:checks++
}
function Tree([string] $Name) {
    $errors = $null; $tokens = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile((Join-Path $desktop $Name), [ref]$tokens, [ref]$errors)
    Assert ($errors.Count -eq 0) "$Name does not parse: $errors"
    $ast
}
function Load-Function([object] $Ast, [string] $Name) {
    $node = @($Ast.FindAll({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $n.Name -ceq $Name }, $true))
    Assert ($node.Count -eq 1) "Expected one function $Name"
    # Install in the caller's script scope, preserving the actual function body.
    . ([scriptblock]::Create(($node[0].Extent.Text -replace ('function ' + [regex]::Escape($Name)), ('function script:' + $Name))))
}
foreach ($name in 'ds-desktop-reports.ps1', 'ds-desktop-autosag.ps1', 'ds-desktop-lib.ps1', 'pls-launch-project.ps1', 'pls-dialog-watch.ps1', 'pls-window-classification.psm1') {
    $null = Tree $name
}
# A PowerShell alias outranks a function with the same name. Exercise the
# actual accessor selected by its native call, with cls still bound to Clear-Host.
# Only this pure accessor is loaded; no native driver entry or user32 is called.
Add-Type @'
using System; using System.Text;
public static class DsPw {
    public static int GetClassName(IntPtr h, StringBuilder value, int length) {
        value.Append("Afx:native-class-fixture"); return value.Length;
    }
}
'@
$classTree = Tree 'pls-windows.ps1'
$classAccessors = @($classTree.FindAll({ param($node)
    $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
    $node.Body.Extent.Text.Contains('[DsPw]::GetClassName')
}, $false))
Assert ($classAccessors.Count -eq 1) 'One accessor must read the native window class'
$clsBefore = (Get-Alias cls).Definition
Load-Function $classTree $classAccessors[0].Name
$classValue = & $classAccessors[0].Name ([IntPtr]101)
Assert ($classValue -ceq 'Afx:native-class-fixture') 'Native class reading must work while the builtin cls alias exists'
Assert ((Get-Alias cls).Definition -ceq $clsBefore) 'Native class reading must preserve the existing cls alias'

Import-Module (Join-Path $desktop 'pls-window-classification.psm1') -Force
$fullTitle = 'PLS-CADD - G:\Nyamagabe\M1.xyz - 1 - [Plan View]'
$frameRow = "101 vis=True en=True owner=0 [Afx:frame] '$fullTitle'"
$about = "202 vis=True en=True owner=101 [#32770] 'About PLS-CADD'"
Assert ((Get-PlsMainFrameRow @($about, $frameRow)) -eq $frameRow) 'About must not replace the main frame'
Assert (Test-PlsProjectFrameTitle $fullTitle 'G:\Nyamagabe\M1.xyz') 'Exact full project path should match'
foreach ($title in 'PLS-CADD - M1.xyz - 1', 'PLS-CADD - G:\Other\M1.xyz - 1', 'PLS-CADD - G:\Nyamagabe\M1.xyz.bak - 1') {
    Assert (-not (Test-PlsProjectFrameTitle $title 'G:\Nyamagabe\M1.xyz')) "Unsafe identity accepted: $title"
}
Assert ($null -eq (Get-PlsMainFrameRow @($about))) 'A lone About dialog cannot be a frame'
$ambiguous = $false
try { Get-PlsMainFrameRow @($frameRow, $frameRow.Replace('101 ', '303 ')) | Out-Null } catch { $ambiguous = $true }
Assert $ambiguous 'Two main frames must be refused'

# Test the actual dialog action function against a native-message stand-in.
Add-Type @'
using System;
public static class DsWatch {
    public static int Commands = 0, Id = 1;
    public static long Parent = 202;
    public static bool Responsive = true;
    public static IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result) {
        result = IntPtr.Zero; return Responsive ? new IntPtr(1) : IntPtr.Zero;
    }
    public static int GetDlgCtrlID(IntPtr h) { return Id; }
    public static IntPtr GetParent(IntPtr h) { return new IntPtr(Parent); }
    public static bool IsWindow(IntPtr h) { return true; }
    public static bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l) {
        if (h.ToInt64() != 202 || msg != 0x111 || w.ToInt64() != 1 || l.ToInt64() != 999) throw new Exception("wrong dialog notification");
        Commands++; return true;
    }
}
'@
$watch = Tree 'pls-dialog-watch.ps1'
Load-Function $watch 'ClickButton'
$events = [System.Collections.ArrayList]::new(); $clickedButtons = @{}; $aboutAttempts = @{}; $clicks = 0
function Journal([hashtable] $Event) { [void]$script:events.Add($Event) }
function Click([long] $Handle) { $script:clicks++ }
function Kids([long] $Handle) { 'mock control evidence' }
for ($i = 0; $i -lt 3; $i++) { Assert ((ClickButton 202 999 'about') -eq 'acted') 'About should send its notification' }
Assert ((ClickButton 202 999 'about') -eq 'timeout') 'About retry must be bounded'
Assert ([DsWatch]::Commands -eq 3 -and $clicks -eq 0) 'About must use WM_COMMAND, with no BM_CLICK'
Assert ($events[-1].event -eq 'action_retry_exhausted' -and $events[-1].controls.Count -eq 1) 'Exhaustion needs evidence'
$aboutAttempts = @{}; [DsWatch]::Parent = 404
Assert ((ClickButton 202 999 'about') -eq 'unknown') 'A button owned by another window must be refused'
[DsWatch]::Parent = 202; [DsWatch]::Id = 2
Assert ((ClickButton 202 999 'about') -eq 'unknown') 'Another control must not be notified'
[DsWatch]::Id = 1; [DsWatch]::Responsive = $false
Assert ((ClickButton 202 999 'about') -eq 'acted') 'An unresponsive About is waited out'
Assert ([DsWatch]::Commands -eq 3) 'An unresponsive dialog must not receive queued commands'
Assert ((ClickButton 202 999 'another_catalogued_dialog') -eq 'acted' -and $clicks -eq 1) 'Other catalogued actions retain their existing first click'

$lib = Tree 'ds-desktop-lib.ps1'
$libText = $lib.Extent.Text
Assert ($libText.IndexOf('$w0 = Watch 300') -lt $libText.IndexOf('Unexpected PLS-CADD main-window title')) 'Startup catalogue must settle before frame title check'
$unknownBranch = $watch.Extent.Text.Substring($watch.Extent.Text.IndexOf('if (-not $entry)'), $watch.Extent.Text.IndexOf('switch ($entry.Action)') - $watch.Extent.Text.IndexOf('if (-not $entry)'))
Assert ($unknownBranch.Contains("event = 'unknown_dialog'") -and -not $unknownBranch.Contains('ClickButton')) 'Unknown prompts must remain evidence-only refusals'

# Exercise attachment revalidation with mocked process and frame enumeration.
$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ('ds-pls-mocks-' + [guid]::NewGuid().ToString('N'))
[void][System.IO.Directory]::CreateDirectory($scratch)
try {
    $windowsMock = Join-Path $scratch 'windows.ps1'
    [System.IO.File]::WriteAllText($windowsMock, 'param([int] $ProcessId); $global:DsTestRows')
    Load-Function $lib 'Assert-DsAttachedProject'
    $script:here = $desktop; $script:run = $scratch
    $script:PlsExecutable = 'C:\Program Files\PLS\pls_cadd\pls_cadd64.exe'
    $script:procId = 4242; $script:frame = 101
    $when = [DateTime]::Parse('2026-09-30T00:00:00Z').ToUniversalTime()
    $script:DsAttachedProject = @{ project_path = 'G:\Nyamagabe\M1.xyz'; start_ticks = $when.Ticks }
    $global:DsTestProcesses = @([pscustomobject]@{ Id = 4242; Path = $PlsExecutable; StartTime = $when })
    $global:DsTestRows = @($frameRow); $global:DsTestResponsive = $true
    function Get-Process { param([string] $Name, [string] $ErrorAction); $global:DsTestProcesses }
    function Import-Module { param([string] $Name, [switch] $Force) }
    function Join-Path([string] $Path, [string] $ChildPath) {
        if ($ChildPath -eq 'pls-windows.ps1') { return $script:windowsMock }
        if ($ChildPath -eq 'pls-section-table-autosag.ps1') { return $script:autosagMock }
        if ($ChildPath -eq 'pls-report-any.ps1') { return $script:reportMock }
        [System.IO.Path]::Combine($Path, $ChildPath)
    }
    function Test-PlsFrameResponsive([long] $Handle, [int] $ProcessId) { $global:DsTestResponsive }
    function Log([string] $Message) {}
    Assert-DsAttachedProject
    $checks++
    foreach ($fault in 'other_project', 'ambiguous', 'wrong_pid', 'reused_pid', 'wrong_executable', 'wrong_frame', 'disabled') {
        $global:DsTestRows = @($frameRow); $global:DsTestResponsive = $true
        $global:DsTestProcesses = @([pscustomobject]@{ Id = 4242; Path = $PlsExecutable; StartTime = $when })
        switch ($fault) {
            other_project { $global:DsTestRows = @($frameRow.Replace('G:\Nyamagabe', 'G:\Other')) }
            ambiguous { $global:DsTestProcesses += [pscustomobject]@{ Id = 1234; Path = $PlsExecutable; StartTime = $when } }
            wrong_pid { $global:DsTestProcesses[0].Id = 1234 }
            reused_pid { $global:DsTestProcesses[0].StartTime = $when.AddSeconds(1) }
            wrong_executable { $global:DsTestProcesses[0].Path = 'C:\Other\pls_cadd64.exe' }
            wrong_frame { $global:DsTestRows = @($frameRow.Replace('101 ', '303 ')) }
            disabled { $global:DsTestResponsive = $false }
        }
        $refused = $false
        try { Assert-DsAttachedProject } catch { $refused = $_.Exception.Message.StartsWith('PLS-CADD attach refused:') }
        Assert $refused "Attachment accepted $fault"
    }

    # Launch fails closed on a running PLS-CADD before anything starts; only
    # attachment accepts a running process.
    Load-Function $lib 'Assert-DsPlsNotRunning'
    Load-Function $lib 'Connect-DsProject'
    function Open-DsProject([string] $ProjectPath) { $script:launches++; @{ project_path = $ProjectPath } }
    $launches = 0; $runningRefused = $false
    try { Connect-DsProject 'G:\Nyamagabe\M1.xyz' 0 | Out-Null } catch { $runningRefused = $_.Exception.Message.Contains('already running') }
    Assert ($runningRefused -and $launches -eq 0) 'Launch must refuse a running PLS-CADD before launching'
    $global:DsTestProcesses = @()
    $null = Connect-DsProject 'G:\Nyamagabe\M1.xyz' 0
    Assert ($launches -eq 1) 'Launch proceeds once no PLS-CADD runs'

    # Execute the report entry body itself, with its native work mocked.
    $entry = Tree 'ds-desktop-reports.ps1'
    $invoke = @($entry.FindAll({ param($n) $n -is [System.Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Invoke-DsEntry' }, $true))[0]
    $body = [scriptblock]::Create($invoke.CommandElements[-1].ScriptBlock.Extent.Text.TrimStart('{').TrimEnd('}'))
    function Assert-DsReportConverter { $script:converterChecks++; throw 'No report PDF converter: (mock)' }
    function New-DsRunDirectory { $scratch }
    function Connect-DsProject { $script:connects++; @{ project_path = 'G:\Nyamagabe\M1.xyz'; project_sha256 = 'abc' } }
    function Invoke-DsReports { [ordered]@{ Usage = @{ rtf = 'G:\reports\Usage.rtf'; verdict = @{ section_violations = 3 } } } }
    function ExitPls { $script:exits++ }
    function Assert-DsAttachedProject { $script:rechecks++ }
    function Get-DsSession([int] $AttachProcessId) { @{ project_left_open = ($AttachProcessId -gt 0) } }
    function Write-DsDocument([string] $Path, [object] $Value) { $script:receipt = $Value }
    $ProjectPath = 'G:\Nyamagabe\M1.xyz'; $RunDirectory = $scratch; $ReportTimeoutSeconds = 60
    $PdfPaper = 'A4'; $IncludeWindWeightSpan = $false
    $converterChecks = 0; $connects = 0; $exits = 0; $rechecks = 0; $receipt = $null
    $AttachProcessId = 4242; $RtfOnly = $true
    & $body
    Assert ($converterChecks -eq 0 -and $exits -eq 0 -and $rechecks -eq 1) 'RTF-only attach must skip PDF converter checks and leave the project open'
    Assert ($receipt.reports.Usage.verdict.section_violations -eq 3 -and -not $receipt.reports.Usage.ContainsKey('pdf')) 'RTF-only must preserve verdicts without PDF fields'
    Assert ($receipt.session.project_left_open -and $receipt.report_format -eq 'rtf_only') 'Receipt must state selected format and session disposition'
    Assert (-not $receipt.Contains('pdf_paper') -and -not $receipt.Contains('pdf_orientation')) 'RTF-only receipt must carry no PDF paper fields'
    $AttachProcessId = 0
    & $body
    Assert ($exits -eq 1) 'Launched RTF-only still exits by default'
    $RtfOnly = $false; $connectsBefore = $connects; $converterRefused = $false
    try { & $body } catch { $converterRefused = $_.Exception.Message.Contains('No report PDF converter:') }
    Assert ($converterRefused -and $converterChecks -eq 1 -and $connects -eq $connectsBefore) 'Default PDF path must require a PDF converter before launch'
    $autosag = (Tree 'ds-desktop-autosag.ps1').Extent.Text
    Assert ($autosag.Contains("if (`$AttachProcessId -eq 0) { ExitPls 'project' } else { Assert-DsAttachedProject }")) 'AutoSag attach must leave the project open'
    Assert ($autosag.Contains("Assert-DsAttachedProject`n    Save 'after autosag'")) 'AutoSag must revalidate attachment before saving'
    $autosagMock = [System.IO.Path]::Combine($scratch, 'autosag.ps1')
    $reportMock = [System.IO.Path]::Combine($scratch, 'report.ps1')
    [System.IO.File]::WriteAllText($autosagMock, @'
param([int] $ProcessId, [long] $MainWindowHandle, [string] $EvidenceDirectory)
'{"evidence_directory":"mock","fill":{"id":38079,"text":"Copy && Fill Column"},"watcher_after_ok":{"outcome":"ready"}}'
'@)
    [System.IO.File]::WriteAllText($reportMock, @'
param([int] $ProcessId, [long] $MainWindowHandle, [int] $CommandId, [string] $OutputPath, [string] $ReportTitlePattern, [string] $JournalPath, [int] $TimeoutSeconds)
'{"output":"mock-section-usage.txt"}'
'@)
    $entry = Tree 'ds-desktop-autosag.ps1'
    $invoke = @($entry.FindAll({ param($n) $n -is [System.Management.Automation.Language.CommandAst] -and $n.GetCommandName() -eq 'Invoke-DsEntry' }, $true))[0]
    $body = [scriptblock]::Create($invoke.CommandElements[-1].ScriptBlock.Extent.Text.TrimStart('{').TrimEnd('}'))
    function Save { $script:saves++ }
    function Verdict { @{ section_violations = 0 } }
    $saves = 0; $exits = 0; $rechecks = 0; $AttachProcessId = 4242
    & $body
    Assert ($saves -eq 1 -and $exits -eq 0 -and $rechecks -eq 4) 'Attached AutoSag must revalidate, save once and remain open'
    Assert ($receipt.saved -and $receipt.session.project_left_open -and $receipt.gate_section_usage.verdict.section_violations -eq 0) 'Attached AutoSag must preserve its gate verdict and session disposition'
    $AttachProcessId = 0
    & $body
    Assert ($exits -eq 1 -and -not $receipt.session.project_left_open) 'Launched AutoSag retains its exit lifecycle'
} finally {
    [System.IO.Directory]::Delete($scratch, $true)
    Remove-Variable DsTestRows, DsTestProcesses, DsTestResponsive -Scope Global -ErrorAction SilentlyContinue
}
"$checks source/mock checks passed; no Windows PLS-CADD executed."
