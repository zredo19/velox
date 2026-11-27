<#
.SYNOPSIS
Idempotently registers the built-in velox-team MCP server for Claude Code profiles and Antigravity CLI.

.PARAMETER Port
Embedded Lazy MCP Gateway port (default: 43111).

.PARAMETER DryRun
Print planned changes without modifying any files or invoking `claude mcp add`.
#>
[CmdletBinding()]
param(
    [ValidateRange(1, 65535)]
    [int]$Port = 43111,

    [switch]$DryRun
)

$ErrorActionPreference = "Stop"
$url = "http://127.0.0.1:$Port/mcp/velox-team"
$homeDir = $env:USERPROFILE

Write-Host "Target velox-team URL: $url"
if ($DryRun) {
    Write-Host "[DRY-RUN] No changes will be written."
}

# --- 1. Claude Code profiles -------------------------------------------------
$claudeTargets = @()
$defaultClaudeDir = Join-Path $homeDir ".claude"
if (Test-Path -LiteralPath $defaultClaudeDir -PathType Container) {
    $claudeTargets += [pscustomobject]@{
        Label     = "~/.claude (default)"
        ConfigDir = $null
    }
}

Get-ChildItem -LiteralPath $homeDir -Directory -Force -Filter ".claude-*" -ErrorAction SilentlyContinue |
    Sort-Object Name |
    ForEach-Object {
        $claudeJson = Join-Path $_.FullName ".claude.json"
        if (Test-Path -LiteralPath $claudeJson -PathType Leaf) {
            $claudeTargets += [pscustomobject]@{
                Label     = "~/$($_.Name)"
                ConfigDir = $_.FullName
            }
        }
    }

$claudeCmd = Get-Command claude -ErrorAction SilentlyContinue
if (-not $claudeCmd) {
    Write-Warning "claude CLI not found on PATH; skipping Claude profile registration."
} elseif ($claudeTargets.Count -eq 0) {
    Write-Host "[SKIP] No Claude profile directories found under $homeDir."
} else {
    $hadPrevConfigDir = Test-Path Env:CLAUDE_CONFIG_DIR
    $prevConfigDir = $env:CLAUDE_CONFIG_DIR

    try {
        foreach ($target in $claudeTargets) {
            if ($null -eq $target.ConfigDir) {
                Remove-Item Env:CLAUDE_CONFIG_DIR -ErrorAction SilentlyContinue
            } else {
                $env:CLAUDE_CONFIG_DIR = $target.ConfigDir
            }

            $alreadyRegistered = $false
            $hasSessionHeader = $false
            try {
                $existingInfo = (& $claudeCmd.Source mcp get velox-team 2>&1) -join "`n"
                if ($LASTEXITCODE -eq 0) {
                    $alreadyRegistered = $true
                    if ($existingInfo -match "X-Velox-Session" -and $existingInfo -match [regex]::Escape($url)) {
                        $hasSessionHeader = $true
                    }
                }
            } catch {
                $alreadyRegistered = $false
            }

            if ($alreadyRegistered -and $hasSessionHeader) {
                Write-Host "[SKIP] Claude ($($target.Label)): velox-team already registered with X-Velox-Session header."
                continue
            }

            if ($DryRun) {
                $dirDisplay = if ($null -eq $target.ConfigDir) { "<unset>" } else { $target.ConfigDir }
                $action = if ($alreadyRegistered) { "update" } else { "register" }
                Write-Host "[DRY-RUN] Claude ($($target.Label)): would $action velox-team -> $url with X-Velox-Session header (CLAUDE_CONFIG_DIR=$dirDisplay)"
            } else {
                if ($alreadyRegistered) {
                    Write-Host "[UPDATE] Claude ($($target.Label)): refreshing velox-team with X-Velox-Session header..."
                    & $claudeCmd.Source mcp remove --scope user velox-team 2>&1 | Out-Null
                } else {
                    Write-Host "[ADD] Claude ($($target.Label)): registering velox-team..."
                }
                & $claudeCmd.Source mcp add --transport http --scope user --header 'X-Velox-Session: ${VELOX_SESSION_ID:-}' velox-team $url
                if ($LASTEXITCODE -ne 0) {
                    throw "claude mcp add failed for $($target.Label) with exit code $LASTEXITCODE"
                }
            }
        }
    } finally {
        if ($hadPrevConfigDir) {
            $env:CLAUDE_CONFIG_DIR = $prevConfigDir
        } else {
            Remove-Item Env:CLAUDE_CONFIG_DIR -ErrorAction SilentlyContinue
        }
    }
}

