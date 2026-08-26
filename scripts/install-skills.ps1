# Install this repository's skills into the current user's agent directories.
# Existing same-name skills are replaced only when this installer owns them.
param(
    [Parameter(Position = 0)]
    [ValidateSet("install", "uninstall")]
    [string]$Mode
)

$ErrorActionPreference = "Stop"
if (-not $Mode) {
    throw "usage: install-skills.ps1 install|uninstall"
}

$Owner = "nisunze/ds-cli"
$LegacyOwner = "nisunze/ds-cli-skills"
$OwnerMarker = ".ds-cli-skills-owner"
$Inventory = ".ds-cli-skills-owned"
$InventoryContract = "ds-cli-skills-install/v1"
$Receipt = ".ds-cli-skills-receipt.json"
$Here = Split-Path -Parent $PSScriptRoot
$SourceRoot = Join-Path $Here "skills"
if (-not (Test-Path -LiteralPath $SourceRoot -PathType Container)) {
    throw "skills directory is missing: $SourceRoot"
}

$CodexHome = if ($env:CODEX_HOME) { $env:CODEX_HOME } else { Join-Path $HOME ".codex" }
$CodexSkills = if ($env:CODEX_SKILLS_DIR) { $env:CODEX_SKILLS_DIR } else { Join-Path $CodexHome "skills" }
$ClaudeSkills = if ($env:CLAUDE_SKILLS_DIR) { $env:CLAUDE_SKILLS_DIR } else { Join-Path (Join-Path $HOME ".claude") "skills" }
$CopilotSkills = if ($env:COPILOT_SKILLS_DIR) { $env:COPILOT_SKILLS_DIR } else { Join-Path (Join-Path $HOME ".copilot") "skills" }
$Targets = @($CodexSkills, $ClaudeSkills, $CopilotSkills) | Select-Object -Unique

# UTF-8 without BOM, LF-terminated, on every PowerShell edition. `Set-Content
# -Encoding utf8NoBOM` exists only from PowerShell 6; Windows PowerShell 5.1 — the
# default `powershell.exe` on every Windows install — rejects the name and the
# installer dies before it writes its first owner marker. LF (not CRLF) keeps the
# marker and inventory byte-identical to what install-skills.sh writes, so either
# installer recognises the other's ownership.
$Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
function Write-Utf8Lines([string]$Path, [string[]]$Lines) {
    [IO.File]::WriteAllText($Path, (($Lines -join "`n") + "`n"), $Utf8NoBom)
}

$SourceSkills = @(Get-ChildItem -LiteralPath $SourceRoot -Directory | Sort-Object Name)
if ($SourceSkills.Count -eq 0) { throw "no skills found under $SourceRoot" }
foreach ($Skill in $SourceSkills) {
    if ($Skill.Name -notmatch '^[a-z0-9]+(?:-[a-z0-9]+)*$' -or
        -not (Test-Path -LiteralPath (Join-Path $Skill.FullName "SKILL.md") -PathType Leaf)) {
        throw "invalid source skill directory: $($Skill.FullName)"
    }
    $Reparse = Get-ChildItem -LiteralPath $Skill.FullName -Force -Recurse |
        Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint } |
        Select-Object -First 1
    if ($Reparse) { throw "source skill contains a link or reparse point: $($Skill.Name)" }
}

function Test-OwnedSkill([string]$Path) {
    $Marker = Join-Path $Path $OwnerMarker
    if (-not (Test-Path -LiteralPath $Marker -PathType Leaf)) { return $false }
    $MarkerOwner = (Get-Content -LiteralPath $Marker -Raw).TrimEnd("`r", "`n")
    return $MarkerOwner -eq $Owner -or $MarkerOwner -eq $LegacyOwner
}

function Read-OwnedInventory([string]$Target) {
    $Path = Join-Path $Target $Inventory
    if (-not (Test-Path -LiteralPath $Path)) { return @() }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "install inventory is not a regular file: $Path"
    }
    $Lines = @(Get-Content -LiteralPath $Path)
    if ($Lines.Count -eq 0 -or $Lines[0] -ne $InventoryContract) {
        throw "refusing an inventory not owned by this installer: $Path"
    }
    foreach ($Name in @($Lines | Select-Object -Skip 1)) {
        if ($Name -notmatch '^[a-z0-9]+(?:-[a-z0-9]+)*$') {
            throw "invalid owned skill name in ${Path}: $Name"
        }
    }
    return @($Lines | Select-Object -Skip 1)
}

function Assert-OwnedDestinations([string]$Target, [string[]]$Names) {
    foreach ($Name in $Names) {
        $Destination = Join-Path $Target $Name
        if (Test-Path -LiteralPath $Destination) {
            if (-not (Test-Path -LiteralPath $Destination -PathType Container) -or
                -not (Test-OwnedSkill $Destination)) {
                throw "refusing to replace or remove unowned skill: $Destination"
            }
        }
    }
}

