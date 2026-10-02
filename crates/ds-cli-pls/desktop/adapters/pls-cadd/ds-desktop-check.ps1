param([Parameter(Mandatory = $true)][string] $ResultPath)
# ds pls desktop check - is this Windows host ready for the PLS-CADD drivers? Reads only.
# The executable, its pinned digest and its version are decided with the same profile and
# Test-PlsExecutableVersion the restore drivers use. The Classic interface and the Project
# Wizard switch have no characterized setting key yet, so they are listed for the operator to
# confirm, with any PLS_CADD.INI lines that mention them as evidence - never guessed.
. (Join-Path $PSScriptRoot 'ds-desktop-lib.ps1')
Invoke-DsEntry $ResultPath 'check' {
    Import-Module (Join-Path $here 'pls-backup-restore-lib.psm1') -Force
    $found = Test-Path -LiteralPath $PlsExecutable -PathType Leaf
    $digest = $null; $fileVersion = $null; $productVersion = $null
    if ($found) {
        $digest = (Get-FileHash -LiteralPath $PlsExecutable -Algorithm SHA256).Hash.ToLowerInvariant()
        $info = [System.Diagnostics.FileVersionInfo]::GetVersionInfo($PlsExecutable)
        $fileVersion = $info.FileVersion; $productVersion = $info.ProductVersion
    }
    $pinned = ([string] $DsProfile.ExecutableSha256).ToLowerInvariant()
    $versionOk = $found -and $fileVersion -and (Test-PlsExecutableVersion $fileVersion ([string] $DsProfile.ProductVersion))
    $ini = Join-Path $env:APPDATA 'PLS\PLS_CADD.INI'
    $iniFound = Test-Path -LiteralPath $ini -PathType Leaf
    $iniLines = @()
    if ($iniFound) { $iniLines = @(Get-Content -LiteralPath $ini | Where-Object { $_ -match '(?i)wizard|classic|ribbon|interface' } | Select-Object -First 40) }
    $word = $null -ne [Type]::GetTypeFromProgID('Word.Application')
    $running = @(Get-Process -Name 'pls_cadd64' -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
    $powershell = $PSVersionTable.PSVersion
    $blockers = @()
    if (-not $found) { $blockers += 'pls_cadd_not_found' }
    elseif ($digest -cne $pinned -or -not $versionOk) { $blockers += 'pls_cadd_mismatch' }
    if ($running.Count -gt 0) { $blockers += 'pls_cadd_running' }
    if (-not $word) { $blockers += 'word_not_found' }
    if ($powershell.Major -ne 5 -or $powershell.Minor -ne 1) { $blockers += 'powershell_not_5_1' }
    $script:DsResult = [ordered]@{
        ready = ($blockers.Count -eq 0)
        blockers = $blockers
        executable = [ordered]@{ path = $PlsExecutable; found = $found; sha256 = $digest; pinned_sha256 = $pinned
            matches_pin = ($digest -ceq $pinned); file_version = $fileVersion; product_version = $productVersion
            expected_version = [string] $DsProfile.ProductVersion; version_ok = [bool] $versionOk }
        pls_cadd_running = $running
        word_registered = $word
        powershell_version = $powershell.ToString()
        operator_confirms = @(
            'PLS-CADD opens with the Classic interface (the drivers post Classic menu command ids)',
            'the Project Wizard is switched off (it would stand between a command and its dialog)'
        )
        settings_file = [ordered]@{ path = $ini; found = $iniFound; matching_lines = $iniLines }
    }
}
