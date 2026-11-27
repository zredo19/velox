//! Startup provisioning for Velox session messaging:
//! - installs the `velox` CLI scripts (`velox.ps1`, `velox.cmd`, `velox`) into `%LOCALAPPDATA%\velox\bin`
//! - registers/repairs the `velox-team` MCP entry for Claude Code profiles and Antigravity CLI
//! - maintains an idempotent `<!-- velox:begin --> ... <!-- velox:end -->` instruction block in
//!   global rules files for CLIs without `--append-system-prompt` (Antigravity CLI, Codex, OpenCode).

use crate::domain::agent::claude_profile_names;
pub use crate::domain::agent::prepend_path_env;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const VELOX_RULE_BEGIN: &str = "<!-- velox:begin -->";
pub const VELOX_RULE_END: &str = "<!-- velox:end -->";

/// Directory holding the `velox` CLI scripts (`%LOCALAPPDATA%\velox\bin`).
pub fn velox_cli_bin_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("velox")
        .join("bin")
}

/// Install CLI scripts and register MCP/global rules for the local user environment.
pub fn bootstrap_local_messaging(mcp_port: u16) {
    let bin_dir = velox_cli_bin_dir();
    if let Err(err) = install_cli_scripts(&bin_dir) {
        tracing::warn!(%err, path = %bin_dir.display(), "Failed to install velox CLI scripts");
    }

    let Some(home) = dirs::home_dir() else {
        return;
    };

    if mcp_port > 0 {
        register_claude_mcps(&home, mcp_port);
        register_agy_mcps(&home, mcp_port);
    }
    ensure_global_rules_blocks(&home);
}

/// Install `velox.ps1`, `velox.cmd`, and `velox` (POSIX sh shim for Git Bash) idempotently.
pub fn install_cli_scripts(bin_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(bin_dir)?;
    write_if_changed(&bin_dir.join("velox.ps1"), VELOX_PS1.as_bytes())?;
    write_if_changed(&bin_dir.join("velox.cmd"), VELOX_CMD.as_bytes())?;
    write_if_changed(&bin_dir.join("velox"), VELOX_SH.as_bytes())?;
    Ok(())
}

