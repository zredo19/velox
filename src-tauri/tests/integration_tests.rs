use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use velox_lib::domain::agent::{
    build_default_presets, decode_powershell_command, ps_single_quote, resolve_spawn_spec,
    AgentKind, AgentPreset,
};
use velox_lib::domain::config::VeloxConfig;
use velox_lib::mcp::config::load_backends_config;
use velox_lib::pty::job_object::ProcessJobObject;
use velox_lib::pty::manager::{ExitCallback, OutputCallback, PtyManager};
use velox_lib::pty::scrollback::RingScrollback;

#[test]
fn default_presets_include_core_agents() {
    let presets = build_default_presets();
    let kinds: Vec<AgentKind> = presets.iter().map(|p| p.kind).collect();
    assert!(kinds.contains(&AgentKind::Claude));
    assert!(kinds.contains(&AgentKind::Agy));
    assert!(kinds.contains(&AgentKind::Codex));
    assert!(kinds.contains(&AgentKind::OpenCode));
    assert!(kinds.contains(&AgentKind::Shell));
}

#[test]
fn resolve_spawn_spec_applies_resume_flags_for_all_agents() {
    let cases = [
        (AgentKind::Claude, "claude.exe", "--resume"),
        (AgentKind::Agy, "agy.exe", "--conversation"),
        (AgentKind::OpenCode, "opencode", "--session"),
        (AgentKind::Codex, "codex", "resume"),
    ];

    for (kind, cmd, expected_flag) in cases {
        let preset = AgentPreset {
            id: "test".to_string(),
            kind,
            label: "Test".to_string(),
            profile: None,
            description: "desc".to_string(),
            accent_color: kind.accent_color().to_string(),
            command: cmd.to_string(),
            args: vec![],
            env: HashMap::new(),
            installed: true,
        };
        let spec = resolve_spawn_spec(&preset, Path::new("."), Some("uuid-123"));
        let lower_prog = spec.program.to_ascii_lowercase();
        assert!(lower_prog.ends_with("pwsh.exe") || lower_prog.ends_with("powershell.exe"));
        assert_eq!(spec.args[0], "-NoLogo");
        assert_eq!(spec.args[1], "-NoExit");
        assert_eq!(spec.args[2], "-EncodedCommand");
        let script = decode_powershell_command(&spec.args[3]).expect("valid EncodedCommand");
        let expected_script = format!(
            "& {} {} {}",
            ps_single_quote(cmd),
            ps_single_quote(expected_flag),
            ps_single_quote("uuid-123")
        );
        assert_eq!(script, expected_script);
        assert_eq!(
            spec.env.get("TERM").map(String::as_str),
            Some("xterm-256color")
        );
        assert_eq!(
            spec.env.get("COLORTERM").map(String::as_str),
            Some("truecolor")
        );
    }
}

#[test]
fn velox_config_toml_roundtrip_preserves_memory_defaults() {
    let original = VeloxConfig::default();
    let encoded = toml::to_string_pretty(&original).expect("serialize config");
    let decoded: VeloxConfig = toml::from_str(&encoded).expect("deserialize config");

    assert_eq!(decoded.scrollback_bytes, 2 * 1024 * 1024);
    assert_eq!(decoded.auto_hibernate_minutes, 20);
    assert!(decoded.lazy_restore_on_startup);
    assert!(decoded.mcp_gateway_enabled);
    assert_eq!(decoded.mcp_gateway_port, 43111);
}

#[test]
fn velox_config_recent_dirs_deduplicates_and_caps_at_twelve() {
    let mut cfg = VeloxConfig::default();
    cfg.recent_dirs.clear();
    for i in 0..16 {
        cfg.record_recent_dir(&format!("C:\\Work\\proj-{i}"));
    }
    cfg.record_recent_dir("c:\\work\\PROJ-10");
    assert_eq!(cfg.recent_dirs.len(), 12);
    assert_eq!(cfg.recent_dirs[0], "c:\\work\\PROJ-10");
}

#[test]
fn ring_scrollback_enforces_exact_capacity_across_multiple_pushes() {
    let mut rb = RingScrollback::new(8);
    rb.push(b"abcd");
    rb.push(b"efgh");
    rb.push(b"ijkl");
    assert_eq!(rb.len(), 8);
    assert_eq!(rb.snapshot(), b"efghijkl");
}

