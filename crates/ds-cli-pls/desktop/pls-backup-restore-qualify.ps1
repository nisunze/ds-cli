param(
    [Parameter(Mandatory = $true)][string] $ExecutablePath,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string] $ExpectedExecutableSha256,
    [Parameter(Mandatory = $true)][string] $CandidateBackupPath,
    [Parameter(Mandatory = $true)][ValidatePattern('^[0-9A-Fa-f]{64}$')][string] $ExpectedCandidateBackupSha256,
    [Parameter(Mandatory = $true)][string] $FirstRestoreDirectory,
    [Parameter(Mandatory = $true)][string] $FreshBackupPath,
    [Parameter(Mandatory = $true)][string] $SecondRestoreDirectory,
    [Parameter(Mandatory = $true)][string] $EvidenceDirectory,
    [string] $ProjectFileName,
    [string] $SourceRoot,
    [string] $ProfilePath = (Join-Path $PSScriptRoot 'pls-backup-restore-profile.psd1'),
    [string] $PlsLogPath = (Join-Path $env:APPDATA 'PLS\temp\PLS-CADD.log'),
    [switch] $AuthorizeSaveBeforeBackup,
    [switch] $AcceptUnlicensedSaps,
    [switch] $Execute
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

Import-Module (Join-Path $PSScriptRoot 'pls-backup-restore-lib.psm1') -Force
$profile = Import-PowerShellDataFile -LiteralPath $ProfilePath
Add-Type -AssemblyName System.Windows.Forms

