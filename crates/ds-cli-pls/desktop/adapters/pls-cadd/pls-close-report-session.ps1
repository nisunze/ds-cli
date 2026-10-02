param(
    [Parameter(Mandatory = $true)]
    [int] $ProcessId,
    [Parameter(Mandatory = $true)]
    [long] $MainWindowHandle,
    [Parameter(Mandatory = $true)]
    [string] $EvidenceDirectory,
    [string] $ProfilePath = (Join-Path $PSScriptRoot 'pls-report-profile.psd1'),
    [int] $TimeoutSeconds = 60
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;

public static class DsGridReportClose {
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
    public static extern bool IsWindowVisible(IntPtr window);

    [DllImport("user32.dll")]
    public static extern bool IsWindowEnabled(IntPtr window);

    [DllImport("user32.dll")]
    public static extern IntPtr GetDlgItem(IntPtr dialog, int controlId);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
}
"@

function Get-Text([IntPtr] $Handle) {
    $text = New-Object System.Text.StringBuilder 32768
    [DsGridReportClose]::GetWindowText($Handle, $text, $text.Capacity) | Out-Null
    return $text.ToString()
}

function Get-Class([IntPtr] $Handle) {
    $text = New-Object System.Text.StringBuilder 256
    [DsGridReportClose]::GetClassName($Handle, $text, $text.Capacity) | Out-Null
    return $text.ToString()
}

function Get-ProcessWindows {
    $windows = New-Object System.Collections.ArrayList
    $callback = [DsGridReportClose+EnumWindowsProc] {
        param([IntPtr] $window, [IntPtr] $parameter)
        $owner = 0
        [DsGridReportClose]::GetWindowThreadProcessId($window, [ref] $owner) | Out-Null
        if ($owner -eq $ProcessId -and [DsGridReportClose]::IsWindowVisible($window)) {
            $windows.Add([ordered]@{
                handle = [long] $window
                title = Get-Text $window
                enabled = [DsGridReportClose]::IsWindowEnabled($window)
            }) | Out-Null
        }
        return $true
    }
    [DsGridReportClose]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
    return @($windows)
}

function Get-DialogBody([IntPtr] $Dialog) {
    $parts = New-Object System.Collections.ArrayList
    $callback = [DsGridReportClose+EnumWindowsProc] {
        param([IntPtr] $child, [IntPtr] $parameter)
        $class = Get-Class $child
        $value = Get-Text $child
        if ([DsGridReportClose]::IsWindowVisible($child) -and
            -not [string]::IsNullOrWhiteSpace($value) -and
            $class -cnotin @('Button', 'Edit', 'ComboBox')) {
            $parts.Add($value.Trim()) | Out-Null
        }
        return $true
    }
    [DsGridReportClose]::EnumChildWindows($Dialog, $callback, [IntPtr]::Zero) | Out-Null
    return ((@($parts) -join ' ') -replace '\s+', ' ').Trim()
}

function Write-Action([object] $Record) {
    $Record['timestamp_utc'] = [DateTime]::UtcNow.ToString('o')
    $json = ($Record | ConvertTo-Json -Compress -Depth 10) + "`n"
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($json)
    $path = Join-Path $EvidenceDirectory 'actions.jsonl'
    $stream = [System.IO.File]::Open($path, [System.IO.FileMode]::Append,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read)
    try {
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
}

if ($TimeoutSeconds -lt 1 -or $TimeoutSeconds -gt 300) {
    throw 'TimeoutSeconds must be between 1 and 300'
}
if (-not (Test-Path -LiteralPath $EvidenceDirectory -PathType Container)) {
    throw "Evidence directory does not exist: $EvidenceDirectory"
}
$profile = Import-PowerShellDataFile -LiteralPath $ProfilePath
$process = Get-Process -Id $ProcessId
if ([long] $process.MainWindowHandle -ne $MainWindowHandle) {
    throw "Main window handle does not belong to PID $ProcessId"
}
if (-not [DsGridReportClose]::PostMessage([IntPtr] $MainWindowHandle, 0x0111,
    [IntPtr] $profile.ExitCommandId, [IntPtr]::Zero)) {
    throw "Could not post exit command $($profile.ExitCommandId)"
}

$savePromptObserved = $false
$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
while ([DateTime]::UtcNow -lt $deadline) {
    Start-Sleep -Milliseconds 200
    $process.Refresh()
    if ($process.HasExited) {
        $record = [ordered]@{
            schema = 'ds.pls.report_session_action.v1'
            action = 'exit_without_save'
            process_id = $ProcessId
            exit_command_id = [int] $profile.ExitCommandId
            save_prompt_observed = $savePromptObserved
            exit_code = $process.ExitCode
        }
        Write-Action $record
        $record | ConvertTo-Json
        exit 0
    }

    $windows = @(Get-ProcessWindows)
    $main = @($windows | Where-Object { $_.handle -eq $MainWindowHandle })
    if ($main.Count -ne 1) {
        throw "Expected one visible PLS-CADD main window while exiting, found $($main.Count)"
    }
    if ($main[0].enabled) { continue }

    $modals = @($windows | Where-Object { $_.handle -ne $MainWindowHandle -and $_.enabled })
    if ($modals.Count -ne 1) {
        throw "Expected one enabled exit modal, found $($modals.Count)"
    }
    if ($savePromptObserved) { throw 'Project-save prompt appeared more than once' }
    $dialog = $modals[0]
    if ([string] $dialog.title -cne [string] $profile.ProductDialogTitle) {
        throw "Unexpected exit dialog title: '$($dialog.title)'"
    }
    $body = Get-DialogBody ([IntPtr] $dialog.handle)
    $allowed = $false
    foreach ($pattern in @($profile.ExitSaveBodyPatterns)) {
        if ($body -match $pattern) { $allowed = $true; break }
    }
    if (-not $allowed) { throw "Unexpected exit dialog body: '$body'" }

    $button = [DsGridReportClose]::GetDlgItem(
        [IntPtr] $dialog.handle, [int] $profile.ExitNoControlId)
    $buttonText = Get-Text $button
    if ($button -eq [IntPtr]::Zero -or (Get-Class $button) -cne 'Button' -or
        @($profile.ExitNoTexts) -cnotcontains $buttonText -or
        -not [DsGridReportClose]::IsWindowVisible($button) -or
        -not [DsGridReportClose]::IsWindowEnabled($button)) {
        throw "Exit control id $($profile.ExitNoControlId) is not the exact enabled No button"
    }
    if (-not [DsGridReportClose]::PostMessage($button, 0x00F5,
        [IntPtr]::Zero, [IntPtr]::Zero)) {
        throw 'Could not decline the project-save prompt'
    }
    $savePromptObserved = $true
}

throw "PLS-CADD did not exit within $TimeoutSeconds seconds"
