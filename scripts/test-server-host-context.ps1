# Native Windows Server launch context regression. Builds only a tiny fixture
# executable; cargo/git are mocked and no DS account, project or service is used.
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ScratchRoot,
    [string]$ScriptsDirectory = $PSScriptRoot
)
$ErrorActionPreference = 'Stop'
if ($env:OS -ne 'Windows_NT' -or $PSVersionTable.PSEdition -ne 'Desktop') {
    throw 'Run this native fixture with Windows PowerShell 5.1.'
}
. (Join-Path $ScriptsDirectory 'windows-server-package.ps1')
$scratch = Assert-ServerLocalPath $ScratchRoot
$scripts = Assert-ServerLocalPath $ScriptsDirectory
foreach ($name in @('start-windows-server.ps1', 'run-windows-server.ps1', 'windows-server-package.ps1')) {
    if (-not (Test-Path -LiteralPath (Join-Path $scripts $name) -PathType Leaf)) {
        throw "Missing test input: $name"
    }
}
$runRoot = Join-Path $scratch ('server-host-context-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($runRoot) | Out-Null
$compilerRoot = Join-Path $runRoot 'compiler'
[IO.Directory]::CreateDirectory($compilerRoot) | Out-Null
$environmentNames = @('DS_CLI_HOST_KIND', 'DS_SERVER_HOST_TEST_RECEIPT',
    'DS_SERVER_HOST_TEST_EXIT', 'LOCALAPPDATA', 'XDG_STATE_HOME',
    'DS_NATIVE_CLIENT_PROFILE_BUNDLE', 'DS_NATIVE_CLIENT_PROFILE_SHA256',
    'DS_NATIVE_CLIENT_PRODUCT_ROOT', 'TEMP', 'TMP')
$previousEnvironment = @{}
foreach ($name in $environmentNames) {
    $previousEnvironment[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
function Assert-Test([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}
function Write-TestText([string]$Path, [string]$Text) {
    [IO.File]::WriteAllText($Path, $Text, [Text.UTF8Encoding]::new($false))
}
$cases = [Collections.Generic.List[object]]::new()
try {
    # All compiler scratch and synthetic state stay under the admitted disk root.
    $env:TEMP = $compilerRoot
    $env:TMP = $compilerRoot
    $env:LOCALAPPDATA = Join-Path $runRoot 'localappdata'
    $env:XDG_STATE_HOME = Join-Path $runRoot 'installed-state'
    $env:DS_NATIVE_CLIENT_PROFILE_BUNDLE = $null
    $env:DS_NATIVE_CLIENT_PROFILE_SHA256 = $null
    $env:DS_NATIVE_CLIENT_PRODUCT_ROOT = $null
    $source = @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Security.Cryptography;
using System.Web.Script.Serialization;

public static class ServerHostFixture
{
    private static readonly JavaScriptSerializer Json = new JavaScriptSerializer();
    private static readonly string Sha = new string('a', 40);
    private static Dictionary<string, object> Fields(params object[] fields)
    {
        var result = new Dictionary<string, object>();
        for (int i = 0; i < fields.Length; i += 2)
            result.Add((string)fields[i], fields[i + 1]);
        return result;
    }
    private static int Answer(object data)
    {
        Console.WriteLine(Json.Serialize(Fields("status", "ok", "data", data)));
        return 0;
    }
    private static string CatalogDigest(string root)
    {
        using (var hash = SHA256.Create())
            return BitConverter.ToString(hash.ComputeHash(File.ReadAllBytes(
                Path.Combine(root, "ds-client-profiles", "catalog.json"))))
                .Replace("-", "").ToLowerInvariant();
    }
    public static int Main(string[] args)
    {
        string root = AppDomain.CurrentDomain.BaseDirectory;
        string call = string.Join(" ", args);
        if (call == "version --output json")
            return Answer(Fields("dirty", false, "profile", "release",
                "target", "x86_64-pc-windows-msvc", "source_sha", Sha,
                "command_kernel_source_sha", Sha, "ds_network_source_sha", Sha,
                "ds_network_source_state", "release_pin",
                "native_client_profile_catalog_sha256", CatalogDigest(root)));
        if (call == "doctor --output json")
            return Answer(Fields("skills", Fields("source_sha", Sha, "status", "ready")));
        if (call == "server engine --output json")
            return Answer(Fields("source_sha", Sha, "profile", "release"));
        if (call == "solar engine --output json")
            return Answer(Fields("source_sha", Sha, "path", Path.Combine(root, "ds-solar.exe")));
        if (call == "report engine --output json")
            return Answer(Fields("path", Path.Combine(root, "reporter.exe"),
                "identity", Fields("source_sha", Sha)));
        if (call == "build-info")
        {
            Console.WriteLine(Json.Serialize(Fields("schema", "ds.engine-build/v1",
                "profile", "release", "target", "x86_64-pc-windows-msvc",
                "build_manifest_sha256", new string('b', 64))));
            return 0;
        }
        if (call == "capabilities server.serve --output json")
            return Answer(Fields("command", Fields("id", "server.serve", "availability", "available")));
        string receipt = Environment.GetEnvironmentVariable("DS_SERVER_HOST_TEST_RECEIPT");
        string exitText = Environment.GetEnvironmentVariable("DS_SERVER_HOST_TEST_EXIT");
        int exitCode;
        if (String.IsNullOrEmpty(receipt) || !Int32.TryParse(exitText, out exitCode))
            return 91;
        File.WriteAllText(receipt, Json.Serialize(Fields(
            "host_kind", Environment.GetEnvironmentVariable("DS_CLI_HOST_KIND"),
            "argv", args, "exit_code", exitCode)));
        Console.WriteLine("native-stub-exit-" + exitCode);
        return exitCode;
    }
}
'@
    # .NET CodeDom lets the fixture pin compiler TempFiles to ScratchRoot.
    $provider = New-Object Microsoft.CSharp.CSharpCodeProvider
    $parameters = New-Object System.CodeDom.Compiler.CompilerParameters
    $parameters.GenerateExecutable = $true
    $parameters.GenerateInMemory = $false
    $parameters.OutputAssembly = Join-Path $compilerRoot 'ds.exe'
    $parameters.TempFiles = New-Object System.CodeDom.Compiler.TempFileCollection($compilerRoot, $false)
    foreach ($assembly in @('System.dll', 'System.Core.dll', 'System.Web.Extensions.dll')) {
        $null = $parameters.ReferencedAssemblies.Add($assembly)
    }
    try { $compiled = $provider.CompileAssemblyFromSource($parameters, [string[]]@($source)) }
    finally { $provider.Dispose() }
    $errors = @($compiled.Errors | Where-Object { -not $_.IsWarning })
    if ($errors.Count) { throw ("Native fixture compilation failed: " + ($errors -join '; ')) }
    $fixtureExe = $parameters.OutputAssembly
    Assert-Test (Test-Path -LiteralPath $fixtureExe -PathType Leaf) 'Native fixture was not produced.'

    $payload = Join-Path $runRoot 'payload'
    $sourceRoot = Join-Path $runRoot 'source'
    $sourceScripts = Join-Path $sourceRoot 'scripts'
    $targetRoot = Join-Path $runRoot 'target'
    foreach ($directory in @($payload, $sourceScripts, (Join-Path $targetRoot 'debug'),
        (Join-Path $payload 'ds-client-profiles'), (Join-Path $payload 'ds-cli-skills'))) {
        [IO.Directory]::CreateDirectory($directory) | Out-Null
    }
    foreach ($name in @('ds.exe', 'ds-solar.exe', 'reporter.exe')) {
        Copy-Item -LiteralPath $fixtureExe -Destination (Join-Path $payload $name)
    }
    Copy-Item -LiteralPath $fixtureExe -Destination (Join-Path $targetRoot 'debug\ds.exe')
    foreach ($name in @('start-windows-server.ps1', 'windows-server-package.ps1')) {
        Copy-Item -LiteralPath (Join-Path $scripts $name) -Destination (Join-Path $payload $name)
    }
    Copy-Item -LiteralPath (Join-Path $scripts 'run-windows-server.ps1') -Destination (Join-Path $sourceScripts 'run-windows-server.ps1')
    Write-TestText (Join-Path $sourceRoot 'Cargo.toml') '[workspace]'
    Write-TestText (Join-Path $payload 'ds-client-profiles\catalog.json') '{}'
    Write-TestText (Join-Path $payload 'ds-cli-skills\receipt.json') '{}'
    $profile = Join-Path $runRoot 'development-catalog.json'
    Write-TestText $profile '{"development":true}'
    $revisions = @{}
    foreach ($repo in @('ds-cli', 'ds-network', 'ds-command-kernel', 'ds-solar', 'ds-network-reporter')) {
        $revisions[$repo] = 'a' * 40
    }
    $manifest = @{ contract = 'ds.windows-server-payload/v1'; lane = 'canary';
        version = '0.1.0'; architecture = 'x86_64'; source_revisions = $revisions;
        files = @(Get-ServerInventory $payload) }
    Write-TestText (Join-Path $payload 'release.json') ($manifest | ConvertTo-Json -Depth 12)

    function Invoke-LauncherCase([string]$Owner, [string]$Mode, [int]$ExitCode) {
        $caseName = "$Owner-$Mode-$ExitCode"
        $receiptPath = Join-Path $runRoot ($caseName + '.json')
        $mockCargoReceipt = Join-Path $runRoot ($caseName + '-cargo.json')
        $env:DS_CLI_HOST_KIND = 'desktop'
        $env:DS_SERVER_HOST_TEST_RECEIPT = $receiptPath
        $env:DS_SERVER_HOST_TEST_EXIT = "$ExitCode"
        $cli = @('server', 'status', '--lane', 'canary', '--output', 'json')
        if ($Owner -eq 'installed') {
            $launcher = Join-Path $payload 'start-windows-server.ps1'
            $invokeParameters = @{ StateDirectory = (Join-Path $runRoot 'installed-server-state') }
        } else {
            $launcher = Join-Path $sourceScripts 'run-windows-server.ps1'
            $invokeParameters = @{ Lane = 'canary'; DevelopmentRoot = (Join-Path $runRoot 'development');
                TargetDirectory = $targetRoot; ProfileCatalog = $profile }
        }
        if ($Mode -eq 'cli') { $invokeParameters.CliArguments = $cli }
        else { $invokeParameters.Workers = 1 }
        # These dynamic-scope functions shadow tools only for this test call.
        # The actual launcher runs unchanged; no external cargo or git is used.
        function cargo {
            Write-TestText $mockCargoReceipt (ConvertTo-Json -InputObject @($args) -Compress)
            $global:LASTEXITCODE = 0
        }
        function git {
            $global:LASTEXITCODE = 0
            if ($args -contains 'rev-parse') { return ('a' * 40) }
            if ($args -contains 'status') { return }
            throw 'The source launcher requested an unexpected git operation.'
        }
        $output = @(& $launcher @invokeParameters)
        $actualExitCode = $LASTEXITCODE
        Assert-Test ($actualExitCode -eq $ExitCode) "$caseName changed native exit code to $actualExitCode."
        Assert-Test ($env:DS_CLI_HOST_KIND -ceq 'desktop') "$caseName leaked Server context into the caller."
        Assert-Test (Test-Path -LiteralPath $receiptPath -PathType Leaf) "$caseName did not execute the native fixture."
        $receipt = Get-Content -LiteralPath $receiptPath -Raw | ConvertFrom-Json
        Assert-Test ($receipt.host_kind -ceq 'server') "$caseName did not pin native Server context."
        Assert-Test ($receipt.exit_code -eq $ExitCode) "$caseName changed fixture exit context."
        Assert-Test ($output -contains "native-stub-exit-$ExitCode") "$caseName changed native output."
        $expectedArgs = if ($Mode -eq 'cli') { $cli } elseif ($Owner -eq 'installed') {
            @('server', 'serve', '--lane', 'canary', '--state-dir',
                (Join-Path $runRoot 'installed-server-state'), '--workers', '1')
        } else {
            @('server', 'serve', '--lane', 'canary', '--state-dir',
                (Join-Path $runRoot 'development\state\ds\server\canary'), '--workers', '1')
        }
        $actualArgsJson = ConvertTo-Json -InputObject @($receipt.argv) -Compress
        $expectedArgsJson = ConvertTo-Json -InputObject @($expectedArgs) -Compress
        Assert-Test ($actualArgsJson -ceq $expectedArgsJson) "$caseName changed native argument forwarding."
        if ($Owner -eq 'source') {
            Assert-Test (Test-Path -LiteralPath $mockCargoReceipt -PathType Leaf) "$caseName did not use the cargo fixture."
            [string[]]$cargoArgs = Get-Content -LiteralPath $mockCargoReceipt -Raw | ConvertFrom-Json
            Assert-Test ($cargoArgs[0] -eq 'build' -and $cargoArgs -contains '--locked' -and
                $cargoArgs -contains (Join-Path $sourceRoot 'Cargo.toml')) "$caseName changed the native build request."
        }
        $cases.Add([pscustomobject]@{ launcher = $Owner; mode = $Mode; exit_code = $ExitCode;
            child_host_kind = $receipt.host_kind; caller_host_kind = $env:DS_CLI_HOST_KIND;
            native_receipt = $receiptPath; passed = $true })
    }

    foreach ($owner in @('installed', 'source')) {
        foreach ($mode in @('serve', 'cli')) {
            foreach ($code in @(0, 23)) { Invoke-LauncherCase $owner $mode $code }
        }
    }
    $result = [pscustomobject]@{ contract = 'ds.server-host-context-test/v1'; passed = $cases.Count;
        scratch = $runRoot; ds_builds = 0; ds_accounts = 0; cases = @($cases.ToArray()) }
    Write-TestText (Join-Path $runRoot 'result.json') ($result | ConvertTo-Json -Depth 8)
    $result | ConvertTo-Json -Depth 8
} finally {
    foreach ($name in $environmentNames) {
        [Environment]::SetEnvironmentVariable($name, $previousEnvironment[$name], 'Process')
    }
}
exit 0
