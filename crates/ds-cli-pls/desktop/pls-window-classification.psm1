Set-StrictMode -Version 2.0

function Split-PlsWindowRows {
    param(
        [Parameter(Mandatory = $true)][object[]] $Rows,
        [Parameter(Mandatory = $true)][long] $MainWindowHandle
    )
    if ($MainWindowHandle -le 0) { throw 'PLS-CADD main window handle is unavailable' }
    $frame = @($Rows | Where-Object {
        [long](($_ -split ' ')[0]) -eq $MainWindowHandle
    })
    if ($frame.Count -ne 1) {
        throw "Expected one PLS-CADD main frame by handle $MainWindowHandle; found $($frame.Count)"
    }
    return [ordered]@{
        frame = [string] $frame[0]
        others = @($Rows | Where-Object {
            [long](($_ -split ' ')[0]) -ne $MainWindowHandle
        })
    }
}

function Get-PlsMainFrameRow {
    param([AllowEmptyCollection()][object[]] $Rows)
    # MainWindowHandle can point at the startup About dialog. Resolve the frame
    # from this process's enumeration instead; a dialog is never a main frame.
    $frames = @($Rows | Where-Object {
        $_ -match "vis=True .*owner=0 \[(?!#32770\])[^\]]+\] 'PLS-CADD(?: - .*|)'$"
    })
    if ($frames.Count -gt 1) { throw 'Multiple PLS-CADD startup windows: more than one main frame' }
    if ($frames.Count -eq 1) { return [string] $frames[0] }
    return $null
}

function Test-PlsProjectFrameTitle {
    param([string] $Title, [string] $ProjectPath)
    # A leaf title or the launch command line cannot prove the currently open
    # path: the operator may have switched to a same-named copy since launch.
    return $Title -match ('^PLS-CADD - ' + [regex]::Escape($ProjectPath) + '(?: - |$)')
}

function Test-PlsFrameResponsive {
    param([long] $Handle, [int] $ProcessId)
    if (-not ('DsPlsFrame' -as [type])) {
        Add-Type @"
using System; using System.Runtime.InteropServices;
public static class DsPlsFrame {
    [DllImport("user32.dll")] public static extern bool IsWindow(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
    [DllImport("user32.dll")] public static extern bool IsWindowEnabled(IntPtr h);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
    [DllImport("user32.dll")] public static extern IntPtr SendMessageTimeout(IntPtr h, uint msg, IntPtr w, IntPtr l, uint flags, uint timeout, out IntPtr result);
}
"@
    }
    $ownerPid = [uint32]0; $answer = [IntPtr]::Zero
    [void][DsPlsFrame]::GetWindowThreadProcessId([IntPtr]$Handle, [ref]$ownerPid)
    return $ownerPid -eq $ProcessId -and [DsPlsFrame]::IsWindow([IntPtr]$Handle) -and
        [DsPlsFrame]::IsWindowVisible([IntPtr]$Handle) -and [DsPlsFrame]::IsWindowEnabled([IntPtr]$Handle) -and
        [DsPlsFrame]::SendMessageTimeout([IntPtr]$Handle, 0, [IntPtr]::Zero, [IntPtr]::Zero, 2, 1000, [ref]$answer) -ne [IntPtr]::Zero
}

Export-ModuleMember -Function Split-PlsWindowRows, Get-PlsMainFrameRow, Test-PlsProjectFrameTitle, Test-PlsFrameResponsive
