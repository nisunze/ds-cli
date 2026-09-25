# INTERIM (2026-09-23, Nyamagabe delivery) — dot-source only. Hand to Codex for
# integration into ../pls-backup-restore-qualify.ps1 and ../pls-backup-restore-lib.psm1.
#
# Loads the qualify driver's and the lib's functions VERBATIM (from their AST) into the
# caller's scope. C3a/C3c/C3d/C3e/C3f are in the production driver/library
# (native C3c verified 2026-09-23). The interim entrypoints remain useful for
# an open-only Restore and report-driving run.
#   C3f native acceptance: the two-restore production qualifier runs Full after
#        close; the interim open-only path runs Protected in pls-close-interim.ps1.
$pls = Split-Path -Parent $PSScriptRoot
$driver = Join-Path $pls 'pls-backup-restore-qualify.ps1'
$lib = Join-Path $pls 'pls-backup-restore-lib.psm1'
$profile = Import-PowerShellDataFile -LiteralPath (Join-Path $pls 'pls-backup-restore-profile.psd1')
$PlsLogPath = Join-Path $env:APPDATA 'PLS\temp\PLS-CADD.log'
Add-Type -AssemblyName System.Windows.Forms

function Get-AstFunctions([string] $Path) {
    $tree = [System.Management.Automation.Language.Parser]::ParseFile($Path, [ref] $null, [ref] $null)
    $map = @{}
    foreach ($fn in $tree.FindAll({ param($n) $n -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $false)) {
        $map[$fn.Name] = $fn.Extent.Text
    }
    return @{ ast = $tree; functions = $map }
}

$driverParsed = Get-AstFunctions $driver
$nativeBlock = @($driverParsed.ast.EndBlock.Statements | Where-Object { $_.Extent.Text -like 'Add-Type @"*DsGridBackupRestoreNative*' })
if ($nativeBlock.Count -ne 1) { throw 'Driver native type block not found exactly once' }
if (-not ('DsGridBackupRestoreNative' -as [type])) { . ([scriptblock]::Create($nativeBlock[0].Extent.Text)) }
$functions = $driverParsed.functions
$libFunctions = (Get-AstFunctions $lib).functions

# C3f is integrated in the production qualifier: full verification runs only
# after close. Keep the interim open-only entrypoint without swallowing a
# verification failure or patching the characterized Restore transition.
$restore = $functions['Invoke-PlsRestore']
$restore = $restore -replace '^function Invoke-PlsRestore', 'function Invoke-PlsRestoreInterim'

# C3a is integrated in the production driver. Keep the interim entrypoint
# used by pls-close-interim.ps1 while sourcing the same one-snapshot body.
$close = $functions['Close-PlsWithoutSaving']
$close = $close -replace '^function Close-PlsWithoutSaving', 'function Close-PlsWithoutSavingInterim'

foreach ($name in $libFunctions.Keys) { . ([scriptblock]::Create($libFunctions[$name])) }
foreach ($name in $functions.Keys) { . ([scriptblock]::Create($functions[$name])) }
. ([scriptblock]::Create($restore))
. ([scriptblock]::Create($close))
