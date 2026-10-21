param(
    [Parameter(Mandatory = $true)][long] $ComboHandle,
    [string] $Select = '',
    [int] $SelectIndex = -1
)
# List a Win32 ComboBox's items (index, text, current selection) and optionally
# select one by exact text (-Select) or index (-SelectIndex). Selection is done
# with CB_SETCURSEL followed by the CBN_SELCHANGE notification to the parent,
# which is what makes the owning dialog react (relabel units, enable fields).
Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class DsCombo {
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, StringBuilder l);
    [DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint msg, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
    [DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
}
"@
$h = [IntPtr] $ComboHandle
$count = [int][DsCombo]::SendMessage($h, 0x0146, [IntPtr]::Zero, [IntPtr]::Zero)   # CB_GETCOUNT
$items = @()
for ($i = 0; $i -lt $count; $i++) {
    $sb = New-Object System.Text.StringBuilder 1024
    [DsCombo]::SendMessage($h, 0x0148, [IntPtr]$i, $sb) | Out-Null                # CB_GETLBTEXT
    $items += $sb.ToString()
}
$target = $SelectIndex
if ($Select -ne '') { $target = [array]::IndexOf($items, $Select); if ($target -lt 0) { throw "combo has no item '$Select'; items: $($items -join ' | ')" } }
if ($target -ge 0) {
    [DsCombo]::SendMessage($h, 0x014E, [IntPtr]$target, [IntPtr]::Zero) | Out-Null   # CB_SETCURSEL
    $parent = [DsCombo]::GetParent($h)
    $id = [DsCombo]::GetDlgCtrlID($h)
    $wparam = [IntPtr] (([long]1 -shl 16) -bor [long]$id)                                 # CBN_SELCHANGE = 1
    [DsCombo]::SendMessage($parent, 0x0111, $wparam, $h) | Out-Null                    # WM_COMMAND
}
$cur = [int][DsCombo]::SendMessage($h, 0x0147, [IntPtr]::Zero, [IntPtr]::Zero)     # CB_GETCURSEL
[ordered]@{ handle = $ComboHandle; count = $count; selected_index = $cur; selected = $(if ($cur -ge 0 -and $cur -lt $count) { $items[$cur] } else { '' }); items = $items } | ConvertTo-Json -Compress
