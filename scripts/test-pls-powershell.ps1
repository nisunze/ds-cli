# Local developer acceptance only. Never dot-source a desktop driver entry.
param(
    [Parameter(Mandatory = $true)][string] $AnalyzerManifest
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
if ($PSVersionTable.PSVersion -lt [version] '7.2') {
    throw 'This developer gate requires PowerShell 7.2+; production drivers still require Windows PowerShell 5.1.'
}
$repo = Split-Path -Parent $PSScriptRoot
$desktop = Join-Path $repo 'crates/ds-cli-pls/desktop'
$bundle = Join-Path $repo 'crates/ds-cli-pls/src/desktop/bundle.rs'
$analyzer = Import-Module -Name $AnalyzerManifest -PassThru -ErrorAction Stop
if ($analyzer.Name -ne 'PSScriptAnalyzer' -or $analyzer.Version -ne [version] '1.24.0') {
    throw 'Pass the manifest of PSScriptAnalyzer 1.24.0 explicitly; no gallery download or silent fallback is performed.'
}
$settings = @{
    Rules = @{
        PSUseCompatibleSyntax = @{ Enable = $true; TargetVersions = @('5.1') }
    }
}
$cases = [System.Collections.Generic.List[string]]::new()

function Assert-That([bool] $Condition, [string] $Message) {
    if (-not $Condition) { throw $Message }
}

function Read-TestAst([string] $Path) {
    $errors = $null
    $tree = [System.Management.Automation.Language.Parser]::ParseFile($Path, [ref] $null, [ref] $errors)
    if (@($errors).Count -ne 0) {
        $first = $errors[0]
        throw "PowerShell parse error in ${Path}:$($first.Extent.StartLineNumber):$($first.Extent.StartColumnNumber): $($first.Message)"
    }
    return $tree
}

function Assert-Refuses([scriptblock] $Body, [string] $Pattern, [string] $Name) {
    $observed = $null
    try { & $Body | Out-Null } catch { $observed = $_.Exception.Message }
    Assert-That ($null -ne $observed -and $observed -match $Pattern) "$Name did not refuse with the expected reason: $observed"
    $cases.Add($Name)
}

function Import-TestFunction([string] $Path, [string] $Name) {
    $tree = Read-TestAst $Path
    $found = @($tree.FindAll({
        param($node)
        $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -ceq $Name
    }, $false))
    Assert-That ($found.Count -eq 1) "Expected exactly one pure $Name definition in $Path"
    # Import only the requested function definition. No top-level profile, Add-Type,
    # application launch, UI/COM call, or driver entry is evaluated.
    return [scriptblock]::Create($found[0].Extent.Text)
}

$declared = @([regex]::Matches((Get-Content -LiteralPath $bundle -Raw), '(?s)script!\(\s*"(?<path>[^"]+)"\s*,\s*"(?<sha>[0-9a-f]{64})"'))
Assert-That ($declared.Count -gt 0) 'No bundle declarations found; the gate must track bundle.rs.'
$paths = @($declared | ForEach-Object { $_.Groups['path'].Value })
Assert-That (@($paths | Select-Object -Unique).Count -eq $paths.Count) 'Duplicate bundle path.'
$disk = @(Get-ChildItem -LiteralPath $desktop -File -Recurse | ForEach-Object {
    $_.FullName.Substring($desktop.Length + 1).Replace('\', '/')
})
$difference = @(Compare-Object ($paths | Sort-Object) ($disk | Sort-Object) -CaseSensitive)
Assert-That ($difference.Count -eq 0) 'The bundle.rs declaration and desktop files differ.'
$cases.Add('exact_bundle_file_set')
$fileEvidence = @()
foreach ($entry in $declared) {
    $relative = $entry.Groups['path'].Value
    Assert-That ($relative -match '^[a-z0-9/-]+\.(ps1|psm1|psd1)$' -and $relative -notmatch '\.\.') "Unsafe or unexpected bundle path: $relative"
    $path = Join-Path $desktop $relative
    $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    Assert-That ($actual -ceq $entry.Groups['sha'].Value) "Bundle pin differs for $relative"
    Read-TestAst $path | Out-Null
    $findings = @(Invoke-ScriptAnalyzer -Path $path -Settings $settings -IncludeRule PSUseCompatibleSyntax)
    Assert-That ($findings.Count -eq 0) "PowerShell 5.1 syntax incompatibility in ${relative}: $($findings | Out-String)"
    $fileEvidence += [ordered]@{ path = $relative; sha256 = $actual }
}
$cases.Add('all_bundle_files_parse_and_target_5_1')

$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ('ds-pls-powershell-' + [guid]::NewGuid().ToString('N'))
[System.IO.Directory]::CreateDirectory($scratch) | Out-Null
try {
    $bad = Join-Path $scratch 'malformed.ps1'
    $good = Join-Path $scratch 'valid.ps1'
    $marker = Join-Path $scratch 'must-not-exist'
    $escapedMarker = $marker.Replace("'", "''")
    [System.IO.File]::WriteAllText($bad, "[System.IO.File]::WriteAllText('$escapedMarker', 'forbidden'); function Keep { 'retained AST' }; function Broken {", [System.Text.UTF8Encoding]::new($false))
    [System.IO.File]::WriteAllText($good, "[System.IO.File]::WriteAllText('$escapedMarker', 'forbidden'); function Keep { 'not invoked' }; function Another { 'also not invoked' }", [System.Text.UTF8Encoding]::new($false))
    Assert-Refuses { Read-TestAst $bad } 'PowerShell parse error' 'malformed_script_parser_negative'

    $newSyntax = '$display = $name ?? ''Unknown''; $message = $ready ? ''Ready'' : ''Not ready'''
    $syntaxErrors = $null
    [System.Management.Automation.Language.Parser]::ParseInput($newSyntax, [ref] $null, [ref] $syntaxErrors) | Out-Null
    Assert-That (@($syntaxErrors).Count -eq 0) 'The PS7 negative must parse successfully on the developer host.'
    $newFindings = @(Invoke-ScriptAnalyzer -ScriptDefinition $newSyntax -Settings $settings -IncludeRule PSUseCompatibleSyntax)
    Assert-That ($newFindings.Count -ge 2) 'The explicit 5.1 syntax rule failed to reject PS7-only syntax.'
    $cases.Add('ps7_syntax_rejected_for_5_1')

    . (Import-TestFunction (Join-Path $desktop 'ds-desktop-lib.ps1') 'Read-DsScriptAst')
    . (Import-TestFunction (Join-Path $desktop 'interim/pls-interim-loader.ps1') 'Get-AstFunctions')
    $rejectedAst = [System.Collections.Generic.List[object]]::new()
    $rejectedFunctions = [System.Collections.Generic.List[object]]::new()
    Assert-Refuses { Read-DsScriptAst $bad | ForEach-Object { $rejectedAst.Add($_) } } 'PowerShell parse error' 'delivery_loader_rejects_malformed_helper'
    Assert-Refuses { Get-AstFunctions $bad | ForEach-Object { $rejectedFunctions.Add($_) } } 'PowerShell parse error' 'interim_loader_rejects_malformed_helper'
    Assert-That ($rejectedAst.Count -eq 0 -and $rejectedFunctions.Count -eq 0) 'A malformed helper returned an AST or functions before refusal.'
    Assert-That (-not (Test-Path -LiteralPath $marker)) 'Rejecting a helper executed its top-level code.'
    $cases.Add('malformed_loaders_return_nothing_and_execute_nothing')
    $validTree = Read-DsScriptAst $good
    $validFunctions = Get-AstFunctions $good
    $astNames = @($validTree.FindAll({ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] }, $false) | ForEach-Object { $_.Name })
    Assert-That (@(Compare-Object @('Another', 'Keep') @($astNames | Sort-Object)).Count -eq 0) 'Valid helper AST functions changed.'
    Assert-That (@(Compare-Object @('Another', 'Keep') @($validFunctions.functions.Keys | Sort-Object)).Count -eq 0) 'Valid helper loader functions changed.'
    Assert-That (-not (Test-Path -LiteralPath $marker)) 'Parsing a helper executed its top-level code.'
    $cases.Add('loaders_preserve_valid_ast_without_execution')

    $libPath = Join-Path $desktop 'ds-desktop-lib.ps1'
    $libAst = Read-TestAst $libPath
    $reportLists = @($libAst.FindAll({ param($node)
        $node -is [System.Management.Automation.Language.AssignmentStatementAst] -and
        $node.Left -is [System.Management.Automation.Language.VariableExpressionAst] -and
        $node.Left.VariablePath.UserPath -ceq 'DsDeliverableReports'
    }, $false))
    Assert-That ($reportLists.Count -eq 1) 'The native menu table must have one authority.'
    # Execute only the literal report table and extracted pure selection function.
    $DsDeliverableReports = & ([scriptblock]::Create($reportLists[0].Right.Extent.Text))
    . (Import-TestFunction $libPath 'Get-DsReportSet')
    $originalReports = $DsDeliverableReports | ConvertTo-Json -Depth 4 -Compress
    $canonicalReports = @(Get-DsReportSet)
    Assert-That (($canonicalReports.k -join '|') -ceq 'Section Usage|Structure Usage|Terrain Clearances|Summary|Section Sag-Tension') 'The default submission must contain exactly the five named reports.'
    Assert-That (($canonicalReports.id -join '|') -ceq '40015|40014|40016|40019|40403') 'Canonical names must preserve characterized native menu identities.'
    Assert-That ($canonicalReports[2].all -eq $true) 'Terrain Clearances must cover all feature codes.'
    Assert-That (($DsDeliverableReports | ConvertTo-Json -Depth 4 -Compress) -ceq $originalReports) 'Selecting reports changed the source menu facts.'
    $supplementedReports = @(Get-DsReportSet $true)
    Assert-That ($supplementedReports.Count -eq 6 -and @($supplementedReports | Where-Object { $_.id -eq 40020 }).Count -eq 1) 'Supplementary wind/weight spans require the explicit option.'
    $cases.Add('canonical_five_reports_exact_native_ids_and_optional_supplement')

    . (Import-TestFunction (Join-Path $desktop 'pls-rtf-to-pdf.ps1') 'Set-PlsReportPaper')
    foreach ($paper in 'A4', 'A3') {
        $sections = @(1, 2 | ForEach-Object { [pscustomobject]@{ PageSetup = [pscustomobject]@{ Orientation = 0; PageWidth = 1; PageHeight = 2 } } })
        Set-PlsReportPaper $sections $paper
        $expectedWidth = $(if ($paper -eq 'A4') { 841.89 } else { 1190.55 })
        $expectedHeight = $(if ($paper -eq 'A4') { 595.28 } else { 841.89 })
        Assert-That (@($sections | Where-Object { $_.PageSetup.Orientation -ne 1 -or $_.PageSetup.PageWidth -ne $expectedWidth -or $_.PageSetup.PageHeight -ne $expectedHeight }).Count -eq 0) "Every $paper report section must use its exact landscape page box."
    }
    $cases.Add('a4_default_and_explicit_a3_all_sections')
    Assert-Refuses { Set-PlsReportPaper @() 'A2' } 'ValidateSet|validation' 'unsupported_report_paper_negative'

    Import-Module (Join-Path $desktop 'pls-window-classification.psm1') -Force -DisableNameChecking
    $handle = [long] 8589934593
    $frame = "$handle vis=True en=True [PLS-CADD] 'same title'"
    $modal = "27 vis=True en=True [Dialog] 'same title'"
    $report = "28 vis=True en=True [Report] 'different title'"
    $rows = @($modal, $frame, $report)
    $before = $rows | ConvertTo-Json -Compress
    $split = Split-PlsWindowRows -Rows $rows -MainWindowHandle $handle
    Assert-That ($split.frame -ceq $frame) 'Frame selection must use the exact 64-bit handle, never the title or first row.'
    Assert-That ($split.others.Count -eq 2 -and $split.others[0] -ceq $modal -and $split.others[1] -ceq $report) 'All non-frame rows must remain in order.'
    Assert-That (($rows | ConvertTo-Json -Compress) -ceq $before) 'Classification mutated its input rows.'
    $cases.Add('exact_handle_same_title_and_preserved_others')
    $single = Split-PlsWindowRows -Rows @($frame) -MainWindowHandle $handle
    Assert-That ($single.frame -ceq $frame -and $single.others.Count -eq 0) 'A single frame must have an empty others array.'
    $cases.Add('single_frame_empty_others')
    Assert-Refuses { Split-PlsWindowRows -Rows @($modal) -MainWindowHandle $handle } 'found 0' 'missing_frame_negative'
    Assert-Refuses { Split-PlsWindowRows -Rows @($frame, $frame) -MainWindowHandle $handle } 'found 2' 'duplicate_frame_negative'
    Assert-Refuses { Split-PlsWindowRows -Rows @($frame) -MainWindowHandle 0 } 'handle is unavailable' 'zero_handle_negative'
    Assert-Refuses { Split-PlsWindowRows -Rows @($frame) -MainWindowHandle -1 } 'handle is unavailable' 'negative_handle_negative'
    Assert-Refuses { Split-PlsWindowRows -Rows @('invalid row', $frame) -MainWindowHandle $handle } 'convert' 'malformed_window_row_negative'
} finally {
    Remove-Item -LiteralPath $scratch -Recurse -Force
}

[ordered]@{
    schema = 'ds.pls.powershell_developer_gate.v1'
    status = 'pass'
    host_version = $PSVersionTable.PSVersion.ToString()
    analyzer_version = $analyzer.Version.ToString()
    analyzer_manifest_sha256 = (Get-FileHash -LiteralPath $AnalyzerManifest -Algorithm SHA256).Hash.ToLowerInvariant()
    target_syntax = '5.1'
    bundle_count = $fileEvidence.Count
    ps7_negative_findings = $newFindings.Count
    bundle_files = $fileEvidence
    case_count = $cases.Count
    cases = @($cases)
    driver_entry_executed = $false
    windows_pls_acceptance = $false
} | ConvertTo-Json -Depth 5
