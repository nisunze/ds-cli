param(
    [Parameter(Mandatory = $true)][long] $Dialog,
    [string] $Texts = '',     # "2755=0,2287=10": edit control id = text (typed with EM_SETSEL + WM_CHAR, read back)
    [string] $Checks = '',    # "2876=1,2877=0": button id = 0/1 (BM_SETCHECK; radios: set the one, clear the others)
    [int] $Accept = 0         # press this button id afterwards (WM_COMMAND BN_CLICKED to the dialog), 0 = none
)
# Fill an already-open PLS dialog by control id and read every value back before accepting. Refuses a control id
# that is not a visible, enabled child of the dialog. For dialogs that appear after a graphical pick (no command id).
$ErrorActionPreference = 'Stop'
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices;
public static class DsFill {
    public delegate bool EnumProc(IntPtr h, IntPtr l);
    [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, StringBuilder l);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
}
"@
$kids = New-Object System.Collections.ArrayList
$cb = [DsFill+EnumProc] { param($h, $l) [void]$kids.Add($h); $true }
[DsFill]::EnumChildWindows([IntPtr]$Dialog, $cb, [IntPtr]::Zero) | Out-Null
function Ctl([int]$id) {
    $h = $kids | Where-Object { [DsFill]::GetDlgCtrlID($_) -eq $id -and [DsFill]::IsWindowVisible($_) -and [DsFill]::IsWindowEnabled($_) } | Select-Object -First 1
    if (-not $h) { throw "control $id is not a visible, enabled child of $Dialog" }
    $h
}
function Text([IntPtr]$h) { $sb = New-Object System.Text.StringBuilder 512; [DsFill]::SendMessage($h, 0x000D, [IntPtr]512, $sb) | Out-Null; $sb.ToString() }
$report = [ordered]@{}
foreach ($pair in ($Checks -split ',' | Where-Object { $_ })) {
    $id, $v = $pair -split '='
    $h = Ctl ([int]$id)
    [DsFill]::SendMessage($h, 0x00F1, [IntPtr]([int]$v), [IntPtr]::Zero) | Out-Null           # BM_SETCHECK
    if ([int]$v -eq 1) { [DsFill]::PostMessage([IntPtr]$Dialog, 0x0111, [IntPtr]([int]$id), $h) | Out-Null }   # BN_CLICKED: let the dialog update
    Start-Sleep -Milliseconds 150
    $report["check $id"] = [int][DsFill]::SendMessage($h, 0x00F0, [IntPtr]::Zero, [IntPtr]::Zero)   # BM_GETCHECK
}
foreach ($pair in ($Texts -split ',' | Where-Object { $_ })) {
    $id, $v = $pair -split '=', 2
    $h = Ctl ([int]$id)
    [DsFill]::SendMessage($h, 0x00B1, [IntPtr]0, [IntPtr](-1)) | Out-Null                        # EM_SETSEL all
    [DsFill]::SendMessage($h, 0x0102, [IntPtr]8, [IntPtr]1) | Out-Null                           # backspace clears the selection
    foreach ($ch in [int[]][char[]]$v) { [DsFill]::SendMessage($h, 0x0102, [IntPtr]$ch, [IntPtr]1) | Out-Null }
    $got = Text $h
    if ($got -ne $v) { throw "edit $id reads '$got', expected '$v'" }
    $report["text $id"] = $got
}
if ($Accept) {
    $h = Ctl $Accept
    [DsFill]::PostMessage([IntPtr]$Dialog, 0x0111, [IntPtr]$Accept, $h) | Out-Null
    $report['accepted'] = $Accept
}
$report | ConvertTo-Json -Compress
