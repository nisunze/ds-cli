param(
    [Parameter(Mandatory = $true)]
    [long] $DialogHandle,
    [Parameter(Mandatory = $true)]
    [string] $ProjectPath,
    [switch] $Open
)

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsGridFileDialog {
    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wParam, string lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wParam, StringBuilder lParam);

    [DllImport("user32.dll")]
    public static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern IntPtr GetDlgItem(IntPtr dialog, int controlId);
}
"@

$dialog = [IntPtr] $DialogHandle
[DsGridFileDialog]::SendMessage($dialog, 0x0468, [IntPtr] 1148, $ProjectPath) | Out-Null
Start-Sleep -Milliseconds 250

$filePath = New-Object System.Text.StringBuilder 32768
[DsGridFileDialog]::SendMessage($dialog, 0x0465, [IntPtr] $filePath.Capacity, $filePath) | Out-Null
Write-Output ("FILE=" + $filePath.ToString())

$folderPath = New-Object System.Text.StringBuilder 32768
[DsGridFileDialog]::SendMessage($dialog, 0x0466, [IntPtr] $folderPath.Capacity, $folderPath) | Out-Null
Write-Output ("FOLDER=" + $folderPath.ToString())

if ($Open) {
    $button = [DsGridFileDialog]::GetDlgItem($dialog, 1)
    if ($button -eq [IntPtr]::Zero) {
        throw "The Open button was not found."
    }
    [DsGridFileDialog]::SendMessage($button, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
}