#[test]
fn process_job_object_lifecycle_does_not_error() {
    let job = ProcessJobObject::new().expect("job creation should succeed");
    job.terminate().expect("terminate should succeed");
}

#[test]
fn loads_existing_lazy_mcp_gateway_backends_if_present() {
    let cfg = VeloxConfig::default();
    let path = Path::new(&cfg.mcp_backends_path);
    let parsed = load_backends_config(path).expect("load backends should succeed");
    if path.is_file() {
        assert!(!parsed.backends.is_empty());
    }
}

#[cfg(windows)]
#[test]
#[ignore = "spawns a real ConPTY PowerShell process"]
fn real_conpty_child_ctrl_c_and_exit_keeps_powershell_prompt_alive() {
    let pty = PtyManager::new(64 * 1024);
    let output = Arc::new(Mutex::new(String::new()));
    let exited = Arc::new(Mutex::new(Option::<Option<i32>>::None));

    let out_cb_buf = Arc::clone(&output);
    let on_output: OutputCallback = Arc::new(move |_id, bytes, _pos| {
        let chunk = String::from_utf8_lossy(bytes);
        out_cb_buf.lock().unwrap().push_str(&chunk);
    });

    let exit_cb_buf = Arc::clone(&exited);
    let on_exit: ExitCallback = Arc::new(move |_id, code| {
        *exit_cb_buf.lock().unwrap() = Some(code);
    });

    // Stand-in child: reads byte 0x03 directly (raw input, like TUI CLIs), prints a marker, and exits.
    let child_script = "[Console]::TreatControlCAsInput = $true; Write-Host 'CHILD_READY'; while ($true) { $k = [Console]::ReadKey($true); if ([int]$k.KeyChar -eq 3) { Write-Host 'CHILD_GOT_CTRL_C'; break } }";
    let preset = AgentPreset {
        id: "standin-agent".to_string(),
        kind: AgentKind::Agy,
        label: "Stand-in".to_string(),
        profile: None,
        description: "Test stand-in".to_string(),
        accent_color: AgentKind::Agy.accent_color().to_string(),
        command: "powershell.exe".to_string(),
        args: vec![
            "-NoLogo".to_string(),
            "-NoProfile".to_string(),
            "-Command".to_string(),
            child_script.to_string(),
        ],
        env: HashMap::new(),
        installed: true,
    };

    let spec = resolve_spawn_spec(&preset, Path::new("."), None);
    let sid = "conpty-test-1";
    pty.spawn(sid, &spec, 100, 30, on_output, on_exit)
        .expect("spawn ConPTY");

    let mut dsr_replies_sent = 0usize;
    let wait_for = |needle: &str, timeout: Duration, dsr_sent: &mut usize| -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            let snap = output.lock().unwrap().clone();
            let dsr_count = snap.matches("\x1b[6n").count();
            while *dsr_sent < dsr_count {
                let _ = pty.write(sid, b"\x1b[1;1R");
                *dsr_sent += 1;
            }
            if snap.contains(needle) {
                return true;
            }
            if exited.lock().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    };

    if !wait_for(
        "CHILD_READY",
        Duration::from_secs(15),
        &mut dsr_replies_sent,
    ) {
        let snap = output.lock().unwrap().clone();
        let ex = *exited.lock().unwrap();
        panic!(
            "child never became ready (alive={}, exited={:?}); output was:\n{:?}",
            pty.is_alive(sid),
            ex,
            snap
        );
    }

    // Send Ctrl+C (0x03) to the child
    pty.write(sid, b"\x03").expect("write Ctrl+C");
    if !wait_for(
        "CHILD_GOT_CTRL_C",
        Duration::from_secs(10),
        &mut dsr_replies_sent,
    ) {
        let snap = output.lock().unwrap().clone();
        panic!(
            "Ctrl+C did not reach stand-in child; output was:\n{:?}",
            snap
        );
    }

    // Give PowerShell a moment to return to its prompt, then verify shell is still alive
    std::thread::sleep(Duration::from_millis(300));
    assert!(pty.is_alive(sid), "outer PowerShell exited prematurely");

    pty.write(sid, b"Write-Output ('STILL' + '_IN_PS')\r")
        .expect("write command to PowerShell");
    if !wait_for(
        "STILL_IN_PS",
        Duration::from_secs(10),
        &mut dsr_replies_sent,
    ) {
        let snap = output.lock().unwrap().clone();
        panic!(
            "outer PowerShell prompt did not respond after child exit; output was:\n{:?}",
            snap
        );
    }

    std::thread::sleep(Duration::from_millis(200));
    pty.write(sid, b"exit\r").expect("exit PowerShell");
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(10) {
        let snap = output.lock().unwrap().clone();
        let dsr_count = snap.matches("\x1b[6n").count();
        while dsr_replies_sent < dsr_count {
            let _ = pty.write(sid, b"\x1b[1;1R");
            dsr_replies_sent += 1;
        }
        if exited.lock().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if exited.lock().unwrap().is_none() {
        let snap = output.lock().unwrap().clone();
        panic!(
            "PowerShell did not exit after `exit`; output was:\n{:?}",
            snap
        );
    }
}

