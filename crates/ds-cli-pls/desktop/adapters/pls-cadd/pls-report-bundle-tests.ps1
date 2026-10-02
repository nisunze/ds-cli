$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$scriptRoot = $PSScriptRoot
$bundle = Join-Path $scriptRoot 'pls-report-bundle.ps1'
$package = Join-Path $scriptRoot 'pls-report-package.ps1'
$profile = Import-PowerShellDataFile (Join-Path $scriptRoot 'pls-report-profile.psd1')
$fixtureRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("ds-pls-report-test-" + [Guid]::NewGuid().ToString('n'))
[System.IO.Directory]::CreateDirectory($fixtureRoot) | Out-Null

function Assert-True([bool] $Condition, [string] $Message) {
    if (-not $Condition) { throw "ASSERTION FAILED: $Message" }
}

try {
    $exe = Join-Path $fixtureRoot 'pls_cadd64.exe'
    $xyz = Join-Path $fixtureRoot 'model.xyz'
    $bak = Join-Path $fixtureRoot 'model.bak'
    [System.IO.File]::WriteAllBytes($exe, [byte[]](1,2,3,4))
    [System.IO.File]::WriteAllBytes($xyz, [byte[]](5,6,7))
    [System.IO.File]::WriteAllBytes($bak, [byte[]](8,9,10))
    $digest = (Get-FileHash $exe -Algorithm SHA256).Hash.ToLowerInvariant()
    $testProfile = Join-Path $fixtureRoot 'profile.psd1'
    $text = (Get-Content (Join-Path $scriptRoot 'pls-report-profile.psd1') -Raw).
        Replace($profile.ExecutableSha256, $digest)
    [System.IO.File]::WriteAllText($testProfile, $text, [System.Text.UTF8Encoding]::new($false))
    $testCoverage = Join-Path $fixtureRoot 'coverage.psd1'
    $coverageText = @"
@{
    Schema = 'ds.pls.report_coverage_profile.v1'
    ProjectFileName = 'model.xyz'
    Reports = @{
        structure_usage = @{
            MinimumBytes = 1
            RequiredLiterals = @('Structure Locations and Usage Report', 'Multiple Structure Minimum Vertical Load (Uplift) Summary')
            ExpectedRows = 2
            FirstStructureName = 'first.012'
            LastStructureName = 'last.012'
        }
        wind_weight_span = @{
            MinimumBytes = 1
            RequiredLiterals = @('Structure Wind and Weight Spans Report', 'Wind & Weight Span Report')
            ExpectedRows = 2
            FirstStructureName = 'first.012'
            LastStructureName = 'last.012'
            UnstrungStructureIds = @()
        }
        summary = @{
            MinimumBytes = 1
            RequiredLiterals = @('Line Statistics:', 'Total number of structures used:', 'Total number of sections:', 'Total number of alignment line angles:', 'Structure List Report', 'Structure Coordinates Report', 'Structure Material List Report', 'Cable Material List Report')
            ExpectedRows = 2
            AllowedStructuresUsed = @(2)
            ExpectedNativeSections = 2
        }
        section_usage = @{
            MinimumBytes = 1
            RequiredLiterals = @('Sections Evaluated')
            ExpectedRows = 2
        }
        section_tension = @{
            MinimumBytes = 1
            RequiredLiterals = @('Section Sagging Data', 'Ruling Span Sag Tension Report')
            ExpectedRows = 2
            ExpectedDetailReports = 2
        }
    }
}
"@
    [System.IO.File]::WriteAllText($testCoverage, $coverageText,
        [System.Text.UTF8Encoding]::new($false))

    $output = Join-Path $fixtureRoot 'result'
    & $bundle -Mode Prepare -OutputDirectory $output -ProjectPath $xyz -BackupPath $bak `
        -Requester 'test' -DsRevision 'deadbeef' -ExecutablePath $exe -ProfilePath $testProfile `
        -CoverageProfilePath $testCoverage `
        -AcceptUnlicensedSaps | Out-Null
    Assert-True (Test-Path (Join-Path $output 'INCOMPLETE.json')) 'Prepare leaves an explicit incomplete marker'

    $failed = $false
    try {
        & $bundle -Mode Finalize -OutputDirectory $output -ProjectPath $xyz -BackupPath $bak `
            -Requester 'test' -DsRevision 'deadbeef' -ExecutablePath $exe -ProfilePath $testProfile `
            -CoverageProfilePath $testCoverage `
            -PlsLogPath $bak | Out-Null
    } catch { $failed = $_.Exception.Message -like 'Required raw report missing:*' }
    Assert-True $failed 'Finalize refuses an incomplete report set'

    $expectedDon = [System.IO.Path]::ChangeExtension($xyz, '.don')
    foreach ($report in $profile.Reports) {
        $body = @(
            'PLS-CADD Version 16.81x64 test'
            "Project Name: '$expectedDon'"
        )
        switch ($report.Key) {
            'structure_usage' {
                $body += @('Structure Locations and Usage Report',
                    '1 first.012 0.0 0.0 0 1.0 0.0 OK OK',
                    '2 last.012 10.0 0.0 0 1.0 0.0 OK OK',
                    '0 structure violations, 0 structure warnings OK',
                    'Multiple Structure Minimum Vertical Load (Uplift) Summary')
            }
            'wind_weight_span' {
                $body += @('Structure Wind and Weight Spans Report', 'Wind & Weight Span Report',
                    'Heaviest Cable Wind & Weight Spans',
                    '1 0.00 N first.012 0.00 data', '2 10.00 N last.012 0.00 data',
                    'Wind & Weight Spans By Attachment Set',
                    '1 0.00 N first.012 0.00 data', '2 10.00 N last.012 0.00 data',
                    'Wind & Weight Spans By Side',
                    '1 0.00 N first.012 0.00 data', '2 10.00 N last.012 0.00 data')
            }
            'summary' {
                $body += @('Line Statistics:', 'Total number of structures used: 2',
                    'Total number of sections: 2', 'Total number of alignment line angles: 1',
                    'Structure List Report',
                    '1 0.0 0.0 10.0 0.0 0.0 0.0 C:\models\first.012',
                    '2 10.0 0.0 0.0 0.0 0.0 0.0 C:\models\last.012',
                    'Structure Coordinates Report', 'Structure Material List Report',
                    'Cable Material List Report')
            }
            'section_usage' {
                $body += @('Sections Evaluated',
                    '1 1:1 2:1 10.0 No cable-one 30 None (1) 0.0',
                    '2 2:1 2:2 20.0 No cable-two 30 None (1) 0.0')
            }
            'section_tension' {
                $body += @('Section Sagging Data',
                    '1 cable-one 1 2 30 10.0 Load RS 25.0 1000.0 2000.0',
                    '2 cable-two 2 2 30 20.0 Initial RS 25.0 1000.0 2000.0',
                    'Ruling Span Sag Tension Report', 'detail one',
                    'Ruling Span Sag Tension Report', 'detail two')
            }
        }
        [System.IO.File]::WriteAllText((Join-Path (Join-Path $output 'reports') $report.FileName),
            (($body -join "`r`n") + "`r`n"), [System.Text.UTF8Encoding]::new($false))
    }
    $actions = foreach ($report in $profile.Reports) {
        $reportPath = Join-Path (Join-Path $output 'reports') $report.FileName
        [ordered]@{
            schema = 'ds.pls.report_action.v1'; action = 'start'; report = $report.Key
            command_id = $report.CommandId; process_id = 4242
        } | ConvertTo-Json -Compress
        if (-not [string]::IsNullOrWhiteSpace([string] $report.OptionDialogTitle)) {
            [ordered]@{
                schema = 'ds.pls.report_action.v1'; action = 'accept_options'; report = $report.Key
                dialog_title = $report.OptionDialogTitle; process_id = 4242
            } | ConvertTo-Json -Compress
        }
        [ordered]@{
            schema = 'ds.pls.report_action.v1'; action = 'open_save_as'; report = $report.Key
            save_as_command_id = $profile.ReportSaveAsCommandId; process_id = 4242
        } | ConvertTo-Json -Compress
        [ordered]@{
            schema = 'ds.pls.report_file.v1'; report = $report.Key
            bytes = (Get-Item $reportPath).Length
            sha256 = (Get-FileHash $reportPath -Algorithm SHA256).Hash.ToLowerInvariant()
            process_id = 4242
            path = $reportPath
            active_report_title = "PLS-CADD - model.xyz - $($report.ReportTitlePattern)"
        } | ConvertTo-Json -Compress
    }
    [System.IO.File]::WriteAllText((Join-Path (Join-Path $output 'evidence') 'actions.jsonl'),
        (($actions -join "`n") + "`n"), [System.Text.UTF8Encoding]::new($false))
    $menuReports = [ordered]@{}
    foreach ($report in $profile.Reports) {
        $menuReports[$report.Key] = [ordered]@{
            command_id = $report.CommandId
            matching_menu_paths = @("Lines > Reports > $($report.Key)")
        }
    }
    $menuEvidence = [ordered]@{
        schema = 'ds.pls.menu_evidence.v1'
        executable_sha256 = $digest
        reports = $menuReports
    } | ConvertTo-Json -Depth 10
    $menuEvidencePath = Join-Path (Join-Path $output 'evidence') 'menu-evidence.json'
    [System.IO.File]::WriteAllText($menuEvidencePath,
        "{`"schema`":`"wrong`"}`n", [System.Text.UTF8Encoding]::new($false))
    $restoreEvidence = [ordered]@{
        schema = 'ds.pls.backup_restore_qualification.v1'
        status = 'qualified_backup_roundtrip_local_only'
        executable = [ordered]@{ sha256 = $digest }
        fresh_pls_backup = [ordered]@{
            path = $bak
            container_sha256 = (Get-FileHash $bak -Algorithm SHA256).Hash.ToLowerInvariant()
            protected_equal_to_candidate = $true
        }
        second_restore = [ordered]@{
            project_opened = $xyz
            full_tree_verified_against_fresh_backup = $true
            protected_tree_verified_against_intended_candidate = $true
        }
    } | ConvertTo-Json -Depth 10
    [System.IO.File]::WriteAllText((Join-Path (Join-Path $output 'evidence') 'restore-evidence.json'),
        ($restoreEvidence + "`n"), [System.Text.UTF8Encoding]::new($false))
    $failed = $false
    try {
        & $bundle -Mode Finalize -OutputDirectory $output -ProjectPath $xyz -BackupPath $bak `
            -Requester 'test' -DsRevision 'deadbeef' -ExecutablePath $exe -ProfilePath $testProfile `
            -CoverageProfilePath $testCoverage `
            -PlsLogPath $bak | Out-Null
    } catch { $failed = $_.Exception.Message -like 'Evidence schema mismatch*' }
    Assert-True $failed 'Finalize refuses semantically invalid menu evidence'
    [System.IO.File]::WriteAllText($menuEvidencePath,
        ($menuEvidence + "`n"), [System.Text.UTF8Encoding]::new($false))
    $sectionUsagePath = Join-Path (Join-Path $output 'reports') 'Section Usage Report.txt'
    $validSectionUsage = [System.IO.File]::ReadAllText($sectionUsagePath)
    [System.IO.File]::WriteAllText($sectionUsagePath,
        ($validSectionUsage -replace '(?m)^2 2:1 2:2 20\.0 No cable-two 30 None \(1\) 0\.0\r?\n', ''), [System.Text.UTF8Encoding]::new($false))
    $failed = $false
    try {
        & $bundle -Mode Finalize -OutputDirectory $output -ProjectPath $xyz -BackupPath $bak `
            -Requester 'test' -DsRevision 'deadbeef' -ExecutablePath $exe -ProfilePath $testProfile `
            -CoverageProfilePath $testCoverage -PlsLogPath $bak | Out-Null
    } catch { $failed = $_.Exception.Message -like "Report 'section_usage' row count mismatch:*" }
    Assert-True $failed 'Finalize refuses a report whose sequential native table is incomplete'
    [System.IO.File]::WriteAllText($sectionUsagePath, $validSectionUsage,
        [System.Text.UTF8Encoding]::new($false))
    $sectionUsageAction = @($actions | Where-Object { $_ -match '"schema":"ds.pls.report_file.v1"' -and $_ -match '"report":"section_usage"' })
    Assert-True ($sectionUsageAction.Count -eq 1) 'section-usage file action fixture is unique'
    $updatedSectionUsageAction = [ordered]@{
        schema = 'ds.pls.report_file.v1'; report = 'section_usage'
        bytes = (Get-Item $sectionUsagePath).Length
        sha256 = (Get-FileHash $sectionUsagePath -Algorithm SHA256).Hash.ToLowerInvariant()
        process_id = 4242
        path = $sectionUsagePath
        active_report_title = 'PLS-CADD - model.xyz - Section Usage Report'
    } | ConvertTo-Json -Compress
    $actionLines = @(Get-Content -LiteralPath (Join-Path (Join-Path $output 'evidence') 'actions.jsonl'))
    $actionLines = @($actionLines | ForEach-Object {
        if ($_ -match '"schema":"ds.pls.report_file.v1"' -and $_ -match '"report":"section_usage"') {
            $updatedSectionUsageAction
        } else { $_ }
    })
    [System.IO.File]::WriteAllText((Join-Path (Join-Path $output 'evidence') 'actions.jsonl'),
        (($actionLines -join "`n") + "`n"), [System.Text.UTF8Encoding]::new($false))
    & $bundle -Mode Finalize -OutputDirectory $output -ProjectPath $xyz -BackupPath $bak `
        -Requester 'test' -DsRevision 'deadbeef' -ExecutablePath $exe -ProfilePath $testProfile `
        -CoverageProfilePath $testCoverage `
        -PlsLogPath $bak | Out-Null
    Assert-True (-not (Test-Path (Join-Path $output 'INCOMPLETE.json'))) 'Finalize removes incomplete marker last'
    $manifest = Get-Content (Join-Path $output 'manifest.json') -Raw | ConvertFrom-Json
    Assert-True ($manifest.artifacts.Count -eq 9) 'Manifest contains five reports plus four evidence artifacts'
    Assert-True ($manifest.status -eq 'complete_with_caveat') 'SAPS caveat is retained in completion status'
    Assert-True ($manifest.caveat.accepted_for_local_verification -eq $true) 'Operator acceptance is explicit'
    Assert-True ($manifest.report_coverage.section_usage.row_count -eq 2) `
        'Manifest records the complete section-usage row count'
    Assert-True ($manifest.report_coverage.cross_report.section_identity_rows_cross_checked -eq 2) `
        'Manifest records full cross-report section identity coverage'

    $desktop = Join-Path $fixtureRoot 'desktop'
    [System.IO.Directory]::CreateDirectory($desktop) | Out-Null
    $backupDigest = (Get-FileHash $bak -Algorithm SHA256).Hash.ToLowerInvariant()
    $receiptJson = & $package -ReportBundleDirectory $output -BackupPath $bak `
        -ExpectedBackupSha256 $backupDigest -PackageBaseName 'Rutsiro-Test-Package' `
        -DesktopDirectory $desktop
    $receipt = $receiptJson | ConvertFrom-Json
    Assert-True ($receipt.status -eq 'verified') 'Desktop package receipt is verified'
    Assert-True (Test-Path -LiteralPath (Join-Path $desktop 'Rutsiro-Test-Package.zip')) `
        'Desktop package ZIP exists'
    Assert-True (Test-Path -LiteralPath (Join-Path $desktop 'Rutsiro-Test-Package.zip.manifest.json')) `
        'Desktop package ZIP receipt exists'
    Assert-True (Test-Path -LiteralPath (Join-Path $desktop 'Rutsiro-Test-Package\native-backup\model.bak')) `
        'fresh native backup is staged in the package'
    $failed = $false
    try {
        & $package -ReportBundleDirectory $output -BackupPath $bak `
            -ExpectedBackupSha256 $backupDigest -PackageBaseName 'Rutsiro-Test-Package' `
            -DesktopDirectory $desktop | Out-Null
    } catch { $failed = $_.Exception.Message -like 'Package output already exists:*' }
    Assert-True $failed 'Desktop packager never overwrites an existing package'

    $saveSource = Get-Content -LiteralPath (Join-Path $scriptRoot 'pls-save-report-file.ps1') -Raw
    Assert-True ($saveSource.Contains("(Get-Class `$save) -cne 'Button'")) `
        'report Save As pins control id 1 to the Button class'
    Assert-True ($saveSource.Contains("(Get-Text `$save) -cnotin @('Save', '&Save', 'OK', '&OK')")) `
        'report Save As pins exact Save/OK text'

    $undefinedRule = @($profile.AllowedPromptRules | Where-Object {
        $_.Name -ceq 'undefined_feature_codes'
    })
    Assert-True ($undefinedRule.Count -eq 1) `
        'report prompt profile defines the undefined-feature rule exactly once'
    $knownUndefinedBody = "39 Undefined feature codes found in terrain. Program doesn't know what these points are or what their required clearances are. 7088 XYZ points with unknown feature codes. 0 PFL points with unknown feature codes. Continue displaying warning messages (click No to redirect this and future messages to a report window for remainder of this operation)?"
    Assert-True ($knownUndefinedBody -match $undefinedRule[0].BodyPattern) `
        'undefined-feature rule accepts only the characterized Rutsiro counts/body'
    Assert-True (-not ($knownUndefinedBody.Replace('7088', '7087') -match $undefinedRule[0].BodyPattern)) `
        'undefined-feature rule rejects an uncharacterized XYZ count'
    Assert-True ($undefinedRule[0].Title -ceq 'Undefined Feature Codes' -and
        [int] $undefinedRule[0].ResponseControlId -eq 7 -and
        $undefinedRule[0].ResponseText -ceq 'No') `
        'undefined-feature rule pins the title and exact No response'

    $promptSource = Get-Content -LiteralPath (Join-Path $scriptRoot 'pls-handle-report-prompt.ps1') -Raw
    Assert-True ($promptSource.Contains("'undefined_feature_codes'")) `
        'report prompt driver exposes the characterized undefined-feature rule'
    $closeSource = Get-Content -LiteralPath (Join-Path $scriptRoot 'pls-close-report-session.ps1') -Raw
    Assert-True ($closeSource.Contains("[IntPtr] `$profile.ExitCommandId")) `
        'report close driver sends only the profiled exit command'
    Assert-True ($closeSource.Contains("[int] `$profile.ExitNoControlId") -and
        $closeSource.Contains("@(`$profile.ExitNoTexts) -cnotcontains `$buttonText")) `
        'report close driver pins the exact No control id and text'
    Assert-True ($closeSource.Contains("throw `"Unexpected exit dialog body: '`$body'`"")) `
        'report close driver fails closed on unknown exit bodies'

    Write-Output 'PASS pls-report-bundle-tests'
} finally {
    [System.IO.Directory]::Delete($fixtureRoot, $true)
}