function Install-Target([string]$Target) {
    New-Item -ItemType Directory -Force -Path $Target | Out-Null
    $OldNames = @(Read-OwnedInventory $Target)
    $NewNames = @($SourceSkills | ForEach-Object Name)
    $ManagedNames = @(@($OldNames) + @($NewNames) | Select-Object -Unique)
    Assert-OwnedDestinations $Target $ManagedNames
    $InventoryPath = Join-Path $Target $Inventory
    $ReceiptPath = Join-Path $Target $Receipt
    if ((Test-Path -LiteralPath $ReceiptPath) -and
        (-not (Test-Path -LiteralPath $InventoryPath -PathType Leaf) -or
         -not (Test-Path -LiteralPath $ReceiptPath -PathType Leaf))) {
        throw "refusing unowned or non-regular install receipt: $ReceiptPath"
    }

    $Stage = Join-Path $Target (".ds-cli-skills-stage." + [guid]::NewGuid().ToString("N"))
    $Backup = Join-Path $Target (".ds-cli-skills-backup." + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Path $Stage, $Backup | Out-Null
    try {
        foreach ($Skill in $SourceSkills) {
            $Staged = Join-Path $Stage $Skill.Name
            Copy-Item -LiteralPath $Skill.FullName -Destination $Staged -Recurse
            Write-Utf8Lines (Join-Path $Staged $OwnerMarker) @($Owner)
        }
        $BundleReceipt = Join-Path $Here "receipt.json"
        if (Test-Path -LiteralPath $BundleReceipt) {
            if (-not (Test-Path -LiteralPath $BundleReceipt -PathType Leaf)) {
                throw "bundle receipt is not a regular file: $BundleReceipt"
            }
            Copy-Item -LiteralPath $BundleReceipt -Destination (Join-Path $Stage $Receipt)
        }
        if (Test-Path -LiteralPath $InventoryPath -PathType Leaf) {
            Copy-Item -LiteralPath $InventoryPath -Destination (Join-Path $Backup $Inventory)
        }
        if (Test-Path -LiteralPath $ReceiptPath -PathType Leaf) {
            Copy-Item -LiteralPath $ReceiptPath -Destination (Join-Path $Backup $Receipt)
        }
        foreach ($Name in $ManagedNames) {
            $Destination = Join-Path $Target $Name
            if (Test-Path -LiteralPath $Destination) {
                Move-Item -LiteralPath $Destination -Destination (Join-Path $Backup $Name)
            }
        }
        foreach ($Name in $NewNames) {
            Move-Item -LiteralPath (Join-Path $Stage $Name) -Destination (Join-Path $Target $Name)
        }
        Remove-Item -LiteralPath $ReceiptPath -Force -ErrorAction SilentlyContinue
        $StagedReceipt = Join-Path $Stage $Receipt
        if (Test-Path -LiteralPath $StagedReceipt -PathType Leaf) {
            $ReceiptTemp = Join-Path $Target ("$Receipt.tmp." + [guid]::NewGuid().ToString("N"))
            Copy-Item -LiteralPath $StagedReceipt -Destination $ReceiptTemp
            Move-Item -Force -LiteralPath $ReceiptTemp -Destination $ReceiptPath
        }
        $InventoryTemp = Join-Path $Target ("$Inventory.tmp." + [guid]::NewGuid().ToString("N"))
        Write-Utf8Lines $InventoryTemp (@($InventoryContract) + @($NewNames))
        Move-Item -Force -LiteralPath $InventoryTemp -Destination $InventoryPath
    } catch {
        foreach ($Name in $NewNames) {
            $Destination = Join-Path $Target $Name
            if (Test-Path -LiteralPath $Destination) { Remove-Item -LiteralPath $Destination -Recurse -Force }
        }
        foreach ($Item in Get-ChildItem -LiteralPath $Backup -Directory -ErrorAction SilentlyContinue) {
            Move-Item -LiteralPath $Item.FullName -Destination (Join-Path $Target $Item.Name)
        }
        Remove-Item -LiteralPath $InventoryPath, $ReceiptPath -Force -ErrorAction SilentlyContinue
        foreach ($Name in @($Inventory, $Receipt)) {
            $Previous = Join-Path $Backup $Name
            if (Test-Path -LiteralPath $Previous -PathType Leaf) {
                Move-Item -LiteralPath $Previous -Destination (Join-Path $Target $Name)
            }
        }
        throw
    } finally {
        Remove-Item -LiteralPath $Stage, $Backup -Recurse -Force -ErrorAction SilentlyContinue
    }
    Write-Host "installed $($NewNames.Count) owned skill(s) -> $Target"
}

function Uninstall-Target([string]$Target) {
    if (-not (Test-Path -LiteralPath $Target -PathType Container)) { return }
    $OldNames = @(Read-OwnedInventory $Target)
    $InventoryPath = Join-Path $Target $Inventory
    if (-not (Test-Path -LiteralPath $InventoryPath -PathType Leaf)) {
        Write-Host "no owned skills -> $Target"
        return
    }
    Assert-OwnedDestinations $Target $OldNames
    foreach ($Name in $OldNames) {
        $Destination = Join-Path $Target $Name
        if (Test-Path -LiteralPath $Destination) {
            Remove-Item -LiteralPath $Destination -Recurse -Force
        }
    }
    Remove-Item -LiteralPath $InventoryPath -Force
    Remove-Item -LiteralPath (Join-Path $Target $Receipt) -Force -ErrorAction SilentlyContinue
    Write-Host "removed $($OldNames.Count) owned skill(s) from $Target"
}

foreach ($Target in $Targets) {
    if (-not [IO.Path]::IsPathRooted($Target)) { throw "agent skill directory must be absolute: $Target" }
    if ($Mode -eq "install") { Install-Target $Target } else { Uninstall-Target $Target }
}
