//! Tailscale helpers. Lane A owns this file. STUB.

use std::net::Ipv4Addr;

/// This machine's Tailscale IPv4 (100.64.0.0/10), or `None` when Tailscale is not running.
pub fn detect_tailscale_ipv4() -> Option<Ipv4Addr> {
    None
}