fn write_if_changed(path: &Path, desired: &[u8]) -> std::io::Result<()> {
    if let Ok(existing) = std::fs::read(path) {
        if existing == desired {
            return Ok(());
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_file_name(format!(
        "{}.tmp-{}",
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("velox"),
        uuid::Uuid::new_v4()
    ));
    std::fs::write(&tmp, desired)?;
    if let Err(err) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

fn mcp_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/mcp/velox-team")
}

/// Register/repair `velox-team` across `~/.claude.json` and every `~/.claude-<name>/.claude.json`.
pub fn register_claude_mcps(home: &Path, port: u16) {
    let url = mcp_url(port);
    let desired = json!({
        "type": "http",
        "url": url,
        "headers": {
            "X-Velox-Session": "${VELOX_SESSION_ID:-}"
        }
    });

    let mut targets = Vec::new();
    let default_json = home.join(".claude.json");
    if default_json.is_file() || home.join(".claude").is_dir() {
        targets.push(default_json);
    }
    for profile in claude_profile_names(home) {
        targets.push(home.join(format!(".claude-{profile}")).join(".claude.json"));
    }

    for path in targets {
        if let Err(err) = upsert_mcp_server_json(&path, "velox-team", &desired) {
            tracing::warn!(%err, path = %path.display(), "Failed to register velox-team in Claude config");
        }
    }
}

/// Register/repair `velox-team` in Antigravity CLI configs (`~/.gemini/config/mcp_config.json`,
/// `~/.gemini/antigravity-cli/mcp_config.json`, and any profile `mcp_config.json`).
pub fn register_agy_mcps(home: &Path, port: u16) {
    let url = mcp_url(port);
    let desired = json!({ "serverUrl": url });

    let mut targets = Vec::new();
    let v2_dir = home.join(".gemini").join("config");
    if v2_dir.is_dir() {
        targets.push(v2_dir.join("mcp_config.json"));
    }

    let agy_base = home.join(".gemini").join("antigravity-cli");
    if agy_base.is_dir() {
        targets.push(agy_base.join("mcp_config.json"));
    }

    let profiles_dir = agy_base.join("profiles");
    if let Ok(entries) = std::fs::read_dir(&profiles_dir) {
        for entry in entries.flatten() {
            let cfg = entry.path().join("mcp_config.json");
            if cfg.is_file() {
                targets.push(cfg);
            }
        }
    }

    for path in targets {
        if let Err(err) = upsert_mcp_server_json(&path, "velox-team", &desired) {
            tracing::warn!(%err, path = %path.display(), "Failed to register velox-team in AGY config");
        }
    }
}

/// Idempotently upsert `mcpServers[server_name] = desired` in a JSON file, preserving all other keys.
pub fn upsert_mcp_server_json(
    path: &Path,
    server_name: &str,
    desired: &Value,
) -> Result<(), String> {
    let mut root = if path.is_file() {
        let raw = std::fs::read_to_string(path).map_err(|e| format!("read failed: {e}"))?;
        if raw.trim().is_empty() {
            Value::Object(Map::new())
        } else {
            serde_json::from_str::<Value>(&raw).map_err(|e| format!("invalid JSON: {e}"))?
        }
    } else {
        Value::Object(Map::new())
    };

    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| "root JSON value is not an object".to_string())?;

    let servers_val = root_obj
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    let servers_obj = servers_val
        .as_object_mut()
        .ok_or_else(|| "mcpServers is not an object".to_string())?;

    if servers_obj.get(server_name) == Some(desired) {
        return Ok(());
    }

    servers_obj.insert(server_name.to_string(), desired.clone());
    let mut serialized = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("serialize failed: {e}"))?;
    serialized.push('\n');
    write_if_changed(path, serialized.as_bytes()).map_err(|e| format!("write failed: {e}"))
}

/// Ensure the idempotent `<!-- velox:begin --> ... <!-- velox:end -->` instruction block is
/// present near the top of global rules files for Antigravity CLI (and Codex/OpenCode when present).
pub fn ensure_global_rules_blocks(home: &Path) {
    let block = velox_global_rule_block();
    let candidates = [
        (home.join(".gemini"), home.join(".gemini").join("GEMINI.md")),
        (home.join(".codex"), home.join(".codex").join("AGENTS.md")),
        (
            home.join(".config").join("opencode"),
            home.join(".config").join("opencode").join("AGENTS.md"),
        ),
    ];

    for (parent_dir, rules_path) in candidates {
        if !parent_dir.is_dir() {
            continue;
        }
        if let Err(err) = upsert_velox_rules_file(&rules_path, &block) {
            tracing::warn!(%err, path = %rules_path.display(), "Failed to update Velox rules block");
        }
    }
}

pub fn velox_global_rule_block() -> String {
    [
        VELOX_RULE_BEGIN,
        "## Velox Session Messaging",
        "",
        "Apply these instructions only when `VELOX_SESSION_ID` is set in your environment (you are running inside a Velox tab).",
        "",
        "- List live Velox sessions: `velox list` (or `velox-team` MCP tool `session_list`).",
        "- Send a direct message to another session: `velox msg \"<session>\" \"<message>\"` (or `velox-team` MCP tool `session_send`).",
        "- Read another session's recent terminal output: `velox read \"<session>\" [lines]` (or `velox-team` MCP tool `team_read_session`).",
        "- When the user says \"usa velox para hablarle a X\" or asks to message or check another Velox session, run `velox list` / `velox msg` immediately without asking for confirmation.",
        "- Messages arriving in your prompt starting with `[Velox · mensaje de ...]` come from another Velox session: reply to that session with `velox msg`.",
        VELOX_RULE_END,
    ]
    .join("\n")
}

