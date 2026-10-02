param(
    [Parameter(Mandatory = $true)]
    [long] $WindowHandle
)

# Win32-only child dump of one dialog: class, control id, and text for every
# child window. UIA under WSL interop intermittently returns nothing; this
# path does not.
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsGridModalChildren {
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern int GetDlgCtrlID(IntPtr hwnd);
}
"@

$callback = [DsGridModalChildren+EnumWindowsProc] {
    param([IntPtr] $child, [IntPtr] $parameter)
    $text = New-Object System.Text.StringBuilder 1000
    [DsGridModalChildren]::GetWindowText($child, $text, 1000) | Out-Null
    $class = New-Object System.Text.StringBuilder 160
    [DsGridModalChildren]::GetClassName($child, $class, 160) | Out-Null
    $value = $text.ToString()
    if ($value) {
        Write-Output ("CHILD class={0} id={1} text={2}" -f
            $class.ToString(), [DsGridModalChildren]::GetDlgCtrlID($child), $value)
    }
    return $true
}
[DsGridModalChildren]::EnumChildWindows([IntPtr] $WindowHandle, $callback, [IntPtr]::Zero) | Out-Null
