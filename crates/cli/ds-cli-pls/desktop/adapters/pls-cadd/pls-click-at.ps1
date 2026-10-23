param(
    [Parameter(Mandatory = $true)][long] $WindowHandle,
    [Parameter(Mandatory = $true)][int] $X,
    [Parameter(Mandatory = $true)][int] $Y,
    [switch] $Double,
    [switch] $MoveCursor
)
# -MoveCursor parks the real cursor on the target's screen point first. Grids that capture
# the mouse on WM_LBUTTONDOWN (PLS Section Table, AfxWnd140s id 131) read GetCursorPos
# for drag/auto-scroll selection, so a click posted while the cursor sits elsewhere
# extends the selection to wherever the cursor is (seen 2026-09-24 on Nyamagabe v18).
# Post a left click (or double click) at client coordinates (X,Y) of one window
# — for owner-drawn lists/grids that have no button ids (Attachment Manager
# list id 1532, report-option grids). Prints the window rect so the caller can
# reason about row geometry. Never used on a computing app; only on an idle
# dialog whose control tree was just enumerated.
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class DsClickAt {
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
    [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
}
"@
$h = [IntPtr] $WindowHandle
$r = New-Object DsClickAt+RECT
[DsClickAt]::GetWindowRect($h, [ref]$r) | Out-Null
if ($MoveCursor) {
    $p = New-Object DsClickAt+POINT; $p.X = $X; $p.Y = $Y
    [DsClickAt]::ClientToScreen($h, [ref]$p) | Out-Null
    [DsClickAt]::SetCursorPos($p.X, $p.Y) | Out-Null
    Start-Sleep -Milliseconds 150
}
$lparam = [IntPtr] (($Y -shl 16) -bor ($X -band 0xFFFF))
[DsClickAt]::PostMessage($h, 0x0201, [IntPtr]1, $lparam) | Out-Null   # WM_LBUTTONDOWN, MK_LBUTTON
[DsClickAt]::PostMessage($h, 0x0202, [IntPtr]0, $lparam) | Out-Null   # WM_LBUTTONUP
if ($Double) {
    [DsClickAt]::PostMessage($h, 0x0203, [IntPtr]1, $lparam) | Out-Null   # WM_LBUTTONDBLCLK
    [DsClickAt]::PostMessage($h, 0x0202, [IntPtr]0, $lparam) | Out-Null
}
"clicked $WindowHandle at client ($X,$Y); window rect $($r.L),$($r.T),$($r.R),$($r.B) size $($r.R-$r.L)x$($r.B-$r.T)"
