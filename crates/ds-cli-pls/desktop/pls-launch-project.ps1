param(
    [Parameter(Mandatory = $true)]
    [string] $ExecutablePath,
    [Parameter(Mandatory = $true)]
    [string] $ProjectPath,
    [Parameter(Mandatory = $true)]
    [string] $ExpectedExecutableSha256,
    [int] $TimeoutSeconds = 90
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Resolve-RegularFile([string] $Path, [string] $Label) {
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer) {
        throw "$Label is a directory: $Path"
    }
    return $item.FullName
}

$executable = Resolve-RegularFile $ExecutablePath 'PLS-CADD executable'
$project = Resolve-RegularFile $ProjectPath 'PLS-CADD project'
if ([System.IO.Path]::GetExtension($project) -ine '.xyz') {
    throw "Project must be the .xyz entry point: $project"
}

$actualExecutableSha256 = (Get-FileHash -LiteralPath $executable -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actualExecutableSha256 -ne $ExpectedExecutableSha256.ToLowerInvariant()) {
    throw "PLS-CADD executable digest mismatch: expected $ExpectedExecutableSha256, got $actualExecutableSha256"
}
$projectSha256 = (Get-FileHash -LiteralPath $project -Algorithm SHA256).Hash.ToLowerInvariant()

$running = @(Get-Process -Name 'pls_cadd64' -ErrorAction SilentlyContinue)
if ($running.Count -ne 0) {
    throw "Refusing to launch while PLS-CADD is already running (PID(s): $($running.Id -join ', '))"
}

$startedAt = [DateTime]::UtcNow
$start = @{
    FilePath = $executable
    ArgumentList = @('"' + $project + '"')
    WorkingDirectory = [System.IO.Path]::GetDirectoryName($project)
    PassThru = $true
}
$process = Start-Process @start

$deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
do {
    Start-Sleep -Milliseconds 250
    $process.Refresh()
    if ($process.HasExited) {
        throw "PLS-CADD exited during startup with code $($process.ExitCode)"
    }
} while ($process.MainWindowHandle -eq [IntPtr]::Zero -and [DateTime]::UtcNow -lt $deadline)

if ($process.MainWindowHandle -eq [IntPtr]::Zero) {
    throw "PLS-CADD did not expose a main window within $TimeoutSeconds seconds"
}

[ordered]@{
    schema = 'ds.pls.launch.v1'
    process_id = $process.Id
    main_window_handle = [long] $process.MainWindowHandle
    executable_path = $executable
    executable_sha256 = $actualExecutableSha256
    project_path = $project
    project_sha256 = $projectSha256
    started_at_utc = $startedAt.ToString('o')
} | ConvertTo-Json -Depth 4
