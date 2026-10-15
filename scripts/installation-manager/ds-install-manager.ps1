[CmdletBinding()]
param(
 [ValidateSet('Setup','Status','Install','Update','Remove','Rollback')][string]$Action='Status',
 [ValidateSet('canary','stable')][string]$Channel='canary',
 [ValidateSet('Core','Desktop','interactions','command-kernel','network','solar','reporter')][string]$Component='Core',
 [string]$CoreArtifact, [string]$CoreReceipt, [string]$CoreSha256,
 [switch]$Yes, [switch]$UI
)
$ErrorActionPreference='Stop'
Set-StrictMode -Version 2
function Fail([string]$Code,[string]$Message) { throw "$Code`: $Message" }
function Digest([string]$Path) { (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
function Ordinary([string]$Path,[bool]$Directory=$false) {
 $item=Get-Item -LiteralPath $Path -Force
 if($item.PSIsContainer -ne $Directory -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { Fail 'manager_path_invalid' 'An ordinary unlinked path is required' }
 return $item
}
function ReadJson([string]$Path,[long]$Max=1048576) {
 $file=Ordinary $Path
 if($file.Length -gt $Max) { Fail 'manager_metadata_invalid' 'Metadata exceeds its bound' }
 return [IO.File]::ReadAllText($file.FullName) | ConvertFrom-Json
}
function PrivateDirectory([string]$Path) {
 if(Test-Path -LiteralPath $Path) {
  $null=Ordinary $Path $true
  $acl=Get-Acl -LiteralPath $Path
  if($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $script:Sid.Value) { Fail 'manager_owner_invalid' 'Installation directory belongs to another account' }
  foreach($rule in $acl.Access) {
   if($rule.AccessControlType -eq 'Allow' -and $rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -ne $script:Sid.Value) { Fail 'manager_owner_invalid' 'Installation directory is not private to this account' }
  }
  return
 }
 $security=New-Object Security.AccessControl.DirectorySecurity
 $security.SetOwner($script:Sid); $security.SetAccessRuleProtection($true,$false)
 $security.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($script:Sid,'FullControl','ContainerInherit, ObjectInherit','None','Allow')))
 (New-Object IO.DirectoryInfo($Path)).Create($security)
}
function Package {
 $manifest=ReadJson (Join-Path $PSScriptRoot 'package.receipt.json')
 if($manifest.schema -ne 'ds.installation-manager-package/v1' -or $manifest.platform -ne 'windows' -or $manifest.arch -ne 'x86_64' -or $manifest.source_sha -notmatch '^[a-f0-9]{40}$' -or $manifest.web_source_sha -notmatch '^[a-f0-9]{40}$') { Fail 'manager_package_invalid' 'Wrong manager package identity' }
 $names=@('ds-install-manager.ps1','install-ds-core-windows.ps1')
 if(@($manifest.files).Count -ne $names.Count) { Fail 'manager_package_invalid' 'Incomplete manager package' }
 $seen=@{}
 foreach($file in $manifest.files) {
  if($file.filename -notin $names -or $seen.ContainsKey($file.filename) -or $file.sha256 -notmatch '^[a-f0-9]{64}$') { Fail 'manager_package_invalid' 'Unowned or repeated package member' }
  $seen[$file.filename]=$true; $path=Join-Path $PSScriptRoot $file.filename; $item=Ordinary $path
  if($item.Length -ne $file.size_bytes -or (Digest $path) -cne $file.sha256) { Fail 'manager_package_invalid' 'Manager package bytes differ from their receipt' }
 }
 return $manifest
}
function CorePath {
 $name=if($Channel -eq 'canary'){'DS Core Canary'}else{'DS Core'}
 return Join-Path $script:Local "Programs\$name\ds.exe"
}
function AdmitCore([string]$Exe) {
 $root=Split-Path $Exe -Parent
 $receiptPath=Join-Path $root 'installation.receipt.json'
 foreach($path in @($root,$Exe,$receiptPath)) {
  $null=Ordinary $path ($path -eq $root)
  $acl=Get-Acl -LiteralPath $path
  if($acl.GetOwner([Security.Principal.SecurityIdentifier]).Value -ne $script:Sid.Value){Fail 'core_owner_invalid' 'Core belongs to another account'}
  foreach($rule in $acl.Access) { if($rule.AccessControlType -eq 'Allow' -and $rule.IdentityReference.Translate([Security.Principal.SecurityIdentifier]).Value -ne $script:Sid.Value){Fail 'core_owner_invalid' 'Core files are not private to this account'} }
 }
 $receipt=ReadJson $receiptPath 2097152
 if($receipt.contract -ne 'ds.core-windows-artifact/v1' -or $receipt.lane -ne $Channel -or $receipt.target -ne 'x86_64-pc-windows-msvc' -or $receipt.executable_sha256 -notmatch '^[a-f0-9]{64}$' -or (Digest $Exe) -cne $receipt.executable_sha256){Fail 'core_identity_invalid' 'Installed Core receipt does not match this lane and executable'}
}
function InvokeCore([string[]]$Arguments) {
 $exe=CorePath
 if(!(Test-Path -LiteralPath $exe -PathType Leaf)) { Fail 'core_missing' 'Install DS Core for this lane first' }
 AdmitCore $exe
 $before=Digest $exe
 $result=& $exe @Arguments --output json
 $exit=$LASTEXITCODE
 if((Digest $exe) -cne $before) { Fail 'core_changed' 'Core changed during the command; retry with its current identity' }
 try { $envelope=($result -join "`n") | ConvertFrom-Json } catch { Fail 'core_incompatible' 'Core returned malformed command output' }
 if($exit -ne 0 -or $envelope.status -ne 'ok') { Fail $envelope.error.code $envelope.error.message }
 return $envelope.data
}
function Release([string]$Name,[string]$Runtime='native') {
 $origin=if($Channel -eq 'canary'){'https://eds-gateway-canary-3c0q477h.ue.gateway.dev'}else{'https://eds-gateway-3c0q477h.ue.gateway.dev'}
 $query=if($Name -eq 'Desktop'){"channel=$Channel"}else{"component=$Name&channel=$Channel&runtime=$Runtime"}
 $response=Invoke-WebRequest -UseBasicParsing -Uri "$origin/api/v1/app_releases?$query" -MaximumRedirection 0 -TimeoutSec 45 -Headers @{'Cache-Control'='no-cache'}
 if($response.RawContentLength -gt 1048576) { Fail 'manager_metadata_invalid' 'Release response exceeds its bound' }
 $body=$response.Content | ConvertFrom-Json
 if(!$body.success -or !$body.release -or $body.release.channel -ne $Channel -or $body.release.status -ne 'active') { Fail 'component_unpublished' 'No active release is available for this selection' }
 return $body.release
}
function Download([object]$Asset,[string]$Path) {
 $url=[uri]$Asset.url
 if($url.Scheme -ne 'https' -or $url.Host -ne 'storage.googleapis.com' -or $url.UserInfo -or $url.Query -or $Asset.sha256 -notmatch '^[a-f0-9]{64}$' -or $Asset.size_bytes -lt 1 -or $Asset.size_bytes -gt 536870912) { Fail 'manager_asset_invalid' 'A bounded immutable release asset is required' }
 if(Test-Path -LiteralPath $Path) { $null=Ordinary $Path; if((Get-Item -LiteralPath $Path).Length -eq $Asset.size_bytes -and (Digest $Path) -ceq $Asset.sha256){return} }
 $partial="$Path.partial"
 Invoke-WebRequest -UseBasicParsing -Uri $url.AbsoluteUri -MaximumRedirection 0 -TimeoutSec 7200 -OutFile $partial
 $null=Ordinary $partial
 if((Get-Item -LiteralPath $partial).Length -ne $Asset.size_bytes -or (Digest $partial) -cne $Asset.sha256) { Fail 'manager_digest_mismatch' 'Downloaded bytes differ; nothing was installed' }
 Move-Item -LiteralPath $partial -Destination $Path -Force
}
function RemoveLegacyDesktop {
 foreach($product in @(@('DS GridDesign','ds-web-desktop.exe'),@('DS GridDesign Canary','ds-web-desktop-canary.exe'))) {
  $root=Join-Path $script:Local $product[0]; $exe=Join-Path $root $product[1]
  if(!(Test-Path -LiteralPath $exe -PathType Leaf)){continue}
  $item=Ordinary $exe
  $version=$item.VersionInfo.ProductVersion
  if($version -notmatch '^(\d+\.\d+\.\d+)'){Fail 'legacy_desktop_identity_invalid' 'Cannot identify the installed Desktop version'}
  if([version]$Matches[1] -ge [version]'0.1.7'){continue}
  $uninstaller=Join-Path $root 'uninstall.exe'; $null=Ordinary $uninstaller
  $process=Start-Process -FilePath $uninstaller -ArgumentList "/S /UPDATE _?=$root" -WindowStyle Hidden -Wait -PassThru
  if($process.ExitCode -ne 0 -or (Test-Path -LiteralPath $exe)){Fail 'legacy_desktop_uninstall_failed' 'The old Desktop uninstaller did not finish; no replacement was installed'}
 }
}
function InstallCore {
 if(Test-Path -LiteralPath (CorePath)) { return InvokeCore -Arguments @('version') }
 if(!$CoreArtifact -or !$CoreReceipt -or $CoreSha256 -notmatch '^[a-f0-9]{64}$') { Fail 'core_publication_unavailable' 'Windows Core automatic download is unavailable until its qualified package is published; a qualified local package, receipt and digest can be supplied' }
 & powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -File (Join-Path $PSScriptRoot 'install-ds-core-windows.ps1') -Artifact $CoreArtifact -Receipt $CoreReceipt -Sha256 $CoreSha256
 if($LASTEXITCODE -ne 0) { Fail 'core_install_failed' 'The owning Core installer refused; existing installations were preserved' }
 return InvokeCore -Arguments @('version')
}
function ComponentAction([string]$Name,[string]$Verb) {
 $runtime=if($Name -in @('interactions','command-kernel')){'wasm'}else{'native'}
 $commandArgs=@('core','component','selection','--component',$Name,'--channel',$Channel,'--runtime',$runtime)
 $selection=InvokeCore -Arguments $commandArgs
 $generation=if($null -eq $selection){0}else{$selection.generation}
 if($Verb -eq 'Status'){return $selection}
 if($Verb -in @('Remove','Rollback')) { return InvokeCore -Arguments @('core','component',$Verb.ToLowerInvariant(),'--component',$Name,'--channel',$Channel,'--runtime',$runtime,'--expected-generation',"$generation",'--yes') }
 $release=Release $Name $runtime
 $path=Join-Path $script:Cache "$Name-release.json"
 [IO.File]::WriteAllText($path,($release | ConvertTo-Json -Depth 30),(New-Object Text.UTF8Encoding($false)))
 $admitted=InvokeCore -Arguments @('core','module-release','--component',$Name,'--channel',$Channel,'--runtime',$runtime,'--file',$path)
 if($runtime -eq 'wasm'){$sha=$admitted.unit_sha256}else{
  $assets=@($release.assets | Where-Object {$_.platform -eq 'windows' -and $_.arch -eq 'x86_64'})
  if($assets.Count -ne 1){Fail 'component_platform_unavailable' 'No unique Windows executable is published'}
  $sha=$assets[0].sha256
 }
 $ready=InvokeCore -Arguments @('core','module-stage','--component',$Name,'--channel',$Channel,'--runtime',$runtime,'--sha256',$sha,'--yes')
 return InvokeCore -Arguments @('core','component','activate','--component',$Name,'--channel',$Channel,'--runtime',$runtime,'--receipt-id',$ready.receiptId,'--expected-generation',"$generation",'--yes')
}
function Status {
 $name=if($Channel -eq 'canary'){'DS GridDesign Canary'}else{'DS GridDesign'}
 $binary=if($Channel -eq 'canary'){'ds-web-desktop-canary.exe'}else{'ds-web-desktop.exe'}
 $desktop=Join-Path $script:Local "$name\$binary"
 return [ordered]@{schema='ds.installation-manager-status/v1';channel=$Channel;manager_source=$script:Manifest.source_sha;core_installed=(Test-Path -LiteralPath (CorePath) -PathType Leaf);desktop_installed=(Test-Path -LiteralPath $desktop -PathType Leaf);desktop_version=$(if(Test-Path -LiteralPath $desktop){(Get-Item -LiteralPath $desktop).VersionInfo.ProductVersion}else{$null});workspace_ready=$null}
}
function Setup {
 $root=Join-Path $script:Local 'Programs\DS Installation Manager'
 PrivateDirectory $root
 $lane=Join-Path $root $Channel; PrivateDirectory $lane
 $versions=Join-Path $lane 'versions'; PrivateDirectory $versions
 $destination=Join-Path $versions (Digest (Join-Path $PSScriptRoot 'package.receipt.json'))
 PrivateDirectory $destination
 foreach($name in @('ds-install-manager.ps1','install-ds-core-windows.ps1','package.receipt.json')) {
  $source=Join-Path $PSScriptRoot $name; $target=Join-Path $destination $name
  if(Test-Path -LiteralPath $target) { if((Digest $target) -cne (Digest $source)){Fail 'manager_package_invalid' 'Existing immutable manager bytes changed'} }
  else { Copy-Item -LiteralPath $source -Destination $target }
 }
 $installed=Join-Path $destination 'ds-install-manager.ps1'
 $pointer=Join-Path $lane 'current.json'
 $receipt=[ordered]@{schema='ds.installation-manager-installation/v1';channel=$Channel;source_sha=$script:Manifest.source_sha;script=$installed;package_sha256=(Digest (Join-Path $destination 'package.receipt.json'));installed_at=[DateTime]::UtcNow.ToString('o')}
 [IO.File]::WriteAllText("$pointer.pending",($receipt | ConvertTo-Json),(New-Object Text.UTF8Encoding($false)))
 Move-Item -LiteralPath "$pointer.pending" -Destination $pointer -Force
 $menu=[Environment]::GetFolderPath('Programs')
 $link=(New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $menu "DS Installation Manager $Channel.lnk"))
 $link.TargetPath=Join-Path $PSHOME 'powershell.exe'; $link.Arguments="-NoProfile -ExecutionPolicy Bypass -File `"$installed`" -Channel $Channel -UI"; $link.Save()
 return $receipt
}
function ShowManager {
 Add-Type -AssemblyName System.Windows.Forms
 Add-Type -AssemblyName System.Drawing
 $form=New-Object Windows.Forms.Form; $form.Text="DS Installation Manager - $Channel"; $form.ClientSize=New-Object Drawing.Size(520,250); $form.StartPosition='CenterScreen'
 $label=New-Object Windows.Forms.Label; $label.Text='Install Core only, or the DS Desktop application.'; $label.SetBounds(20,20,480,30); $form.Controls.Add($label)
 $choice=New-Object Windows.Forms.ComboBox; $choice.DropDownStyle='DropDownList'; $choice.Items.AddRange(@('Core only','DS Desktop')); $choice.SelectedIndex=0; $choice.SetBounds(20,60,300,30); $form.Controls.Add($choice)
 $facts=New-Object Windows.Forms.Label; $facts.Text=((Status | ConvertTo-Json) -join "`n"); $facts.SetBounds(20,105,480,100); $form.Controls.Add($facts)
 $install=New-Object Windows.Forms.Button; $install.Text='Install'; $install.SetBounds(340,60,140,30); $form.Controls.Add($install)
 $install.Add_Click({$selected=if($choice.SelectedIndex -eq 0){'Core'}else{'Desktop'}; $launchArgs="-NoExit -NoProfile -ExecutionPolicy Bypass -File `"$PSCommandPath`" -Action Install -Component $selected -Channel $Channel -Yes"; if($CoreArtifact -and $CoreReceipt -and $CoreSha256){$launchArgs+=" -CoreArtifact `"$CoreArtifact`" -CoreReceipt `"$CoreReceipt`" -CoreSha256 $CoreSha256"}; Start-Process -FilePath (Join-Path $PSHOME 'powershell.exe') -ArgumentList $launchArgs | Out-Null})
 [void]$form.ShowDialog()
}
$script:Completed=@(); $script:CurrentComponent=$Component
try {
 if($env:OS -ne 'Windows_NT' -or ![Environment]::Is64BitProcess -or [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() -ne 'X64'){Fail 'manager_platform_unavailable' 'Use native 64-bit Windows PowerShell'}
 $script:Sid=[Security.Principal.WindowsIdentity]::GetCurrent().User
 $script:Local=[Environment]::GetFolderPath('LocalApplicationData')
 $script:Manifest=Package
 if($Action -eq 'Setup') { if(!$Yes){Fail 'confirmation_required' 'Use -Yes to install DS Installation Manager'}; Setup | ConvertTo-Json -Depth 12; return }
 if($UI){ShowManager;return}
 if($Action -eq 'Status') { if($Component -in @('Core','Desktop')){Status | ConvertTo-Json -Depth 12}else{ComponentAction $Component Status | ConvertTo-Json -Depth 30}; return }
 if(!$Yes){Fail 'confirmation_required' 'Use -Yes for the selected installation action'}
 $script:Cache=Join-Path $script:Local 'DS Installation Manager Cache'; PrivateDirectory $script:Cache
 if($Component -eq 'Core') { if($Action -notin @('Install','Update')){Fail 'core_lifecycle_unavailable' 'Core removal and rollback require their qualified owner; no data was changed'}; InstallCore | ConvertTo-Json -Depth 20; return }
 if($Component -eq 'Desktop') {
  if($Action -notin @('Install','Update')){Fail 'desktop_lifecycle_unavailable' 'Use the existing Desktop uninstaller with data-preserving semantics'}
  $release=Release Desktop
  $product=if($Channel -eq 'canary'){'rw.datasolutions.desktop.canary'}else{'rw.datasolutions.desktop'}
  if($release.product_id -ne $product){Fail 'manager_asset_invalid' 'Desktop release crosses product identity'}
  $assets=@($release.assets | Where-Object {$_.platform -eq 'windows' -and $_.arch -eq 'x86_64' -and $_.kind -eq 'nsis'})
  if($assets.Count -ne 1){Fail 'desktop_publication_unavailable' 'No unique Windows Desktop installer is published'}
  $path=Join-Path $script:Cache "$($assets[0].sha256)-setup.exe"; Download $assets[0] $path
  RemoveLegacyDesktop
  $setup=Start-Process -FilePath $path -ArgumentList '/S' -WindowStyle Hidden -PassThru -Wait
  if($setup.ExitCode -ne 0){Fail 'desktop_install_failed' "The owning installer exited $($setup.ExitCode)"}
  $script:Completed+=@('Desktop'); $script:CurrentComponent='Core'
  $null=InstallCore; $script:Completed+=@('Core'); $script:CurrentComponent='interactions'
  $null=ComponentAction interactions Install; $script:Completed+=@('interactions'); $script:CurrentComponent='command-kernel'
  $null=ComponentAction command-kernel Install; $script:Completed+=@('command-kernel')
  Status | ConvertTo-Json -Depth 20; return
 }
 ComponentAction $Component $Action | ConvertTo-Json -Depth 30
} catch {
 if($script:Completed.Count){[ordered]@{schema='ds.installation-manager-result/v1';status='partial';completed=$script:Completed;blocked_component=$script:CurrentComponent;message=$_.Exception.Message} | ConvertTo-Json -Depth 8}
 Write-Error -ErrorRecord $_ -ErrorAction Continue; exit 1
}
