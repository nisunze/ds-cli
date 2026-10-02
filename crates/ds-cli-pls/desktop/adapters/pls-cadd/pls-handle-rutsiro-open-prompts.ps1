param(
    [Parameter(Mandatory = $true)][int]$ProcessId,
    [Parameter(Mandatory = $true)][long]$MainWindowHandle,
    [Parameter(Mandatory = $true)][string]$JournalPath,
    [int]$TimeoutSeconds = 120
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsRutsiroOpenPrompts {
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc callback, IntPtr parameter);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern IntPtr GetDlgItem(IntPtr dialog, int controlId);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);
}
"@

function Get-WindowTextValue([IntPtr]$Handle) {
    $buffer = [Text.StringBuilder]::new(32768)
    [DsRutsiroOpenPrompts]::GetWindowText($Handle, $buffer, $buffer.Capacity) | Out-Null
    $buffer.ToString()
}

function Get-ProcessDialogs {
    $found = [Collections.ArrayList]::new()
    $callback = [DsRutsiroOpenPrompts+EnumWindowsProc] {
        param([IntPtr]$window, [IntPtr]$unused)
        $owner = [uint32]0
        [DsRutsiroOpenPrompts]::GetWindowThreadProcessId($window, [ref]$owner) | Out-Null
        if ($owner -eq $ProcessId -and [long]$window -ne $MainWindowHandle -and
            [DsRutsiroOpenPrompts]::IsWindowVisible($window) -and
            [DsRutsiroOpenPrompts]::IsWindowEnabled($window)) {
            $found.Add($window) | Out-Null
        }
        $true
    }
    [DsRutsiroOpenPrompts]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
    @($found)
}

function Get-DialogBody([IntPtr]$Dialog) {
    $texts = [Collections.ArrayList]::new()
    $callback = [DsRutsiroOpenPrompts+EnumWindowsProc] {
        param([IntPtr]$child, [IntPtr]$unused)
        $value = Get-WindowTextValue $child
        if (-not [string]::IsNullOrWhiteSpace($value)) { $texts.Add($value) | Out-Null }
        $true
    }
    [DsRutsiroOpenPrompts]::EnumChildWindows($Dialog, $callback, [IntPtr]::Zero) | Out-Null
    $texts -join ([string][char]10)
}

$rules = @(
    [ordered]@{ name='problem_reading_structure_file'; title='Problem reading structure file'; body="File='[^']+\\structures\\[^']+'"; id=7; texts=@('No','&No') },
    [ordered]@{ name='section_integrity'; title='Problem in verify_struct_section_integrity'; body='Section \d+ starts or ends on non dead end attachment point'; id=7; texts=@('No','&No') },
    [ordered]@{ name='bad_phase_count_de'; title='Bad number of phases on DE structure'; body='DE structures on opposite ends of section do not have the same number of phases'; id=4; texts=@('Never','&Never') },
    [ordered]@{ name='about_pls_cadd_16_81'; title='About PLS-CADD'; body='PLS-CADD\s+Version 16\.81x64'; id=1; texts=@('OK','&OK') },
    [ordered]@{ name='tip_of_the_day'; title='Tip of the Day'; body='Did you know\?'; id=1; texts=@('Close','&Close') },
    [ordered]@{ name='insufficient_criteria'; title='PLS-CADD'; body='Insufficient criteria to verify structure strength of '; id=7; texts=@('No','&No') }
)

$utf8NoBom = [Text.UTF8Encoding]::new($false)
$handled = 0
$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
while ([DateTime]::UtcNow -lt $deadline) {
    $process = Get-Process -Id $ProcessId -ErrorAction Stop
    $dialogs = @(Get-ProcessDialogs)
    if ($dialogs.Count -eq 0) {
        if ([DsRutsiroOpenPrompts]::IsWindowEnabled([IntPtr]$MainWindowHandle)) { break }
        Start-Sleep -Milliseconds 250
        continue
    }
    if ($dialogs.Count -ne 1) { throw "Expected at most one enabled dialog, found $($dialogs.Count)." }
    $dialog = [IntPtr]$dialogs[0]
    $title = Get-WindowTextValue $dialog
    $body = Get-DialogBody $dialog
    if ($title -ceq 'Error Log' -and [string]::IsNullOrWhiteSpace($body)) {
        if (-not [DsRutsiroOpenPrompts]::PostMessage($dialog, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero)) {
            throw 'Could not close the exact empty Error Log window.'
        }
        $record = [ordered]@{
            action = 'close_error_log'; title = $title
            timestamp_utc = [DateTime]::UtcNow.ToString('o')
        } | ConvertTo-Json -Compress -Depth 3
        [IO.File]::AppendAllText($JournalPath, $record + [string][char]10, $utf8NoBom)
        $handled++
        Start-Sleep -Milliseconds 350
        continue
    }
    $matches = @($rules | Where-Object { $_.title -ceq $title -and $body -match $_.body })
    if ($matches.Count -ne 1) { throw "Unrecognized PLS-CADD dialog '$title': $body" }
    $rule = $matches[0]
    $button = [DsRutsiroOpenPrompts]::GetDlgItem($dialog, $rule.id)
    if ($button -eq [IntPtr]::Zero) { throw "Dialog '$title' lacks control $($rule.id)." }
    $buttonText = Get-WindowTextValue $button
    if ($buttonText -cnotin $rule.texts) { throw "Dialog '$title' control text '$buttonText' is not allowed." }
    if (-not [DsRutsiroOpenPrompts]::PostMessage($button, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero)) {
        throw "Could not click '$buttonText' on '$title'."
    }
    $record = [ordered]@{
        action = 'answer_open_prompt'; rule = $rule.name; title = $title
        control_id = $rule.id; control_text = $buttonText; body = $body
        timestamp_utc = [DateTime]::UtcNow.ToString('o')
    } | ConvertTo-Json -Compress -Depth 5
    [IO.File]::AppendAllText($JournalPath, $record + [string][char]10, $utf8NoBom)
    $handled++
    Start-Sleep -Milliseconds 350
}

if (-not [DsRutsiroOpenPrompts]::IsWindowEnabled([IntPtr]$MainWindowHandle)) {
    $titles = @(Get-ProcessDialogs | ForEach-Object { Get-WindowTextValue $_ })
    throw "PLS-CADD is still blocked after $TimeoutSeconds seconds: $($titles -join '; ')"
}
[pscustomobject]@{ process_id=$ProcessId; handled=$handled; main_enabled=$true; title=(Get-Process -Id $ProcessId).MainWindowTitle } | ConvertTo-Json