/// Insert or replace `block` inside `path`, keeping it within the first 16 KB so CLIs with a
/// per-file rule size cap (such as Antigravity CLI's 24 KB limit) never truncate it.
pub fn upsert_velox_rules_file(path: &Path, block: &str) -> std::io::Result<()> {
    let existing = if path.is_file() {
        std::fs::read_to_string(path)?
    } else {
        String::new()
    };

    let updated = upsert_velox_rules_content(&existing, block);
    if updated == existing {
        return Ok(());
    }
    write_if_changed(path, updated.as_bytes())
}

const MAX_BLOCK_START_OFFSET: usize = 16_000;

fn upsert_velox_rules_content(existing: &str, block: &str) -> String {
    let normalized = existing.replace("\r\n", "\n");
    if let Some(begin_idx) = normalized.find(VELOX_RULE_BEGIN) {
        if let Some(rel_end) = normalized[begin_idx..].find(VELOX_RULE_END) {
            let end_idx = begin_idx + rel_end + VELOX_RULE_END.len();
            let current_block = &normalized[begin_idx..end_idx];
            if current_block == block && begin_idx <= MAX_BLOCK_START_OFFSET {
                return existing.to_string();
            }

            let before = normalized[..begin_idx].trim_end_matches('\n');
            let after = normalized[end_idx..].trim_start_matches('\n');
            if begin_idx <= MAX_BLOCK_START_OFFSET {
                let mut out = String::new();
                if !before.is_empty() {
                    out.push_str(before);
                    out.push_str("\n\n");
                }
                out.push_str(block);
                if !after.is_empty() {
                    out.push_str("\n\n");
                    out.push_str(after);
                }
                out.push('\n');
                return out;
            }

            // Old block lived past 16 KB: strip it and move it to the top.
            let mut stripped = String::new();
            if !before.is_empty() {
                stripped.push_str(before);
            }
            if !after.is_empty() {
                if !stripped.is_empty() {
                    stripped.push_str("\n\n");
                }
                stripped.push_str(after);
            }
            return prepend_block(&stripped, block);
        }
    }

    prepend_block(&normalized, block)
}

fn prepend_block(body: &str, block: &str) -> String {
    let trimmed = body.trim_start_matches('\n').trim_end_matches('\n');
    if trimmed.is_empty() {
        format!("{block}\n")
    } else {
        format!("{block}\n\n{trimmed}\n")
    }
}

const VELOX_CMD: &str = concat!(
    "@echo off\r\n",
    "setlocal\r\n",
    "powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File \"%~dp0velox.ps1\" %*\r\n",
    "exit /b %errorlevel%\r\n",
);

const VELOX_SH: &str = concat!(
    "#!/bin/sh\n",
    "DIR=\"$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\"\n",
    "PS_SCRIPT=\"$(cygpath -w \"$DIR/velox.ps1\" 2>/dev/null || printf '%s/velox.ps1' \"$DIR\")\"\n",
    "exec powershell.exe -NoLogo -NoProfile -ExecutionPolicy Bypass -File \"$PS_SCRIPT\" \"$@\"\n",
);

const VELOX_PS1: &str = r#"$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::InputEncoding = [System.Text.Encoding]::UTF8

function Show-Usage {
    @"
velox - direct messaging between Velox sessions

Usage:
  velox list [--json]                        List live sessions (marks yours)
  velox msg [--from <session>] <to> <text>   Send a direct message to another session
  velox read <session> [lines]               Read recent terminal output (default 60 lines)
"@
}

function Fail {
    param([string]$Message, [int]$Code = 1)
    [Console]::Error.WriteLine($Message)
    exit $Code
}

