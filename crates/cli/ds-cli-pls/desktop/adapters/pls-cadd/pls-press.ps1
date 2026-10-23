param([Parameter(Mandatory = $true)][long] $Button)
# Press one dialog button the way pls-dialog-watch does: post the button's own notification,
# WM_COMMAND(id, BN_CLICKED), to its parent. A posted BM_CLICK (pls-windows.ps1 -Click) silently does
# nothing when the dialog is not the active window, e.g. under nested modal loops.
Add-Type @"
using System; using System.Runtime.InteropServices;
public static class DsPress {
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
}
"@
$h = [IntPtr]$Button
$id = [DsPress]::GetDlgCtrlID($h)
[DsPress]::PostMessage([DsPress]::GetParent($h), 0x0111, [IntPtr]($id -band 0xFFFF), $h) | Out-Null
"pressed $Button (id $id)"
