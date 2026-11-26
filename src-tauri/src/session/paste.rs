//! Pasting images and dropped files into a terminal.
//!
//! A terminal can only receive text, so an image is saved to a file and its path is pasted.
//! When the shell session is currently inside `ssh`, the file is first copied to that host
//! by running the same `ssh` command once more, and the remote path is pasted instead.
//! Nothing here runs until the user pastes or drops something.

use crate::domain::error::{VeloxError, VeloxResult};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tokio::io::AsyncWriteExt;

pub const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const STAGED_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(60);

/// ssh options that take a value (OpenSSH synopsis).
const VALUE_OPTIONS: &str = "BbcDEeFIiJLlmOoPpQRSWw";
/// Options that must not be repeated on the upload connection: forwards, tty and background flags.
const DROPPED_OPTIONS: &str = "DLRWNfntTMsO";

/// Everything needed to open one more connection to the host an `ssh` process is talking to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SshTarget {
    pub program: String,
    pub options: Vec<String>,
    pub destination: String,
}

pub fn is_image(path: &Path) -> bool {
    path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
        matches!(
            e.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
        )
    })
}

/// Only known image extensions reach a file name, which keeps the remote command fixed.
pub fn sanitize_extension(extension: &str) -> &'static str {
    match extension.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "jpg",
        "gif" => "gif",
        "webp" => "webp",
        "bmp" => "bmp",
        _ => "png",
    }
}

fn unique_name(extension: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("velox-clipboard-{nanos}.{}", sanitize_extension(extension))
}

fn staging_dir() -> PathBuf {
    std::env::temp_dir().join("velox-clipboard")
}

/// Saves clipboard bytes locally, removing staged files older than a day.
pub fn stage(extension: &str, data: &[u8]) -> VeloxResult<PathBuf> {
    let dir = staging_dir();
    std::fs::create_dir_all(&dir)?;
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let stale = entry
                .metadata()
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age > STAGED_MAX_AGE);
            if stale {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let path = dir.join(unique_name(extension));
    std::fs::write(&path, data)?;
    Ok(path)
}

/// Quotes a local path for pasting when it contains spaces.
pub fn quote_local(path: &str) -> String {
    if path.contains(' ') {
        format!("\"{path}\"")
    } else {
        path.to_string()
    }
}

/// Parses the argv of a running `ssh`. Returns `None` for non-interactive uses
/// (a remote command without `-t`, such as the ssh that `git` spawns, or `-N` tunnels).
pub fn parse_ssh_argv(argv: &[String]) -> Option<SshTarget> {
    let (program, rest) = argv.split_first()?;
    let mut options = Vec::new();
    let mut destination: Option<String> = None;
    let mut has_command = false;
    let mut forced_tty = false;
    let mut no_shell = false;

    let mut iter = rest.iter().peekable();
    while let Some(arg) = iter.next() {
        if arg == "--" {
            if destination.is_none() {
                destination = iter.next().cloned();
            }
            has_command = iter.peek().is_some();
            break;
        }
        let Some(cluster) = arg.strip_prefix('-').filter(|c| !c.is_empty()) else {
            if destination.is_none() {
                destination = Some(arg.clone());
                continue;
            }
            has_command = true;
            break;
        };

        // A cluster is any number of flags, optionally ending in one option and its value.
        for (index, flag) in cluster.char_indices() {
            forced_tty |= flag == 't';
            no_shell |= flag == 'N' || flag == 'W';
            if VALUE_OPTIONS.contains(flag) {
                let attached = &cluster[index + flag.len_utf8()..];
                let value = if attached.is_empty() {
                    iter.next().cloned()?
                } else {
                    attached.to_string()
                };
                if !DROPPED_OPTIONS.contains(flag) {
                    options.push(format!("-{flag}"));
                    options.push(value);
                }
                break;
            }
            if !DROPPED_OPTIONS.contains(flag) {
                options.push(format!("-{flag}"));
            }
        }
    }

    if no_shell || (has_command && !forced_tty) {
        return None;
    }
    Some(SshTarget {
        program: program.clone(),
        options,
        destination: destination?,
    })
}

