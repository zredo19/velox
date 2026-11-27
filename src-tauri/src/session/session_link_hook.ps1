param([string]$AgentKind = "")

function Exit-Hook {
    if ($AgentKind -eq "agy") {
        Write-Output "{}"
    }
    exit 0
}

$veloxSessionId = $env:VELOX_SESSION_ID
if ([string]::IsNullOrWhiteSpace($veloxSessionId)) { Exit-Hook }
$safeVeloxId = ($veloxSessionId.Trim() -replace '[^A-Za-z0-9_-]', '')
if ([string]::IsNullOrWhiteSpace($safeVeloxId)) { Exit-Hook }

$inputText = [Console]::In.ReadToEnd()
if ([string]::IsNullOrWhiteSpace($inputText)) { Exit-Hook }

try {
    $payload = $inputText | ConvertFrom-Json
} catch {
    Exit-Hook
}
if ($null -eq $payload) { Exit-Hook }

$agentSessionId = $null
$transcriptPath = $null
$cwd = $null
$source = $null

if ($AgentKind -eq "claude") {
    if ($payload.agent_id -is [string] -and -not [string]::IsNullOrWhiteSpace($payload.agent_id)) { Exit-Hook }
    if ($payload.hook_event_name -eq "SubagentStop") { Exit-Hook }
    if ($payload.session_id -is [string]) { $agentSessionId = $payload.session_id.Trim() }
    if ($payload.transcript_path -is [string]) { $transcriptPath = $payload.transcript_path }
    if ($payload.cwd -is [string]) { $cwd = $payload.cwd }
    if ($payload.source -is [string]) { $source = $payload.source }
} elseif ($AgentKind -eq "agy") {
    if ($payload.conversationId -is [string]) { $agentSessionId = $payload.conversationId.Trim() }
    if ($payload.transcriptPath -is [string]) { $transcriptPath = $payload.transcriptPath }
    if ($payload.workspacePaths -and $payload.workspacePaths.Count -gt 0 -and $payload.workspacePaths[0] -is [string]) {
        $cwd = $payload.workspacePaths[0]
    }
    $source = "pre-invocation"
} else {
    Exit-Hook
}

if ([string]::IsNullOrWhiteSpace($agentSessionId)) { Exit-Hook }

$linksDir = $env:VELOX_AGENT_LINKS_DIR
if ([string]::IsNullOrWhiteSpace($linksDir)) {
    $baseDir = if (-not [string]::IsNullOrWhiteSpace($env:VELOX_CONFIG_DIR)) {
        $env:VELOX_CONFIG_DIR
    } else {
        Join-Path $env:APPDATA "velox"
    }
    $linksDir = Join-Path $baseDir "agent-links"
}

try {
    if (-not (Test-Path -LiteralPath $linksDir -PathType Container)) {
        New-Item -ItemType Directory -Path $linksDir -Force | Out-Null
    }
    $linkPath = Join-Path $linksDir "$safeVeloxId.json"
    $record = [ordered]@{
        veloxSessionId = $safeVeloxId
        agent = $AgentKind
        agentSessionId = $agentSessionId
        transcriptPath = $transcriptPath
        cwd = $cwd
        source = $source
        updatedAtMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    }
    $json = $record | ConvertTo-Json -Compress
    $tmpPath = "$linkPath.tmp.$PID"
    [System.IO.File]::WriteAllText($tmpPath, $json, [System.Text.UTF8Encoding]::new($false))
    Move-Item -LiteralPath $tmpPath -Destination $linkPath -Force
} catch {
    if ($tmpPath -and (Test-Path -LiteralPath $tmpPath -PathType Leaf)) {
        Remove-Item -LiteralPath $tmpPath -Force -ErrorAction SilentlyContinue
    }
}

Exit-Hook
