param(
    [Parameter(Mandatory = $true)]
    [int] $ProcessId,
    [Parameter(Mandatory = $true)]
    [long] $MainWindowHandle
)

# One-shot state probe for a running PLS-CADD: every visible top-level window of
# the process (so a blocking modal is obvious), the buttons inside any modal, and
# the full frame menu tree with command ids. Written as a -File script because
# inline -Command with nested quoting silently produces no output under WSL interop.
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsGridPlsState {
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern int GetDlgCtrlID(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern bool IsWindowEnabled(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint pid);

    [DllImport("user32.dll")]
    public static extern IntPtr GetMenu(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern int GetMenuItemCount(IntPtr menu);

    [DllImport("user32.dll")]
    public static extern uint GetMenuItemID(IntPtr menu, int position);

    [DllImport("user32.dll")]
    public static extern IntPtr GetSubMenu(IntPtr menu, int position);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetMenuString(IntPtr menu, uint item, StringBuilder text, int count, uint flags);
}
"@

function Get-Text([IntPtr] $handle) {
    $text = New-Object System.Text.StringBuilder 400
    [DsGridPlsState]::GetWindowText($handle, $text, 400) | Out-Null
    return $text.ToString()
}

function Get-Class([IntPtr] $handle) {
    $text = New-Object System.Text.StringBuilder 160
    [DsGridPlsState]::GetClassName($handle, $text, 160) | Out-Null
    return $text.ToString()
}

$tops = New-Object System.Collections.ArrayList
$topCallback = [DsGridPlsState+EnumWindowsProc] {
    param([IntPtr] $handle, [IntPtr] $parameter)
    $owner = 0
    [DsGridPlsState]::GetWindowThreadProcessId($handle, [ref] $owner) | Out-Null
    if ($owner -eq $ProcessId -and [DsGridPlsState]::IsWindowVisible($handle)) {
        $tops.Add($handle) | Out-Null
    }
    return $true
}
[DsGridPlsState]::EnumWindows($topCallback, [IntPtr]::Zero) | Out-Null

Write-Output "=== TOP-LEVEL WINDOWS ==="
foreach ($handle in $tops) {
    Write-Output ("TOP handle={0} enabled={1} class={2} title={3}" -f
        $handle, [DsGridPlsState]::IsWindowEnabled($handle), (Get-Class $handle), (Get-Text $handle))
    if ([long] $handle -ne $MainWindowHandle) {
        $childCallback = [DsGridPlsState+EnumWindowsProc] {
            param([IntPtr] $child, [IntPtr] $parameter)
            $class = Get-Class $child
            if ($class -eq 'Button') {
                Write-Output ("   BTN handle={0} id={1} enabled={2} text={3}" -f
                    $child, [DsGridPlsState]::GetDlgCtrlID($child), [DsGridPlsState]::IsWindowEnabled($child), (Get-Text $child))
            }
            return $true
        }
        [DsGridPlsState]::EnumChildWindows($handle, $childCallback, [IntPtr]::Zero) | Out-Null
    }
}

Write-Output "=== FRAME MENU ==="
function Show-Menu([IntPtr] $menu, [string] $prefix) {
    $count = [DsGridPlsState]::GetMenuItemCount($menu)
    for ($position = 0; $position -lt $count; $position++) {
        $text = New-Object System.Text.StringBuilder 400
        [DsGridPlsState]::GetMenuString($menu, [uint32] $position, $text, $text.Capacity, 0x400) | Out-Null
        Write-Output ("MENU {0}{1} id={2}" -f $prefix, $text.ToString(), [DsGridPlsState]::GetMenuItemID($menu, $position))
        $child = [DsGridPlsState]::GetSubMenu($menu, $position)
        if ($child -ne [IntPtr]::Zero) { Show-Menu $child ($prefix + "  ") }
    }
}
$menu = [DsGridPlsState]::GetMenu([IntPtr] $MainWindowHandle)
if ($menu -ne [IntPtr]::Zero) { Show-Menu $menu "" } else { Write-Output "(no frame menu)" }
