param(
    [Parameter(Mandatory = $true)][ValidateSet('Launch', 'Open', 'SaveAs', 'RunStructureFiles', 'Exit')][string] $Action,
    [string] $Path = '',
    [string] $Template = '',
    [string] $Destination = '',
    [switch] $PolExtension,
    [string] $ReportPath = '',
    [int] $TimeoutSec = 900,
    # several PLS-POLE instances can run side by side (one model each): -ProcessId picks the instance to drive,
    # Launch -Another starts one more and prints its pid
    [int] $ProcessId = 0,
    [switch] $Another
)
# Drive PLS-POLE 16.81 on host Magese (verified 2026-09-26).
#   Launch                          start PLS-POLE through Explorer (outlives this tool call) and close About / Tip
#   Open -Path m.POL                File > Open (57601)
#   SaveAs -Path m.POL              File > Save As (38014): PLS-POLE writes the model natively (input 21.4)
#   RunStructureFiles -Template 'a-w-%C.%L' -Destination dir [-PolExtension] [-ReportPath r.txt]
#                                   Model > Run (32828) on a model whose General Data analysis option is 2 (Method 1
#                                   file) or 4 (Method 2 file): answers 'Specify File Names for Allowable Spans/
#                                   Interaction Diagrams Structure Files' (template 1021, '.pol extension' 1022,
#                                   destination button 1024 -> folder picker Edit 1152 + Select Folder), waits for
#                                   'Creating Allowable Spans...' to end, copies the results report to -ReportPath.
#                                   %L is the length in the [GLOBAL] UNITS system: set UNITS=1 (SI) for .012 / .014.
#   Exit                            File > Exit (57665)
# Dialog text is read with pls-safe-dump.ps1 only: a WM_GETTEXT to a PLS table control crashed PLS-POLE.
$here = $PSScriptRoot
$ErrorActionPreference = 'Stop'
Add-Type -Namespace PlsPole -Name W -MemberDefinition @'
public delegate bool EnumProc(IntPtr h, IntPtr l);
[DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr p, EnumProc f, IntPtr l);
[DllImport("user32.dll")] public static extern int GetDlgCtrlID(IntPtr h);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, System.Text.StringBuilder s, int n);
[DllImport("user32.dll")] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, string l);
[DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern IntPtr SendMessage(IntPtr h, uint m, IntPtr w, System.Text.StringBuilder l);
[DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
'@
function Proc {
    if ($script:ProcessId) { return Get-Process -Id $script:ProcessId -ErrorAction SilentlyContinue }
    Get-Process pls_pole64 -ErrorAction SilentlyContinue | Select-Object -First 1
}
function Visible { $p = Proc; if (-not $p) { throw 'PLS-POLE is not running' }; & "$here\pls-windows.ps1" -ProcessId $p.Id | Where-Object { $_ -match 'vis=True' } }
function Main { [IntPtr][long](Proc).MainWindowHandle }
function WaitTitle([string]$pattern, [int]$sec) {
    for ($i = 0; $i -lt $sec * 2; $i++) {
        $x = Visible | Where-Object { $_ -match $pattern } | Select-Object -First 1
        if ($x) { return [IntPtr][long]($x -replace ' .*', '') }
        Start-Sleep -Milliseconds 500
    }
    [IntPtr]::Zero
}
function Text([IntPtr]$h) { $sb = New-Object System.Text.StringBuilder 2048; [PlsPole.W]::SendMessage($h, 0x000D, [IntPtr]2048, $sb) | Out-Null; $sb.ToString() }
function Ctl([IntPtr]$root, [int]$id, [string]$class) {
    $script:hit = [IntPtr]::Zero
    $cb = [PlsPole.W+EnumProc] { param($h, $l)
        $c = New-Object System.Text.StringBuilder 32; [PlsPole.W]::GetClassName($h, $c, 32) | Out-Null
        if ($script:hit -eq [IntPtr]::Zero -and [PlsPole.W]::GetDlgCtrlID($h) -eq $id -and $c.ToString() -eq $class) { $script:hit = $h }; $true }
    [PlsPole.W]::EnumChildWindows($root, $cb, [IntPtr]::Zero) | Out-Null
    if ($script:hit -eq [IntPtr]::Zero) { throw "no $class id $id" }
    $script:hit
}
function TypeInto([IntPtr]$edit, [string]$s) {
    [PlsPole.W]::SendMessage($edit, 0x00B1, [IntPtr]0, [IntPtr](-1)) | Out-Null
    foreach ($ch in [int[]][char[]]$s) { [PlsPole.W]::SendMessage($edit, 0x0102, [IntPtr]$ch, [IntPtr]1) | Out-Null }
    if ((Text $edit) -ne $s) { throw "typed '$(Text $edit)'" }
}
function FileDialog([IntPtr]$d, [int]$editId, [string]$path) {
    # Navigate, then name: a full path typed while the dialog shows another folder was resolved against that folder
    # ('Path does not exist'). OK must carry the button handle. Both confirmed on PLS-CADD's dialogs 2026-09-26.
    TypeInto (Ctl $d $editId 'Edit') ([IO.Path]::GetDirectoryName($path))
    [PlsPole.W]::PostMessage($d, 0x0111, [IntPtr]1, (Ctl $d 1 'Button')) | Out-Null
    Start-Sleep 2
    TypeInto (Ctl $d $editId 'Edit') ([IO.Path]::GetFileName($path))
    [PlsPole.W]::PostMessage($d, 0x0111, [IntPtr]1, (Ctl $d 1 'Button')) | Out-Null
}
function Post([IntPtr]$h, [int]$id) { [PlsPole.W]::PostMessage($h, 0x0111, [IntPtr]$id, [IntPtr]0) | Out-Null }
function Idle([int]$sec) {   # main window enabled and no other window: the only 'done' state
    for ($i = 0; $i -lt $sec * 2; $i++) {
        $v = @(Visible)
        if ($v.Count -eq 1 -and $v[0] -match 'en=True') { return $true }
        # Two characterized end-of-run boxes (host Magese 2026-09-26), answered so the run can finish:
        #  - 'File "<component library>" has been changed on disk by another user ...' (a parallel instance or a
        #    regenerated canonical library): Yes (6) = continue the save; regenerate the canonical libraries from
        #    their spec afterwards (PLS rewrites the file in its own format);
        #  - 'Encountered errors, check Analysis Results Report for details!': OK (2); the report (copied to
        #    -ReportPath) names the rays PLS-POLE marked invalid (conservative: no capacity there).
        foreach ($row in @($v | Where-Object { $_ -match "\] 'PLS-POLE'$" })) {
            $h = [long]($row -replace ' .*', '')
            $body = (& "$here\pls-dialog-text.ps1" -WindowHandle $h 2>$null | Where-Object { $_ -match 'id=65535' }) -join ' '
            $answer = if ($body -match 'has been changed on disk by another user') { 6 } elseif ($body -match 'Encountered errors, check Analysis Results Report') { 2 } else { 0 }
            if ($answer) {
                $btn = (& "$here\pls-safe-dump.ps1" -Dialog $h | Where-Object { $_ -match " id=$answer Button" } | Select-Object -First 1) -replace ' .*', ''
                if ($btn) { & "$here\pls-press.ps1" -Button ([long]$btn) | Out-Null; "answered ${answer}: $($body.Substring(0, [Math]::Min(90, $body.Length)))" | Write-Host }
            }
        }
        Start-Sleep -Milliseconds 500
    }
    $false
}

switch ($Action) {
    'Launch' {
        $before = @(Get-Process pls_pole64 -ErrorAction SilentlyContinue | ForEach-Object Id)
        if ($before.Count -and -not $Another) { throw 'PLS-POLE is already running (use -Another for one more instance)' }
        Start-Process explorer.exe -ArgumentList '"C:\Program Files\PLS\pls_pole\pls_pole64.exe"'
        for ($i = 0; $i -lt 60; $i++) {
            $new = Get-Process pls_pole64 -ErrorAction SilentlyContinue | Where-Object { $before -notcontains $_.Id } | Select-Object -First 1
            if ($new -and $new.MainWindowHandle -ne 0) { $script:ProcessId = $new.Id; break }
            Start-Sleep -Milliseconds 500
        }
        if (-not $script:ProcessId) { throw 'the new PLS-POLE instance did not appear' }
        "pid=$($script:ProcessId)"
        for ($i = 0; $i -lt 40; $i++) {
            $box = Visible | Where-Object { $_ -match "\] '(About PLS-POLE|Tip of the Day)'$" } | Select-Object -First 1
            if ($box) { Post ([IntPtr][long]($box -replace ' .*', '')) 1; "closed $($box -replace '.*\] ', '')"; Start-Sleep 1; continue }
            if ((Idle 1) -and $i -gt 6) { break }
        }
        "running: $(Visible)"
    }
    'Open' {
        Post (Main) 57601
        $d = WaitTitle "\] 'Open'$" 15; if ($d -eq [IntPtr]::Zero) { throw "no Open dialog: $(Visible)" }
        FileDialog $d 1148 $Path
        if (-not (Idle 60)) { throw "not idle after open: $(Visible)" }
        "opened: $(Visible)"
    }
    'SaveAs' {
        Post (Main) 38014
        $d = WaitTitle "\] 'Save As'$" 15; if ($d -eq [IntPtr]::Zero) { throw "no Save As dialog: $(Visible)" }
        FileDialog $d 1001 $Path
        $c = WaitTitle "\] 'Confirm Save As'$" 3
        if ($c -ne [IntPtr]::Zero) {
            $script:yes = [IntPtr]::Zero
            $cb = [PlsPole.W+EnumProc] { param($h, $l) if ((Text $h) -eq '&Yes') { $script:yes = $h }; $true }
            [PlsPole.W]::EnumChildWindows($c, $cb, [IntPtr]::Zero) | Out-Null
            [PlsPole.W]::SendMessage($script:yes, 0x00F5, [IntPtr]0, [IntPtr]0) | Out-Null
        }
        if (-not (Idle 30)) { throw "not idle after save: $(Visible)" }
        $f = Get-Item -LiteralPath $Path; "saved $($f.FullName) ($($f.Length) bytes)"
    }
    'RunStructureFiles' {
        New-Item -ItemType Directory -Force $Destination | Out-Null
        Post (Main) 32828
        # Characterized (host Magese 2026-09-26): when the family's .lic was regenerated after PLS-POLE read it, the
        # run first asks 'Loads file "<lic>" has been changed on disk since it was read by this program. Press "Yes" to
        # reread ...'. Yes (6) = reread: the file on disk is the one the run must use.
        $d = WaitTitle "Specify File Names for Allowable Spans" 6
        if ($d -eq [IntPtr]::Zero) {
            foreach ($row in @(Visible | Where-Object { $_ -match "\] 'PLS-POLE'$" })) {
                $h = [long]($row -replace ' .*', '')
                $body = (& "$here\pls-dialog-text.ps1" -WindowHandle $h 2>$null | Where-Object { $_ -match 'id=65535' }) -join ' '
                if ($body -match 'Loads file .* has been changed on disk since it was read by this program') {
                    $btn = (& "$here\pls-safe-dump.ps1" -Dialog $h | Where-Object { $_ -match ' id=6 Button' } | Select-Object -First 1) -replace ' .*', ''
                    if ($btn) { & "$here\pls-press.ps1" -Button ([long]$btn) | Out-Null; "answered 6 (reread loads): $body" | Write-Host }
                }
            }
            $d = WaitTitle "Specify File Names for Allowable Spans" 30
        }
        if ($d -eq [IntPtr]::Zero) { throw "no 'Specify File Names' dialog (is the analysis option 2 or 4?): $(Visible)" }
        $tpl = Ctl $d 1021 'Edit'; [PlsPole.W]::SendMessage($tpl, 0x000C, [IntPtr]0, $Template) | Out-Null
        if ((Text $tpl) -ne $Template) { throw 'template not set' }
        $pol = Ctl $d 1022 'Button'; [PlsPole.W]::SendMessage($pol, 0x00F1, [IntPtr]([int][bool]$PolExtension), [IntPtr]::Zero) | Out-Null
        [PlsPole.W]::PostMessage($d, 0x0111, [IntPtr]1024, (Ctl $d 1024 'Button')) | Out-Null   # destination picker
        $fp = WaitTitle "\] 'Select directory" 15; if ($fp -eq [IntPtr]::Zero) { throw 'no folder picker' }
        TypeInto (Ctl $fp 1152 'Edit') $Destination
        # IDOK must carry the 'Select Folder' button handle: posted with a null lParam the destination did not change
        # and PLS-POLE later exited (2026-09-26)
        # The typed path makes the picker walk folder by folder (leaving the last segment in the box) and the first
        # Select Folder may only close the autocomplete list: press it until the picker closes (2026-09-26).
        for ($i = 0; $i -lt 5 -and (WaitTitle "\] 'Select directory" 1) -ne [IntPtr]::Zero; $i++) {
            [PlsPole.W]::PostMessage($fp, 0x0111, [IntPtr]1, (Ctl $fp 1 'Button')) | Out-Null
            Start-Sleep 2
        }
        if ((WaitTitle "\] 'Select directory" 1) -ne [IntPtr]::Zero) { throw 'folder picker did not close' }
        $dest = Text (Ctl $d 1024 'Button')
        if ($dest.TrimEnd('\') -ne $Destination.TrimEnd('\')) { throw "destination is '$dest'" }
        "template $Template, .pol extension $([bool]$PolExtension), destination $dest"
        Post $d 1
        if (-not (Idle $TimeoutSec)) { throw "analysis did not finish in $TimeoutSec s: $(Visible)" }
        Get-ChildItem -LiteralPath $Destination | Sort-Object LastWriteTime | ForEach-Object { "file $($_.Name) $($_.Length)" }
        if ($ReportPath) {
            Set-Clipboard -Value 'empty'
            [PlsPole.W]::SendMessage((Main), 0x0111, [IntPtr]57642, [IntPtr]0) | Out-Null    # Select All (report view)
            [PlsPole.W]::SendMessage((Main), 0x0111, [IntPtr]57634, [IntPtr]0) | Out-Null    # Copy
            Start-Sleep 1
            $t = Get-Clipboard -Raw
            if ($t -notmatch 'PLS-POLE') { throw 'results report not captured' }
            Set-Content -LiteralPath $ReportPath -Value $t -Encoding UTF8; "report $ReportPath ($($t.Length) chars)"
        }
    }
    'Exit' {
        Post (Main) 57665
        for ($i = 0; $i -lt 20 -and (Proc); $i++) { Start-Sleep -Milliseconds 500 }
        if (Proc) { "still running: $(Visible)" } else { 'PLS-POLE closed' }
    }
}