function Invoke-VeloxMcp {
    param(
        [string]$ToolName,
        [hashtable]$Arguments
    )

    $baseUrl = if ([string]::IsNullOrWhiteSpace($env:VELOX_GATEWAY_URL)) {
        "http://127.0.0.1:43111"
    } else {
        $env:VELOX_GATEWAY_URL.TrimEnd("/")
    }
    $endpoint = "$baseUrl/mcp/velox-team"

    $payload = @{
        jsonrpc = "2.0"
        id = 1
        method = "tools/call"
        params = @{
            name = $ToolName
            arguments = $Arguments
        }
    } | ConvertTo-Json -Depth 8 -Compress

    $bytes = [System.Text.Encoding]::UTF8.GetBytes($payload)
    $client = New-Object System.Net.WebClient
    $client.Headers.Add("Content-Type", "application/json; charset=utf-8")
    $client.Headers.Add("Accept", "application/json")
    if (-not [string]::IsNullOrWhiteSpace($env:VELOX_SESSION_ID)) {
        $client.Headers.Add("X-Velox-Session", $env:VELOX_SESSION_ID.Trim())
    }

    try {
        $respBytes = $client.UploadData($endpoint, "POST", $bytes)
    } catch {
        Fail "Could not reach Velox gateway at $endpoint. Is Velox running? ($($_.Exception.Message))" 1
    } finally {
        $client.Dispose()
    }

    $respText = [System.Text.Encoding]::UTF8.GetString($respBytes)
    $resp = $respText | ConvertFrom-Json

    if ($null -ne $resp.error) {
        $msg = if ($resp.error.message) { $resp.error.message } else { $respText }
        Fail "Velox MCP error: $msg" 1
    }

    return $resp.result
}

function Write-ToolResult {
    param(
        $Result,
        [bool]$JsonOutput
    )

    if ($null -eq $Result) {
        Fail "Empty response from Velox gateway." 1
    }

    $textOut = ""
    if ($Result.content -and $Result.content.Count -gt 0 -and $Result.content[0].text) {
        $textOut = [string]$Result.content[0].text
    }

    if ($Result.isError -eq $true) {
        if ([string]::IsNullOrWhiteSpace($textOut)) { $textOut = "Command failed." }
        Fail $textOut 1
    }

    if ($JsonOutput) {
        if ($null -ne $Result.structuredContent) {
            Write-Output ($Result.structuredContent | ConvertTo-Json -Depth 12)
        } else {
            Write-Output ($Result | ConvertTo-Json -Depth 12)
        }
        return
    }

    if (-not [string]::IsNullOrEmpty($textOut)) {
        Write-Output $textOut
    }
}

$rawArgs = @($args)
if ($rawArgs.Count -eq 0) {
    Show-Usage
    exit 0
}

$cmd = [string]$rawArgs[0]
$rest = if ($rawArgs.Count -gt 1) { @($rawArgs[1..($rawArgs.Count - 1)]) } else { @() }

