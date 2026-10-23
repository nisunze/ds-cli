param(
    [Parameter(Mandatory = $true)]
    [int] $ProcessId,
    [Parameter(Mandatory = $true)]
    [long] $MainWindowHandle,
    [Parameter(Mandatory = $true)]
    [ValidateSet('structure_usage', 'wind_weight_span', 'summary', 'section_usage', 'section_tension')]
    [string] $Report,
    [Parameter(Mandatory = $true)]
    [ValidateSet('Start', 'AcceptOptions', 'SaveAs')]
    [string] $Action,
    [Parameter(Mandatory = $true)]
    [string] $EvidenceDirectory,
    [string] $OutputPath,
    [string] $ProfilePath = (Join-Path $PSScriptRoot 'pls-report-profile.psd1')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class DsGridReportRun {
    public delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr window, StringBuilder text, int count);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr window, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool IsWindowEnabled(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wParam, string lParam);

    [DllImport("user32.dll")]
    public static extern IntPtr SendMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern IntPtr GetDlgItem(IntPtr dialog, int controlId);
}
"@

function Get-Title([IntPtr] $Handle) {
    $text = New-Object System.Text.StringBuilder 1024
    [DsGridReportRun]::GetWindowText($Handle, $text, $text.Capacity) | Out-Null
    return $text.ToString()
}

function Get-Class([IntPtr] $Handle) {
    $text = New-Object System.Text.StringBuilder 256
    [DsGridReportRun]::GetClassName($Handle, $text, $text.Capacity) | Out-Null
    return $text.ToString()
}

function Get-TopWindows {
    $windows = New-Object System.Collections.ArrayList
    $callback = [DsGridReportRun+EnumWindowsProc] {
        param([IntPtr] $handle, [IntPtr] $parameter)
        $owner = 0
        [DsGridReportRun]::GetWindowThreadProcessId($handle, [ref] $owner) | Out-Null
        if ($owner -eq $ProcessId -and [DsGridReportRun]::IsWindowVisible($handle)) {
            $windows.Add($handle) | Out-Null
        }
        return $true
    }
    [DsGridReportRun]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
    return @($windows)
}

function Get-UniqueWindow([string] $ExactTitle) {
    $matches = @(Get-TopWindows | Where-Object { (Get-Title $_) -ceq $ExactTitle })
    if ($matches.Count -ne 1) {
        throw "Expected one '$ExactTitle' window, found $($matches.Count)"
    }
    return $matches[0]
}

function Assert-NoBlockingModal {
    if (-not [DsGridReportRun]::IsWindowEnabled([IntPtr] $MainWindowHandle)) {
        $tops = @(Get-TopWindows | Where-Object { [long] $_ -ne $MainWindowHandle } |
            ForEach-Object { Get-Title $_ })
        throw "PLS-CADD main window is disabled by another window; handle it explicitly: $($tops -join '; ')"
    }
}

function Write-Action([object] $Record) {
    if (-not (Test-Path -LiteralPath $EvidenceDirectory -PathType Container)) {
        throw "Evidence directory does not exist: $EvidenceDirectory"
    }
    $Record['timestamp_utc'] = [DateTime]::UtcNow.ToString('o')
    $Record['process_id'] = $ProcessId
    $Record['main_window_handle'] = $MainWindowHandle
    $json = $Record | ConvertTo-Json -Compress -Depth 10
    $path = Join-Path $EvidenceDirectory 'actions.jsonl'
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($json + "`n")
    $stream = [System.IO.File]::Open($path, [System.IO.FileMode]::Append,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read)
    try {
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
}

$profile = Import-PowerShellDataFile -LiteralPath $ProfilePath
$definition = @($profile.Reports | Where-Object { $_.Key -eq $Report })
if ($definition.Count -ne 1) { throw "Profile does not define report '$Report' exactly once" }
$definition = $definition[0]
$process = Get-Process -Id $ProcessId
if ([long] $process.MainWindowHandle -ne $MainWindowHandle) {
    throw "Main window handle does not belong to PID $ProcessId"
}

if ($Action -eq 'Start') {
    Assert-NoBlockingModal
    if (-not [DsGridReportRun]::PostMessage([IntPtr] $MainWindowHandle, 0x0111,
        [IntPtr] $definition.CommandId, [IntPtr]::Zero)) {
        throw "Could not post command $($definition.CommandId)"
    }
    $record = [ordered]@{
        schema = 'ds.pls.report_action.v1'
        action = 'start'
        report = $Report
        command_id = $definition.CommandId
    }
    Write-Action $record
    $record | ConvertTo-Json
    exit 0
}

if ($Action -eq 'AcceptOptions') {
    if ([string]::IsNullOrWhiteSpace($definition.OptionDialogTitle)) {
        throw "Report '$Report' has no characterized options dialog"
    }
    $dialog = Get-UniqueWindow $definition.OptionDialogTitle
    $button = [DsGridReportRun]::GetDlgItem($dialog, 1)
    if ($button -eq [IntPtr]::Zero -or (Get-Class $button) -cne 'Button' -or
        (Get-Title $button) -cnotin @('OK', '&OK') -or
        -not [DsGridReportRun]::IsWindowVisible($button) -or
        -not [DsGridReportRun]::IsWindowEnabled($button)) {
        throw "Exact enabled OK control id 1 absent from '$($definition.OptionDialogTitle)'"
    }
    if (-not [DsGridReportRun]::PostMessage($button, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero)) {
        throw "Could not accept '$($definition.OptionDialogTitle)'"
    }
    $record = [ordered]@{
        schema = 'ds.pls.report_action.v1'
        action = 'accept_options'
        report = $Report
        dialog_title = $definition.OptionDialogTitle
    }
    Write-Action $record
    $record | ConvertTo-Json
    exit 0
}

if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    throw '-OutputPath is mandatory for SaveAs'
}
$parent = [System.IO.Path]::GetDirectoryName($OutputPath)
if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
    throw "Output parent does not exist: $parent"
}
if (Test-Path -LiteralPath $OutputPath) {
    throw "Output already exists: $OutputPath"
}
$title = $process.MainWindowTitle
if ($title.IndexOf($definition.ReportTitlePattern, [StringComparison]::OrdinalIgnoreCase) -lt 0) {
    throw "Active document '$title' does not match expected report '$($definition.ReportTitlePattern)'"
}
Assert-NoBlockingModal
if (-not [DsGridReportRun]::PostMessage([IntPtr] $MainWindowHandle, 0x0111,
    [IntPtr] $profile.ReportSaveAsCommandId, [IntPtr]::Zero)) {
    throw "Could not post report Save As command $($profile.ReportSaveAsCommandId)"
}
$record = [ordered]@{
    schema = 'ds.pls.report_action.v1'
    action = 'open_save_as'
    report = $Report
    save_as_command_id = $profile.ReportSaveAsCommandId
    requested_output_path = $OutputPath
    note = 'Call pls-save-report-file.ps1 only after proving the unique exact Save As dialog.'
}
Write-Action $record
$record | ConvertTo-Json
