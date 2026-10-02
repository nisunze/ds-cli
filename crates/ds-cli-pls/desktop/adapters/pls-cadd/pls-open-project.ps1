param(
    [Parameter(Mandatory = $true)]
    [long] $DialogHandle,
    [Parameter(Mandatory = $true)]
    [long] $EditHandle,
    [Parameter(Mandatory = $true)]
    [string] $ProjectPath
)

Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DsGridOpenDialog {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern bool SetWindowText(IntPtr hWnd, string text);

    [DllImport("user32.dll")]
    public static extern IntPtr SetFocus(IntPtr hWnd);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hWnd);
}
"@

$dialog = [IntPtr] $DialogHandle
$edit = [IntPtr] $EditHandle
if (-not [DsGridOpenDialog]::SetWindowText($edit, $ProjectPath)) {
    throw "Could not set the PLS-CADD project path."
}
[DsGridOpenDialog]::SetForegroundWindow($dialog) | Out-Null
[DsGridOpenDialog]::SetFocus($edit) | Out-Null
Start-Sleep -Milliseconds 250
[System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
