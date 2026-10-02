param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Prepare', 'Finalize')]
    [string] $Mode,
    [Parameter(Mandatory = $true)]
    [string] $OutputDirectory,
    [Parameter(Mandatory = $true)]
    [string] $ProjectPath,
    [Parameter(Mandatory = $true)]
    [string] $BackupPath,
    [Parameter(Mandatory = $true)]
    [string] $Requester,
    [Parameter(Mandatory = $true)]
    [string] $DsRevision,
    [string] $ExecutablePath = 'C:\Program Files\PLS\pls_cadd\pls_cadd64.exe',
    [string] $ProfilePath = (Join-Path $PSScriptRoot 'pls-report-profile.psd1'),
    [string] $CoverageProfilePath = (Join-Path $PSScriptRoot 'pls-rutsiro-report-coverage.psd1'),
    [string] $PlsLogPath = (Join-Path $env:APPDATA 'PLS\temp\PLS-CADD.log'),
    [switch] $AcceptUnlicensedSaps
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Resolve-RegularFile([string] $Path, [string] $Label) {
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer) { throw "$Label is a directory: $Path" }
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "$Label may not be a reparse point: $Path"
    }
    return $item.FullName
}

function Get-Artifact([string] $Root, [string] $Path, [string] $Role) {
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer) { throw "Artifact is a directory: $Path" }
    if (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "Artifact may not be a reparse point: $Path"
    }
    $rootUri = [Uri] ((Resolve-Path -LiteralPath $Root).Path.TrimEnd('\') + '\')
    $itemUri = [Uri] $item.FullName
    $relative = [Uri]::UnescapeDataString($rootUri.MakeRelativeUri($itemUri).ToString())
    [ordered]@{
        role = $Role
        path = $relative
        bytes = $item.Length
        sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}

function Write-JsonCreateNew([string] $Path, [object] $Value) {
    $json = $Value | ConvertTo-Json -Depth 20
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($json + "`n")
    $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write, [System.IO.FileShare]::None)
    try {
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally {
        $stream.Dispose()
    }
}

function Test-SameFullPath([string] $Left, [string] $Right) {
    return [System.IO.Path]::GetFullPath($Left).Equals(
        [System.IO.Path]::GetFullPath($Right),
        [System.StringComparison]::OrdinalIgnoreCase)
}

function Read-NativeReportText([string] $Path) {
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $text = [System.Text.Encoding]::GetEncoding(1252).GetString($bytes)
    if ($text.StartsWith('{\rtf', [StringComparison]::Ordinal)) {
        # RTF represents a literal path separator as two backslashes. Protect
        # those before removing control words, otherwise a path segment such as
        # `\\structures` is misread as the `\structures` control word.
        $escapedBackslash = '__DS_RTF_LITERAL_BACKSLASH__'
        $text = $text.Replace('\\', $escapedBackslash)
        $text = [Regex]::Replace($text, '\\par[d]?\b', "`n")
        $text = [Regex]::Replace($text, "\\'[0-9a-fA-F]{2}", ' ')
        $text = [Regex]::Replace($text, '\\[a-zA-Z]+-?\d*\s?', ' ')
        $text = $text.Replace('{', ' ').Replace('}', ' ').Replace($escapedBackslash, '\')
    }
    return $text.Replace("`r`n", "`n").Replace("`r", "`n")
}

function Get-ExactMarkerIndex {
    param([string[]] $Lines, [string] $Marker, [string] $ReportKey)
    $matches = New-Object System.Collections.ArrayList
    for ($index = 0; $index -lt $Lines.Count; $index++) {
        if ($Lines[$index].Trim() -ceq $Marker) { $matches.Add($index) | Out-Null }
    }
    if ($matches.Count -ne 1) {
        throw "Report '$ReportKey' must contain exactly one '$Marker' marker; found $($matches.Count)"
    }
    return [int] $matches[0]
}

function Get-BoundedLines {
    param([string[]] $Lines, [int] $Start, [int] $End, [string] $ReportKey)
    if ($Start -ge $End - 1) { throw "Report '$ReportKey' has an empty or reversed native table" }
    return @($Lines[($Start + 1)..($End - 1)])
}

function Assert-SequentialRecords {
    param([object[]] $Records, [int] $ExpectedRows, [string] $ReportKey)
    if ($Records.Count -ne $ExpectedRows) {
        throw "Report '$ReportKey' row count mismatch: expected $ExpectedRows, got $($Records.Count)"
    }
    $seen = @{}
    for ($index = 0; $index -lt $Records.Count; $index++) {
        $expectedId = $index + 1
        $actualId = [int] $Records[$index].id
        if ($actualId -ne $expectedId) {
            throw "Report '$ReportKey' IDs are not the exact sequential vector 1..${ExpectedRows}: row $expectedId has id $actualId"
        }
        if ($seen.ContainsKey($actualId)) { throw "Report '$ReportKey' repeats id $actualId" }
        $seen[$actualId] = $true
    }
}

function Assert-ExpectedIdVector {
    param([object[]] $Records, [int[]] $ExpectedIds, [string] $Label)
    if ($Records.Count -ne $ExpectedIds.Count) {
        throw "$Label row count mismatch: expected $($ExpectedIds.Count), got $($Records.Count)"
    }
    for ($index = 0; $index -lt $ExpectedIds.Count; $index++) {
        if ([int] $Records[$index].id -ne [int] $ExpectedIds[$index]) {
            throw "$Label ID vector mismatch at row $($index + 1)"
        }
    }
}

function ConvertTo-DoubleInvariant([string] $Value) {
    return [double]::Parse($Value, [System.Globalization.CultureInfo]::InvariantCulture)
}

function Assert-ReportCoverage {
    param(
        [Parameter(Mandatory = $true)][string] $Path,
        [Parameter(Mandatory = $true)][object] $Report,
        [Parameter(Mandatory = $true)][object] $Coverage,
        [Parameter(Mandatory = $true)][string] $ExpectedDonPath
    )

    $item = Get-Item -LiteralPath $Path -Force
    if ($item.Length -lt [long] $Coverage.MinimumBytes) {
        throw "Report '$($Report.Key)' is too small for its characterized coverage: $($item.Length) < $($Coverage.MinimumBytes) bytes"
    }
    $text = Read-NativeReportText $item.FullName
    $lines = @($text -split "`n")
    $headers = @($lines | Where-Object { $_.Trim() -match '^PLS-CADD Version 16\.81x64\b' })
    if ($headers.Count -ne 1) { throw "Report '$($Report.Key)' must have one pinned PLS-CADD 16.81x64 header" }
    $projectLine = "Project Name: '$ExpectedDonPath'"
    if (@($lines | Where-Object { $_.Trim() -ceq $projectLine }).Count -ne 1) {
        throw "Report '$($Report.Key)' is not stamped with the exact restored project '$ExpectedDonPath'"
    }
    foreach ($literal in @($Coverage.RequiredLiterals)) {
        if ($text.IndexOf([string] $literal, [StringComparison]::Ordinal) -lt 0) {
            throw "Report '$($Report.Key)' lacks required native marker '$literal'"
        }
    }

    $evidence = [ordered]@{
        minimum_bytes = [long] $Coverage.MinimumBytes
        actual_bytes = [long] $item.Length
        exact_project_don = $ExpectedDonPath
        required_literals = @($Coverage.RequiredLiterals)
    }
    $records = New-Object System.Collections.ArrayList

    switch ([string] $Report.Key) {
        'structure_usage' {
            $start = Get-ExactMarkerIndex $lines 'Structure Locations and Usage Report' $Report.Key
            $summaryRows = New-Object System.Collections.ArrayList
            for ($index = $start + 1; $index -lt $lines.Count; $index++) {
                $match = [Regex]::Match($lines[$index],
                    '^\s*(\d+) structure violations, (\d+) structure warnings\s+(OK|NG)\s*$')
                if ($match.Success) {
                    $summaryRows.Add([ordered]@{ index = $index; match = $match }) | Out-Null
                }
            }
            if ($summaryRows.Count -ne 1) { throw "Report '$($Report.Key)' must contain one violation summary" }
            foreach ($line in (Get-BoundedLines $lines $start ([int] $summaryRows[0].index) $Report.Key)) {
                $match = [Regex]::Match($line, '^\s*(\d+)\s+(\S+)\s+(.*)$')
                if (-not $match.Success) { continue }
                $rest = $match.Groups[3].Value
                $numbers = [Regex]::Matches($rest,
                    '(?<!\S)[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?!\S)')
                $verdicts = [Regex]::Matches($rest, '(?<!\S)(?:OK|NG)(?!\S)')
                if ($numbers.Count -lt 4 -or $verdicts.Count -lt 2) { continue }
                $records.Add([ordered]@{
                    id = [int] $match.Groups[1].Value
                    name = $match.Groups[2].Value
                    overall = $verdicts[$verdicts.Count - 1].Value
                }) | Out-Null
            }
            Assert-SequentialRecords @($records) ([int] $Coverage.ExpectedRows) $Report.Key
            if ([string] $records[0].name -cne [string] $Coverage.FirstStructureName -or
                [string] $records[$records.Count - 1].name -cne [string] $Coverage.LastStructureName) {
                throw "Report '$($Report.Key)' immutable endpoint structure names do not match"
            }
            $violationCount = @($records | Where-Object { $_.overall -ceq 'NG' }).Count
            if ([int] $summaryRows[0].match.Groups[1].Value -ne $violationCount) {
                throw "Report '$($Report.Key)' violation summary does not reconcile to its rows"
            }
            $evidence['row_count'] = $records.Count
            $evidence['sequential_ids'] = $true
            $evidence['violation_count'] = $violationCount
        }
        'wind_weight_span' {
            $mainStart = Get-ExactMarkerIndex $lines 'Heaviest Cable Wind & Weight Spans' $Report.Key
            $attachmentStart = Get-ExactMarkerIndex $lines 'Wind & Weight Spans By Attachment Set' $Report.Key
            $sideStart = Get-ExactMarkerIndex $lines 'Wind & Weight Spans By Side' $Report.Key
            if ($mainStart -ge $attachmentStart -or $attachmentStart -ge $sideStart) {
                throw "Report '$($Report.Key)' table markers are out of order"
            }
            $rowPattern = '^\s*(\d+)\s+([-+]?\d+(?:\.\d+)?)\s+([A-Za-z])\s+(\S+)\s+([-+]?\d+(?:\.\d+)?)\s+'
            foreach ($line in (Get-BoundedLines $lines $mainStart $attachmentStart $Report.Key)) {
                $match = [Regex]::Match($line, $rowPattern)
                if ($match.Success) {
                    $records.Add([ordered]@{ id = [int] $match.Groups[1].Value; name = $match.Groups[4].Value }) | Out-Null
                }
            }
            Assert-SequentialRecords @($records) ([int] $Coverage.ExpectedRows) $Report.Key
            if ([string] $records[0].name -cne [string] $Coverage.FirstStructureName -or
                [string] $records[$records.Count - 1].name -cne [string] $Coverage.LastStructureName) {
                throw "Report '$($Report.Key)' immutable endpoint structure names do not match"
            }
            $expectedSecondaryIds = @(1..([int] $Coverage.ExpectedRows) | Where-Object {
                [int[]] $Coverage.UnstrungStructureIds -notcontains $_
            })
            $attachmentRecords = New-Object System.Collections.ArrayList
            foreach ($line in (Get-BoundedLines $lines $attachmentStart $sideStart $Report.Key)) {
                $match = [Regex]::Match($line, $rowPattern)
                if ($match.Success) { $attachmentRecords.Add([ordered]@{ id = [int] $match.Groups[1].Value }) | Out-Null }
            }
            $sideRecords = New-Object System.Collections.ArrayList
            foreach ($line in @($lines[($sideStart + 1)..($lines.Count - 1)])) {
                $match = [Regex]::Match($line, $rowPattern)
                if ($match.Success) { $sideRecords.Add([ordered]@{ id = [int] $match.Groups[1].Value }) | Out-Null }
            }
            Assert-ExpectedIdVector @($attachmentRecords) $expectedSecondaryIds "Report '$($Report.Key)' attachment-set table"
            Assert-ExpectedIdVector @($sideRecords) $expectedSecondaryIds "Report '$($Report.Key)' side table"
            $evidence['main_row_count'] = $records.Count
            $evidence['attachment_structure_count'] = $attachmentRecords.Count
            $evidence['side_structure_count'] = $sideRecords.Count
            $evidence['unstrung_structure_ids'] = @([int[]] $Coverage.UnstrungStructureIds)
        }
        'summary' {
            $lineStatistics = Get-ExactMarkerIndex $lines 'Line Statistics:' $Report.Key
            $listStart = Get-ExactMarkerIndex $lines 'Structure List Report' $Report.Key
            $coordinates = Get-ExactMarkerIndex $lines 'Structure Coordinates Report' $Report.Key
            $structureMaterial = Get-ExactMarkerIndex $lines 'Structure Material List Report' $Report.Key
            $cableMaterial = Get-ExactMarkerIndex $lines 'Cable Material List Report' $Report.Key
            if (-not ($lineStatistics -lt $listStart -and $listStart -lt $coordinates -and
                $coordinates -lt $structureMaterial -and $structureMaterial -lt $cableMaterial)) {
                throw "Report '$($Report.Key)' native subreports are missing or out of order"
            }
            $rowPattern = '^\s*(\d+)\s+[-+]?\d+(?:\.\d+)?(?:\s+[-+]?\d+(?:\.\d+)?){5}\s+(.+?)\s*$'
            foreach ($line in (Get-BoundedLines $lines $listStart $coordinates $Report.Key)) {
                $match = [Regex]::Match($line, $rowPattern)
                if ($match.Success) {
                    $records.Add([ordered]@{
                        id = [int] $match.Groups[1].Value
                        name = [System.IO.Path]::GetFileName($match.Groups[2].Value.Trim())
                    }) | Out-Null
                }
            }
            Assert-SequentialRecords @($records) ([int] $Coverage.ExpectedRows) $Report.Key
            $structuresFacts = @($lines | ForEach-Object {
                $match = [Regex]::Match($_, '^\s*Total number of structures used:\s*(\d+)\s*$')
                if ($match.Success) { [int] $match.Groups[1].Value }
            })
            $sectionFacts = @($lines | ForEach-Object {
                $match = [Regex]::Match($_, '^\s*Total number of sections:\s*(\d+)\s*$')
                if ($match.Success) { [int] $match.Groups[1].Value }
            })
            if ($structuresFacts.Count -ne 1 -or
                [int[]] $Coverage.AllowedStructuresUsed -notcontains [int] $structuresFacts[0]) {
                throw "Report '$($Report.Key)' structures-used fact is not an allowed native value"
            }
            if ($sectionFacts.Count -ne 1 -or [int] $sectionFacts[0] -ne [int] $Coverage.ExpectedNativeSections) {
                throw "Report '$($Report.Key)' native section count mismatch"
            }
            $evidence['structure_list_rows'] = $records.Count
            $evidence['structures_used_fact'] = [int] $structuresFacts[0]
            $evidence['native_sections_fact'] = [int] $sectionFacts[0]
            $evidence['terminal_material_subreports_present'] = $true
        }
        'section_usage' {
            $start = Get-ExactMarkerIndex $lines 'Sections Evaluated' $Report.Key
            $rowPattern = '^\s*(\d+)\s+(\d+):(\d+)\s+(\d+):(\d+)\s+([-+]?\d+(?:\.\d+)?)\s+(Yes|No)\s+(.+)$'
            foreach ($line in @($lines[($start + 1)..($lines.Count - 1)])) {
                $match = [Regex]::Match($line, $rowPattern)
                if (-not $match.Success) { continue }
                $from = [int] $match.Groups[2].Value
                $to = [int] $match.Groups[4].Value
                if ($from -lt 1 -or $from -gt 1129 -or $to -lt 1 -or $to -gt 1129 -or
                    [int] $match.Groups[3].Value -lt 1 -or [int] $match.Groups[5].Value -lt 1) {
                    throw "Report '$($Report.Key)' has an invalid structure/set endpoint"
                }
                $records.Add([ordered]@{
                    id = [int] $match.Groups[1].Value
                    from = $from
                    to = $to
                    ruling_span = ConvertTo-DoubleInvariant $match.Groups[6].Value
                    tail = $match.Groups[8].Value.Trim()
                }) | Out-Null
            }
            Assert-SequentialRecords @($records) ([int] $Coverage.ExpectedRows) $Report.Key
            $evidence['row_count'] = $records.Count
            $evidence['sequential_ids'] = $true
        }
        'section_tension' {
            $start = Get-ExactMarkerIndex $lines 'Section Sagging Data' $Report.Key
            $detailIndexes = New-Object System.Collections.ArrayList
            for ($index = $start + 1; $index -lt $lines.Count; $index++) {
                if ($lines[$index].Trim() -ceq 'Ruling Span Sag Tension Report') {
                    $detailIndexes.Add($index) | Out-Null
                }
            }
            if ($detailIndexes.Count -ne [int] $Coverage.ExpectedDetailReports) {
                throw "Report '$($Report.Key)' detail-report count mismatch: expected $($Coverage.ExpectedDetailReports), got $($detailIndexes.Count)"
            }
            $rowPattern = '^\s*(\d+)\s+(.+?)\s+(\d+)\s+(\d+)\s+([-+]?\d+(?:\.\d+)?)\s+([-+]?\d+(?:\.\d+)?)\s+(Load RS|Initial RS|Creep RS)\s+([-+]?\d+(?:\.\d+)?)\s+([-+]?\d+(?:\.\d+)?)\s+([-+]?\d+(?:\.\d+)?)(?:\s|$)'
            foreach ($line in (Get-BoundedLines $lines $start ([int] $detailIndexes[0]) $Report.Key)) {
                $match = [Regex]::Match($line, $rowPattern)
                if (-not $match.Success) { continue }
                $records.Add([ordered]@{
                    id = [int] $match.Groups[1].Value
                    cable = $match.Groups[2].Value.Trim()
                    from = [int] $match.Groups[3].Value
                    to = [int] $match.Groups[4].Value
                    ruling_span = ConvertTo-DoubleInvariant $match.Groups[6].Value
                }) | Out-Null
            }
            Assert-SequentialRecords @($records) ([int] $Coverage.ExpectedRows) $Report.Key
            $evidence['row_count'] = $records.Count
            $evidence['sequential_ids'] = $true
            $evidence['detail_report_count'] = $detailIndexes.Count
        }
        default { throw "No bounded native report parser for '$($Report.Key)'" }
    }
    return [pscustomobject]@{ evidence = $evidence; records = @($records) }
}

function Assert-CrossReportCoverage([hashtable] $ParsedReports) {
    foreach ($key in @('structure_usage', 'wind_weight_span', 'summary', 'section_usage', 'section_tension')) {
        if (-not $ParsedReports.ContainsKey($key)) { throw "Missing parsed report '$key'" }
    }
    $usage = @($ParsedReports['structure_usage'])
    $wind = @($ParsedReports['wind_weight_span'])
    $summary = @($ParsedReports['summary'])
    for ($index = 0; $index -lt $usage.Count; $index++) {
        if ([int] $usage[$index].id -ne [int] $wind[$index].id -or
            [int] $usage[$index].id -ne [int] $summary[$index].id -or
            [string] $usage[$index].name -cne [string] $wind[$index].name -or
            [string] $usage[$index].name -cne [string] $summary[$index].name) {
            throw "Structure identity differs across native reports at row $($index + 1)"
        }
    }
    $sectionUsage = @($ParsedReports['section_usage'])
    $sectionTension = @($ParsedReports['section_tension'])
    for ($index = 0; $index -lt $sectionUsage.Count; $index++) {
        $left = $sectionUsage[$index]
        $right = $sectionTension[$index]
        if ([int] $left.id -ne [int] $right.id -or [int] $left.from -ne [int] $right.from -or
            [int] $left.to -ne [int] $right.to -or
            [Math]::Abs([double] $left.ruling_span - [double] $right.ruling_span) -gt 0.051 -or
            ([string] $left.tail).IndexOf([string] $right.cable,
                [StringComparison]::OrdinalIgnoreCase) -lt 0) {
            throw "Section identity differs across native reports at row $($index + 1)"
        }
    }
    return [ordered]@{
        structure_identity_rows_cross_checked = $usage.Count
        section_identity_rows_cross_checked = $sectionUsage.Count
        dsgrid_canonical_tension_rows = 3788
        native_pls_sections = $sectionUsage.Count
        count_namespaces_intentionally_distinct = $true
    }
}

function Read-JsonEvidence([string] $Path, [string] $ExpectedSchema) {
    $resolved = Resolve-RegularFile $Path 'evidence file'
    $value = Get-Content -LiteralPath $resolved -Raw | ConvertFrom-Json
    if ([string] $value.schema -cne $ExpectedSchema) {
        throw "Evidence schema mismatch in '$([System.IO.Path]::GetFileName($Path))': expected '$ExpectedSchema'"
    }
    return $value
}

$profile = Import-PowerShellDataFile -LiteralPath $ProfilePath
$coverageProfileFile = Resolve-RegularFile $CoverageProfilePath 'report coverage profile'
$coverageProfile = Import-PowerShellDataFile -LiteralPath $coverageProfileFile
if ([string] $coverageProfile.Schema -cne 'ds.pls.report_coverage_profile.v1') {
    throw 'Unsupported report coverage profile schema'
}
$executable = Resolve-RegularFile $ExecutablePath 'PLS-CADD executable'
$project = Resolve-RegularFile $ProjectPath 'restored project entry point'
$backup = Resolve-RegularFile $BackupPath 'source backup'
if ([System.IO.Path]::GetExtension($project) -ine '.xyz') {
    throw "Project must be the restored .xyz entry point: $project"
}
if ([System.IO.Path]::GetExtension($backup) -ine '.bak') {
    throw "Backup must have a .bak extension: $backup"
}
if ([System.IO.Path]::GetFileName($project) -cne [string] $coverageProfile.ProjectFileName) {
    throw "Coverage profile project mismatch: expected '$($coverageProfile.ProjectFileName)', got '$([System.IO.Path]::GetFileName($project))'"
}
$coverageProfileDigest = (Get-FileHash -LiteralPath $coverageProfileFile -Algorithm SHA256).Hash.ToLowerInvariant()

$executableDigest = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
if ($executableDigest -ne $profile.ExecutableSha256) {
    throw "Executable digest does not match the version profile"
}

if ($Mode -eq 'Prepare') {
    if (Test-Path -LiteralPath $OutputDirectory) {
        throw "Output directory already exists: $OutputDirectory"
    }
    [System.IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
    [System.IO.Directory]::CreateDirectory((Join-Path $OutputDirectory 'reports')) | Out-Null
    [System.IO.Directory]::CreateDirectory((Join-Path $OutputDirectory 'evidence')) | Out-Null

    $request = [ordered]@{
        schema = 'ds.pls.report_bundle_request.v1'
        status = 'prepared'
        prepared_at_utc = [DateTime]::UtcNow.ToString('o')
        requester = $Requester
        ds_revision = $DsRevision
        product_profile = $profile.ProductVersion
        executable_path = $executable
        executable_sha256 = $executableDigest
        coverage_profile_path = $coverageProfileFile
        coverage_profile_sha256 = $coverageProfileDigest
        restored_project_path = $project
        restored_project_sha256 = (Get-FileHash -LiteralPath $project -Algorithm SHA256).Hash.ToLowerInvariant()
        source_backup_path = $backup
        source_backup_sha256 = (Get-FileHash -LiteralPath $backup -Algorithm SHA256).Hash.ToLowerInvariant()
        required_reports = @($profile.Reports | ForEach-Object {
            [ordered]@{
                key = $_.Key
                command_id = $_.CommandId
                expected_file = ('reports/' + $_.FileName)
            }
        })
        caveat = [ordered]@{
            code = 'saps_unlicensed_pls_16_81'
            accepted_for_local_verification = [bool] $AcceptUnlicensedSaps
            text = 'PLS-CADD 16.81 reports SAPS is not licensed and substitutes Initial RS for unsupported wire conditions 2/3; PLS-CADD states results will be incorrect. These reports are characterization/local verification only unless rerun on a SAPS-equipped supported installation or explicitly accepted by the responsible operator.'
        }
    }
    Write-JsonCreateNew (Join-Path $OutputDirectory 'request.json') $request
    Write-JsonCreateNew (Join-Path $OutputDirectory 'INCOMPLETE.json') ([ordered]@{
        schema = 'ds.pls.incomplete.v1'
        reason = 'five raw PLS-CADD reports and run evidence have not yet been finalized'
    })
    $request | ConvertTo-Json -Depth 20
    exit 0
}

$requestPath = Join-Path $OutputDirectory 'request.json'
$incompletePath = Join-Path $OutputDirectory 'INCOMPLETE.json'
if (-not (Test-Path -LiteralPath $requestPath -PathType Leaf) -or
    -not (Test-Path -LiteralPath $incompletePath -PathType Leaf)) {
    throw 'Output directory was not prepared or has already been finalized'
}
$request = Get-Content -LiteralPath $requestPath -Raw | ConvertFrom-Json
if ($request.requester -cne $Requester -or $request.ds_revision -cne $DsRevision) {
    throw 'Requester or DS revision differs from Prepare'
}
if ([string] $request.coverage_profile_sha256 -cne $coverageProfileDigest) {
    throw 'Report coverage profile changed since Prepare'
}
if ($request.source_backup_sha256 -ne (Get-FileHash -LiteralPath $backup -Algorithm SHA256).Hash.ToLowerInvariant()) {
    throw 'Source backup changed since Prepare'
}
if ($request.restored_project_sha256 -ne (Get-FileHash -LiteralPath $project -Algorithm SHA256).Hash.ToLowerInvariant()) {
    throw 'Restored project .xyz changed since Prepare'
}

$artifacts = New-Object System.Collections.ArrayList
$coverageResults = [ordered]@{}
$parsedReports = @{}
$expectedDonPath = [System.IO.Path]::ChangeExtension($project, '.don')
foreach ($report in $profile.Reports) {
    $path = Join-Path (Join-Path $OutputDirectory 'reports') $report.FileName
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Required raw report missing: $($report.FileName)"
    }
    $item = Get-Item -LiteralPath $path
    if ($item.Length -eq 0) { throw "Required raw report is empty: $($report.FileName)" }
    if (-not $coverageProfile.Reports.ContainsKey([string] $report.Key)) {
        throw "Coverage profile does not define report '$($report.Key)'"
    }
    $parsed = Assert-ReportCoverage $path $report `
        $coverageProfile.Reports[[string] $report.Key] $expectedDonPath
    $coverageResults[$report.Key] = $parsed.evidence
    $parsedReports[$report.Key] = @($parsed.records)
    $artifacts.Add((Get-Artifact $OutputDirectory $path $report.Key)) | Out-Null
}
$coverageResults['cross_report'] = Assert-CrossReportCoverage $parsedReports

$evidenceDirectory = Join-Path $OutputDirectory 'evidence'
$requiredEvidence = @('actions.jsonl', 'menu-evidence.json', 'restore-evidence.json')
foreach ($leaf in $requiredEvidence) {
    $path = Join-Path $evidenceDirectory $leaf
    if (-not (Test-Path -LiteralPath $path -PathType Leaf) -or (Get-Item -LiteralPath $path).Length -eq 0) {
        throw "Required run evidence missing or empty: $leaf"
    }
}

$actionRecords = @(Get-Content -LiteralPath (Join-Path $evidenceDirectory 'actions.jsonl') |
    Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
    ForEach-Object { $_ | ConvertFrom-Json })
$menuEvidence = Read-JsonEvidence (Join-Path $evidenceDirectory 'menu-evidence.json') 'ds.pls.menu_evidence.v1'
if ([string] $menuEvidence.executable_sha256 -cne $executableDigest) {
    throw 'Menu evidence was not produced from the pinned PLS-CADD executable'
}
$restoreEvidence = Read-JsonEvidence (Join-Path $evidenceDirectory 'restore-evidence.json') 'ds.pls.backup_restore_qualification.v1'
if ([string] $restoreEvidence.status -cne 'qualified_backup_roundtrip_local_only' -or
    [string] $restoreEvidence.executable.sha256 -cne $executableDigest -or
    -not [bool] $restoreEvidence.fresh_pls_backup.protected_equal_to_candidate -or
    -not [bool] $restoreEvidence.second_restore.full_tree_verified_against_fresh_backup -or
    -not [bool] $restoreEvidence.second_restore.protected_tree_verified_against_intended_candidate) {
    throw 'Restore evidence does not prove the required two-restore protected round trip'
}
if ([string] $restoreEvidence.fresh_pls_backup.container_sha256 -cne
        [string] $request.source_backup_sha256 -or
    -not (Test-SameFullPath ([string] $restoreEvidence.fresh_pls_backup.path) $backup) -or
    -not (Test-SameFullPath ([string] $restoreEvidence.second_restore.project_opened) $project)) {
    throw 'Report source is not the exact last PLS-created backup and second restored project'
}

$reportProcessIds = New-Object System.Collections.ArrayList
foreach ($report in $profile.Reports) {
    $menuProperty = $menuEvidence.reports.PSObject.Properties[$report.Key]
    if ($null -eq $menuProperty -or
        [int] $menuProperty.Value.command_id -ne [int] $report.CommandId -or
        @($menuProperty.Value.matching_menu_paths).Count -lt 1) {
        throw "Menu evidence does not prove command $($report.CommandId) for report '$($report.Key)'"
    }
    $starts = @($actionRecords | Where-Object {
        $_.schema -eq 'ds.pls.report_action.v1' -and $_.action -eq 'start' -and
        $_.report -eq $report.Key -and $_.command_id -eq $report.CommandId
    })
    $saveActions = @($actionRecords | Where-Object {
        $_.schema -eq 'ds.pls.report_action.v1' -and $_.action -eq 'open_save_as' -and
        $_.report -eq $report.Key -and $_.save_as_command_id -eq $profile.ReportSaveAsCommandId
    })
    $optionActions = @($actionRecords | Where-Object {
        $_.schema -eq 'ds.pls.report_action.v1' -and $_.action -eq 'accept_options' -and
        $_.report -eq $report.Key
    })
    $files = @($actionRecords | Where-Object {
        $_.schema -eq 'ds.pls.report_file.v1' -and $_.report -eq $report.Key
    })
    if ($starts.Count -ne 1 -or $saveActions.Count -ne 1 -or $files.Count -ne 1) {
        throw "Action evidence incomplete or ambiguous for report '$($report.Key)'"
    }
    if ([string]::IsNullOrWhiteSpace([string] $report.OptionDialogTitle)) {
        if ($optionActions.Count -ne 0) {
            throw "Unexpected options-dialog action for report '$($report.Key)'"
        }
    } elseif ($optionActions.Count -ne 1 -or
        [string] $optionActions[0].dialog_title -cne [string] $report.OptionDialogTitle) {
        throw "Options-dialog evidence incomplete or mismatched for report '$($report.Key)'"
    }
    $artifact = @($artifacts | Where-Object { $_.role -eq $report.Key })
    if ($artifact.Count -ne 1 -or $files[0].sha256 -ne $artifact[0].sha256 -or
        [long] $files[0].bytes -ne [long] $artifact[0].bytes -or
        -not (Test-SameFullPath ([string] $files[0].path) (Join-Path (Join-Path $OutputDirectory 'reports') $report.FileName)) -or
        ([string] $files[0].active_report_title).IndexOf(
            [string] $report.ReportTitlePattern,
            [StringComparison]::OrdinalIgnoreCase) -lt 0) {
        throw "Saved-file action evidence does not match raw bytes for report '$($report.Key)'"
    }
    $ids = @(@($starts[0].process_id, $saveActions[0].process_id, $files[0].process_id) |
        ForEach-Object { [int] $_ } | Sort-Object -Unique)
    if ($ids.Count -ne 1 -or $ids[0] -le 0) {
        throw "Report '$($report.Key)' was not produced in one identified PLS-CADD process"
    }
    $reportProcessIds.Add($ids[0]) | Out-Null
}
if (@($reportProcessIds | Sort-Object -Unique).Count -ne 1) {
    throw 'The five reports were not produced in one PLS-CADD process'
}

$logCopy = Join-Path $evidenceDirectory 'PLS-CADD.log'
if (-not (Test-Path -LiteralPath $logCopy -PathType Leaf)) {
    Copy-Item -LiteralPath (Resolve-RegularFile $PlsLogPath 'PLS-CADD log') -Destination $logCopy
}
foreach ($evidence in Get-ChildItem -LiteralPath $evidenceDirectory -File | Sort-Object Name) {
    $artifacts.Add((Get-Artifact $OutputDirectory $evidence.FullName ('evidence_' + $evidence.Name))) | Out-Null
}

$manifest = [ordered]@{
    schema = 'ds.pls.report_bundle_manifest.v1'
    status = 'complete_with_caveat'
    finalized_at_utc = [DateTime]::UtcNow.ToString('o')
    requester = $Requester
    ds_revision = $DsRevision
    product_profile = $profile.ProductVersion
    executable_sha256 = $executableDigest
    source_backup_sha256 = $request.source_backup_sha256
    restored_project_sha256 = $request.restored_project_sha256
    report_source_identity_pinned_to_prepare = $true
    report_process_id = [int] $reportProcessIds[0]
    backup_restore_qualification_status = $restoreEvidence.status
    report_coverage = $coverageResults
    caveat = $request.caveat
    artifacts = @($artifacts | Sort-Object path)
}
$manifestPath = Join-Path $OutputDirectory 'manifest.json'
Write-JsonCreateNew $manifestPath $manifest
[System.IO.File]::Delete($incompletePath)
$manifest | ConvertTo-Json -Depth 20
