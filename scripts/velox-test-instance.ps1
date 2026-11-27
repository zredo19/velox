<#
.SYNOPSIS
Starts a second, isolated Velox instance on 127.0.0.1 for local Velox Online end-to-end testing.

.PARAMETER Exe
Path to velox.exe to launch.

.PARAMETER Token
Shared Velox Online team token (must match the main instance's [team].token).

.PARAMETER Name
Display name for the isolated test instance (default: TestB).

.PARAMETER ConfigDir
Isolated config directory passed via VELOX_CONFIG_DIR (default: "$env:TEMP\velox-test-b").

.PARAMETER Reset
Overwrite config.toml in ConfigDir even if it already exists.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$Exe,

    [Parameter(Mandatory = $true)]
    [string]$Token,

    [string]$Name = "TestB",

    [string]$ConfigDir = (Join-Path $env:TEMP "velox-test-b"),

    [switch]$Reset
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($Token)) {
    throw "Token must not be empty."
}
if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) {
    throw "Velox executable not found at: $Exe"
}

function Escape-TomlString {
    param([string]$Value)
    return $Value.Replace('\', '\\').Replace('"', '\"')
}

if (-not (Test-Path -LiteralPath $ConfigDir -PathType Container)) {
    New-Item -ItemType Directory -Path $ConfigDir -Force | Out-Null
}

$configPath = Join-Path $ConfigDir "config.toml"
if ($Reset -or -not (Test-Path -LiteralPath $configPath -PathType Leaf)) {
    $escapedName = Escape-TomlString $Name
    $escapedToken = Escape-TomlString $Token

    $toml = @"
mcpGatewayPort = 43211
lazyRestoreOnStartup = false
autoHibernateMinutes = 0

[team]
enabled = true
displayName = "$escapedName"
listenPort = 43220
bindHost = "127.0.0.1"
token = "$escapedToken"

[[team.peers]]
name = "Main"
host = "127.0.0.1"
port = 43120
"@

    $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
    [System.IO.File]::WriteAllText($configPath, $toml + [Environment]::NewLine, $utf8NoBom)
    Write-Host "Wrote isolated test config: $configPath"
} else {
    Write-Host "Using existing test config (pass -Reset to overwrite): $configPath"
}

Write-Host ""
Write-Host "Point your MAIN Velox instance (%APPDATA%\velox\config.toml) at this test instance:"
Write-Host "------------------------------------------------------------------------"
Write-Host "[team]"
Write-Host "enabled = true"
Write-Host "listenPort = 43120"
Write-Host 'bindHost = "127.0.0.1"'
Write-Host "token = `"$(Escape-TomlString $Token)`""
Write-Host ""
Write-Host "[[team.peers]]"
Write-Host "name = `"$(Escape-TomlString $Name)`""
Write-Host 'host = "127.0.0.1"'
Write-Host "port = 43220"
Write-Host "------------------------------------------------------------------------"
Write-Host ""
Write-Host "Launching isolated Velox with VELOX_CONFIG_DIR=$ConfigDir ..."

$hadPrev = Test-Path Env:VELOX_CONFIG_DIR
$prev = $env:VELOX_CONFIG_DIR
try {
    $env:VELOX_CONFIG_DIR = $ConfigDir
    $proc = Start-Process -FilePath $Exe -PassThru
    Write-Host "Started $Name (PID $($proc.Id)) using MCP port 43211 and team port 43220."
} finally {
    if ($hadPrev) {
        $env:VELOX_CONFIG_DIR = $prev
    } else {
        Remove-Item Env:VELOX_CONFIG_DIR -ErrorAction SilentlyContinue
    }
}