/// Finds the interactive `ssh` running inside the process tree rooted at `root_pid`.
pub fn find_ssh_target(root_pid: u32) -> Option<SshTarget> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_exe(UpdateKind::Always),
    );

    let root = Pid::from_u32(root_pid);
    let mut found: Vec<(u64, SshTarget)> = Vec::new();
    for (pid, process) in system.processes() {
        let name = process.name().to_string_lossy().to_ascii_lowercase();
        if name != "ssh.exe" && name != "ssh" {
            continue;
        }
        // Walk up the parents; the depth limit guards against recycled-PID cycles.
        let mut current = Some(*pid);
        let descends = (0..32).any(|_| {
            let hit = current == Some(root);
            current = current
                .and_then(|p| system.process(p))
                .and_then(|p| p.parent());
            hit
        });
        if !descends {
            continue;
        }
        let mut argv: Vec<String> = process
            .cmd()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        // Upload with the very same ssh binary, so it reads the same config and keys.
        if let (Some(first), Some(exe)) = (argv.first_mut(), process.exe()) {
            *first = exe.to_string_lossy().into_owned();
        }
        if let Some(target) = parse_ssh_argv(&argv) {
            found.push((process.start_time(), target));
        }
    }
    found
        .into_iter()
        .max_by_key(|(start, _)| *start)
        .map(|(_, t)| t)
}

/// Copies `data` to `/tmp` on the ssh host and returns the remote path.
pub async fn upload(target: &SshTarget, extension: &str, data: &[u8]) -> VeloxResult<String> {
    let remote_path = format!("/tmp/{}", unique_name(extension));
    let mut command = tokio::process::Command::new(&target.program);
    command
        // First, so these win over anything repeated from the original command line.
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=8"])
        .args(["-o", "ClearAllForwardings=yes", "-T"])
        .args(&target.options)
        .arg(&target.destination)
        .arg(format!("umask 077 && cat > {remote_path}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW

    let mut child = command.spawn()?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| VeloxError::Pty("ssh upload has no stdin".into()))?;

    let transfer = async move {
        // If ssh dies early the write fails; its stderr below explains why.
        let written = stdin.write_all(data).await;
        drop(stdin);
        let output = child.wait_with_output().await?;
        if output.status.success() {
            written?;
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr
            .lines()
            .last()
            .unwrap_or("ssh failed")
            .trim()
            .to_string();
        Err(VeloxError::Pty(format!(
            "could not copy to {}: {reason}{}",
            target.destination,
            if reason.contains("Permission denied") {
                " (key-based login is required)"
            } else {
                ""
            }
        )))
    };

    tokio::time::timeout(UPLOAD_TIMEOUT, transfer)
        .await
        .map_err(|_| VeloxError::Pty("ssh upload timed out".into()))??;
    Ok(remote_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Option<SshTarget> {
        let argv: Vec<String> = line.split_whitespace().map(str::to_string).collect();
        parse_ssh_argv(&argv)
    }

    #[test]
    fn keeps_connection_options_in_any_position() {
        let target = parse("ssh -p 2222 -i key.pem ubuntu@host -v -o StrictHostKeyChecking=no")
            .expect("interactive");
        assert_eq!(target.destination, "ubuntu@host");
        assert_eq!(
            target.options,
            [
                "-p",
                "2222",
                "-i",
                "key.pem",
                "-v",
                "-o",
                "StrictHostKeyChecking=no"
            ]
        );
    }

    #[test]
    fn handles_attached_values_and_clusters() {
        let target = parse("ssh -vp2222 -Jjump vps").expect("interactive");
        assert_eq!(target.options, ["-v", "-p", "2222", "-J", "jump"]);
        assert_eq!(target.destination, "vps");
    }

    #[test]
    fn drops_forwards_and_tty_flags() {
        let target = parse("ssh -tt -L 8080:localhost:80 vps tmux attach").expect("tty forced");
        assert!(target.options.is_empty());
        assert_eq!(target.destination, "vps");
    }

    #[test]
    fn ignores_non_interactive_ssh() {
        assert_eq!(parse("ssh git@github.com git-upload-pack repo.git"), None);
        assert_eq!(parse("ssh -N -L 5432:db:5432 vps"), None);
        assert_eq!(parse("ssh -V"), None);
    }

    #[test]
    fn extension_never_reaches_the_shell_unchecked() {
        assert_eq!(sanitize_extension("JPEG"), "jpg");
        assert_eq!(sanitize_extension("png; rm -rf /"), "png");
    }

    #[test]
    fn quotes_only_paths_with_spaces() {
        assert_eq!(quote_local(r"C:\a\b.png"), r"C:\a\b.png");
        assert_eq!(quote_local(r"C:\a b\c.png"), r#""C:\a b\c.png""#);
    }
}
