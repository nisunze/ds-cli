param(
    [Parameter(Mandatory = $true)]
    [long] $MainWindowHandle
)

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsGridPlsInspect {
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern int GetDlgCtrlID(IntPtr hwnd);

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

function Show-Menu([IntPtr] $menu, [string] $prefix) {
    $count = [DsGridPlsInspect]::GetMenuItemCount($menu)
    for ($position = 0; $position -lt $count; $position++) {
        $text = New-Object System.Text.StringBuilder 512
        [DsGridPlsInspect]::GetMenuString($menu, [uint32] $position, $text, $text.Capacity, 0x400) | Out-Null
        $id = [DsGridPlsInspect]::GetMenuItemID($menu, $position)
        Write-Output ("MENU {0}{1} id={2}" -f $prefix, $text.ToString(), $id)
        $child = [DsGridPlsInspect]::GetSubMenu($menu, $position)
        if ($child -ne [IntPtr]::Zero) {
            Show-Menu $child ($prefix + "  ")
        }
    }
}

$main = [IntPtr] $MainWindowHandle
$menu = [DsGridPlsInspect]::GetMenu($main)
if ($menu -ne [IntPtr]::Zero) {
    Show-Menu $menu ""
}

$callback = [DsGridPlsInspect+EnumWindowsProc] {
    param([IntPtr] $handle, [IntPtr] $parameter)
    $class = New-Object System.Text.StringBuilder 256
    $text = New-Object System.Text.StringBuilder 1024
    [DsGridPlsInspect]::GetClassName($handle, $class, $class.Capacity) | Out-Null
    [DsGridPlsInspect]::GetWindowText($handle, $text, $text.Capacity) | Out-Null
    $id = [DsGridPlsInspect]::GetDlgCtrlID($handle)
    Write-Output ("CHILD handle={0} id={1} class={2} text={3}" -f $handle, $id, $class.ToString(), $text.ToString())
    return $true
}
[DsGridPlsInspect]::EnumChildWindows($main, $callback, [IntPtr]::Zero) | Out-Null