Add-Type @"
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class DsGridBackupRestoreNative {
    public delegate bool EnumWindowsProc(IntPtr hwnd, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern bool EnumWindows(EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll")]
    public static extern bool EnumChildWindows(IntPtr parent, EnumWindowsProc callback, IntPtr parameter);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetWindowText(IntPtr hwnd, StringBuilder text, int count);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern int GetClassName(IntPtr hwnd, StringBuilder text, int count);

    [DllImport("user32.dll")]
    public static extern int GetDlgCtrlID(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern uint GetWindowThreadProcessId(IntPtr hwnd, out uint processId);

    [DllImport("user32.dll")]
    public static extern bool IsWindowVisible(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern bool IsWindowEnabled(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern IntPtr GetDlgItem(IntPtr dialog, int controlId);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wParam, string lParam);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    public static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wParam, StringBuilder lParam);

    [DllImport("user32.dll")]
    public static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll")]
    public static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr SendMessageTimeout(IntPtr hwnd, uint message, IntPtr wParam,
        IntPtr lParam, uint flags, uint timeout, out IntPtr result);

    [DllImport("kernel32.dll")]
    public static extern uint GetCurrentThreadId();

    [DllImport("user32.dll")]
    public static extern IntPtr GetForegroundWindow();

    [DllImport("user32.dll")]
    public static extern bool AttachThreadInput(uint sourceThread, uint targetThread, bool attach);

    [DllImport("user32.dll")]
    public static extern bool SetForegroundWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern bool BringWindowToTop(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern IntPtr SetActiveWindow(IntPtr hwnd);

    [DllImport("user32.dll")]
    public static extern IntPtr SetFocus(IntPtr hwnd);
}
"@

function Get-NativeText([IntPtr] $Handle) {
    $text = New-Object System.Text.StringBuilder 32768
    [DsGridBackupRestoreNative]::GetWindowText($Handle, $text, $text.Capacity) | Out-Null
    return $text.ToString()
}

function Get-NativeClass([IntPtr] $Handle) {
    $text = New-Object System.Text.StringBuilder 512
    [DsGridBackupRestoreNative]::GetClassName($Handle, $text, $text.Capacity) | Out-Null
    return $text.ToString()
}

function Get-ProcessWindows([int] $ProcessId) {
    $windows = New-Object System.Collections.ArrayList
    $callback = [DsGridBackupRestoreNative+EnumWindowsProc] {
        param([IntPtr] $handle, [IntPtr] $parameter)
        $owner = [uint32] 0
        [DsGridBackupRestoreNative]::GetWindowThreadProcessId($handle, [ref] $owner) | Out-Null
        if ($owner -eq $ProcessId -and [DsGridBackupRestoreNative]::IsWindowVisible($handle)) {
            $windows.Add([ordered]@{
                handle = [long] $handle
                title = Get-NativeText $handle
                class = Get-NativeClass $handle
                enabled = [DsGridBackupRestoreNative]::IsWindowEnabled($handle)
            }) | Out-Null
        }
        return $true
    }
    [DsGridBackupRestoreNative]::EnumWindows($callback, [IntPtr]::Zero) | Out-Null
    return @($windows)
}

function Get-DialogChildren([long] $DialogHandle) {
    $children = New-Object System.Collections.ArrayList
    $callback = [DsGridBackupRestoreNative+EnumWindowsProc] {
        param([IntPtr] $handle, [IntPtr] $parameter)
        $children.Add([ordered]@{
            handle = [long] $handle
            id = [DsGridBackupRestoreNative]::GetDlgCtrlID($handle)
            title = Get-NativeText $handle
            class = Get-NativeClass $handle
            visible = [DsGridBackupRestoreNative]::IsWindowVisible($handle)
            enabled = [DsGridBackupRestoreNative]::IsWindowEnabled($handle)
        }) | Out-Null
        return $true
    }
    [DsGridBackupRestoreNative]::EnumChildWindows([IntPtr] $DialogHandle, $callback, [IntPtr]::Zero) | Out-Null
    return @($children)
}

function Get-DialogBody([long] $DialogHandle) {
    $parts = @((Get-DialogChildren $DialogHandle) | Where-Object {
        $_.visible -and $_.title -and $_.class -notin @('Button', 'Edit', 'ComboBox')
    } | ForEach-Object { $_.title.Trim() } | Where-Object { $_ })
    return (($parts -join ' ') -replace '\s+', ' ').Trim()
}

function Get-ExactButtonById {
    param(
        [Parameter(Mandatory = $true)][long] $DialogHandle,
        [Parameter(Mandatory = $true)][int] $ControlId,
        [Parameter(Mandatory = $true)][string[]] $ExpectedTexts
    )

    $handle = [DsGridBackupRestoreNative]::GetDlgItem([IntPtr] $DialogHandle, $ControlId)
    if ($handle -eq [IntPtr]::Zero) { throw "Dialog has no control id $ControlId" }
    $class = Get-NativeClass $handle
    $text = Get-NativeText $handle
    if ($class -cne 'Button' -or -not [DsGridBackupRestoreNative]::IsWindowVisible($handle) -or
        -not [DsGridBackupRestoreNative]::IsWindowEnabled($handle) -or
        $ExpectedTexts -cnotcontains $text) {
        throw "Control id $ControlId is not the expected enabled button (class='$class', text='$text')"
    }
    return [long] $handle
}

function Find-ExactButtonByText {
    param(
        [Parameter(Mandatory = $true)][long] $DialogHandle,
        [Parameter(Mandatory = $true)][string[]] $ExpectedTexts,
        [switch] $AllowMissing
    )

    $matches = @((Get-DialogChildren $DialogHandle) | Where-Object {
        $_.class -ceq 'Button' -and $_.visible -and $_.enabled -and
        $ExpectedTexts -ccontains $_.title
    })
    if ($matches.Count -eq 0 -and $AllowMissing) { return 0L }
    if ($matches.Count -ne 1) {
        throw "Expected exactly one enabled button with text [$($ExpectedTexts -join ', ')]; found $($matches.Count)"
    }
    return [long] $matches[0].handle
}

function Invoke-Button([long] $Handle) {
    if (-not [DsGridBackupRestoreNative]::PostMessage([IntPtr] $Handle, 0x00F5,
            [IntPtr]::Zero, [IntPtr]::Zero)) {
        throw "Failed to post BM_CLICK to validated button handle $Handle"
    }
}

function Invoke-VerifiedRestoreOpen([object] $Dialog, [string] $ExpectedTitle,
    [string] $ExpectedBody) {
    $body = Get-DialogBody $Dialog.handle
    if ($Dialog.title -cne $ExpectedTitle -or $Dialog.class -cne '#32770' -or
        $body -cne $ExpectedBody) {
        throw "Restore completion identity mismatch: title='$($Dialog.title)', body='$body'"
    }
    $yes = @((Get-DialogChildren $Dialog.handle) | Where-Object {
        $_.id -eq 6 -and $_.class -ceq 'Button' -and
        $_.visible -and $_.enabled -and $_.title -ceq '&Yes'
    })
    if ($yes.Count -ne 1) {
        throw "Expected one enabled ID 6 &Yes control; found $($yes.Count)"
    }
    $yesHandle = [IntPtr] ([long] $yes[0].handle)
    if ([DsGridBackupRestoreNative]::GetDlgItem([IntPtr] $Dialog.handle, 6) -ne $yesHandle) {
        throw 'Restore Yes handle does not match dialog control ID 6'
    }
    $result = [IntPtr]::Zero
    $sent = [DsGridBackupRestoreNative]::SendMessageTimeout([IntPtr] $Dialog.handle,
        0x0111, [IntPtr] 6, $yesHandle, 0x0002, 5000, [ref] $result)
    if ($sent -eq [IntPtr]::Zero) {
        throw "IDYES restore command failed: Win32 $([Runtime.InteropServices.Marshal]::GetLastWin32Error())"
    }
}

function Send-PlsCommand([long] $MainWindowHandle, [int] $CommandId) {
    if (-not [DsGridBackupRestoreNative]::PostMessage([IntPtr] $MainWindowHandle, 0x0111,
            [IntPtr] $CommandId, [IntPtr]::Zero)) {
        throw "Failed to post PLS-CADD command $CommandId"
    }
}

function Write-Journal([string] $Event, [object] $Data) {
    $entry = [ordered]@{
        at_utc = [DateTime]::UtcNow.ToString('o')
        event = $Event
        data = $Data
    }
    $line = ($entry | ConvertTo-Json -Depth 20 -Compress) + "`n"
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($line)
    $stream = [System.IO.File]::Open($script:JournalPath, [System.IO.FileMode]::Append,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read)
    try {
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
}

function Get-ModalWindows([System.Diagnostics.Process] $Process, [long] $MainWindowHandle) {
    $windows = @(Get-ProcessWindows $Process.Id)
    $main = @($windows | Where-Object { $_.handle -eq $MainWindowHandle })
    if ($main.Count -ne 1) {
        throw "Expected one visible PLS-CADD main window, found $($main.Count)"
    }
    # PLS keeps modeless diagnostics such as Error Log as separate top-level
    # windows.  They are not a prompt and must not be clicked.  A blocking
    # modal is characterized by a disabled main frame and one enabled dialog;
    # disabled modeless windows are excluded from the modal count.
    if ($main[0].enabled) { return @() }
    return @($windows | Where-Object {
        $_.handle -ne $MainWindowHandle -and $_.enabled
    })
}

function Wait-ForSingleDialog {
    param(
        [Parameter(Mandatory = $true)][System.Diagnostics.Process] $Process,
        [Parameter(Mandatory = $true)][long] $MainWindowHandle,
        [Parameter(Mandatory = $true)][string[]] $ExpectedExactTitles,
        [int] $TimeoutSeconds = 90
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $Process.Refresh()
        if ($Process.HasExited) { throw "PLS-CADD exited unexpectedly with code $($Process.ExitCode)" }
        $windows = @(Get-ModalWindows $Process $MainWindowHandle)
        if ($windows.Count -gt 0) {
            $unexpected = @($windows | Where-Object { $ExpectedExactTitles -cnotcontains $_.title })
            if ($unexpected.Count -ne 0) {
                throw "Unexpected PLS-CADD top-level window(s): $(@($unexpected.title) -join ', ')"
            }
            if ($windows.Count -ne 1) {
                throw "Expected one PLS-CADD dialog, found $($windows.Count): $(@($windows.title) -join ', ')"
            }
            return $windows[0]
        }
        Start-Sleep -Milliseconds 200
    }
    throw "Timed out waiting for dialog title: $($ExpectedExactTitles -join ' or ')"
}

function Set-CommonFileDialogPath {
    param(
        [Parameter(Mandatory = $true)][object] $Dialog,
        [Parameter(Mandatory = $true)][string] $ExpectedExactTitle,
        [Parameter(Mandatory = $true)][string] $Path,
        [Parameter(Mandatory = $true)][string[]] $AcceptTexts
    )

    if ($Dialog.title -cne $ExpectedExactTitle -or $Dialog.class -cne '#32770') {
        throw "Unexpected common-file dialog identity: class='$($Dialog.class)', title='$($Dialog.title)'"
    }
    $children = @(Get-DialogChildren $Dialog.handle)
    $edits = @($children | Where-Object {
        $_.id -eq [int] $profile.Controls.CommonFileName -and
        $_.class -ceq 'Edit' -and $_.visible -and $_.enabled
    })
    # The modern (IFileDialog) Save picker has no 1148 edit: its filename field is the Edit
    # with id 1001 (the Address bar also carries 1001 but is a ToolbarWindow32, not an Edit).
    # Seen on DESKTOP-T24AHBE 2026-09-24 for 'Backup'; catalogue entry backup_file.
    if ($edits.Count -eq 0) {
        $edits = @($children | Where-Object {
            $_.id -eq 1001 -and $_.class -ceq 'Edit' -and $_.visible -and $_.enabled
        })
    }
    if ($edits.Count -ne 1) {
        throw "Common-file dialog must expose exactly one enabled filename Edit with id $($profile.Controls.CommonFileName); found $($edits.Count)"
    }
    $editHandle = [IntPtr] ([long] $edits[0].handle)
    [DsGridBackupRestoreNative]::SendMessage($editHandle, 0x000C,
        [IntPtr]::Zero, $Path) | Out-Null
    Start-Sleep -Milliseconds 250
    $value = New-Object System.Text.StringBuilder 32768
    [DsGridBackupRestoreNative]::SendMessage($editHandle, 0x000D,
        [IntPtr] $value.Capacity, $value) | Out-Null
    $observed = $value.ToString()
    if (-not $observed.Equals($Path, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Common-file dialog path verification failed: expected '$Path', got '$observed'"
    }
    $accept = Get-ExactButtonById $Dialog.handle ([int] $profile.Controls.Accept) $AcceptTexts
    Invoke-Button $accept
    Write-Journal 'common_file_dialog_submitted' ([ordered]@{ title = $Dialog.title; path = $Path })
}

function Select-FreshRestoreDirectory {
    param(
        [Parameter(Mandatory = $true)][object] $Dialog,
        [Parameter(Mandatory = $true)][string] $Directory
    )

    if ($Dialog.title -cne $profile.DialogTitles.RestoreDirectory -or $Dialog.class -cne '#32770') {
        throw "Unexpected restore-directory dialog identity: class='$($Dialog.class)', title='$($Dialog.title)'"
    }
    if (Test-Path -LiteralPath $Directory) {
        throw "Fresh restore directory appeared before selection: $Directory"
    }
    [System.IO.Directory]::CreateDirectory($Directory) | Out-Null
    # On the Google Drive mount (G:) a folder just created through the file system can stay
    # invisible to the Windows shell for a moment: the picker's address bar then answers
    # "Windows can't find '<path>'" (a 'File Explorer' box owned by the picker) and stays where
    # it was (2026-09-25, v19 cap6 proof run 3). Wait until the shell resolves the new folder,
    # and after such a box close it and type the path again (at most 3 attempts).
    $shell = New-Object -ComObject Shell.Application
    $waited = 0
    while (-not $shell.NameSpace($Directory) -and $waited -lt 30000) { Start-Sleep -Milliseconds 500; $waited += 500 }
    if (-not $shell.NameSpace($Directory)) { throw "The shell cannot resolve the fresh restore directory after 30 s: $Directory" }
    Write-Journal 'fresh_restore_directory_shell_visible' ([ordered]@{ path = $Directory; waited_ms = $waited })

    # Control 1152 is only the shell dialog's Folder *name* field. Setting it
    # does not navigate and caused PLS to stage a restore against the current
    # Examples directory. Navigate through the shell address bar instead, then
    # authenticate the dialog's actual address toolbar. This pinned modern
    # folder picker returns an empty CDM_GETFOLDERPATH even after successful
    # navigation, so that legacy message is not evidence of its selection.
    $dialogHandle = [IntPtr] ([long] $Dialog.handle)
    $targetPid = [uint32] 0
    $targetThread = [DsGridBackupRestoreNative]::GetWindowThreadProcessId(
        $dialogHandle, [ref] $targetPid)
    $currentThread = [DsGridBackupRestoreNative]::GetCurrentThreadId()
    $expectedAddress = "Address: $Directory"
    $observed = @()
    for ($attempt = 1; $attempt -le 3 -and $observed.Count -ne 1; $attempt++) {
        $foregroundPid = [uint32] 0
        $foregroundThread = [DsGridBackupRestoreNative]::GetWindowThreadProcessId(
            [DsGridBackupRestoreNative]::GetForegroundWindow(), [ref] $foregroundPid)
        try {
            if ($foregroundThread -ne 0 -and $foregroundThread -ne $currentThread) {
                [DsGridBackupRestoreNative]::AttachThreadInput(
                    $currentThread, $foregroundThread, $true) | Out-Null
            }
            if ($targetThread -ne 0 -and $targetThread -ne $currentThread) {
                [DsGridBackupRestoreNative]::AttachThreadInput(
                    $currentThread, $targetThread, $true) | Out-Null
            }
            [DsGridBackupRestoreNative]::SetForegroundWindow($dialogHandle) | Out-Null
            [DsGridBackupRestoreNative]::BringWindowToTop($dialogHandle) | Out-Null
            [DsGridBackupRestoreNative]::SetActiveWindow($dialogHandle) | Out-Null
            [DsGridBackupRestoreNative]::SetFocus($dialogHandle) | Out-Null
            Start-Sleep -Milliseconds 250
            [System.Windows.Forms.SendKeys]::SendWait('^l')
            Start-Sleep -Milliseconds 150
            [System.Windows.Forms.SendKeys]::SendWait($Directory)
            [System.Windows.Forms.SendKeys]::SendWait('{ENTER}')
        } finally {
            if ($targetThread -ne 0 -and $targetThread -ne $currentThread) {
                [DsGridBackupRestoreNative]::AttachThreadInput(
                    $currentThread, $targetThread, $false) | Out-Null
            }
            if ($foregroundThread -ne 0 -and $foregroundThread -ne $currentThread) {
                [DsGridBackupRestoreNative]::AttachThreadInput(
                    $currentThread, $foregroundThread, $false) | Out-Null
            }
        }
        $notFound = $null
        $deadline = [DateTime]::UtcNow.AddSeconds([int] $profile.DialogTimeoutSeconds)
        do {
            Start-Sleep -Milliseconds 200
            $observed = @((Get-DialogChildren $Dialog.handle) | Where-Object {
                $_.id -eq 1001 -and $_.class -ceq 'ToolbarWindow32' -and
                $_.visible -and $_.enabled -and
                ([string] $_.title).Equals($expectedAddress,
                    [System.StringComparison]::OrdinalIgnoreCase)
            })
            $notFound = @(Get-ProcessWindows ([int] $targetPid) | Where-Object { $_.title -ceq 'File Explorer' }) | Select-Object -First 1
        } while ($observed.Count -ne 1 -and -not $notFound -and
            [DateTime]::UtcNow -lt $deadline)
        if ($notFound -and $observed.Count -ne 1) {
            Write-Journal 'restore_directory_shell_not_found' ([ordered]@{ path = $Directory; attempt = $attempt })
            $box = [IntPtr] ([long] $notFound.handle)
            [DsGridBackupRestoreNative]::PostMessage($box, 0x0010, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null   # WM_CLOSE
            for ($i = 0; $i -lt 20 -and [DsGridBackupRestoreNative]::IsWindowVisible($box); $i++) { Start-Sleep -Milliseconds 250 }
            if ([DsGridBackupRestoreNative]::IsWindowVisible($box)) { throw "The shell's 'File Explorer' box did not close" }
            Start-Sleep -Seconds 2
        }
    }
    if ($observed.Count -ne 1) {
        $addresses = @((Get-DialogChildren $Dialog.handle) | Where-Object {
            $_.class -ceq 'ToolbarWindow32' -and $_.title -like 'Address:*'
        } | ForEach-Object { $_.title })
        throw "Restore-directory navigation verification failed: expected '$expectedAddress', found [$($addresses -join '; ')]"
    }
    $accept = Get-ExactButtonById $Dialog.handle ([int] $profile.Controls.Accept) `
        @($profile.ExactButtons.RestoreSelectDirectory)
    Invoke-Button $accept
    # the picker stays visible (children already gone) for a moment after Select Folder; the
    # restore loop must not read it as a second request (2026-09-25, v19 cap6 proof run 4)
    for ($i = 0; $i -lt 40 -and [DsGridBackupRestoreNative]::IsWindowVisible($dialogHandle); $i++) { Start-Sleep -Milliseconds 250 }
    if ([DsGridBackupRestoreNative]::IsWindowVisible($dialogHandle)) { throw "Restore-directory picker still open 10 s after Select Folder: $Directory" }
    Write-Journal 'fresh_restore_directory_selected' ([ordered]@{ path = $Directory })
}

function Set-RestoreCommonPath {
    param(
        [Parameter(Mandatory = $true)][object] $Dialog,
        [Parameter(Mandatory = $true)][string] $ExpectedCommonPath
    )
    $edits = @((Get-DialogChildren $Dialog.handle) | Where-Object {
        $_.id -eq [int] $profile.Controls.RestoreCommonPath -and
        $_.class -ceq 'Edit' -and $_.visible -and $_.enabled
    })
    if ($edits.Count -ne 1) {
        throw "Common-path dialog must expose exactly one enabled Edit with id $($profile.Controls.RestoreCommonPath); found $($edits.Count)"
    }
    $edit = [IntPtr] ([long] $edits[0].handle)
    [DsGridBackupRestoreNative]::SendMessage($edit, 0x000C, [IntPtr]::Zero,
        $ExpectedCommonPath) | Out-Null
    $value = New-Object System.Text.StringBuilder 32768
    [DsGridBackupRestoreNative]::SendMessage($edit, 0x000D,
        [IntPtr] $value.Capacity, $value) | Out-Null
    if (-not $value.ToString().Equals($ExpectedCommonPath,
            [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "Restore common-path readback mismatch: expected '$ExpectedCommonPath', got '$($value.ToString())'"
    }
    $newPath = Get-ExactButtonById $Dialog.handle ([int] $profile.Controls.RestoreNewPath) `
        @($profile.ExactButtons.RestoreNewPath)
    Invoke-Button $newPath
}

function Invoke-KnownOpenPrompt {
    param(
        [Parameter(Mandatory = $true)][object] $Dialog,
        [AllowEmptyCollection()][Parameter(Mandatory = $true)]
        [System.Collections.ArrayList] $PromptEvidence
    )

    $body = Get-DialogBody $Dialog.handle
    foreach ($rule in $profile.AllowedOpenPrompts) {
        if ($Dialog.title -ceq $rule.Title -and $body -match $rule.BodyPattern) {
            $button = Get-ExactButtonById $Dialog.handle ([int] $rule.ResponseControlId) @($rule.ResponseTexts)
            $PromptEvidence.Add([ordered]@{ name = $rule.Name; title = $Dialog.title; body = $body }) | Out-Null
            Invoke-Button $button
            Write-Journal 'known_open_prompt_handled' ([ordered]@{ name = $rule.Name; title = $Dialog.title; body = $body })
            return $true
        }
    }
    return $false
}

function Start-PlsBare {
    param([string] $Executable, [int] $TimeoutSeconds)

    $process = Start-Process -FilePath $Executable -WorkingDirectory ([System.IO.Path]::GetDirectoryName($Executable)) -PassThru
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    do {
        Start-Sleep -Milliseconds 250
        $process.Refresh()
        if ($process.HasExited) { throw "PLS-CADD exited during startup with code $($process.ExitCode)" }
    } while ($process.MainWindowHandle -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)
    if ($process.MainWindowHandle -eq [IntPtr]::Zero) {
        throw "PLS-CADD did not expose a main window within $TimeoutSeconds seconds"
    }
    $title = Get-NativeText $process.MainWindowHandle
    if ($title -notmatch '^PLS-CADD(?:\s|$)') {
        throw "Unexpected PLS-CADD main-window title: '$title'"
    }
    Write-Journal 'pls_started' ([ordered]@{
        process_id = $process.Id
        main_window_handle = [long] $process.MainWindowHandle
        main_window_title = $title
    })
    return $process
}

function Dismiss-KnownStartupPrompts {
    param(
        [Parameter(Mandatory = $true)][System.Diagnostics.Process] $Process,
        [Parameter(Mandatory = $true)][long] $MainWindowHandle,
        [AllowEmptyCollection()][Parameter(Mandatory = $true)]
        [System.Collections.ArrayList] $PromptEvidence
    )

    $deadline = [DateTime]::UtcNow.AddSeconds([int] $profile.StartupTimeoutSeconds)
    $quietSince = $null
    while ([DateTime]::UtcNow -lt $deadline) {
        $Process.Refresh()
        if ($Process.HasExited) { throw "PLS-CADD exited while handling startup prompts" }
        $windows = @(Get-ModalWindows $Process $MainWindowHandle)
        if ($windows.Count -eq 0) {
            if ($null -eq $quietSince) { $quietSince = [DateTime]::UtcNow }
            if (([DateTime]::UtcNow - $quietSince).TotalSeconds -ge 1) { return }
            Start-Sleep -Milliseconds 150
            continue
        }
        $quietSince = $null
        if ($windows.Count -ne 1) {
            throw "Multiple PLS-CADD startup windows: $(@($windows.title) -join ', ')"
        }
        $dialog = $windows[0]
        $body = Get-DialogBody $dialog.handle
        $rules = @($profile.AllowedStartupPrompts | Where-Object {
            $dialog.title -ceq $_.Title -and $body -match $_.BodyPattern
        })
        if ($rules.Count -ne 1) {
            throw "Unexpected PLS-CADD startup window: title='$($dialog.title)', body='$body'"
        }
        $rule = $rules[0]
        $button = Get-ExactButtonById $dialog.handle ([int] $rule.ResponseControlId) @($rule.ResponseTexts)
        $PromptEvidence.Add([ordered]@{
            phase = 'startup'
            name = $rule.Name
            title = $dialog.title
            body = $body
        }) | Out-Null
        Invoke-Button $button
        Write-Journal 'known_startup_prompt_handled' ([ordered]@{
            name = $rule.Name
            title = $dialog.title
            body = $body
        })
        Start-Sleep -Milliseconds 200
    }
    throw "PLS-CADD startup prompts did not settle within $($profile.StartupTimeoutSeconds) seconds"
}

function Invoke-PlsRestore {
    param(
        [Parameter(Mandatory = $true)][string] $Executable,
        [Parameter(Mandatory = $true)][string] $NativeBackup,
        [Parameter(Mandatory = $true)][object] $Inventory,
        [Parameter(Mandatory = $true)][string] $RestoreDirectory,
        [AllowEmptyCollection()][Parameter(Mandatory = $true)]
        [System.Collections.ArrayList] $PromptEvidence,
        [AllowEmptyCollection()][Parameter(Mandatory = $true)]
        [System.Collections.ArrayList] $RepairEvidence
    )

    if (Test-Path -LiteralPath $RestoreDirectory) {
        throw "Restore target must not exist: $RestoreDirectory"
    }
    $process = Start-PlsBare $Executable ([int] $profile.StartupTimeoutSeconds)
    $main = [long] $process.MainWindowHandle
    Dismiss-KnownStartupPrompts $process $main $PromptEvidence
    $existing = @(Get-ModalWindows $process $main)
    if ($existing.Count -ne 0) {
        throw "PLS-CADD started with unexpected top-level window(s): $(@($existing.title) -join ', ')"
    }
    Send-PlsCommand $main ([int] $profile.Commands.Restore)
    $fileDialog = Wait-ForSingleDialog $process $main @([string] $profile.DialogTitles.RestoreFile) ([int] $profile.DialogTimeoutSeconds)
    Set-CommonFileDialogPath $fileDialog ([string] $profile.DialogTitles.RestoreFile) $NativeBackup @('Open', '&Open', 'OK', '&OK')

    $destinationSelected = $false
    $destinationPickerRequested = $false
    $commonDialogRequested = $false
    $commonDialogAccepted = $false
    $mappingAccepted = $false
    $openAccepted = $false
    $preOpenPresence = $null
    $openDeadline = $null
    $nativeBackupLeaf = [System.IO.Path]::GetFileName($NativeBackup)
    $expectedScanBody = "Scanning '$nativeBackupLeaf'..."
    $expectedCompletionTitle = "Restore Backup of $nativeBackupLeaf"
    $expectedRestoredFileCount = [int] $Inventory.counts.files
    $expectedProjectFile = [string] $Inventory.project_file
    $expectedProjectLeaf = [System.IO.Path]::GetFileName($expectedProjectFile)
    $expectedProjectPath = [System.IO.Path]::GetFullPath(
        (Join-Path $RestoreDirectory $expectedProjectFile))
    $expectedCompletionBody = "$expectedRestoredFileCount files restored, 0 files skipped Would you like to open project '$expectedProjectPath'?"
    $restoreMemberDisplays = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::OrdinalIgnoreCase)
    foreach ($member in @($Inventory.members | Where-Object { $_.kind -ne 'directory' })) {
        $relative = [string] $member.relative_path
        $display = [System.IO.Path]::GetFullPath((Join-Path $RestoreDirectory $relative))
        if ([string]::IsNullOrWhiteSpace($display)) {
            throw "Candidate inventory member has no restorable leaf: '$relative'"
        }
        if (-not $restoreMemberDisplays.Add($display)) {
            throw "Candidate inventory members collide in PLS restore-progress display: '$display'"
        }
    }
    $actualInventoryFileCount = @($Inventory.members | Where-Object { $_.kind -ne 'directory' }).Count
    if ($expectedRestoredFileCount -ne $actualInventoryFileCount) {
        throw "Candidate inventory file count is inconsistent: declared $expectedRestoredFileCount, found $actualInventoryFileCount members"
    }
    $repairCount = 0
    $quietSince = $null
    $deadline = [DateTime]::UtcNow.AddSeconds([int] $profile.BackupTimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $process.Refresh()
        if ($process.HasExited) { throw "PLS-CADD exited during restore with code $($process.ExitCode)" }
        $windows = @(Get-ModalWindows $process $main)
        if ($windows.Count -gt 1) {
            throw "Multiple PLS-CADD top-level windows during restore: $(@($windows.title) -join ', ')"
        }
        if ($windows.Count -eq 0) {
            if ($destinationSelected -and $openAccepted) {
                if ($null -eq $quietSince) { $quietSince = [DateTime]::UtcNow }
                $project = Join-Path $RestoreDirectory ([string] $Inventory.project_file)
                if ($null -ne $openDeadline -and [DateTime]::UtcNow -ge $openDeadline) {
                    throw "Restored project did not open in the PLS-CADD frame: $($process.MainWindowTitle)"
                }
                if (([DateTime]::UtcNow - $quietSince).TotalSeconds -ge 3 -and
                    (Test-Path -LiteralPath $project -PathType Leaf) -and
                    $process.MainWindowTitle.Contains($expectedProjectLeaf)) {
                    Write-Journal 'restore_open_quiet' ([ordered]@{ restore_directory = $RestoreDirectory; project = $project })
                    if ($null -eq $preOpenPresence) {
                        throw 'Restore completed without a pre-open tree identity check'
                    }
                    return [ordered]@{
                        process = $process
                        main_window_handle = $main
                        project_path = $project
                        pre_open_presence_verification = $preOpenPresence
                    }
                }
            }
            Start-Sleep -Milliseconds 200
            continue
        }
        $quietSince = $null
        $dialog = $windows[0]
        $body = Get-DialogBody $dialog.handle
        Write-Journal 'restore_dialog_observed' ([ordered]@{ title = $dialog.title; class = $dialog.class; body = $body })

        # A common dialog can remain visible briefly after its synchronous
        # button click. Do not reinterpret that same handle as the next
        # Restore Backup dialog; wait for it to be destroyed.
        if ($dialog.handle -eq $fileDialog.handle) {
            Start-Sleep -Milliseconds 100
            continue
        }
        # PLS briefly replaces the common picker with a distinct, empty
        # `Restore Backup` progress dialog while parsing the selected native
        # stream. It has no operator decision and must disappear into the
        # mapping dialog; observe it without clicking under the overall
        # restore timeout.
        if (-not $destinationSelected -and -not $mappingAccepted -and
            $dialog.title -ceq $profile.DialogTitles.RestoreFile -and
            $dialog.class -ceq '#32770' -and
            ($body.Length -eq 0 -or $body -ceq $expectedScanBody)) {
            Start-Sleep -Milliseconds 100
            continue
        }
        # Once mapping is accepted, PLS reports each member being restored in
        # the same no-action dialog. The fixed framing and inventory-derived
        # leaf allowlist prevent an unrelated prompt from being treated as
        # harmless progress.
        if ($destinationSelected -and $mappingAccepted -and
            $dialog.title -ceq $profile.DialogTitles.RestoreFile -and
            $dialog.class -ceq '#32770' -and
            $body.Length -eq 0) {
            Start-Sleep -Milliseconds 100
            continue
        }
        if ($destinationSelected -and $mappingAccepted -and
            $dialog.title -ceq $profile.DialogTitles.RestoreFile -and
            $dialog.class -ceq '#32770' -and
            $body -cmatch '^Restoring "([^"]+)"$') {
            $restoringLeaf = $Matches[1]
            if (-not $restoreMemberDisplays.Contains($restoringLeaf)) {
                throw "Restore progress referenced a member absent from the candidate inventory: '$restoringLeaf'"
            }
            Write-Journal 'restore_member_progress_observed' ([ordered]@{ member_leaf = $restoringLeaf })
            Start-Sleep -Milliseconds 100
            continue
        }
        # Observed 2026-09-24 (Nyamagabe v17cap4-r2, Drive restore target): PLS can re-show
        # the exact-filename scan box after mapping is accepted, before member progress.
        # Same no-action parsing dialog as above: observe it, never click.
        if ($destinationSelected -and $mappingAccepted -and -not $openAccepted -and
            $dialog.title -ceq $profile.DialogTitles.RestoreFile -and
            $dialog.class -ceq '#32770' -and
            $body -ceq $expectedScanBody) {
            Write-Journal 'restore_rescan_observed' ([ordered]@{ body = $body })
            Start-Sleep -Milliseconds 100
            continue
        }

        # PLS briefly reports completion before the inventory-checked open-project decision.
        # This progress dialog has no operator action.
        if ($destinationSelected -and $mappingAccepted -and -not $openAccepted -and
            $dialog.title -ceq $profile.DialogTitles.RestoreFile -and
            $dialog.class -ceq '#32770' -and
            $body -ceq 'Restore complete') {
            Start-Sleep -Milliseconds 100
            continue
        }

        if ($dialog.title -ceq $profile.DialogTitles.RestoreDirectory) {
            if ($destinationSelected) { throw 'Restore requested its destination directory more than once' }
            if (-not $destinationPickerRequested) {
                throw 'Restore directory picker appeared before the governed mapping control was invoked'
            }
            Select-FreshRestoreDirectory $dialog $RestoreDirectory
            $destinationSelected = $true
            continue
        }
        if ($dialog.title -ceq $profile.DialogTitles.RestoreCommonPath) {
            if (-not $commonDialogRequested) {
                throw 'Common-path dialog appeared before its governed mapping control was invoked'
            }
            if ($commonDialogAccepted) { throw 'Restore common-path dialog returned after acceptance' }
            if (-not $destinationPickerRequested) {
                Set-RestoreCommonPath $dialog ([string] $Inventory.project_source_root)
                $destinationPickerRequested = $true
                Write-Journal 'restore_common_path_authenticated' ([ordered]@{
                    source_root = $Inventory.project_source_root
                })
                continue
            }
            if (-not $destinationSelected) {
                Start-Sleep -Milliseconds 100
                continue
            }
            $accept = Get-ExactButtonById $dialog.handle ([int] $profile.Controls.Accept) @('OK', '&OK')
            Invoke-Button $accept
            $commonDialogAccepted = $true
            Write-Journal 'restore_common_path_accepted' ([ordered]@{ destination = $RestoreDirectory })
            continue
        }
        if ($dialog.title -ceq $profile.DialogTitles.RestoreMapping) {
            $pick = Find-ExactButtonByText $dialog.handle @($profile.ExactButtons.RestorePickDirectory) -AllowMissing
            if (-not $destinationSelected -and -not $destinationPickerRequested -and $pick -ne 0) {
                Invoke-Button $pick
                $commonDialogRequested = $true
                Write-Journal 'restore_pick_directory_clicked' ([ordered]@{ title = $dialog.title })
                continue
            }
            if (-not $destinationSelected) {
                if ($commonDialogRequested) {
                    Start-Sleep -Milliseconds 100
                    continue
                }
                throw 'Restore mapping dialog did not expose the exact common-directory control'
            }
            if (-not $commonDialogAccepted) {
                Start-Sleep -Milliseconds 100
                continue
            }
            if ($mappingAccepted) { throw 'Restore mapping dialog returned after it was accepted' }
            $accept = Get-ExactButtonById $dialog.handle ([int] $profile.Controls.Accept) @('OK', '&OK')
            Invoke-Button $accept
            $mappingAccepted = $true
            Write-Journal 'restore_mapping_accepted' ([ordered]@{ destination = $RestoreDirectory })
            continue
        }
        if ($dialog.title -ceq $expectedCompletionTitle -and $body -ceq $expectedCompletionBody) {
            if (-not $destinationSelected) { throw 'PLS-CADD offered to open a project before the fresh destination was selected' }
            if (-not $mappingAccepted) { throw 'PLS-CADD offered to open a project before directory mapping was accepted' }
            if ($null -ne $preOpenPresence) { throw 'PLS-CADD offered to open the restored project more than once' }
            # Restore can rewrite text line endings and embedded paths before
            # opening. Here only the inventory's paths and file count are checked.
            $preOpenPresence = Test-PlsRestoredTree $Inventory $RestoreDirectory 'Full' @($profile.AllowedFreshRestoreExtras) -PresenceOnly
            if ([int] $preOpenPresence.verified_files -ne $expectedRestoredFileCount) {
                throw "Pre-open restore count mismatch: expected $expectedRestoredFileCount, verified $($preOpenPresence.verified_files)"
            }
            Write-Journal 'restore_pre_open_tree_identity_verified' $preOpenPresence
            Invoke-VerifiedRestoreOpen $dialog $expectedCompletionTitle $expectedCompletionBody
            $openAccepted = $true
            $openDeadline = [DateTime]::UtcNow.AddSeconds([int] $profile.DialogTimeoutSeconds)
            Write-Journal 'restored_project_open_accepted' ([ordered]@{ body = $body })
            continue
        }
        if ($dialog.title -match $profile.RepairTitlePattern) {
            throw "Restored project requires repair and is not eligible for final backup: title='$($dialog.title)', body='$body'"
        }
        if (Invoke-KnownOpenPrompt $dialog $PromptEvidence) { continue }
        if ($dialog.title -ceq $profile.DialogTitles.RestoreReport) {
            $accept = Find-ExactButtonByText $dialog.handle @('OK', '&OK', 'Close', '&Close') -AllowMissing
            if ($accept -eq 0) {
                throw 'Restore Backup Report did not expose an exact OK/Close button'
            }
            Invoke-Button $accept
            Write-Journal 'restore_report_closed' ([ordered]@{ body = $body })
            continue
        }
        throw "Unexpected restore/open dialog: title='$($dialog.title)', body='$body'"
    }
    throw "Restore/open did not complete within $($profile.BackupTimeoutSeconds) seconds"
}

function Set-SafeBackupOptions([object] $Dialog) {
    if ($Dialog.title -cne $profile.DialogTitles.BackupOptions -or $Dialog.class -cne '#32770') {
        throw "Unexpected Backup Options dialog identity: class='$($Dialog.class)', title='$($Dialog.title)'"
    }
    $controls = @((Get-DialogChildren $Dialog.handle) | Where-Object { $_.class -ceq 'Button' -and $_.title })
    $observed = New-Object System.Collections.ArrayList
    foreach ($control in $controls) {
        $state = [long] [DsGridBackupRestoreNative]::SendMessage([IntPtr] ([long] $control.handle),
            0x00F0, [IntPtr]::Zero, [IntPtr]::Zero)
        $dangerous = $false
        foreach ($pattern in $profile.DangerousBackupOptionTextPatterns) {
            if ($control.title -match $pattern) { $dangerous = $true; break }
        }
        if ($dangerous -and $state -ne 0) {
            [DsGridBackupRestoreNative]::SendMessage([IntPtr] ([long] $control.handle),
                0x00F1, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null
            $verified = [long] [DsGridBackupRestoreNative]::SendMessage([IntPtr] ([long] $control.handle),
                0x00F0, [IntPtr]::Zero, [IntPtr]::Zero)
            if ($verified -ne 0) { throw "Could not disable dangerous backup option '$($control.title)'" }
            $state = $verified
        }
        $observed.Add([ordered]@{ id = $control.id; text = $control.title; check_state = $state; dangerous = $dangerous }) | Out-Null
    }
    $accept = Get-ExactButtonById $Dialog.handle ([int] $profile.Controls.Accept) @('OK', '&OK')
    Invoke-Button $accept
    Write-Journal 'backup_options_accepted' ([ordered]@{ controls = @($observed) })
    return @($observed)
}

function Invoke-SavePromptResponse {
    param([object] $Dialog, [bool] $Authorize)

    $body = Get-DialogBody $Dialog.handle
    if ($Dialog.title -cne $profile.DialogTitles.Product -or $body -notmatch $profile.SaveBeforeBackupBodyPattern) {
        throw "Unexpected save-before-backup prompt: title='$($Dialog.title)', body='$body'"
    }
    if ($Authorize) {
        $button = Find-ExactButtonByText $Dialog.handle @($profile.ExactButtons.Yes + @('OK', '&OK'))
        $response = 'save_authorized'
    } else {
        $button = Find-ExactButtonByText $Dialog.handle @($profile.ExactButtons.No + $profile.ExactButtons.Cancel)
        $response = 'save_declined'
    }
    Invoke-Button $button
    Write-Journal 'save_before_backup_prompt' ([ordered]@{ body = $body; response = $response })
}

function Wait-StableRegularFile([string] $Path, [int] $TimeoutSeconds) {
    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $lastLength = -1L
    $stable = 0
    while ([DateTime]::UtcNow -lt $deadline) {
        if (Test-Path -LiteralPath $Path -PathType Leaf) {
            $item = Get-Item -LiteralPath $Path -Force
            if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Output backup became a reparse point: $Path"
            }
            if ($item.Length -gt 0 -and $item.Length -eq $lastLength) { $stable++ } else { $stable = 0 }
            $lastLength = $item.Length
            if ($stable -ge 4) {
                $stream = [System.IO.File]::Open($item.FullName, [System.IO.FileMode]::Open,
                    [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
                $stream.Dispose()
                return $item.FullName
            }
        }
        Start-Sleep -Milliseconds 500
    }
    throw "Backup output did not become stable within $TimeoutSeconds seconds: $Path"
}

function Invoke-PlsBackup {
    param(
        [Parameter(Mandatory = $true)][System.Diagnostics.Process] $Process,
        [Parameter(Mandatory = $true)][long] $MainWindowHandle,
        [Parameter(Mandatory = $true)][string] $OutputPath,
        [Parameter(Mandatory = $true)][bool] $AuthorizeSave
    )

    if (Test-Path -LiteralPath $OutputPath) { throw "Fresh backup output already exists: $OutputPath" }
    Send-PlsCommand $MainWindowHandle ([int] $profile.Commands.Backup)
    $fileSubmitted = $false
    $optionsAccepted = $false
    $completionObserved = $false
    $savePromptObserved = $false
    $backupFileDialogHandle = 0L
    $deadline = [DateTime]::UtcNow.AddSeconds([int] $profile.BackupTimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $Process.Refresh()
        if ($Process.HasExited) { throw "PLS-CADD exited during backup with code $($Process.ExitCode)" }
        $windows = @(Get-ModalWindows $Process $MainWindowHandle)
        if ($windows.Count -gt 1) {
            throw "Multiple PLS-CADD top-level windows during backup: $(@($windows.title) -join ', ')"
        }
        if ($windows.Count -eq 0) {
            if ($fileSubmitted -and $optionsAccepted -and (Test-Path -LiteralPath $OutputPath -PathType Leaf)) {
                Wait-StableRegularFile $OutputPath ([int] $profile.BackupTimeoutSeconds) | Out-Null
                Write-Journal 'fresh_backup_stable' ([ordered]@{ path = $OutputPath; completion_prompt_observed = $completionObserved })
                return [ordered]@{
                    save_prompt_observed = $savePromptObserved
                    backup_options_observed = $optionsAccepted
                    completion_prompt_observed = $completionObserved
                }
            }
            Start-Sleep -Milliseconds 200
            continue
        }
        $dialog = $windows[0]
        $body = Get-DialogBody $dialog.handle
        Write-Journal 'backup_dialog_observed' ([ordered]@{ title = $dialog.title; class = $dialog.class; body = $body })
        if ($fileSubmitted -and $dialog.handle -eq $backupFileDialogHandle) {
            Start-Sleep -Milliseconds 100
            continue
        }
        if ($dialog.title -ceq $profile.DialogTitles.Product -and $body -match $profile.SaveBeforeBackupBodyPattern) {
            if ($savePromptObserved) { throw 'Save-before-backup prompt appeared more than once' }
            Invoke-SavePromptResponse $dialog $AuthorizeSave
            $savePromptObserved = $true
            continue
        }
        if ($dialog.title -ceq $profile.DialogTitles.BackupOptions) {
            if ($optionsAccepted) { throw 'Backup Options appeared more than once' }
            Set-SafeBackupOptions $dialog | Out-Null
            $optionsAccepted = $true
            continue
        }
        # This installation reports completion in a box titled 'Backup' ("N files backed up
        # from M project"), OK id 2 - catalogue backup_done, Rutsiro heal run 2026-08-18.
        # Invoked with WM_COMMAND like the restore Yes (C3c), not a posted BM_CLICK.
        if ($fileSubmitted -and $optionsAccepted -and $dialog.title -ceq $profile.DialogTitles.BackupFile -and
            $body -match '^\d+ files backed up from \d+ projects?$') {
            $accept = Get-ExactButtonById $dialog.handle 2 @('OK', '&OK')
            $result = [IntPtr]::Zero
            $sent = [DsGridBackupRestoreNative]::SendMessageTimeout([IntPtr] $dialog.handle,
                0x0111, [IntPtr] 2, [IntPtr] $accept, 0x0002, 5000, [ref] $result)
            if ($sent -eq [IntPtr]::Zero) { throw 'Backup completion OK command failed' }
            $completionObserved = $true
            Write-Journal 'backup_completion_prompt' ([ordered]@{ title = $dialog.title; body = $body })
            continue
        }
        if ($dialog.title -ceq $profile.DialogTitles.BackupFile) {
            if ($fileSubmitted) { throw 'Backup file dialog appeared more than once' }
            Set-CommonFileDialogPath $dialog ([string] $profile.DialogTitles.BackupFile) $OutputPath @('Save', '&Save', 'OK', '&OK')
            $backupFileDialogHandle = [long] $dialog.handle
            $fileSubmitted = $true
            continue
        }
        if ($dialog.title -ceq $profile.DialogTitles.Product -and $body -match '^\d+ files backed up$') {
            $accept = Get-ExactButtonById $dialog.handle ([int] $profile.Controls.Accept) @('OK', '&OK')
            Invoke-Button $accept
            $completionObserved = $true
            Write-Journal 'backup_completion_prompt' ([ordered]@{ body = $body })
            continue
        }
        throw "Unexpected backup dialog: title='$($dialog.title)', body='$body'"
    }
    throw "PLS-CADD backup did not complete within $($profile.BackupTimeoutSeconds) seconds"
}

function Close-PlsWithoutSaving {
    param([System.Diagnostics.Process] $Process, [long] $MainWindowHandle)

    Send-PlsCommand $MainWindowHandle ([int] $profile.Commands.Exit)
    $deadline = [DateTime]::UtcNow.AddSeconds([int] $profile.ExitTimeoutSeconds)
    while ([DateTime]::UtcNow -lt $deadline) {
        $Process.Refresh()
        if ($Process.HasExited) {
            Write-Journal 'pls_exited' ([ordered]@{ process_id = $Process.Id; exit_code = $Process.ExitCode; save_response = 'not_saved' })
            return
        }
        # PLS can destroy its main frame before Process.HasExited flips. Once
        # the frame is gone, keep waiting for process exit, but refuse any
        # remaining enabled top-level dialog instead of treating it as success.
        $topLevel = @(Get-ProcessWindows $Process.Id)
        $frame = @($topLevel | Where-Object { $_.handle -eq $MainWindowHandle })
        if ($frame.Count -eq 0) {
            $unexpected = @($topLevel | Where-Object {
                $_.enabled -or $_.class -ceq '#32770'
            })
            if ($unexpected.Count -ne 0) {
                throw "Unexpected PLS-CADD window while exiting: $(@($unexpected.title) -join ', ')"
            }
            Start-Sleep -Milliseconds 200
            continue
        }
        if ($frame.Count -ne 1) { throw "Expected one PLS-CADD frame while exiting, found $($frame.Count)" }
        # Use the same enumeration that proved the frame exists. A second
        # enumeration can observe its destruction before HasExited flips.
        $windows = @()
        if (-not $frame[0].enabled) {
            $windows = @($topLevel | Where-Object {
                $_.handle -ne $MainWindowHandle -and $_.enabled
            })
        }
        if ($windows.Count -gt 1) {
            throw "Multiple PLS-CADD top-level windows while exiting: $(@($windows.title) -join ', ')"
        }
        if ($windows.Count -eq 1) {
            $dialog = $windows[0]
            $body = Get-DialogBody $dialog.handle
            $matches = $false
            foreach ($pattern in $profile.ExitSaveBodyPatterns) {
                if ($body -match $pattern) { $matches = $true; break }
            }
            if ($dialog.title -cne $profile.DialogTitles.Product -or -not $matches) {
                throw "Unexpected exit dialog: title='$($dialog.title)', body='$body'"
            }
            $decline = Find-ExactButtonByText $dialog.handle @($profile.ExactButtons.No + $profile.ExactButtons.Cancel)
            Invoke-Button $decline
            Write-Journal 'exit_save_declined' ([ordered]@{ body = $body })
        }
        Start-Sleep -Milliseconds 200
    }
    throw "PLS-CADD did not exit within $($profile.ExitTimeoutSeconds) seconds"
}

function Assert-FreshAbsolutePath([string] $Path, [string] $Label, [bool] $ParentMustExist) {
    if (-not [System.IO.Path]::IsPathRooted($Path)) { throw "$Label must be absolute: $Path" }
    if (Test-Path -LiteralPath $Path) { throw "$Label already exists: $Path" }
    $parent = [System.IO.Path]::GetDirectoryName($Path)
    if ([string]::IsNullOrWhiteSpace($parent)) { throw "$Label has no parent directory: $Path" }
    if ($ParentMustExist -and -not (Test-Path -LiteralPath $parent -PathType Container)) {
        throw "$Label parent does not exist: $parent"
    }
    $parentItem = Get-Item -LiteralPath $parent -Force
    if (($parentItem.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "$Label parent is a reparse point: $parent"
    }
}

function Copy-FileCreateNew([string] $Source, [string] $Destination) {
    $input = [System.IO.File]::Open($Source, [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read, [System.IO.FileShare]::ReadWrite)
    $output = [System.IO.File]::Open($Destination, [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try {
        $input.CopyTo($output)
        $output.Flush($true)
    } finally {
        $output.Dispose()
        $input.Dispose()
    }
}

$script:JournalPath = $null
$activeProcess = $null
$completed = $false
try {
    if (-not $Execute) {
        throw 'Refusing to launch PLS-CADD without the explicit -Execute switch'
    }
    if ($profile.Schema -cne 'ds.pls.backup_restore_profile.v1') { throw 'Unsupported backup/restore profile schema' }
    $executable = Assert-PlsRegularFile $ExecutablePath 'PLS-CADD executable'
    $candidate = Assert-PlsRegularFile $CandidateBackupPath 'candidate backup'
    $actualExecutableDigest = Get-PlsFileSha256 $executable
    if ($actualExecutableDigest -cne $ExpectedExecutableSha256.ToLowerInvariant() -or
        $actualExecutableDigest -cne ([string] $profile.ExecutableSha256).ToLowerInvariant()) {
        throw "PLS-CADD executable digest does not match both caller and version profile: $actualExecutableDigest"
    }
    $version = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($executable)
    if (-not (Test-PlsExecutableVersion $version.FileVersion ([string] $profile.ProductVersion))) {
        throw "PLS-CADD executable version mismatch: expected $($profile.ProductVersion), got $($version.FileVersion)"
    }
    $actualCandidateDigest = Get-PlsFileSha256 $candidate
    if ($actualCandidateDigest -cne $ExpectedCandidateBackupSha256.ToLowerInvariant()) {
        throw "Candidate backup digest mismatch: expected $ExpectedCandidateBackupSha256, got $actualCandidateDigest"
    }
    if ([System.IO.Path]::GetExtension($FreshBackupPath) -ine '.bak') {
        throw "FreshBackupPath must end in .bak: $FreshBackupPath"
    }
    Assert-FreshAbsolutePath $FirstRestoreDirectory 'first restore directory' $true
    Assert-FreshAbsolutePath $SecondRestoreDirectory 'second restore directory' $true
    Assert-FreshAbsolutePath $FreshBackupPath 'fresh PLS backup' $true
    Assert-FreshAbsolutePath $EvidenceDirectory 'evidence directory' $true
    $targets = @($FirstRestoreDirectory, $SecondRestoreDirectory, $FreshBackupPath, $EvidenceDirectory) |
        ForEach-Object { [System.IO.Path]::GetFullPath($_).TrimEnd('\') }
    if (@($targets | Sort-Object -Unique).Count -ne $targets.Count) { throw 'Output targets must be distinct' }
    foreach ($left in $targets) {
        foreach ($right in $targets) {
            if ($left -ceq $right) { continue }
            if ($left.StartsWith($right + '\', [System.StringComparison]::OrdinalIgnoreCase)) {
                throw "Output targets may not contain one another: '$left' below '$right'"
            }
        }
    }
    $running = @(Get-Process -Name 'pls_cadd64' -ErrorAction SilentlyContinue)
    if ($running.Count -ne 0) {
        throw "Refusing to run while PLS-CADD is already running (PID(s): $($running.Id -join ', '))"
    }

    [System.IO.Directory]::CreateDirectory($EvidenceDirectory) | Out-Null
    $script:JournalPath = Join-Path $EvidenceDirectory 'journal.jsonl'
    [System.IO.File]::Open($script:JournalPath, [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::Read).Dispose()
    Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'INCOMPLETE.json') ([ordered]@{
        schema = 'ds.pls.backup_restore_incomplete.v1'
        reason = 'two-restore qualification has not completed'
        candidate_backup_sha256 = $actualCandidateDigest
    })
    Write-Journal 'preflight_complete' ([ordered]@{
        executable = $executable
        executable_sha256 = $actualExecutableDigest
        executable_version = $version.FileVersion
        candidate_backup = $candidate
        candidate_backup_sha256 = $actualCandidateDigest
        save_before_backup_authorized = [bool] $AuthorizeSaveBeforeBackup
    })

    $candidatePayload = Resolve-PlsNativeBackupPayload $candidate $actualCandidateDigest (Join-Path $EvidenceDirectory 'candidate-payload')
    $candidateInventory = Get-PlsNativeBackupInventory $candidatePayload.native_path $ProjectFileName $SourceRoot
    Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'candidate-inventory.json') $candidateInventory
    Write-Journal 'candidate_inventory_verified' ([ordered]@{
        native_sha256 = $candidateInventory.native_backup_sha256
        project_file = $candidateInventory.project_file
        counts = $candidateInventory.counts
        digests = $candidateInventory.digests
    })

    $logStart = [ordered]@{ exists = $false; bytes = 0; sha256 = $null }
    if (Test-Path -LiteralPath $PlsLogPath -PathType Leaf) {
        $logFile = Assert-PlsRegularFile $PlsLogPath 'PLS-CADD log'
        $logStart = [ordered]@{
            exists = $true
            bytes = (Get-Item -LiteralPath $logFile).Length
            sha256 = Get-PlsFileSha256 $logFile
        }
    }

    $promptEvidence = New-Object System.Collections.ArrayList
    $repairEvidence = New-Object System.Collections.ArrayList
    $first = Invoke-PlsRestore $executable $candidatePayload.native_path $candidateInventory $FirstRestoreDirectory $promptEvidence $repairEvidence
    $activeProcess = $first.process

    $backupRun = Invoke-PlsBackup $activeProcess $first.main_window_handle $FreshBackupPath ([bool] $AuthorizeSaveBeforeBackup)
    $freshBackupDigest = Get-PlsFileSha256 $FreshBackupPath
    $freshPayload = Resolve-PlsNativeBackupPayload $FreshBackupPath $freshBackupDigest (Join-Path $EvidenceDirectory 'fresh-backup-payload')
    $freshInventory = Get-PlsNativeBackupInventory $freshPayload.native_path $ProjectFileName $FirstRestoreDirectory
    $protectedComparison = Compare-PlsProtectedInventories $candidateInventory $freshInventory
    if (-not $protectedComparison.equal) {
        throw "Fresh PLS backup changed protected candidate content: $($protectedComparison.differences -join ', ')"
    }
    Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'fresh-backup-inventory.json') $freshInventory
    Close-PlsWithoutSaving $activeProcess $first.main_window_handle
    $activeProcess = $null
    $firstFull = Test-PlsRestoredTree $candidateInventory $FirstRestoreDirectory 'Full' @($profile.AllowedFreshRestoreExtras)
    $firstProtected = Test-PlsRestoredTree $candidateInventory $FirstRestoreDirectory 'Protected'
    Write-Journal 'first_restore_post_close_full_tree_verified' $firstFull
    Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'first-restore-verification.json') ([ordered]@{
        pre_open_presence = $first.pre_open_presence_verification
        full = $firstFull
        protected = $firstProtected
    })

    $second = Invoke-PlsRestore $executable $freshPayload.native_path $freshInventory $SecondRestoreDirectory $promptEvidence $repairEvidence
    $activeProcess = $second.process
    Close-PlsWithoutSaving $activeProcess $second.main_window_handle
    $activeProcess = $null
    $secondFull = Test-PlsRestoredTree $freshInventory $SecondRestoreDirectory 'Full' @($profile.AllowedFreshRestoreExtras)
    $secondProtectedCandidate = Test-PlsRestoredTree $candidateInventory $SecondRestoreDirectory 'Protected'
    Write-Journal 'second_restore_post_close_full_tree_verified' $secondFull
    Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'second-restore-verification.json') ([ordered]@{
        pre_open_presence = $second.pre_open_presence_verification
        full_against_fresh_backup = $secondFull
        protected_against_intended_candidate = $secondProtectedCandidate
    })

    if ((Get-PlsFileSha256 $candidate) -cne $actualCandidateDigest) { throw 'Candidate backup changed during qualification' }
    if ((Get-PlsFileSha256 $executable) -cne $actualExecutableDigest) { throw 'PLS-CADD executable changed during qualification' }

    $logEnd = [ordered]@{ exists = $false; bytes = 0; sha256 = $null; evidence_path = $null }
    $warningMatches = @()
    if (Test-Path -LiteralPath $PlsLogPath -PathType Leaf) {
        $logFile = Assert-PlsRegularFile $PlsLogPath 'PLS-CADD log'
        $logCopy = Join-Path $EvidenceDirectory 'PLS-CADD.log'
        Copy-FileCreateNew $logFile $logCopy
        $logText = [System.IO.File]::ReadAllText($logCopy)
        $warningMatches = @($logText -split "`r?`n" | Where-Object {
            $_ -match '(?i)(SAPS|Unsupported Wire Condition|Results will be incorrect|warning|error)'
        } | Select-Object -Last 1000)
        $logEnd = [ordered]@{
            exists = $true
            bytes = (Get-Item -LiteralPath $logCopy).Length
            sha256 = Get-PlsFileSha256 $logCopy
            evidence_path = 'PLS-CADD.log'
        }
    }
    Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'warnings.json') ([ordered]@{
        schema = 'ds.pls.backup_restore_warnings.v1'
        log_before = $logStart
        log_after = $logEnd
        matched_lines = $warningMatches
        dialogs = @($promptEvidence)
        repair_dialogs = @($repairEvidence)
    })

    $manifest = [ordered]@{
        schema = 'ds.pls.backup_restore_qualification.v1'
        status = 'qualified_backup_roundtrip_local_only'
        completed_at_utc = [DateTime]::UtcNow.ToString('o')
        executable = [ordered]@{
            path = $executable
            sha256 = $actualExecutableDigest
            file_version = $version.FileVersion
            product_version = $version.ProductVersion
        }
        candidate = [ordered]@{
            path = $candidate
            container_format = $candidatePayload.container_format
            container_sha256 = $actualCandidateDigest
            native_sha256 = $candidateInventory.native_backup_sha256
            project_file = $candidateInventory.project_file
            counts = $candidateInventory.counts
            digests = $candidateInventory.digests
        }
        first_restore = [ordered]@{
            directory = $FirstRestoreDirectory
            project_opened = $first.project_path
            full_tree_verified = $true
            protected_candidate_verified = $true
        }
        fresh_pls_backup = [ordered]@{
            path = $FreshBackupPath
            container_format = $freshPayload.container_format
            container_sha256 = $freshBackupDigest
            native_sha256 = $freshInventory.native_backup_sha256
            counts = $freshInventory.counts
            digests = $freshInventory.digests
            protected_equal_to_candidate = $protectedComparison.equal
            save_before_backup_authorized = [bool] $AuthorizeSaveBeforeBackup
            ui = $backupRun
        }
        second_restore = [ordered]@{
            directory = $SecondRestoreDirectory
            project_opened = $second.project_path
            full_tree_verified_against_fresh_backup = $true
            protected_tree_verified_against_intended_candidate = $true
        }
        repairs = [ordered]@{
            count = $repairEvidence.Count
            remove_missing_references_used = $false
            actions = @($repairEvidence)
        }
        warnings = [ordered]@{
            known_dialog_count = $promptEvidence.Count
            log_match_count = $warningMatches.Count
            evidence = 'warnings.json'
        }
        caveat = [ordered]@{
            code = 'saps_unlicensed_pls_16_81'
            accepted_for_local_verification = [bool] $AcceptUnlicensedSaps
            shipment_grade = $false
            text = 'PLS-CADD 16.81 on this host lacks SAPS and may substitute Initial RS for unsupported wire conditions 2/3; PLS-CADD states affected results are incorrect. This qualification proves backup/restore integrity only. Engineering submission still requires a SAPS-equipped supported run or explicit responsible-operator acceptance.'
        }
        source_drift_detected = $false
        save_after_open_without_explicit_authority = $false
        evidence_files = @(
            'candidate-inventory.json',
            'first-restore-verification.json',
            'fresh-backup-inventory.json',
            'second-restore-verification.json',
            'warnings.json',
            'journal.jsonl'
        )
    }
    Write-PlsJsonCreateNew (Join-Path $EvidenceDirectory 'manifest.json') $manifest
    [System.IO.File]::Delete((Join-Path $EvidenceDirectory 'INCOMPLETE.json'))
    $completed = $true
    $manifest | ConvertTo-Json -Depth 30
} catch {
    if ($null -ne $script:JournalPath -and (Test-Path -LiteralPath $script:JournalPath -PathType Leaf)) {
        try {
            Write-Journal 'qualification_failed' ([ordered]@{
                message = $_.Exception.Message
                active_process_id = if ($null -ne $activeProcess) { $activeProcess.Id } else { $null }
                process_left_for_operator = ($null -ne $activeProcess)
            })
        } catch {}
    }
    if (Test-Path -LiteralPath $EvidenceDirectory -PathType Container) {
        $failurePath = Join-Path $EvidenceDirectory 'failure.json'
        if (-not (Test-Path -LiteralPath $failurePath)) {
            try {
                Write-PlsJsonCreateNew $failurePath ([ordered]@{
                    schema = 'ds.pls.backup_restore_failure.v1'
                    failed_at_utc = [DateTime]::UtcNow.ToString('o')
                    message = $_.Exception.Message
                    active_process_id = if ($null -ne $activeProcess) { $activeProcess.Id } else { $null }
                    process_left_for_operator = ($null -ne $activeProcess)
                    destructive_recovery_attempted = $false
                })
            } catch {}
        }
    }
    throw
} finally {
    if (-not $completed -and $null -ne $activeProcess) {
        # Deliberately do not kill or blindly close PLS-CADD after an unexpected
        # modal. The evidence records the PID and the operator can inspect it.
    }
}
