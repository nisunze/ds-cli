param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [Parameter(Mandatory = $true)][long] $WindowHandle,
    [Parameter(Mandatory = $true)][int] $X,
    [Parameter(Mandatory = $true)][int] $Y,
    [string] $Choose = '',
    [int] $ExpectedId = 0
)
# Right-click at client (X,Y) of one window with the real cursor parked there, read the popup
# menu (#32768) the process shows (MN_GETHMENU -> GetMenuString/ID/State, read-only), and
# optionally choose ONE item: its raw text (with '&') and command id must both match exactly
# and it must be enabled; Down keys move the highlight until exactly that item carries
# MF_HILITE, then Enter. Without -Choose the menu is closed with Escape. Anything else throws.
# Characterized 2026-09-24 on the PLS-CADD 16.81 Section Table grid (context menu of 22 items).
$ErrorActionPreference = 'Stop'
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class DsCtxMenu {
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr FindWindowEx(IntPtr p, IntPtr a, string cls, string title);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
    [DllImport("user32.dll")] public static extern uint GetMenuItemID(IntPtr m, int p);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetMenuString(IntPtr m, uint i, StringBuilder s, int n, uint f);
    [DllImport("user32.dll")] public static extern uint GetMenuState(IntPtr m, uint i, uint f);
}
"@
function PopupMenuWindow {
    $h = [IntPtr]::Zero
    while (($h = [DsCtxMenu]::FindWindowEx([IntPtr]::Zero, $h, '#32768', $null)) -ne [IntPtr]::Zero) {
        $p = [uint32]0; [DsCtxMenu]::GetWindowThreadProcessId($h, [ref]$p) | Out-Null
        if ($p -eq $ProcessId -and [DsCtxMenu]::IsWindowVisible($h)) { return $h }
    }
    return [IntPtr]::Zero
}
if ((PopupMenuWindow) -ne [IntPtr]::Zero) { throw 'a popup menu is already open' }
$pt = New-Object DsCtxMenu+POINT; $pt.X = $X; $pt.Y = $Y
[DsCtxMenu]::ClientToScreen([IntPtr]$WindowHandle, [ref]$pt) | Out-Null
[DsCtxMenu]::SetCursorPos($pt.X, $pt.Y) | Out-Null
Start-Sleep -Milliseconds 150
$l = [IntPtr](($Y -shl 16) -bor ($X -band 0xFFFF))
[DsCtxMenu]::PostMessage([IntPtr]$WindowHandle, 0x0204, [IntPtr]2, $l) | Out-Null   # WM_RBUTTONDOWN
[DsCtxMenu]::PostMessage([IntPtr]$WindowHandle, 0x0205, [IntPtr]0, $l) | Out-Null   # WM_RBUTTONUP
$menuWnd = [IntPtr]::Zero
for ($i = 0; $i -lt 20 -and $menuWnd -eq [IntPtr]::Zero; $i++) { Start-Sleep -Milliseconds 200; $menuWnd = PopupMenuWindow }
if ($menuWnd -eq [IntPtr]::Zero) { throw 'no popup menu appeared' }
$m = [DsCtxMenu]::SendMessage($menuWnd, 0x01E1, [IntPtr]::Zero, [IntPtr]::Zero)      # MN_GETHMENU
$n = [DsCtxMenu]::GetMenuItemCount($m)
$items = @(for ($i = 0; $i -lt $n; $i++) {
    $sb = New-Object System.Text.StringBuilder 256; [DsCtxMenu]::GetMenuString($m, [uint32]$i, $sb, 256, 0x400) | Out-Null
    [ordered]@{ index = $i; id = [int][DsCtxMenu]::GetMenuItemID($m, $i); text = $sb.ToString(); state = [int][DsCtxMenu]::GetMenuState($m, [uint32]$i, 0x400) }
})
$chosen = $null
if ($Choose -eq '') {
    [DsCtxMenu]::PostMessage($menuWnd, 0x0100, [IntPtr]0x1B, [IntPtr]0x00010001) | Out-Null   # VK_ESCAPE
} else {
    $target = @($items | Where-Object { $_.text -ceq $Choose -and $_.id -eq $ExpectedId })
    if ($target.Count -ne 1) { [DsCtxMenu]::PostMessage($menuWnd, 0x0100, [IntPtr]0x1B, [IntPtr]0x00010001) | Out-Null; throw "menu has no single item '$Choose' id $ExpectedId" }
    $t = [uint32]$target[0].index
    if (($target[0].state -band 0x3) -ne 0) { [DsCtxMenu]::PostMessage($menuWnd, 0x0100, [IntPtr]0x1B, [IntPtr]0x00010001) | Out-Null; throw "item '$Choose' is disabled" }
    for ($k = 0; $k -lt $n + 2 -and (([DsCtxMenu]::GetMenuState($m, $t, 0x400) -band 0x80) -eq 0); $k++) {
        [DsCtxMenu]::PostMessage($menuWnd, 0x0100, [IntPtr]0x28, [IntPtr]0x00500001) | Out-Null  # VK_DOWN
        Start-Sleep -Milliseconds 250
    }
    $lit = @(for ($i = 0; $i -lt $n; $i++) { if (([DsCtxMenu]::GetMenuState($m, [uint32]$i, 0x400) -band 0x80) -ne 0) { $i } })
    if ($lit.Count -ne 1 -or $lit[0] -ne $t) { [DsCtxMenu]::PostMessage($menuWnd, 0x0100, [IntPtr]0x1B, [IntPtr]0x00010001) | Out-Null; throw "highlight is not exactly '$Choose': $($lit -join ',')" }
    [DsCtxMenu]::PostMessage($menuWnd, 0x0100, [IntPtr]0x0D, [IntPtr]0x001C0001) | Out-Null     # VK_RETURN
    $chosen = [ordered]@{ index = [int]$t; id = $ExpectedId; text = $Choose; down_keys = $k }
}
Start-Sleep -Milliseconds 500
if ((PopupMenuWindow) -ne [IntPtr]::Zero) { throw 'popup menu still open after the choice' }
[ordered]@{ schema = 'ds.pls.context_menu.v1'; window = $WindowHandle; x = $X; y = $Y; items = $items; chosen = $chosen } | ConvertTo-Json -Depth 4 -Compress
