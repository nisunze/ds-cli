param(
    [Parameter(Mandatory = $true)]
    [long] $WindowHandle,
    [Parameter(Mandatory = $true)]
    [int] $CommandId,
    [switch] $Post
)

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DsGridPlsCommand {
    [DllImport("user32.dll")]
    public static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);
}
"@

if ($Post) {
    [DsGridPlsCommand]::PostMessage([IntPtr] $WindowHandle, 0x0111, [IntPtr] $CommandId, [IntPtr]::Zero) | Out-Null
} else {
    [DsGridPlsCommand]::SendMessage([IntPtr] $WindowHandle, 0x0111, [IntPtr] $CommandId, [IntPtr]::Zero) | Out-Null
}
