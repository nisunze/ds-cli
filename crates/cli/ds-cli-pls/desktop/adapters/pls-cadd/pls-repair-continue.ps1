param(
    [int] $MaxPrompts = 400
)

# Clear PLS-CADD Project Repair Wizard prompts non-destructively.
#
# The wizard's five options are Win32 GROUP BOXES (class "Button", ids 1813-1817),
# not clickable buttons — BM_CLICK on them does nothing. The real push buttons are
# Manual Search (1691), Program Search (1692), History Search (1693),
# Remove (1694) and Cancel (2). The "Continue without finding the file" group box
# (1816) is realised by Cancel, so IDCANCEL is the documented non-mutating choice:
# it skips the missing file without rewriting a path or blanking a reference.
# Remove (1694/1817) blanks the reference and is never used here.
Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsGridPlsRepair {
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);
}
"@

$WM_COMMAND = 0x0111
$IDCANCEL = 2
$TITLE = 'PLS-CADD Project Repair Wizard'

function Find-Wizard {
    $script:wizard = [IntPtr]::Zero
    $callback = [DsGridPlsRepair+EnumWindowsProc] {
        param([IntPtr] $handle, [IntPtr] $parameter)
        if (-not [DsGridPlsRepair]::IsWindowVisible($handle)) { return $true }
        $text = New-Object System.Text.StringBuilder 512
        [DsGridPlsRepair]::GetWindowText($handle, $text, 512) | Out-Null
        if ($text.ToString().StartsWith($TITLE)) {
            $script:wizard = $handle
            return $false
        }
        return $true
    }
    [DsGridPlsRepair]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
    return $script:wizard
}

$dismissed = 0
for ($i = 0; $i -lt $MaxPrompts; $i++) {
    $wizard = Find-Wizard
    if ($wizard -eq [IntPtr]::Zero) { break }
    [DsGridPlsRepair]::PostMessage($wizard, $WM_COMMAND, [IntPtr] $IDCANCEL, [IntPtr]::Zero) | Out-Null
    $dismissed++
    Start-Sleep -Milliseconds 500
}

Write-Output ("dismissed={0} remaining={1}" -f $dismissed, (Find-Wizard))
