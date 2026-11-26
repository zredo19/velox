# Lane 6 (`velox-settings-qa`) — Settings Modal & Rust Integration Test Suite

**Model Tier:** DeepSeek V4.1 Flash (Dedicated Settings UI + Integration Verification Lane)
**Skills to follow:** Read `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox\.agents\skills\rust-best-practices\SKILL.md` and `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox\docs\ARCHITECTURE.md`.

## Owned Files (CREATE/EDIT ONLY THESE FILES)
- `src/components/SettingsModal.tsx` (NEW)
- `src-tauri/tests/integration_tests.rs` (NEW)

Do NOT modify any other file (`App.tsx`, `Sidebar.tsx`, `TitleBar.tsx`, `Cargo.toml`, `src-tauri/src/*`). The Coordinator will wire `SettingsModal` into `App.tsx` after all lanes settle.

## Exact Requirements

### 1. `src/components/SettingsModal.tsx`
Build a crisp, intentional dark industrial-minimalist Settings modal matching `docs/ARCHITECTURE.md` design tokens (`#171B26` surface, `border-white/[0.09]`, clean sentence case, `tabular-nums` numerals, zero SaaS clutter):
- Export exact interface & component:
  ```ts
  export interface SettingsModalProps {
    open: boolean;
    onClose: () => void;
    config: VeloxConfig | null;
    onSave: (next: VeloxConfig) => Promise<void>;
  }
  export function SettingsModal({ open, onClose, config, onSave }: SettingsModalProps)
  ```
- Fields (all bound to local draft state synced when `open` / `config` changes):
  1. **Memory & Hibernation**
     - `Lazy restore on startup` (toggle switch: restores saved sessions at `0 MB` Sleeping until clicked)
     - `Auto-hibernate idle sessions` (number input / quick presets: `Off (0)`, `10m`, `20m`, `30m`, `60m`)
     - `Scrollback buffer per pane` (segmented selector: `1 MB`, `2 MB`, `4 MB`, `8 MB` mapping to `scrollbackBytes`)
  2. **Lazy MCP Gateway**
     - `Enable embedded Lazy MCP Gateway` (toggle switch)
     - `Gateway port` (numeric input, default `43111`)
     - `Backends config path` (text input for `backends.json`)
  3. **Terminal & Workspace**
     - `Default working directory` (text input + native folder picker button calling `veloxApi.pickDirectory`)
     - `Font family` (text input)
     - `Font size` (range / +/- stepper from `10` to `20` px with live monospace preview line)
- Footer with `"Cancel"` and `"Save changes"` (shows subtle `"Saved"` feedback, calls `onSave(draft)` then `onClose()`, handles `Escape` key and backdrop click cleanly).

### 2. `src-tauri/tests/integration_tests.rs`
Write comprehensive Rust integration tests importing `velox_lib::*` (remember crate lib name is `velox_lib` in `src-tauri/Cargo.toml`!):
- Test `domain::agent`:
  - `default_presets_include_core_agents`: verifies `build_default_presets()` includes `agy`, `codex`, `opencode`, `shell`, and at least one `claude` preset.
  - `resolve_spawn_spec_applies_resume_flags_for_all_agents`: verifies Claude `--resume`, AGY `--conversation`, OpenCode `--session`, Codex `resume`, and `TERM`/`COLORTERM` env injection.
- Test `domain::config`:
  - `velox_config_toml_roundtrip_preserves_memory_defaults`: serializes and deserializes `VeloxConfig::default()` through `toml`, verifying `scrollback_bytes == 2_097_152`, `lazy_restore_on_startup == true`, `auto_hibernate_minutes == 20`, `mcp_gateway_port == 43111`.
  - `velox_config_recent_dirs_deduplicates_and_caps_at_twelve`: verifies case-insensitive dedup, most-recent-first ordering, and 12-item cap.
- Test `pty::scrollback`:
  - `ring_scrollback_enforces_exact_capacity_across_multiple_pushes`: verifies multiple pushes exceeding capacity keep exact trailing bytes.
- Test `pty::job_object`:
  - `process_job_object_lifecycle_does_not_error`: verifies `ProcessJobObject::new()` succeeds and drops cleanly.

## Verification
1. From `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox`, run `pnpm exec tsc --noEmit`.
2. From `C:\Users\Asus\OneDrive\Escritorio\Proyectos\velox\src-tauri`, run `cargo test --test integration_tests`.
Report concise summary when green.
