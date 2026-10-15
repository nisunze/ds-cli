# Native Windows regression checks. Uses only a disposable Temp package and
# harmless fixture executables; never installs a real product or touches data.
[CmdletBinding()]
param(
 [string]$ManagerSource=(Join-Path $PSScriptRoot 'ds-install-manager.ps1'),
 [string]$CoreInstallerSource=(Join-Path $PSScriptRoot '..\..\..\ds-web\scripts\core\install-ds-core-windows.ps1')
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version 2
if($env:OS -ne 'Windows_NT' -or ![Environment]::Is64BitProcess){throw 'Native Windows PowerShell is required'}
$testRoot=Join-Path ([IO.Path]::GetTempPath()) ('ds-manager-native-tests-'+[guid]::NewGuid().ToString('N'))
$null=New-Item -ItemType Directory -Path $testRoot
$packageRoot=Join-Path $testRoot 'package'; $null=New-Item -ItemType Directory -Path $packageRoot
$manager=Join-Path $packageRoot 'ds-install-manager.ps1'
Copy-Item -LiteralPath $ManagerSource -Destination $manager
Copy-Item -LiteralPath $CoreInstallerSource -Destination (Join-Path $packageRoot 'install-ds-core-windows.ps1')
$script:Checks=0
function Check([bool]$Value,[string]$Message){if(!$Value){throw $Message};$script:Checks++}
function SaveReceipt($Receipt){[IO.File]::WriteAllText((Join-Path $packageRoot 'package.receipt.json'),($Receipt|ConvertTo-Json -Depth 12),(New-Object Text.UTF8Encoding($false)))}
function FreshReceipt {
 return [pscustomobject]@{schema='ds.installation-manager-package/v1';platform='windows';arch='x86_64';source_sha=('a'*40);web_source_sha=('b'*40);files=@(@('ds-install-manager.ps1','install-ds-core-windows.ps1')|ForEach-Object{[pscustomobject]@{filename=$_;sha256=(Get-FileHash -LiteralPath (Join-Path $packageRoot $_) -Algorithm SHA256).Hash.ToLowerInvariant();size_bytes=(Get-Item -LiteralPath (Join-Path $packageRoot $_)).Length}})}
}
function RunManager([string[]]$Extra=@()){
 $ErrorActionPreference='Continue'
 $output=& powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File $manager @Extra 2>&1
 return @{code=$LASTEXITCODE;text=(($output|ForEach-Object{$_.ToString()})-join "`n")}
}
function Refuses([string]$Needle,[string[]]$Extra=@('-Action','Status')){
 $result=RunManager $Extra
 Check ($result.code -ne 0) "Expected refusal: $Needle"
 Check ($result.text.Contains($Needle)) "Wrong refusal, expected ${Needle}: $($result.text)"
}
try {
 $tokens=$null;$errors=$null
 $null=[Management.Automation.Language.Parser]::ParseFile($manager,[ref]$tokens,[ref]$errors)
 Check (@($errors).Count -eq 0) 'Manager does not parse under native Windows PowerShell'
 SaveReceipt (FreshReceipt)
 $result=RunManager @('-Action','Status','-Channel','canary')
 Check ($result.code -eq 0) "Status failed: $($result.text)"
 $status=$result.text|ConvertFrom-Json
 Check ($status.schema -eq 'ds.installation-manager-status/v1' -and $status.channel -eq 'canary') 'Status envelope identity differs'
 Check ($null -eq $status.workspace_ready) 'Status falsely claims workspace readiness'
 Refuses 'confirmation_required' @('-Action','Setup')
 [IO.File]::WriteAllText((Join-Path $packageRoot 'package.receipt.json'),'{')
 Refuses 'System.ArgumentException'
 $receipt=FreshReceipt;$receipt.files[1]=$receipt.files[0];SaveReceipt $receipt;Refuses 'manager_package_invalid'
 $receipt=FreshReceipt;$receipt.files[1].filename='unowned.ps1';SaveReceipt $receipt;Refuses 'manager_package_invalid'
 $receipt=FreshReceipt;$receipt.files[0].sha256=('0'*64);SaveReceipt $receipt;Refuses 'manager_package_invalid'
 $receipt=FreshReceipt;$receipt.source_sha='moving-run';SaveReceipt $receipt;Refuses 'manager_package_invalid'
 $receipt=FreshReceipt;$receipt.web_source_sha='moving-run';SaveReceipt $receipt;Refuses 'manager_package_invalid'
 $receipt=FreshReceipt;$receipt.files=@($receipt.files[0]);SaveReceipt $receipt;Refuses 'manager_package_invalid'
 $receipt=FreshReceipt;$receipt.files[0].size_bytes++;SaveReceipt $receipt;Refuses 'manager_package_invalid'
 SaveReceipt (FreshReceipt)

 # Load the actual private function after its safe Status entrypoint. Redirect
 # only this test's host root; real installed products are never selected.
 $null=. $manager -Action Status -Channel canary
 $fixtureLocal=Join-Path $testRoot 'host';$null=New-Item -ItemType Directory -Path $fixtureLocal
 $script:Local=$fixtureLocal
 $sentinels=@('Programs\DS Core\ds.exe','Programs\DS Core Canary\ds.exe','DataSolutions\addons\network\stable\retained.bin','rw.datasolutions.desktop\work.bin','.local\state\ds\queue.bin')
 $before=@{}
 foreach($relative in $sentinels){$path=Join-Path $fixtureLocal $relative;$null=New-Item -ItemType Directory -Force -Path (Split-Path $path -Parent);[IO.File]::WriteAllText($path,'DO NOT TOUCH');$before[$path]=(Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash}
 $compiled=Join-Path $testRoot 'compiled';$null=New-Item -ItemType Directory -Path $compiled
 foreach($version in @('0.1.6','0.1.7','0.2.0')){
  $class='FixtureDesktop'+$version.Replace('.','')
  $code="using System.Reflection; [assembly: AssemblyFileVersion(`"$version.0`")] [assembly: AssemblyInformationalVersion(`"$version`")] public static class $class { public static void Main() {} }"
  Add-Type -TypeDefinition $code -OutputAssembly (Join-Path $compiled "$version.exe") -OutputType ConsoleApplication
 }
 $escapedRoot=$fixtureLocal.Replace('\','\\').Replace('"','\"')
 $uninstallCode=@"
using System;
using System.IO;
using System.Reflection;
public static class FixtureUninstaller {
 public static int Main(string[] args) {
  string folder=Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
  string admitted=Path.GetFullPath("$escapedRoot");
  if(!String.Equals(Path.GetDirectoryName(folder),admitted,StringComparison.OrdinalIgnoreCase)) return 9;
  string name=Path.GetFileName(folder);
  if(name!="DS GridDesign" && name!="DS GridDesign Canary") return 10;
  string exe=Path.Combine(folder,name=="DS GridDesign" ? "ds-web-desktop.exe" : "ds-web-desktop-canary.exe");
  File.WriteAllText(Path.Combine(folder,"fixture-uninstall.marker"),String.Join("|",args));
  File.Delete(exe);
  return 0;
 }
}
"@
 Add-Type -TypeDefinition $uninstallCode -OutputAssembly (Join-Path $compiled 'uninstall.exe') -OutputType ConsoleApplication
 function FixtureDesktop([string]$Product,[string]$Version){
  $dir=Join-Path $fixtureLocal $Product;$null=New-Item -ItemType Directory -Force -Path $dir
  $exeName=if($Product -eq 'DS GridDesign'){'ds-web-desktop.exe'}else{'ds-web-desktop-canary.exe'}
  Copy-Item -LiteralPath (Join-Path $compiled "$Version.exe") -Destination (Join-Path $dir $exeName) -Force
  Copy-Item -LiteralPath (Join-Path $compiled 'uninstall.exe') -Destination (Join-Path $dir 'uninstall.exe') -Force
  $marker=Join-Path $dir 'fixture-uninstall.marker';if(Test-Path -LiteralPath $marker){Remove-Item -LiteralPath $marker -Force}
  Check ((Get-Item -LiteralPath (Join-Path $dir $exeName)).VersionInfo.ProductVersion -eq $Version) 'Fixture ProductVersion differs'
 }
 FixtureDesktop 'DS GridDesign' '0.1.6';FixtureDesktop 'DS GridDesign Canary' '0.1.7'
 RemoveLegacyDesktop
 Check (!(Test-Path -LiteralPath (Join-Path $fixtureLocal 'DS GridDesign\ds-web-desktop.exe'))) '0.1.6 Stable was retained'
 Check (Test-Path -LiteralPath (Join-Path $fixtureLocal 'DS GridDesign Canary\ds-web-desktop-canary.exe')) '0.1.7 Canary was removed'
 $marker=Join-Path $fixtureLocal 'DS GridDesign\fixture-uninstall.marker'
 Check ((Get-Content -LiteralPath $marker -Raw).StartsWith('/S|/UPDATE|_?=')) 'Legacy uninstall did not preserve UPDATE semantics'
 Check (!(Test-Path -LiteralPath (Join-Path $fixtureLocal 'DS GridDesign Canary\fixture-uninstall.marker'))) 'Protected 0.1.7 uninstaller was executed'
 FixtureDesktop 'DS GridDesign' '0.1.7';FixtureDesktop 'DS GridDesign Canary' '0.2.0'
 RemoveLegacyDesktop
 foreach($name in @('DS GridDesign','DS GridDesign Canary')){Check (!(Test-Path -LiteralPath (Join-Path $fixtureLocal "$name\fixture-uninstall.marker"))) '0.1.7 or newer uninstaller was executed'}
 FixtureDesktop 'DS GridDesign' '0.2.0';FixtureDesktop 'DS GridDesign Canary' '0.1.6'
 RemoveLegacyDesktop
 Check (Test-Path -LiteralPath (Join-Path $fixtureLocal 'DS GridDesign\ds-web-desktop.exe')) '0.2.0 Stable was removed'
 Check (!(Test-Path -LiteralPath (Join-Path $fixtureLocal 'DS GridDesign Canary\ds-web-desktop-canary.exe'))) '0.1.6 Canary was retained'
 foreach($path in $before.Keys){Check ((Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash -ceq $before[$path]) 'Core, component or data sentinel was changed'}
 # A completed exact shell phase is reusable; a new publication or changed
 # installed image must return to the existing installer owner.
 $script:Cache=Join-Path $fixtureLocal 'manager-cache';PrivateDirectory $script:Cache
 $asset=[pscustomobject]@{sha256=('a'*64);source_sha=('b'*40)}
 FixtureDesktop 'DS GridDesign Canary' '0.2.0'
 Check (!(DesktopMatches $asset)) 'An unrecorded shell was treated as digest-verified'
 RecordDesktop $asset
 Check (DesktopMatches $asset) 'Verified completed shell phase was not reused'
 $different=[pscustomobject]@{sha256=('c'*64);source_sha=('b'*40)}
 Check (!(DesktopMatches $different)) 'A newer installer identity was silently skipped'
 $image=DesktopPath;[IO.File]::AppendAllText($image,'changed fixture image')
 Check (!(DesktopMatches $asset)) 'A changed installed image reused an old receipt'
 FixtureDesktop 'DS GridDesign Canary' '0.2.0'
 # Execute the real native UI constructor: defaults and required dependency
 # state must survive callback invocation after its constructor returns.
 Check ([DsInstallationManagerPlatform]::IsX64()) 'Native Windows architecture discovery failed'
 $form=ManagerForm
 try {
  $formChecks=@($form.Controls | Where-Object {$_ -is [Windows.Forms.CheckBox]})
  Check ($formChecks.Count -eq 4) 'Manager does not show the four declared product choices'
  $coreChoice=@($formChecks | Where-Object {$_.Text -like 'DS Core*'})[0]
  $desktopChoice=@($formChecks | Where-Object {$_.Text -eq 'DS Desktop'})[0]
  $uiChoices=@($formChecks | Where-Object {$_.Text -match 'required by Desktop'})
  Check ($coreChoice.Checked -and !$coreChoice.Enabled) 'Core is not a checked read-only dependency'
  Check ($desktopChoice.Checked -and $desktopChoice.Enabled) 'Desktop is not selected by default'
  Check ($uiChoices.Count -eq 2 -and @($uiChoices | Where-Object {!$_.Checked -or $_.Enabled}).Count -eq 0) 'Desktop UI requirements are not checked read-only choices'
  $desktopChoice.Checked=$false
  Check (@($uiChoices | Where-Object {$_.Checked}).Count -eq 0 -and $coreChoice.Checked) 'Core-only choice did not clear only the visual requirements'
  $desktopChoice.Checked=$true
  Check (@($uiChoices | Where-Object {!$_.Checked}).Count -eq 0) 'Restoring Desktop did not restore UI requirements'
 }finally{$form.Dispose()}
 function FreshCoreRelease {
  $sources=[ordered]@{};foreach($name in @('ds-web','ds-cli','ds-network','ds-command-kernel','ds-solar','ds-network-reporter')){$sources[$name]='c'*40}
  $assets=@()
  foreach($kind in @('core-zip','core-artifact-receipt')){
   $filename=if($kind -eq 'core-zip'){'ds-core-windows-canary.zip'}else{'artifact.receipt.json'}
   $assets += [pscustomobject]@{platform='windows';arch='x86_64';kind=$kind;protocol='ds.core-release/v1';url="https://storage.googleapis.com/electrification-rural-desktop-releases/core/canary/0.1.0-r1/$filename";sha256=('d'*64);size_bytes=1024;source_sha=('c'*40);source_revisions=[pscustomobject]$sources;built_at='2026-10-15T00:00:00Z';engines=@()}
  }
  # Latest catalog can carry an older independently qualified Windows pair.
  return [pscustomobject]@{schema='ds.module-release/v1';component='core';runtime='native';channel='canary';status='active';product_id='rw.datasolutions.core.canary';protocol='ds.core-release/v1';version='0.1.0-r2';assets=$assets}
 }
 $admitted=CoreAssets (FreshCoreRelease)
 Check ($admitted.version -eq '0.1.0' -and $admitted.zip.kind -eq 'core-zip') 'Carried independently released Core pair was rejected'
 function RefuseCore($Release){$refused=$false;try{$null=CoreAssets $Release}catch{$refused=$_.Exception.Message.Contains('core_publication_')};Check $refused 'Malformed Core publication was admitted'}
 $r=FreshCoreRelease;$r.channel='stable';RefuseCore $r
 $r=FreshCoreRelease;$r.product_id='rw.datasolutions.desktop.canary';RefuseCore $r
 $r=FreshCoreRelease;$r.runtime='wasm';RefuseCore $r
 $r=FreshCoreRelease;$r.assets[1].url=$r.assets[1].url.Replace('0.1.0-r1','0.1.0-r2');RefuseCore $r
 $r=FreshCoreRelease;$r.assets[1].size_bytes=2097153;RefuseCore $r
 $r=FreshCoreRelease;$r.assets[1].kind='core-zip';RefuseCore $r
 $r=FreshCoreRelease;$r.assets[1].source_revisions.'ds-cli'='e'*40;RefuseCore $r
 $r=FreshCoreRelease;$r.assets[1].url=$r.assets[1].url+'?token=mutable';RefuseCore $r
 $r=FreshCoreRelease;$r.assets[1].built_at='2026-10-15T01:00:00Z';RefuseCore $r
 $r=FreshCoreRelease;$r.assets=@($r.assets[0]);RefuseCore $r
 Write-Output "Native Windows manager checks: $script:Checks passed"
} finally {
 # Bounded disposable test cleanup, never a product or caller supplied path.
 $absolute=[IO.Path]::GetFullPath($testRoot)
 $temp=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([char]'\')
 if([IO.Path]::GetDirectoryName($absolute) -ne $temp -or [IO.Path]::GetFileName($absolute) -notmatch '^ds-manager-native-tests-[a-f0-9]{32}$'){throw 'Refusing cleanup outside owned temporary test root'}
 if(Test-Path -LiteralPath $absolute){Remove-Item -LiteralPath $absolute -Recurse -Force}
}
