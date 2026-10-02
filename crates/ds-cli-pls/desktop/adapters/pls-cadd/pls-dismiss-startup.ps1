param(
    [Parameter(Mandatory = $true)][int] $ProcessId,
    [int] $TimeoutSeconds = 90
)
# Dismiss the startup modals of a freshly launched PLS-CADD (About PLS-CADD -> OK id 1,
# Tip of the Day -> Close id 1, any owned prompt with a No id 7 is answered No, any with
# only an OK/Close id 1 is accepted) until the frame window is enabled. Prints the frame
# title. Never presses Yes.
$here = $PSScriptRoot
Import-Module (Join-Path $here 'pls-window-classification.psm1') -Force
$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
$journal = [System.Collections.ArrayList]::new()
while ([DateTime]::UtcNow -lt $deadline) {
    Start-Sleep -Seconds 3
    $wins = & "$here\pls-windows.ps1" -ProcessId $ProcessId | Where-Object { $_ -match 'vis=True' }
    $main = [long] (Get-Process -Id $ProcessId -ErrorAction Stop).MainWindowHandle
    if ($main -eq 0) { continue }
    if (@($wins | Where-Object { [long](($_ -split ' ')[0]) -eq $main }).Count -eq 0) {
        continue
    }
    $classified = Split-PlsWindowRows @($wins) $main
    $frame = $classified.frame
    $modals = @($classified.others | Where-Object { $_ -notmatch "'Error Log'" })
    if (-not $modals -and $frame -match 'en=True') { break }
    foreach ($m in $modals) {
        $h = [long](($m -split ' ')[0])
        $kids = & "$here\pls-windows.ps1" -ProcessId $ProcessId -Children $h
        $text = ($kids | Where-Object { $_ -match 'id=(65535|20|1500|1004) ' }) -join ' | '
        $no = ($kids | Where-Object { $_ -match "id=7 .*'&No'" }) -replace '^\s*child (\d+).*', '$1' | Select-Object -First 1
        $ok = ($kids | Where-Object { $_ -match "id=1 .*'(&?OK|&?Close)'" }) -replace '^\s*child (\d+).*', '$1' | Select-Object -First 1
        $btn = if ($no) { $no } else { $ok }
        [void]$journal.Add("$m :: $($text.Substring(0, [Math]::Min(160, $text.Length))) -> $(if ($no) {'No'} elseif ($ok) {'OK/Close'} else {'no button'})")
        if ($btn) { & "$here\pls-windows.ps1" -ProcessId $ProcessId -Click ([long]$btn) | Out-Null }
    }
}
[ordered]@{ schema = 'ds.pls.startup.v1'; title = (Get-Process -Id $ProcessId).MainWindowTitle; journal = @($journal) } | ConvertTo-Json -Depth 3
