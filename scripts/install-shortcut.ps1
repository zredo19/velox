<#
.SYNOPSIS
Creates or repairs Velox shortcuts on the Desktop and Start Menu so they target velox.exe and use its embedded icon (<exe>,0).

.PARAMETER Exe
Path to velox.exe (defaults to <repo>\src-tauri\target\release\velox.exe).

.PARAMETER Name
Shortcut display name without extension (default: Velox).

.PARAMETER DesktopDir
Override Desktop folder path (defaults to current user's Desktop folder, including OneDrive Desktop when redirected).

.PARAMETER StartMenuDir
Override Start Menu Programs folder path (defaults to current user's Start Menu Programs folder).

.PARAMETER NoIconCacheRefresh
Skip triggering Windows Shell/Explorer icon refresh after updating shortcuts.

.PARAMETER DryRun
Show planned shortcut changes without creating or modifying files.
#>
[CmdletBinding()]
param(
    [string]$Exe,

    [string]$Name = "Velox",

    [string]$DesktopDir = [Environment]::GetFolderPath("Desktop"),

    [string]$StartMenuDir = [Environment]::GetFolderPath("Programs"),

    [switch]$NoIconCacheRefresh,

    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($Exe)) {
    $Exe = Join-Path $repoRoot "src-tauri\target\release\velox.exe"
}

if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) {
    throw "Velox executable not found at: $Exe"
}

$exeFull = (Resolve-Path -LiteralPath $Exe).Path
$workDir = Split-Path -Parent $exeFull
$iconLocation = "$exeFull,0"
$description = "Velox Agentic Development Environment"

$targetDirs = @()
if (-not [string]::IsNullOrWhiteSpace($DesktopDir)) {
    $targetDirs += [pscustomobject]@{ Label = "Desktop"; Dir = $DesktopDir }
}
if (-not [string]::IsNullOrWhiteSpace($StartMenuDir)) {
    $targetDirs += [pscustomobject]@{ Label = "Start Menu"; Dir = $StartMenuDir }
}

if ($targetDirs.Count -eq 0) {
    throw "No shortcut target directories configured."
}

$shell = New-Object -ComObject WScript.Shell
$changedAny = $false

function Sync-Shortcut {
    param(
        [string]$Label,
        [string]$Path
    )

    $exists = Test-Path -LiteralPath $Path -PathType Leaf
    $lnk = $shell.CreateShortcut($Path)

    $needsUpdate = (-not $exists) -or
        ($lnk.TargetPath -ne $exeFull) -or
        ($lnk.WorkingDirectory -ne $workDir) -or
        ($lnk.IconLocation -ne $iconLocation)

    if (-not $needsUpdate) {
        Write-Host "[OK] $Label ($Path): already points to $exeFull (icon $iconLocation)."
        return $false
    }

    $action = if ($exists) { "UPDATE" } else { "CREATE" }
    if ($DryRun) {
        Write-Host "[DRY-RUN] [$action] $Label ($Path): Target='$exeFull' Icon='$iconLocation' WorkDir='$workDir'"
        return $false
    }

    $parent = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }

    $lnk.TargetPath = $exeFull
    $lnk.WorkingDirectory = $workDir
    $lnk.IconLocation = $iconLocation
    $lnk.Description = $description
    $lnk.Save()

    Write-Host "[$action] $Label ($Path): Target='$exeFull' Icon='$iconLocation'"
    return $true
}

# Create/repair the canonical Desktop and Start Menu shortcuts
$canonicalPaths = @{}
foreach ($entry in $targetDirs) {
    $lnkPath = Join-Path $entry.Dir "$Name.lnk"
    $canonicalPaths[$lnkPath.ToLowerInvariant()] = $true
    if (Sync-Shortcut -Label $entry.Label -Path $lnkPath) {
        $changedAny = $true
    }

    # Repair any other *velox*.lnk in the same folder pointing to a missing target/icon
    if (Test-Path -LiteralPath $entry.Dir -PathType Container) {
        Get-ChildItem -LiteralPath $entry.Dir -Filter "*velox*.lnk" -File -ErrorAction SilentlyContinue |
            ForEach-Object {
                $full = $_.FullName
                if ($canonicalPaths.ContainsKey($full.ToLowerInvariant())) {
                    return
                }
                $extra = $shell.CreateShortcut($full)
                $rawIconPath = ($extra.IconLocation -split ',')[0].Trim()
                $targetMissing = [string]::IsNullOrWhiteSpace($extra.TargetPath) -or
                    (-not (Test-Path -LiteralPath $extra.TargetPath -PathType Leaf))
                $iconMissing = [string]::IsNullOrWhiteSpace($rawIconPath) -or
                    (-not (Test-Path -LiteralPath $rawIconPath -PathType Leaf))
                if ($targetMissing -or $iconMissing) {
                    if (Sync-Shortcut -Label "$($entry.Label) (stale)" -Path $full) {
                        $changedAny = $true
                    }
                }
            }
    }
}

if (-not $DryRun -and -not $NoIconCacheRefresh -and $changedAny) {
    try {
        Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class VeloxShellNotify {
    [DllImport("shell32.dll")]
    public static extern void SHChangeNotify(int wEventId, uint uFlags, IntPtr dwItem1, IntPtr dwItem2);
}
"@ -ErrorAction SilentlyContinue
        # SHCNE_ASSOCCHANGED (0x08000000) refreshes Explorer shortcut & exe icon caches
        [VeloxShellNotify]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
    } catch {}

    $ie4u = Join-Path $env:SystemRoot "System32\ie4uinit.exe"
    if (Test-Path -LiteralPath $ie4u -PathType Leaf) {
        try { & $ie4u -show 2>$null | Out-Null } catch {}
    }
}
