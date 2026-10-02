param([Parameter(Mandatory = $true)][long] $Dialog, [switch] $All)
# List a PLS dialog's child controls: handle, id, class, enabled, check state, parent, text.
# WM_GETTEXT goes ONLY to standard Win32 classes. PLS's own grid/table/list controls are listed as '<not read>':
# a cross-process WM_GETTEXT to one of them crashed PLS-POLE 16.81 (exit 182) on 2026-09-26.
# -All includes hidden controls (other tab pages).
Add-Type -Namespace PlsSafeDump -Name W -MemberDefinition @'
public delegate bool EnumProc(IntPtr h, IntPtr l);
[DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
[DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
[DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
[DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, System.Text.StringBuilder l);
[DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
'@
$safe = 'Static', 'Button', 'Edit', 'ComboBox', 'ComboBoxEx32', '#32770'
$list = New-Object System.Collections.ArrayList
$cb = [PlsSafeDump.W+EnumProc] { param($h, $l) [void]$list.Add($h); $true }
[PlsSafeDump.W]::EnumChildWindows([IntPtr]$Dialog, $cb, [IntPtr]::Zero) | Out-Null
foreach ($h in $list) {
    $c = New-Object System.Text.StringBuilder 128; [PlsSafeDump.W]::GetClassName($h, $c, 128) | Out-Null; $cls = $c.ToString()
    if (-not $All -and -not [PlsSafeDump.W]::IsWindowVisible($h)) { continue }
    $txt = '<not read>'; $chk = ''
    if ($safe -contains $cls) {
        $t = New-Object System.Text.StringBuilder 4096; [PlsSafeDump.W]::SendMessage($h, 0x000D, [IntPtr]4096, $t) | Out-Null; $txt = $t.ToString()
        if ($cls -eq 'Button') { $chk = [int][PlsSafeDump.W]::SendMessage($h, 0x00F0, [IntPtr]::Zero, [IntPtr]::Zero) }
    }
    "{0} id={1} {2} en={3} chk={4} p={5} ""{6}""" -f $h, [PlsSafeDump.W]::GetDlgCtrlID($h), $cls, [PlsSafeDump.W]::IsWindowEnabled($h), $chk, [PlsSafeDump.W]::GetParent($h), $txt
}