# --- 2. Antigravity CLI (AGY) mcp_config.json files --------------------------
$agyBase = Join-Path $homeDir ".gemini\antigravity-cli"
$agyConfigs = @()

$v2AgyConfig = Join-Path $homeDir ".gemini\config\mcp_config.json"
if (Test-Path -LiteralPath $v2AgyConfig -PathType Leaf) {
    $agyConfigs += $v2AgyConfig
}

$rootAgyConfig = Join-Path $agyBase "mcp_config.json"
if (Test-Path -LiteralPath $rootAgyConfig -PathType Leaf) {
    $agyConfigs += $rootAgyConfig
}

$agyProfilesDir = Join-Path $agyBase "profiles"
if (Test-Path -LiteralPath $agyProfilesDir -PathType Container) {
    Get-ChildItem -LiteralPath $agyProfilesDir -Directory -ErrorAction SilentlyContinue |
        Sort-Object Name |
        ForEach-Object {
            $profileCfg = Join-Path $_.FullName "mcp_config.json"
            if (Test-Path -LiteralPath $profileCfg -PathType Leaf) {
                $agyConfigs += $profileCfg
            }
        }
}

if ($agyConfigs.Count -eq 0) {
    Write-Host "[SKIP] No Antigravity mcp_config.json files found under $agyBase."
} else {
    $utf8NoBom = [System.Text.UTF8Encoding]::new($false)

    foreach ($cfgPath in $agyConfigs) {
        $raw = Get-Content -LiteralPath $cfgPath -Raw -Encoding UTF8
        if ([string]::IsNullOrWhiteSpace($raw)) {
            $doc = [pscustomobject]@{}
        } else {
            $doc = $raw | ConvertFrom-Json
        }

        if (-not ($doc.PSObject.Properties.Name -contains "mcpServers") -or $null -eq $doc.mcpServers) {
            $doc | Add-Member -MemberType NoteProperty -Name "mcpServers" -Value ([pscustomobject]@{}) -Force
        }

        $existingEntry = $null
        if ($doc.mcpServers.PSObject.Properties.Name -contains "velox-team") {
            $existingEntry = $doc.mcpServers."velox-team"
        }

        if ($null -ne $existingEntry -and $existingEntry.serverUrl -eq $url) {
            Write-Host "[SKIP] AGY ($cfgPath): velox-team already points to $url."
            continue
        }

        $backupPath = "$cfgPath.bak-velox-team"
        if ($DryRun) {
            Write-Host "[DRY-RUN] AGY ($cfgPath): would write backup '$backupPath' and set mcpServers.velox-team.serverUrl = '$url'."
            continue
        }

        Copy-Item -LiteralPath $cfgPath -Destination $backupPath -Force
        $entry = [pscustomobject]@{ serverUrl = $url }
        $doc.mcpServers | Add-Member -MemberType NoteProperty -Name "velox-team" -Value $entry -Force

        $updatedJson = $doc | ConvertTo-Json -Depth 16
        [System.IO.File]::WriteAllText($cfgPath, $updatedJson + [Environment]::NewLine, $utf8NoBom)
        Write-Host "[UPDATE] AGY ($cfgPath): backup saved to '$backupPath'; registered velox-team -> $url."
    }
}
