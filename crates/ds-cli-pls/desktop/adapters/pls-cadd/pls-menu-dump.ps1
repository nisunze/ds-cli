param(
    [Parameter(Mandatory = $true)][long] $WindowHandle
)
# Dump the live menu of a PLS program window (PLS-CADD, PLS-POLE, TOWER): one line per command,
# "<id>`t<Menu > Sub > Item>". Read-only; used to characterize command ids before posting them.
Add-Type -Namespace DsGrid -Name MenuDump -MemberDefinition @'
[DllImport("user32.dll")] public static extern IntPtr GetMenu(IntPtr h);
[DllImport("user32.dll")] public static extern int GetMenuItemCount(IntPtr m);
[DllImport("user32.dll")] public static extern IntPtr GetSubMenu(IntPtr m, int pos);
[DllImport("user32.dll")] public static extern uint GetMenuItemID(IntPtr m, int pos);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetMenuStringW(IntPtr m, uint item, System.Text.StringBuilder s, int n, uint flags);
'@
function Walk([IntPtr]$m, [string]$path) {
    $n = [DsGrid.MenuDump]::GetMenuItemCount($m)
    for ($i = 0; $i -lt $n; $i++) {
        $sb = New-Object System.Text.StringBuilder 256
        [DsGrid.MenuDump]::GetMenuStringW($m, [uint32]$i, $sb, 256, 0x400) | Out-Null   # MF_BYPOSITION
        $label = $sb.ToString()
        $sub = [DsGrid.MenuDump]::GetSubMenu($m, $i)
        if ($sub -ne [IntPtr]::Zero) { Walk $sub ($(if ($path) { "$path > $label" } else { $label })) }
        else {
            $id = [DsGrid.MenuDump]::GetMenuItemID($m, $i)
            if ($label) { "{0}`t{1} > {2}" -f $id, $path, $label }
        }
    }
}
$menu = [DsGrid.MenuDump]::GetMenu([IntPtr]$WindowHandle)
if ($menu -eq [IntPtr]::Zero) { throw "window $WindowHandle has no menu" }
Walk $menu ''
