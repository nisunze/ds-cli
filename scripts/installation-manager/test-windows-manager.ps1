# Native Windows regression checks. Uses only a disposable Temp package and
# harmless fixture executables; never installs a real product or touches data.
[CmdletBinding()]
param(
 [string]$ManagerSource,
 [string]$CoreInstallerSource
)
if(!$ManagerSource){$ManagerSource=Join-Path $PSScriptRoot 'ds-install-manager.ps1'}
if(!$CoreInstallerSource){$CoreInstallerSource=Join-Path $PSScriptRoot '..\..\..\ds-web\scripts\core\install-ds-core-windows.ps1'}
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
 $script:Cache=Join-Path $fixtureLocal 'cleanup-cache';PrivateDirectory $script:Cache
 $download=Join-Path $script:Cache 'completed-setup.exe';[IO.File]::WriteAllText($download,'download')
 $retained=Join-Path $script:Cache 'desktop-canary-installation.json';[IO.File]::WriteAllText($retained,'receipt')
 RemoveCompletedDownload $download
 Check (!(Test-Path -LiteralPath $download)) 'Completed installer was retained'
 Check (Test-Path -LiteralPath $retained) 'Unrelated retained receipt was deleted'
 RemoveCompletedDownload $download
 $external=Join-Path $fixtureLocal 'caller-artifact.zip';[IO.File]::WriteAllText($external,'caller')
 $refused=$false;try{RemoveCompletedDownload $external}catch{$refused=$_.Exception.Message.Contains('manager_cleanup_invalid')}
 Check $refused 'Cleanup accepted an external artifact'
 Check (Test-Path -LiteralPath $external) 'Caller artifact was deleted'
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
 $native=FreshCoreRelease;$native.PSObject.Properties.Remove('runtime');foreach($asset in $native.assets){$asset.PSObject.Properties.Remove('engines')}
 Check ((CoreAssets $native).version -eq '0.1.0') 'Existing native wire omission was rejected'
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
 # Execute the actual ensure path with a bounded native-owner fixture.
 # Matching units must still admit public metadata, without restaging workers.
 $script:NativeOnly=$false;$script:StageRefused=$false
 $script:EnsureCalls=@();$script:EnsureSha='a'*64;$script:AdmittedSha='a'*64;$script:SelectionState='installed';$script:CaptureSha='a'*64;$script:CaptureGeneration=7;$script:CaptureRefused=$false;$script:CoreAvailable=$true
 function Release([string]$Name,[string]$Runtime='native'){return [pscustomobject]@{component=$Name;runtime=$Runtime}}
 function InvokeCore([string[]]$Arguments,[string]$ExpectedCommand,[int]$ExpectedContract=0){
  if($Arguments[0] -eq 'version'){$script:EnsureCalls+=@('version');return [pscustomobject]@{version='0.1.0'}}
  if($Arguments[0] -eq 'capabilities'){$script:EnsureCalls+=@('capabilities');return [pscustomobject]@{command=[pscustomobject]@{id='core.serve';contract=2;availability=$(if($script:CoreAvailable){'available'}else{'unavailable'})}}}
  $verb=$Arguments[1];$script:EnsureCalls+=@(($Arguments -join ' '))
  if($script:NativeOnly -and $Arguments -contains '--runtime'){throw 'unknown_flag: --runtime'}
  if($script:StageRefused -and $verb -eq 'module-stage'){throw 'module_artifact_invalid: native stage refused'}
  if($verb -eq 'component' -and $Arguments[2] -in @('remove','rollback')){return [pscustomobject]@{generation=8}}
  if($verb -eq 'component' -and $Arguments[2] -eq 'selection'){return [pscustomobject]@{schema='ds.ui-module-selection/v1';generation=7;selection=[pscustomobject]@{state=$script:SelectionState;active=[pscustomobject]@{unit=[pscustomobject]@{unit_sha256=$script:EnsureSha}}}}}
  if($verb -eq 'component' -and $Arguments[2] -eq 'capture'){
   if($script:CaptureRefused){throw 'module_artifact_invalid: native capture refused corrupt bytes'}
   $name=$Arguments[4]
   $assets=@(1..4 | ForEach-Object {[pscustomobject]@{sha256=('d'*64);size_bytes=100}})
   return [pscustomobject]@{generation=$script:CaptureGeneration;receipt=[pscustomobject]@{schema='ds.ui-module-installation/v1';unit=[pscustomobject]@{unit_sha256=$script:CaptureSha;release=[pscustomobject]@{component=$name;channel=$Channel;runtime='wasm';assets=$assets}}}}
  }
  if($verb -eq 'module-release'){return [pscustomobject]@{unit_sha256=$script:AdmittedSha}}
  if($verb -eq 'module-stage'){return [pscustomobject]@{receiptId='fixture-receipt'}}
  if($verb -eq 'component' -and $Arguments[2] -eq 'activate'){return [pscustomobject]@{generation=8}}
  throw 'Unexpected native-owner fixture operation'
 }
 $ensured=ComponentAction interactions Install
 Check ($ensured.generation -eq 7) 'Exact installed UI unit advanced its generation'
 Check ($script:EnsureCalls.Count -eq 2 -and $script:EnsureCalls[1] -like 'core module-release *') 'Exact unit skipped public admission or restaged workers'
 $script:EnsureCalls=@();$script:AdmittedSha='b'*64
 $changed=ComponentAction interactions Update
 Check ($changed.generation -eq 8 -and $script:EnsureCalls.Count -eq 4) 'A changed admitted UI unit bypassed stage and activation'
 Check ($script:EnsureCalls[3] -like '*--expected-generation 7*') 'Changed unit lost its native generation fence'
 # Desktop Update must ensure only missing requirements. A newer public UI
 # identity is deliberately available; the existing captured unit stays active.
 $Action='Update';$script:EnsureCalls=@();$script:Completed=@()
 EnsureDesktopRequirements
 Check (($script:EnsureCalls -join '|') -eq 'version|capabilities|core component selection --component interactions --channel canary --runtime wasm|core component capture --component interactions --channel canary --runtime wasm|core component selection --component command-kernel --channel canary --runtime wasm|core component capture --component command-kernel --channel canary --runtime wasm') 'Desktop Update advanced independent dependencies or skipped native capture'
 Check (($script:Completed -join ',') -eq 'Core,interactions,command-kernel') 'Desktop requirement completion differs'
 # Manager dispatch fixtures retain exact owner state and digests; actual
 # filesystem/rename/busy behavior is covered by the native protected-owner test.
 $protectedOwner=Join-Path $packageRoot 'install-ds-core-windows.ps1';$protectedBytes=[IO.File]::ReadAllBytes($protectedOwner)
 try {
  foreach($case in @('valid','cross-lane','missing-current','partial-identity','extra-field','wrong-pending')){
   $response=[ordered]@{schema='ds.core-installation-state/v1';channel='canary';pending=$false;current=[ordered]@{receipt_sha256=('a'*64);executable_sha256=('b'*64)};previous=$null}
   if($case -eq 'cross-lane'){$response.channel='stable'}
   if($case -eq 'missing-current'){$response.Remove('current')}
   if($case -eq 'partial-identity'){$response.current.Remove('executable_sha256')}
   if($case -eq 'extra-field'){$response.path='unowned'}
   if($case -eq 'wrong-pending'){$response.pending='false'}
   $json=($response|ConvertTo-Json -Depth 8 -Compress).Replace("'","''")
   [IO.File]::WriteAllText($protectedOwner,"param([string]`$Action,[string]`$Channel)`nWrite-Output '$json'`n")
   $refused=$false;try{$admission=InvokeCoreInstaller @('-Action','Status','-Channel','canary')}catch{$refused=$_.Exception.Message.Contains('core_install_incompatible')}
   Check ($refused -eq ($case -ne 'valid')) "Protected owner response $case was not admitted/refused exactly"
  }
 }finally{[IO.File]::WriteAllBytes($protectedOwner,$protectedBytes)}
 $script:CoreOwnerCalls=@();$script:CoreOwnerPending=$false;$script:CoreOwnerRefused=$false
 function InvokeCoreInstaller([string[]]$Arguments){
  $script:CoreOwnerCalls+=@(($Arguments -join ' '))
  if($script:CoreOwnerRefused -and $Arguments[1] -ne 'Status'){throw 'core_install_failed: fixture owner refused'}
  if($Arguments[1] -eq 'Recover'){$script:CoreOwnerPending=$false}
  return [pscustomobject]@{schema='ds.core-installation-state/v1';channel='canary';pending=$script:CoreOwnerPending;current=[pscustomobject]@{receipt_sha256=('a'*64);executable_sha256=('b'*64)};previous=[pscustomobject]@{receipt_sha256=('c'*64);executable_sha256=('d'*64)}}
 }
 $CoreArtifact=Join-Path $testRoot 'core-fixture.zip';[IO.File]::WriteAllText($CoreArtifact,'fixture')
 $CoreReceipt=Join-Path $testRoot 'core-fixture-receipt.json';$CoreSha256='e'*64
 $coreMetadata=[ordered]@{contract='ds.core-windows-artifact/v1';lane='canary';target='x86_64-pc-windows-msvc';sha256=$CoreSha256}
 [IO.File]::WriteAllText($CoreReceipt,($coreMetadata|ConvertTo-Json))
 $script:EnsureCalls=@();$null=InstallCore Update
 Check ($script:CoreOwnerCalls.Count -eq 2 -and $script:CoreOwnerCalls[0] -ceq '-Action Status -Channel canary') 'Core update skipped protected state admission'
 $expected='-ExpectedReceiptSha256 '+('a'*64)+' -ExpectedExecutableSha256 '+('b'*64)+' -ExpectedPreviousReceiptSha256 '+('c'*64)+' -ExpectedPreviousExecutableSha256 '+('d'*64)
 Check ($script:CoreOwnerCalls[1].StartsWith('-Action Update -Channel canary -Artifact ') -and $script:CoreOwnerCalls[1].EndsWith($expected)) 'Core update lost exact current/retained receipt and executable fences'
 Check (($script:EnsureCalls -join '|') -ceq 'version') 'Core update used another runtime or started a worker'
 $script:CoreOwnerCalls=@();$null=InstallCore Install
 Check ($script:CoreOwnerCalls.Count -eq 0) 'Ordinary install implicitly replaced existing Core'
 $coreMetadata.lane='stable';[IO.File]::WriteAllText($CoreReceipt,($coreMetadata|ConvertTo-Json));$script:CoreOwnerCalls=@();$refused=$false
 try{$null=InstallCore Update}catch{$refused=$_.Exception.Message.Contains('core_publication_invalid')}
 Check ($refused -and $script:CoreOwnerCalls.Count -eq 1) 'Cross-lane local receipt reached the mutation owner'
 $coreMetadata.lane='canary';[IO.File]::WriteAllText($CoreReceipt,($coreMetadata|ConvertTo-Json))
 $CoreArtifact='';$CoreReceipt='';$CoreSha256=''
 foreach($verb in @('Remove','Rollback')){
  $script:CoreOwnerCalls=@();$script:EnsureCalls=@();$null=CoreLifecycle $verb
  Check ($script:CoreOwnerCalls.Count -eq 2 -and $script:CoreOwnerCalls[1] -ceq "-Action $verb -Channel canary $expected") 'Core lifecycle crossed its exact retained identity or lane'
  Check ($script:EnsureCalls.Count -eq 0) 'Core lifecycle executed Core instead of its protected installation owner'
 }
 $script:CoreOwnerCalls=@();$script:CoreOwnerPending=$true;$null=CoreLifecycle Rollback
 Check ($script:CoreOwnerCalls.Count -eq 3 -and $script:CoreOwnerCalls[1] -ceq '-Action Recover -Channel canary') 'Interrupted mutation skipped explicit same-lane owner recovery'
 $script:CoreOwnerRefused=$true;$refused=$false
 try{$null=CoreLifecycle Remove}catch{$refused=$_.Exception.Message.Contains('core_install_failed')}
 Check $refused 'Core lifecycle hid an owning refusal';$script:CoreOwnerRefused=$false
 $script:CoreAvailable=$false;$refused=$false
 try{EnsureDesktopRequirements}catch{$refused=$_.Exception.Message.Contains('core_incompatible')}
 Check $refused 'Incompatible Core was treated as a missing dependency'
 $script:CoreAvailable=$true
 foreach($case in @('corrupt','crossed','generation')){
  $script:EnsureCalls=@();$script:CaptureRefused=($case -eq 'corrupt');$script:CaptureSha=if($case -eq 'crossed'){'b'*64}else{'a'*64};$script:CaptureGeneration=if($case -eq 'generation'){8}else{7};$refused=$false
  try{$null=ComponentAction interactions Install -PreserveInstalled}catch{$refused=$true}
  Check $refused "Required UI $case capture was reused"
  Check (@($script:EnsureCalls | Where-Object {$_ -like '*module-stage*' -or $_ -like '*activate*' -or $_ -like '*module-release*'}).Count -eq 0) 'A refused capture silently installed another unit'
 }
 $script:CaptureRefused=$false;$script:CaptureSha='a'*64;$script:CaptureGeneration=7;$script:SelectionState='removed';$script:EnsureCalls=@()
 $missing=ComponentAction interactions Install -PreserveInstalled
 Check ($missing.generation -eq 8 -and $script:EnsureCalls.Count -eq 4) 'Removed required UI did not use normal public admission/stage/activation'
 Check ($script:EnsureCalls[3] -like '*--expected-generation 7*') 'Required UI installation lost its existing generation fence'
 # Older installed Core admits native modules but has no --runtime flag.
 # New Core has the same native default. Exercise the real manager workflow
 # against both contracts; UI units must retain their explicit WASM routing.
 function Release([string]$Name,[string]$Runtime='native'){
  return [pscustomobject]@{component=$Name;runtime=$Runtime;assets=@([pscustomobject]@{platform='windows';arch='x86_64';sha256=('f'*64)})}
 }
 foreach($legacy in @($true,$false)){
  $script:NativeOnly=$legacy
  foreach($name in @('network','solar','reporter')){
   foreach($verb in @('Status','Install','Update','Remove','Rollback')){
    $script:EnsureCalls=@()
    $result=ComponentAction $name $verb
    Check (@($script:EnsureCalls | Where-Object {$_ -match '--runtime'}).Count -eq 0) 'Native component required a runtime flag unsupported by installed Core'
    Check ($script:EnsureCalls[0] -ceq "core component selection --component $name --channel canary") 'Native selection crossed its component or channel'
    if($verb -eq 'Status'){
     Check ($script:EnsureCalls.Count -eq 1 -and $result.generation -eq 7) 'Native Status performed an installation effect'
    }elseif($verb -in @('Remove','Rollback')){
     Check ($script:EnsureCalls.Count -eq 2 -and $result.generation -eq 8) 'Native lifecycle used an unexpected owner'
     Check ($script:EnsureCalls[1] -ceq "core component $($verb.ToLowerInvariant()) --component $name --channel canary --expected-generation 7 --yes") 'Native lifecycle lost its generation or confirmation fence'
    }else{
     Check ($script:EnsureCalls.Count -eq 4 -and $result.generation -eq 8) 'Native installation skipped admission, staging or activation'
     Check ($script:EnsureCalls[1] -like "core module-release --component $name --channel canary --file *") 'Native release did not use the owning admission command'
     Check ($script:EnsureCalls[2] -ceq "core module-stage --component $name --channel canary --sha256 $('f'*64) --yes") 'Native staging crossed the published artifact digest'
     Check ($script:EnsureCalls[3] -ceq "core component activate --component $name --channel canary --receipt-id fixture-receipt --expected-generation 7 --yes") 'Native activation lost its receipt or generation fence'
    }
   }
  }
 }
 $script:NativeOnly=$true;$script:StageRefused=$true;$script:EnsureCalls=@();$refused=$false
 try{$null=ComponentAction reporter Update}catch{$refused=$_.Exception.Message.Contains('module_artifact_invalid')}
 Check $refused 'A refused native stage was hidden'
 Check (@($script:EnsureCalls | Where-Object {$_ -like '*activate*'}).Count -eq 0) 'Native stage refusal still activated a component'
 $script:NativeOnly=$false;$script:StageRefused=$false
 foreach($name in @('interactions','command-kernel')){
  foreach($verb in @('Remove','Rollback')){
   $script:EnsureCalls=@();$null=ComponentAction $name $verb
   Check ($script:EnsureCalls.Count -eq 2 -and @($script:EnsureCalls | Where-Object {$_ -notmatch '--runtime wasm'}).Count -eq 0) 'UI lifecycle lost explicit WASM routing'
  }
 }
 Write-Output "Native Windows manager checks: $script:Checks passed"
} finally {
 # Bounded disposable test cleanup, never a product or caller supplied path.
 $absolute=[IO.Path]::GetFullPath($testRoot)
 $temp=[IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd([char]'\')
 if([IO.Path]::GetDirectoryName($absolute) -ne $temp -or [IO.Path]::GetFileName($absolute) -notmatch '^ds-manager-native-tests-[a-f0-9]{32}$'){throw 'Refusing cleanup outside owned temporary test root'}
 if(Test-Path -LiteralPath $absolute){Remove-Item -LiteralPath $absolute -Recurse -Force}
}
