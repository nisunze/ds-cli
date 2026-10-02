param(
    [Parameter(Mandatory = $true)]
    [int] $ProcessId,
    [Parameter(Mandatory = $true)]
    [long] $MainWindowHandle,
    [Parameter(Mandatory = $true)]
    [ValidateSet('opposite_direction_warning', 'insufficient_strength_criteria', 'undefined_feature_codes')]
    [string] $Rule,
    [Parameter(Mandatory = $true)]
    [string] $EvidenceDirectory,
    [string] $ProfilePath = (Join-Path $PSScriptRoot 'pls-report-profile.psd1')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsGridKnownPrompt {
    public delegate bool EnumWindowsProc(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc callback, IntPtr parameter);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr window);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr dialog, int controlId);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
}
"@

function Get-Text([IntPtr] $Handle) {
    $text = New-Object System.Text.StringBuilder 32768
    [DsGridKnownPrompt]::GetWindowText($Handle, $text, $text.Capacity) | Out-Null
    return $text.ToString()
}

$profile = Import-PowerShellDataFile -LiteralPath $ProfilePath
$definition = @($profile.AllowedPromptRules | Where-Object { $_.Name -eq $Rule })
if ($definition.Count -ne 1) { throw "Prompt rule '$Rule' is not uniquely defined" }
$definition = $definition[0]
$process = Get-Process -Id $ProcessId
if ([long] $process.MainWindowHandle -ne $MainWindowHandle) {
    throw "Main window handle does not belong to PID $ProcessId"
}
if ([DsGridKnownPrompt]::IsWindowEnabled([IntPtr] $MainWindowHandle)) {
    throw 'Main window is enabled; no blocking prompt may be answered'
}

$modals = New-Object System.Collections.ArrayList
$callback = [DsGridKnownPrompt+EnumWindowsProc] {
    param([IntPtr] $window, [IntPtr] $parameter)
    $owner = 0
    [DsGridKnownPrompt]::GetWindowThreadProcessId($window, [ref] $owner) | Out-Null
    if ($owner -eq $ProcessId -and [long] $window -ne $MainWindowHandle -and
        [DsGridKnownPrompt]::IsWindowVisible($window) -and [DsGridKnownPrompt]::IsWindowEnabled($window)) {
        $modals.Add($window) | Out-Null
    }
    return $true
}
[DsGridKnownPrompt]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
if ($modals.Count -ne 1) { throw "Expected one enabled modal, found $($modals.Count)" }
$modal = [IntPtr] $modals[0]
$title = Get-Text $modal
if ($title -cne $definition.Title) {
    throw "Prompt title mismatch: expected '$($definition.Title)', got '$title'"
}

$texts = New-Object System.Collections.ArrayList
$childCallback = [DsGridKnownPrompt+EnumWindowsProc] {
    param([IntPtr] $window, [IntPtr] $parameter)
    $text = Get-Text $window
    if (-not [string]::IsNullOrWhiteSpace($text)) { $texts.Add($text) | Out-Null }
    return $true
}
[DsGridKnownPrompt]::EnumChildWindows($modal, $childCallback, [IntPtr]::Zero) | Out-Null
$body = $texts -join "`n"
if ($body -notmatch $definition.BodyPattern) {
    throw "Prompt body does not match rule '$Rule'"
}
$button = [DsGridKnownPrompt]::GetDlgItem($modal, $definition.ResponseControlId)
if ($button -eq [IntPtr]::Zero -or (Get-Text $button) -cne $definition.ResponseText) {
    throw "Prompt response control does not exactly match '$($definition.ResponseText)'"
}
if (-not [DsGridKnownPrompt]::PostMessage($modal, 0x0111,
    [IntPtr] $definition.ResponseControlId, [IntPtr]::Zero)) {
    throw "Could not post response for prompt '$Rule'"
}

$record = [ordered]@{
    schema = 'ds.pls.prompt_action.v1'
    timestamp_utc = [DateTime]::UtcNow.ToString('o')
    process_id = $ProcessId
    rule = $Rule
    title = $title
    body_sha256 = $null
    response_control_id = $definition.ResponseControlId
    response_text = $definition.ResponseText
}
$sha = [System.Security.Cryptography.SHA256]::Create()
try {
    $record.body_sha256 = ([BitConverter]::ToString($sha.ComputeHash(
        [System.Text.Encoding]::UTF8.GetBytes($body)))).Replace('-', '').ToLowerInvariant()
} finally { $sha.Dispose() }
$json = $record | ConvertTo-Json -Compress
Add-Content -LiteralPath (Join-Path $EvidenceDirectory 'actions.jsonl') -Value $json -Encoding UTF8
$record | ConvertTo-Json
