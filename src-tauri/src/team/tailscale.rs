//! Tailscale IPv4 detection and validation helpers.

use std::io::Read;
use std::net::Ipv4Addr;
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(60);
const CMD_TIMEOUT: Duration = Duration::from_secs(3);
#[cfg(windows)]
const TAILSCALE_FALLBACK_PATH: &str = r"C:\Program Files\Tailscale\tailscale.exe";

type TailscaleCacheEntry = Option<(Instant, Option<Ipv4Addr>)>;

static TAILSCALE_CACHE: OnceLock<Mutex<TailscaleCacheEntry>> = OnceLock::new();

/// Returns `true` if `ip` belongs to the Tailscale CGNAT range (`100.64.0.0/10`).
pub fn is_tailscale_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    octets[0] == 100 && (64..=127).contains(&octets[1])
}

/// Parses the first line of `tailscale ip -4` output as a Tailscale CGNAT IPv4 (`100.64.0.0/10`).
pub fn parse_tailscale_ip(output: &str) -> Option<Ipv4Addr> {
    let first_line = output.trim().lines().next()?.trim();
    let ip: Ipv4Addr = first_line.parse().ok()?;
    is_tailscale_ipv4(ip).then_some(ip)
}

fn run_tailscale_ip(program: &str) -> Option<Ipv4Addr> {
    let mut cmd = Command::new(program);
    cmd.args(["ip", "-4"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + CMD_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut out = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.read_to_string(&mut out);
                }
                return parse_tailscale_ip(&out);
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

fn query_tailscale_ipv4() -> Option<Ipv4Addr> {
    if let Some(ip) = run_tailscale_ip("tailscale") {
        return Some(ip);
    }
    #[cfg(windows)]
    {
        if std::path::Path::new(TAILSCALE_FALLBACK_PATH).is_file() {
            return run_tailscale_ip(TAILSCALE_FALLBACK_PATH);
        }
    }
    None
}

/// Returns this machine's Tailscale IPv4 (`100.64.0.0/10`), cached for 60 seconds,
/// or `None` when Tailscale is not running or unreachable.
pub fn detect_tailscale_ipv4() -> Option<Ipv4Addr> {
    let cache = TAILSCALE_CACHE.get_or_init(|| Mutex::new(None));
    {
        let guard = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((fetched_at, cached)) = *guard {
            if fetched_at.elapsed() < CACHE_TTL {
                return cached;
            }
        }
    }

    let detected = query_tailscale_ipv4();
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some((Instant::now(), detected));
    detected
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tailscale_ip_accepts_cgnat_range() {
        assert_eq!(
            parse_tailscale_ip("100.64.0.1\n"),
            Some(Ipv4Addr::new(100, 64, 0, 1))
        );
        assert_eq!(
            parse_tailscale_ip("  100.101.102.103 \r\n100.64.0.2\r\n"),
            Some(Ipv4Addr::new(100, 101, 102, 103))
        );
        assert_eq!(
            parse_tailscale_ip("100.127.255.254"),
            Some(Ipv4Addr::new(100, 127, 255, 254))
        );
    }

    #[test]
    fn parse_tailscale_ip_rejects_outside_cgnat_and_invalid() {
        assert_eq!(parse_tailscale_ip("100.63.255.255"), None);
        assert_eq!(parse_tailscale_ip("100.128.0.0"), None);
        assert_eq!(parse_tailscale_ip("192.168.1.10"), None);
        assert_eq!(parse_tailscale_ip("127.0.0.1"), None);
        assert_eq!(parse_tailscale_ip("fd7a:115c:a1e0::1"), None);
        assert_eq!(parse_tailscale_ip(""), None);
        assert_eq!(parse_tailscale_ip("   \n  "), None);
        assert_eq!(parse_tailscale_ip("not-an-ip"), None);
    }
}