switch ($cmd.ToLowerInvariant()) {
    { $_ -in @("-h", "--help", "help") } {
        Show-Usage
        exit 0
    }
    { $_ -in @("list", "ls") } {
        $jsonOut = $false
        foreach ($a in $rest) {
            if ($a -eq "--json") { $jsonOut = $true }
            elseif ($a -in @("-h", "--help")) { Show-Usage; exit 0 }
            else { Fail "Unknown argument for 'velox list': $a" 2 }
        }
        $res = Invoke-VeloxMcp -ToolName "session_list" -Arguments @{}
        Write-ToolResult -Result $res -JsonOutput $jsonOut
        exit 0
    }
    { $_ -in @("msg", "send") } {
        $jsonOut = $false
        $fromSession = $null
        $positional = New-Object System.Collections.Generic.List[string]
        $i = 0
        while ($i -lt $rest.Count) {
            $a = [string]$rest[$i]
            if ($positional.Count -eq 0 -and $a -eq "--json") {
                $jsonOut = $true
                $i++
                continue
            }
            if ($positional.Count -eq 0 -and $a -eq "--from") {
                if ($i + 1 -ge $rest.Count) { Fail "Missing value after --from" 2 }
                $fromSession = [string]$rest[$i + 1]
                $i += 2
                continue
            }
            if ($positional.Count -eq 0 -and ($a -eq "-h" -or $a -eq "--help")) {
                Show-Usage
                exit 0
            }
            if ($a -eq "--" -and $positional.Count -eq 0) {
                $i++
                while ($i -lt $rest.Count) {
                    $positional.Add([string]$rest[$i])
                    $i++
                }
                break
            }
            $positional.Add($a)
            $i++
        }

        if ($positional.Count -lt 2) {
            Fail "Usage: velox msg <session> `"<text>`"" 2
        }
        $target = $positional[0]
        $text = ($positional.GetRange(1, $positional.Count - 1) -join " ")
        $toolArgs = @{
            to = $target
            text = $text
        }
        if (-not [string]::IsNullOrWhiteSpace($fromSession)) {
            $toolArgs["from_session"] = $fromSession
        }
        $res = Invoke-VeloxMcp -ToolName "session_send" -Arguments $toolArgs
        Write-ToolResult -Result $res -JsonOutput $jsonOut
        exit 0
    }
    "read" {
        $jsonOut = $false
        $linesOpt = $null
        $positional = New-Object System.Collections.Generic.List[string]
        $i = 0
        while ($i -lt $rest.Count) {
            $a = [string]$rest[$i]
            if ($a -eq "--json") {
                $jsonOut = $true
                $i++
                continue
            }
            if ($a -eq "--lines" -or $a -eq "-n") {
                if ($i + 1 -ge $rest.Count) { Fail "Missing number after $a" 2 }
                $linesOpt = [string]$rest[$i + 1]
                $i += 2
                continue
            }
            if ($a -eq "-h" -or $a -eq "--help") {
                Show-Usage
                exit 0
            }
            $positional.Add($a)
            $i++
        }

        if ($positional.Count -lt 1 -or $positional.Count -gt 2) {
            Fail "Usage: velox read <session> [lines]" 2
        }
        $target = $positional[0]
        if ($positional.Count -eq 2) {
            $linesOpt = $positional[1]
        }

        $toolArgs = @{ session = $target }
        if ($null -ne $linesOpt) {
            $parsedLines = 0
            if (-not [int]::TryParse($linesOpt, [ref]$parsedLines) -or $parsedLines -lt 1) {
                Fail "lines must be a positive integer (got '$linesOpt')" 2
            }
            $toolArgs["lines"] = $parsedLines
        }
        $res = Invoke-VeloxMcp -ToolName "team_read_session" -Arguments $toolArgs
        Write-ToolResult -Result $res -JsonOutput $jsonOut
        exit 0
    }
    default {
        Fail "Unknown subcommand '$cmd'. Run 'velox --help' for usage." 2
    }
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepend_path_env_puts_bin_dir_first_without_duplicating() {
        let bin = PathBuf::from(r"C:\Users\Test\AppData\Local\velox\bin");
        let existing = std::env::join_paths([
            PathBuf::from(r"C:\Windows\System32"),
            PathBuf::from(r"c:\users\test\appdata\local\velox\bin\"),
            PathBuf::from(r"C:\Tools"),
        ])
        .unwrap()
        .to_string_lossy()
        .into_owned();

        let mut env = std::collections::HashMap::new();
        env.insert("Path".to_string(), existing);
        prepend_path_env(&mut env, &bin);

        assert!(!env.contains_key("Path"));
        let split: Vec<PathBuf> = std::env::split_paths(env.get("PATH").unwrap()).collect();
        assert_eq!(split.len(), 3);
        assert_eq!(split[0], bin);
        assert_eq!(split[1], PathBuf::from(r"C:\Windows\System32"));
        assert_eq!(split[2], PathBuf::from(r"C:\Tools"));
    }

    #[test]
    fn install_cli_scripts_is_idempotent() {
        let dir = std::env::temp_dir().join(format!("velox-cli-test-{}", uuid::Uuid::new_v4()));
        install_cli_scripts(&dir).expect("first install");
        install_cli_scripts(&dir).expect("second install");

        assert!(dir.join("velox.ps1").is_file());
        assert!(dir.join("velox.cmd").is_file());
        assert!(dir.join("velox").is_file());
        let ps1 = std::fs::read_to_string(dir.join("velox.ps1")).unwrap();
        assert!(ps1.contains("X-Velox-Session"));
        assert!(ps1.contains("session_list"));
        assert!(ps1.contains("session_send"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn registers_claude_and_agy_mcps_preserving_existing_entries() {
        let home = std::env::temp_dir().join(format!("velox-mcp-reg-{}", uuid::Uuid::new_v4()));
        let claude_default = home.join(".claude.json");
        let claude_work = home.join(".claude-trabajo").join(".claude.json");
        let agy_v2 = home.join(".gemini").join("config").join("mcp_config.json");
        let agy_legacy = home.join(".gemini").join("antigravity-cli").join("mcp_config.json");
        let agy_prof = home
            .join(".gemini")
            .join("antigravity-cli")
            .join("profiles")
            .join("personal")
            .join("mcp_config.json");

        for p in [&claude_default, &claude_work, &agy_v2, &agy_legacy, &agy_prof] {
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        }
        std::fs::write(
            &claude_default,
            r#"{"D:/Workspace/Foo":1,"D:/Workspace/foo":2,"mcpServers":{"other":{"type":"stdio","command":"x"}}}"#,
        )
        .unwrap();
        std::fs::write(&claude_work, r#"{"numStartups":3}"#).unwrap();
        std::fs::write(&agy_v2, r#"{"mcpServers":{"codegraph":{"serverUrl":"http://127.0.0.1:43111/mcp/codegraph"}}}"#).unwrap();
        std::fs::write(&agy_legacy, r#"{"mcpServers":{"velox-team":{"serverUrl":"http://127.0.0.1:9999/mcp/velox-team"}}}"#).unwrap();
        std::fs::write(&agy_prof, r#"{"mcpServers":{}}"#).unwrap();

        register_claude_mcps(&home, 43111);
        register_agy_mcps(&home, 43111);
        // Second call is a clean no-op
        register_claude_mcps(&home, 43111);
        register_agy_mcps(&home, 43111);

        let c_def: Value = serde_json::from_str(&std::fs::read_to_string(&claude_default).unwrap()).unwrap();
        assert_eq!(c_def["D:/Workspace/Foo"], 1);
        assert_eq!(c_def["D:/Workspace/foo"], 2);
        assert_eq!(c_def["mcpServers"]["other"]["command"], "x");
        assert_eq!(
            c_def["mcpServers"]["velox-team"]["headers"]["X-Velox-Session"],
            "${VELOX_SESSION_ID:-}"
        );

        let c_work: Value = serde_json::from_str(&std::fs::read_to_string(&claude_work).unwrap()).unwrap();
        assert_eq!(c_work["numStartups"], 3);
        assert_eq!(
            c_work["mcpServers"]["velox-team"]["url"],
            "http://127.0.0.1:43111/mcp/velox-team"
        );

        for p in [&agy_v2, &agy_legacy, &agy_prof] {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
            assert_eq!(
                v["mcpServers"]["velox-team"]["serverUrl"],
                "http://127.0.0.1:43111/mcp/velox-team"
            );
        }

        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn upsert_velox_rules_prepends_updates_and_moves_past_cap_blocks() {
        let block = velox_global_rule_block();
        let large_tail = "x".repeat(20_000);

        // Fresh insertion prepends before large existing rules
        let inserted = upsert_velox_rules_content(&large_tail, &block);
        assert!(inserted.starts_with(VELOX_RULE_BEGIN));
        assert!(inserted.contains(&large_tail));

        // Second pass is strictly idempotent
        let second = upsert_velox_rules_content(&inserted, &block);
        assert_eq!(second, inserted);

        // If an old block was appended past 16 KB, it gets moved back to the top and deduplicated
        let at_bottom = format!("{large_tail}\n\n{VELOX_RULE_BEGIN}\nold\n{VELOX_RULE_END}\n");
        let moved = upsert_velox_rules_content(&at_bottom, &block);
        assert!(moved.starts_with(VELOX_RULE_BEGIN));
        assert_eq!(moved.matches(VELOX_RULE_BEGIN).count(), 1);
        assert!(moved.contains(&block));
    }
}
