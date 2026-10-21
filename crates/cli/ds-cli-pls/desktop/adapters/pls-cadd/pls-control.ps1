param(
    [long] $SetText = 0, [string] $Text = "",
    [long] $GetCheck = 0,
    [long] $SetCheck = 0, [int] $Checked = 0,
    [long] $GetText = 0
)
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsCtl {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, string l);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, StringBuilder l);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
}
"@
if ($SetText -ne 0) { [DsCtl]::SendMessage([IntPtr]$SetText, 0x000C, [IntPtr]::Zero, $Text) | Out-Null; "set $SetText = '$Text'" }
if ($GetCheck -ne 0) { "check $GetCheck = " + [DsCtl]::SendMessage([IntPtr]$GetCheck, 0x00F0, [IntPtr]::Zero, [IntPtr]::Zero) }
if ($SetCheck -ne 0) { [DsCtl]::SendMessage([IntPtr]$SetCheck, 0x00F1, [IntPtr]$Checked, [IntPtr]::Zero) | Out-Null; "setcheck $SetCheck = $Checked" }
if ($GetText -ne 0) { $sb = New-Object System.Text.StringBuilder 65536; [DsCtl]::SendMessage([IntPtr]$GetText, 0x000D, [IntPtr]65536, $sb) | Out-Null; $sb.ToString() }
