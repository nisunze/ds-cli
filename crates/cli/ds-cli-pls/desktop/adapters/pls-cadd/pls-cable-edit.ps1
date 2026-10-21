param(
    [Parameter(Mandatory = $true)][string] $Source,
    [Parameter(Mandatory = $true)][string] $SaveAs,
    [hashtable] $Fields = @{},
    [switch] $Regenerate,
    [string] $ReportPath = ''
)
# Author a PLS-CADD cable file natively in PLS-CADD 16.81's Cable Data editor (File > Edit Cable File):
# open $Source, set fields by control id, optionally regenerate the linear-elastic + creep-temperature-shift
# polynomials, save as $SaveAs, and optionally write PLS-CADD's Cable Data Report of the saved file to $ReportPath.
#
# Needs a running, idle PLS-CADD (launch it with: Start-Process explorer.exe '"C:\Program Files\PLS\pls_cadd\pls_cadd64.exe"').
# The editor shows and saves in the [GLOBAL] UNITS system (pls-ini-set.py); set UNITS=1 first for SI files.
#
# Physical-page field ids (verified 2026-09-26, 16.81): 2220 file, 2221 description, 2269 manufacturer,
# 2228 stock number, 2258 cable type (combo), 2261 size label, 2263/2264 outer strands number/diameter,
# 2265/2266 core strands, 2222 area, 2223 diameter, 2224 unit weight, 2225 ultimate tension, 2226 final modulus,
# 2257 thermal expansion, 1679 creep temperature shift, 2280 default tension, 2237 independent wires,
# radios 1529 nonlinear / 1530 linear + creep temperature increase / 1531 linear + proportional creep.
# Electrical page: 2471/2472 and 2478/2479 AC resistance/temperature pairs, 1683/1684 DC resistance/temperature.
# -Regenerate: set 1679 BEFORE it (PLS derives c0 = -E*alpha*shift only when the model switch happens).
$here = $PSScriptRoot
Add-Type -Namespace PlsCable -Name W -MemberDefinition @'
public delegate bool EnumProc(IntPtr h, IntPtr l);
[DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
[DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
[DllImport("user32.dll")] public static extern IntPtr GetParent(IntPtr h);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, string l);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, System.Text.StringBuilder l);
[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
'@
$ErrorActionPreference = 'Stop'
$proc = Get-Process pls_cadd64 | Select-Object -First 1
$main = [IntPtr][long]$proc.MainWindowHandle
function Visible { & "$here\pls-windows.ps1" -ProcessId $proc.Id | Where-Object { $_ -match 'vis=True' } }
function WaitTitle([string]$title, [int]$sec) {
    for ($i = 0; $i -lt $sec * 2; $i++) {
        $x = Visible | Where-Object { $_ -match [regex]::Escape("] '$title'") } | Select-Object -First 1
        if ($x) { return [IntPtr][long]($x -replace ' .*', '') }
        Start-Sleep -Milliseconds 500
    }
    return [IntPtr]::Zero
}
function Controls([IntPtr]$root) {
    $map = @{}
    $cb = [PlsCable.W+EnumProc] { param($h, $l) $id = [PlsCable.W]::GetDlgCtrlID($h); if (-not $map.ContainsKey($id)) { $map[$id] = $h }; $true }
    [PlsCable.W]::EnumChildWindows($root, $cb, [IntPtr]::Zero) | Out-Null
    $map
}
function Text([IntPtr]$h) { $sb = New-Object System.Text.StringBuilder 2048; [PlsCable.W]::SendMessage($h, 0x000D, [IntPtr]2048, $sb) | Out-Null; $sb.ToString() }
function EditById([IntPtr]$root, [int]$id) {   # file dialogs nest ComboBoxEx32 > ComboBox > Edit, all with the same id
    $found = [IntPtr]::Zero
    $cb = [PlsCable.W+EnumProc] { param($h, $l)
        $c = New-Object System.Text.StringBuilder 32; [PlsCable.W]::GetClassName($h, $c, 32) | Out-Null
        if ($c.ToString() -eq 'Edit' -and [PlsCable.W]::GetDlgCtrlID($h) -eq $id -and $script:found -eq [IntPtr]::Zero) { $script:found = $h }; $true }
    $script:found = [IntPtr]::Zero
    [PlsCable.W]::EnumChildWindows($root, $cb, [IntPtr]::Zero) | Out-Null
    if ($script:found -eq [IntPtr]::Zero) { throw "no Edit id $id" }
    $script:found
}
function TypeInto([IntPtr]$edit, [string]$s) {   # modern file dialogs ignore WM_SETTEXT: type characters
    [PlsCable.W]::SendMessage($edit, 0x00B1, [IntPtr]0, [IntPtr](-1)) | Out-Null
    foreach ($ch in [int[]][char[]]$s) { [PlsCable.W]::SendMessage($edit, 0x0102, [IntPtr]$ch, [IntPtr]1) | Out-Null }
    if ((Text $edit) -ne $s) { throw "typed '$(Text $edit)' instead of '$s'" }
}
function PressOk([IntPtr]$dlg) {   # IDOK must carry the button handle: with a null lParam the file dialog resolved the
    $script:okb = [IntPtr]::Zero      # typed path against its current folder ('Path does not exist', 2026-09-26)
    $cb = [PlsCable.W+EnumProc] { param($h, $l)
        $c = New-Object System.Text.StringBuilder 32; [PlsCable.W]::GetClassName($h, $c, 32) | Out-Null
        if ($script:okb -eq [IntPtr]::Zero -and $c.ToString() -eq 'Button' -and [PlsCable.W]::GetDlgCtrlID($h) -eq 1) { $script:okb = $h }; $true }
    [PlsCable.W]::EnumChildWindows($dlg, $cb, [IntPtr]::Zero) | Out-Null
    if ($script:okb -eq [IntPtr]::Zero) { throw 'no OK/Save button (id 1)' }
    [PlsCable.W]::PostMessage($dlg, 0x0111, [IntPtr]1, $script:okb) | Out-Null
}
function Radio([hashtable]$ctl, [int]$id) {
    foreach ($o in 1529, 1530, 1531) { if ($ctl[$o]) { [PlsCable.W]::SendMessage($ctl[$o], 0x00F1, [IntPtr]0, [IntPtr]::Zero) | Out-Null } }
    [PlsCable.W]::SendMessage($ctl[$id], 0x00F1, [IntPtr]1, [IntPtr]::Zero) | Out-Null
    # BN_CLICKED goes to the button's page, posted: the switch can raise a modal warning
    [PlsCable.W]::PostMessage([PlsCable.W]::GetParent($ctl[$id]), 0x0111, [IntPtr]$id, $ctl[$id]) | Out-Null
    Start-Sleep -Milliseconds 800
}
function ModalBody([IntPtr]$d) { ((& "$here\pls-safe-dump.ps1" -Dialog ([long]$d) | Where-Object { $_ -match 'id=65535 Static' }) -replace '.*p=\d+ ', '') -join ' ' }
function OpenCable([string]$path) {
    [PlsCable.W]::PostMessage($main, 0x0111, [IntPtr]40048, [IntPtr]0) | Out-Null        # File > Edit Cable File
    $od = WaitTitle 'Open Cable File' 15; if ($od -eq [IntPtr]::Zero) { throw 'no Open Cable File dialog' }
    TypeInto (EditById $od 1148) $path
    PressOk $od
    $cd = WaitTitle 'Cable Data' 15; if ($cd -eq [IntPtr]::Zero) { throw "no Cable Data dialog for $path" }
    $cd
}

# 1. open and set
$cd = OpenCable $Source
$ctl = Controls $cd
foreach ($k in $Fields.Keys) {
    $h = $ctl[[int]$k]; if (-not $h) { throw "no control $k in Cable Data" }
    [PlsCable.W]::SendMessage($h, 0x000C, [IntPtr]0, [string]$Fields[$k]) | Out-Null      # WM_SETTEXT (plain edits)
    if ((Text $h) -ne [string]$Fields[$k]) { throw "control $k reads '$(Text $h)' after setting '$($Fields[$k])'" }
}
# 2. regenerate polynomials: nonlinear, then back to linear + temperature shift (PLS rebuilds a0..c4)
if ($Regenerate) {
    Radio $ctl 1529
    $w = WaitTitle 'Cable Model Change Warning' 3
    if ($w -ne [IntPtr]::Zero) { throw "unexpected warning entering the nonlinear model: $(ModalBody $w)" }
    Radio $ctl 1530
    $w = WaitTitle 'Cable Model Change Warning' 10
    if ($w -eq [IntPtr]::Zero) { throw 'no Cable Model Change Warning on returning to the linear model' }
    $body = ModalBody $w
    if ($body -notmatch 'Switch to linear elastic cable model') { throw "unexpected warning: $body" }
    [PlsCable.W]::PostMessage($w, 0x0111, [IntPtr]1, [IntPtr]0) | Out-Null
    Start-Sleep 2
}
$ctl = Controls $cd
function Checked([IntPtr]$h) { [int][PlsCable.W]::SendMessage($h, 0x00F0, [IntPtr]::Zero, [IntPtr]::Zero) }
"model: $(if (Checked $ctl[1530]) {'linear + creep temperature increase'} elseif (Checked $ctl[1531]) {'linear + proportional creep'} else {'nonlinear'})"
foreach ($id in 2221, 2222, 2223, 2224, 2225, 2226, 2257, 1679, 2244) { "  {0,5} = '{1}'" -f $id, (Text $ctl[$id]) }
# 3. OK -> Save Cable File As
[PlsCable.W]::PostMessage($cd, 0x0111, [IntPtr]1, [IntPtr]0) | Out-Null
$sd = WaitTitle 'Save Cable File As' 15; if ($sd -eq [IntPtr]::Zero) { throw 'no Save Cable File As dialog' }
# Navigate first, then name: a full path typed while the dialog shows another folder was resolved against that folder
# ('<current>\<name>\...: Path does not exist'). Typing the target folder + OK moves the dialog there and keeps it open.
TypeInto (EditById $sd 1001) ([IO.Path]::GetDirectoryName($SaveAs))
PressOk $sd
Start-Sleep 2
if ((WaitTitle 'Save Cable File As' 5) -eq [IntPtr]::Zero) { throw 'Save Cable File As closed while navigating' }
TypeInto (EditById $sd 1001) ([IO.Path]::GetFileName($SaveAs))
PressOk $sd
$cf = WaitTitle 'Confirm Save As' 4
if ($cf -ne [IntPtr]::Zero) {
    $yes = [IntPtr]::Zero
    $cb = [PlsCable.W+EnumProc] { param($h, $l) if ((Text $h) -eq '&Yes') { $script:yes = $h }; $true }
    [PlsCable.W]::EnumChildWindows($cf, $cb, [IntPtr]::Zero) | Out-Null
    if ($script:yes -eq [IntPtr]::Zero) { throw 'Confirm Save As has no &Yes button' }
    [PlsCable.W]::SendMessage($script:yes, 0x00F5, [IntPtr]0, [IntPtr]0) | Out-Null      # BM_CLICK
    'overwrite confirmed'
}
Start-Sleep 3
if (Visible | Where-Object { $_ -notmatch "\] 'PLS-CADD" }) { throw "a dialog is still open: $(Visible)" }
$f = Get-Item -LiteralPath $SaveAs; "saved $($f.FullName) ($($f.Length) bytes, $($f.LastWriteTime))"
# 4. PLS-CADD's own Cable Data Report of the saved file
if ($ReportPath) {
    $cd = OpenCable $SaveAs
    [PlsCable.W]::PostMessage($cd, 0x0111, [IntPtr]57001, [IntPtr]0) | Out-Null         # Cable Data Report button
    Start-Sleep 4
    [PlsCable.W]::SendMessage($cd, 0x0111, [IntPtr]2, [IntPtr]0) | Out-Null               # close the editor unchanged
    Start-Sleep 2
    Set-Clipboard -Value 'empty'
    [PlsCable.W]::SendMessage($main, 0x0111, [IntPtr]57642, [IntPtr]0) | Out-Null         # Edit > Select All
    [PlsCable.W]::SendMessage($main, 0x0111, [IntPtr]57634, [IntPtr]0) | Out-Null         # Edit > Copy
    Start-Sleep 1
    $t = Get-Clipboard -Raw
    if ($t -notmatch [regex]::Escape([IO.Path]::GetFileName($SaveAs))) { throw 'Cable Data Report not captured' }
    Set-Content -LiteralPath $ReportPath -Value $t -Encoding UTF8
    "report $ReportPath"
}
