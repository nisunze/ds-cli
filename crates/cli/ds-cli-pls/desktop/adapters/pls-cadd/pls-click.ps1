param(
    [Parameter(Mandatory = $true)]
    [long] $ButtonHandle
)

Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DsGridPlsClick {
    [DllImport("user32.dll")]
    public static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);
}
"@

[DsGridPlsClick]::SendMessage([IntPtr] $ButtonHandle, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