#[cfg(windows)]
#[test]
#[ignore = "spawns a real ConPTY PowerShell wrapper with ping -t and verifies kill() frees the grandchild"]
fn real_conpty_kill_frees_grandchild_process() {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

    let pty = PtyManager::new(64 * 1024);
    let output = Arc::new(Mutex::new(String::new()));
    let out_buf = Arc::clone(&output);
    let on_output: OutputCallback = Arc::new(move |_id, bytes, _pos| {
        out_buf
            .lock()
            .unwrap()
            .push_str(&String::from_utf8_lossy(bytes));
    });
    let on_exit: ExitCallback = Arc::new(move |_id, _code| {});

    let preset = AgentPreset {
        id: "ping-agent".to_string(),
        kind: AgentKind::Agy,
        label: "Ping".to_string(),
        profile: None,
        description: "Test grandchild cleanup".to_string(),
        accent_color: AgentKind::Agy.accent_color().to_string(),
        command: "ping.exe".to_string(),
        args: vec!["-t".to_string(), "127.0.0.1".to_string()],
        env: HashMap::new(),
        installed: true,
    };

    let spec = resolve_spawn_spec(&preset, Path::new("."), None);
    let sid = "conpty-kill-grandchild";
    let root_pid = pty
        .spawn(sid, &spec, 100, 30, on_output, on_exit)
        .expect("spawn ConPTY wrapper");

    let mut sys = System::new();
    let find_ping_descendant = |sys: &mut System| -> Option<u32> {
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        for (pid, proc_info) in sys.processes() {
            if !proc_info
                .name()
                .to_string_lossy()
                .eq_ignore_ascii_case("ping.exe")
            {
                continue;
            }
            let mut cur = proc_info.parent();
            let mut depth = 0;
            while let Some(parent_pid) = cur {
                if parent_pid.as_u32() == root_pid {
                    return Some(pid.as_u32());
                }
                depth += 1;
                if depth > 16 {
                    break;
                }
                cur = sys.process(parent_pid).and_then(|p| p.parent());
            }
        }
        None
    };

    let mut dsr_sent = 0usize;
    let mut ping_pid: Option<u32> = None;
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(15) {
        let snap = output.lock().unwrap().clone();
        let dsr_count = snap.matches("\x1b[6n").count();
        while dsr_sent < dsr_count {
            let _ = pty.write(sid, b"\x1b[1;1R");
            dsr_sent += 1;
        }
        if let Some(found) = find_ping_descendant(&mut sys) {
            ping_pid = Some(found);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    let ping_pid = match ping_pid {
        Some(p) => p,
        None => {
            let _ = pty.kill(sid);
            let snap = output.lock().unwrap().clone();
            panic!(
                "never found ping.exe descendant of root_pid={root_pid} (program={}); output:\n{:?}",
                spec.program, snap
            );
        }
    };

    pty.kill(sid).expect("pty.kill should succeed");

    let mut ping_gone = false;
    let wait_dead = Instant::now();
    while wait_dead.elapsed() < Duration::from_secs(3) {
        sys.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        if sys.process(Pid::from_u32(ping_pid)).is_none() {
            ping_gone = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    if !ping_gone {
        // Safety cleanup so we never leave our test ping.exe running
        if let Some(p) = sys.process(Pid::from_u32(ping_pid)) {
            let _ = p.kill();
        }
        if let Some(r) = sys.process(Pid::from_u32(root_pid)) {
            let _ = r.kill();
        }
        panic!(
            "grandchild ping.exe (pid={ping_pid}) survived pty.kill() for wrapper program={}",
            spec.program
        );
    }
}
