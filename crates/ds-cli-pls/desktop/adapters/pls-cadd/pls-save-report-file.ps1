param(
    [Parameter(Mandatory = $true)]
    [int] $ProcessId,
    [Parameter(Mandatory = $true)]
    [long] $MainWindowHandle,
    [Parameter(Mandatory = $true)]
    [long] $DialogHandle,
    [Parameter(Mandatory = $true)]
    [ValidateSet('structure_usage', 'wind_weight_span', 'summary', 'section_usage', 'section_tension')]
    [string] $Report,
    [Parameter(Mandatory = $true)]
    [string] $ExpectedReportTitlePattern,
    [Parameter(Mandatory = $true)]
    [string] $OutputPath,
    [Parameter(Mandatory = $true)]
    [string] $EvidenceDirectory,
    [int] $TimeoutSeconds = 30
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

Add-Type @"
using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;

public static class DsGridReportSave {
    public delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr window, StringBuilder text, int count);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr window, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern int GetDlgCtrlID(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool IsWindowEnabled(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool IsWindow(IntPtr window);

    [DllImport("user32.dll")]
    public static extern IntPtr GetDlgItem(IntPtr dialog, int controlId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern bool SetWindowText(IntPtr window, string text);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
}
"@

function Get-Text([IntPtr] $Handle) {
    $text = New-Object System.Text.StringBuilder 32768
    [DsGridReportSave]::GetWindowText($Handle, $text, $text.Capacity) | Out-Null
    return $text.ToString()
}

function Get-Class([IntPtr] $Handle) {
    $text = New-Object System.Text.StringBuilder 256
    [DsGridReportSave]::GetClassName($Handle, $text, $text.Capacity) | Out-Null
    return $text.ToString()
}

$process = Get-Process -Id $ProcessId
if ([long] $process.MainWindowHandle -ne $MainWindowHandle) {
    throw "Main window handle does not belong to PID $ProcessId"
}
if ($process.MainWindowTitle.IndexOf($ExpectedReportTitlePattern,
    [StringComparison]::OrdinalIgnoreCase) -lt 0) {
    throw "Active document is not the expected report: $($process.MainWindowTitle)"
}
$dialog = [IntPtr] $DialogHandle
$owner = 0
[DsGridReportSave]::GetWindowThreadProcessId($dialog, [ref] $owner) | Out-Null
if ($owner -ne $ProcessId -or (Get-Text $dialog) -cne 'Save As' -or
    -not [DsGridReportSave]::IsWindowVisible($dialog) -or
    -not [DsGridReportSave]::IsWindowEnabled($dialog)) {
    throw 'Save dialog identity/state mismatch'
}

$enabledTops = New-Object System.Collections.ArrayList
$topCallback = [DsGridReportSave+EnumWindowsProc] {
    param([IntPtr] $window, [IntPtr] $parameter)
    $windowOwner = 0
    [DsGridReportSave]::GetWindowThreadProcessId($window, [ref] $windowOwner) | Out-Null
    if ($windowOwner -eq $ProcessId -and [long] $window -ne $MainWindowHandle -and
        [DsGridReportSave]::IsWindowVisible($window) -and [DsGridReportSave]::IsWindowEnabled($window)) {
        $enabledTops.Add($window) | Out-Null
    }
    return $true
}
[DsGridReportSave]::EnumWindows($topCallback, [IntPtr]::Zero) | Out-Null
if ($enabledTops.Count -ne 1 -or [long] $enabledTops[0] -ne $DialogHandle) {
    throw "Save As is not the unique enabled modal; refusing to type or click"
}

$outputItem = [System.IO.FileInfo] $OutputPath
if ($outputItem.Extension -inotin @('.txt', '.rtf')) {
    throw 'Only characterized native report output (.txt or .rtf) is allowed'
}
if (-not (Test-Path -LiteralPath $outputItem.DirectoryName -PathType Container)) {
    throw "Output parent does not exist: $($outputItem.DirectoryName)"
}
if (Test-Path -LiteralPath $outputItem.FullName) {
    throw "Output already exists: $($outputItem.FullName)"
}
if (-not (Test-Path -LiteralPath $EvidenceDirectory -PathType Container)) {
    throw "Evidence directory does not exist: $EvidenceDirectory"
}

$edits = New-Object System.Collections.ArrayList
$childCallback = [DsGridReportSave+EnumWindowsProc] {
    param([IntPtr] $window, [IntPtr] $parameter)
    $class = New-Object System.Text.StringBuilder 128
    [DsGridReportSave]::GetClassName($window, $class, $class.Capacity) | Out-Null
    if ($class.ToString() -eq 'Edit') { $edits.Add($window) | Out-Null }
    return $true
}
[DsGridReportSave]::EnumChildWindows($dialog, $childCallback, [IntPtr]::Zero) | Out-Null

$fileEdit = @($edits | Where-Object {
    [DsGridReportSave]::GetDlgCtrlID($_) -in @(1001, 1148, 1152)
})
if ($fileEdit.Count -ne 1) {
    throw "Could not identify exactly one filename edit control (found $($fileEdit.Count)); refusing blind input"
}
$fileEdit = [IntPtr] $fileEdit[0]
if (-not [DsGridReportSave]::SetWindowText($fileEdit, $outputItem.FullName)) {
    throw 'Could not set filename control'
}
Start-Sleep -Milliseconds 250
$readback = Get-Text $fileEdit
if ($readback -cne $outputItem.FullName) {
    throw "Filename readback mismatch: '$readback'"
}

$save = [DsGridReportSave]::GetDlgItem($dialog, 1)
if ($save -eq [IntPtr]::Zero -or (Get-Class $save) -cne 'Button' -or
    (Get-Text $save) -cnotin @('Save', '&Save', 'OK', '&OK') -or
    -not [DsGridReportSave]::IsWindowVisible($save) -or
    -not [DsGridReportSave]::IsWindowEnabled($save)) {
    throw 'Exact enabled Save/OK Button control id 1 is absent'
}
if (-not [DsGridReportSave]::PostMessage($save, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero)) {
    throw 'Could not post the Save command'
}

$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
$lastLength = -1L
$stable = 0
do {
    Start-Sleep -Milliseconds 250
    if (Test-Path -LiteralPath $outputItem.FullName -PathType Leaf) {
        $length = (Get-Item -LiteralPath $outputItem.FullName).Length
        if ($length -gt 0 -and $length -eq $lastLength) { $stable++ } else { $stable = 0 }
        $lastLength = $length
    }
} while ($stable -lt 2 -and [DateTime]::UtcNow -lt $deadline)
if ($stable -lt 2) { throw "Report file was not created nonempty and stable within $TimeoutSeconds seconds" }
if ([DsGridReportSave]::IsWindow($dialog) -and [DsGridReportSave]::IsWindowVisible($dialog)) {
    throw 'Save As dialog remains visible after output appeared; refusing ambiguous completion'
}
$header = Get-Content -LiteralPath $outputItem.FullName -Raw
if ($header.IndexOf('PLS-CADD Version', [StringComparison]::Ordinal) -lt 0) {
    throw 'Saved file lacks the native PLS-CADD report header'
}

$record = [ordered]@{
    schema = 'ds.pls.report_file.v1'
    timestamp_utc = [DateTime]::UtcNow.ToString('o')
    process_id = $ProcessId
    report = $Report
    active_report_title = $process.MainWindowTitle
    path = $outputItem.FullName
    bytes = (Get-Item -LiteralPath $outputItem.FullName).Length
    sha256 = (Get-FileHash -LiteralPath $outputItem.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
}
$json = $record | ConvertTo-Json -Compress
Add-Content -LiteralPath (Join-Path $EvidenceDirectory 'actions.jsonl') -Value $json -Encoding UTF8
$record | ConvertTo-Json
