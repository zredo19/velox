use crate::domain::agent::AgentPreset;
use crate::domain::error::VeloxResult;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub fn list_agent_presets(state: State<'_, AppState>) -> VeloxResult<Vec<AgentPreset>> {
    Ok(state.sessions.list_presets())
}

#[tauri::command]
pub fn list_recent_dirs(state: State<'_, AppState>) -> VeloxResult<Vec<String>> {
    Ok(state.sessions.list_recent_dirs())
}

/// Open the modern Windows `IFileOpenDialog` folder picker on a dedicated STA thread,
/// owned by the Velox window so it stays modal and in front without freezing the UI.
#[tauri::command]
pub async fn pick_directory(
    window: tauri::WebviewWindow,
    default_path: Option<String>,
) -> VeloxResult<Option<String>> {
    #[cfg(windows)]
    {
        let owner_hwnd = window.hwnd().map(|h| h.0 as isize).unwrap_or(0);
        let (tx, rx) = tokio::sync::oneshot::channel();

        std::thread::Builder::new()
            .name("velox-folder-picker".to_string())
            .spawn(move || {
                let res = run_folder_picker_dialog(owner_hwnd, default_path);
                let _ = tx.send(res);
            })?;

        rx.await.map_err(|e| {
            crate::domain::error::VeloxError::Io(std::io::Error::other(format!(
                "Folder picker thread aborted: {e}"
            )))
        })?
    }

    #[cfg(not(windows))]
    {
        let _ = (window, default_path);
        Ok(None)
    }
}

#[cfg(windows)]
fn resolve_initial_folder(default_path: Option<&str>) -> Option<std::path::PathBuf> {
    let raw = default_path?.trim();
    if raw.is_empty() {
        return None;
    }
    let mut candidate = std::path::Path::new(raw);
    while !candidate.is_dir() {
        candidate = candidate.parent()?;
    }
    Some(candidate.to_path_buf())
}

#[cfg(windows)]
fn run_folder_picker_dialog(
    owner_hwnd: isize,
    default_path: Option<String>,
) -> VeloxResult<Option<String>> {
    use crate::domain::error::VeloxError;
    use windows::core::{w, HSTRING};
    use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE,
    };
    use windows::Win32::UI::Shell::{
        FileOpenDialog, IFileOpenDialog, IShellItem, SHCreateItemFromParsingName,
        FOS_FORCEFILESYSTEM, FOS_NOCHANGEDIR, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS,
        SIGDN_FILESYSPATH,
    };

    struct ComApartmentGuard;
    impl Drop for ComApartmentGuard {
        fn drop(&mut self) {
            // SAFETY: Constructed only after CoInitializeEx succeeded on this dedicated STA thread.
            unsafe { CoUninitialize() };
        }
    }

    struct CoTaskMemStrGuard(windows::core::PWSTR);
    impl Drop for CoTaskMemStrGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: PWSTR was allocated by IShellItem::GetDisplayName via COM task allocator.
                unsafe { CoTaskMemFree(Some(self.0 .0.cast())) };
            }
        }
    }

    // SAFETY: Fresh OS thread with no prior COM apartment; initializes STA for IFileOpenDialog.
    unsafe {
        CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE)
            .ok()
            .map_err(|e| {
                VeloxError::Io(std::io::Error::other(format!(
                    "COM initialization failed: {e}"
                )))
            })?;
    }
    let _com = ComApartmentGuard;

    // SAFETY: COM STA is active; FileOpenDialog CLSID is standard Windows Vista+ shell dialog.
    let dialog: IFileOpenDialog = unsafe { CoCreateInstance(&FileOpenDialog, None, CLSCTX_ALL) }
        .map_err(|e| {
            VeloxError::Io(std::io::Error::other(format!(
                "Failed to open folder picker: {e}"
            )))
        })?;

    unsafe {
        let opts = dialog.GetOptions().map_err(|e| {
            VeloxError::Io(std::io::Error::other(format!("GetOptions failed: {e}")))
        })?;
        dialog
            .SetOptions(
                opts | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | FOS_NOCHANGEDIR,
            )
            .map_err(|e| {
                VeloxError::Io(std::io::Error::other(format!("SetOptions failed: {e}")))
            })?;
        let _ = dialog.SetTitle(w!("Select workspace folder"));

        if let Some(initial) = resolve_initial_folder(default_path.as_deref()) {
            let wide_path = HSTRING::from(initial.as_os_str());
            if let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(&wide_path, None) {
                let _ = dialog.SetFolder(&folder);
            }
        }

        let owner = if owner_hwnd != 0 {
            Some(HWND(owner_hwnd as *mut std::ffi::c_void))
        } else {
            None
        };

        match dialog.Show(owner) {
            Ok(()) => {
                let item = dialog.GetResult().map_err(|e| {
                    VeloxError::Io(std::io::Error::other(format!("GetResult failed: {e}")))
                })?;
                let pwstr = item.GetDisplayName(SIGDN_FILESYSPATH).map_err(|e| {
                    VeloxError::Io(std::io::Error::other(format!("GetDisplayName failed: {e}")))
                })?;
                let _guard = CoTaskMemStrGuard(pwstr);
                let path = pwstr.to_string().map_err(|e| {
                    VeloxError::Io(std::io::Error::other(format!("Invalid UTF-16 path: {e}")))
                })?;
                Ok(Some(path))
            }
            Err(err) if err.code() == ERROR_CANCELLED.to_hresult() => Ok(None),
            Err(err) => Err(VeloxError::Io(std::io::Error::other(format!(
                "Folder picker failed: {err}"
            )))),
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::{resolve_initial_folder, run_folder_picker_dialog};

    #[test]
    fn resolve_initial_folder_falls_back_to_existing_ancestor() {
        let temp = std::env::temp_dir();
        let non_existent = temp.join("velox-nonexistent-dir-12345").join("nested");
        let resolved = resolve_initial_folder(non_existent.to_str());
        assert_eq!(resolved.as_deref(), Some(temp.as_path()));
    }

    #[test]
    fn resolve_initial_folder_ignores_empty_input() {
        assert_eq!(resolve_initial_folder(None), None);
        assert_eq!(resolve_initial_folder(Some("   ")), None);
    }

    #[test]
    #[ignore = "opens real Windows IFileOpenDialog briefly and closes it via WM_CLOSE"]
    fn folder_picker_smoke_test_closes_cleanly_with_ok_none() {
        use std::time::{Duration, Instant};
        use windows::core::BOOL;
        use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetWindowThreadProcessId, IsWindowVisible, PostMessageW,
            WM_CLOSE,
        };

        struct TargetSearch {
            self_pid: u32,
            found: Vec<HWND>,
        }

        unsafe extern "system" fn enum_own_dialogs(hwnd: HWND, lparam: LPARAM) -> BOOL {
            let search = unsafe { &mut *(lparam.0 as *mut TargetSearch) };
            let mut pid: u32 = 0;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid == search.self_pid && unsafe { IsWindowVisible(hwnd).as_bool() } {
                let mut class_buf = [0u16; 64];
                let len = unsafe { GetClassNameW(hwnd, &mut class_buf) };
                let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
                if class_name == "#32770" {
                    search.found.push(hwnd);
                }
            }
            BOOL(1)
        }

        let (tx, rx) = std::sync::mpsc::channel();
        let initial = std::env::temp_dir().to_string_lossy().into_owned();
        let handle = std::thread::Builder::new()
            .name("folder-picker-smoke".to_string())
            .spawn(move || {
                let res = run_folder_picker_dialog(0, Some(initial));
                let _ = tx.send(res);
            })
            .expect("spawn picker thread");

        // Let the dialog appear (~2s), then close ONLY our own test process's #32770 dialog.
        std::thread::sleep(Duration::from_secs(2));
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut closed_any = false;
        while Instant::now() < deadline {
            let mut search = TargetSearch {
                self_pid: std::process::id(),
                found: Vec::new(),
            };
            unsafe {
                let _ = EnumWindows(
                    Some(enum_own_dialogs),
                    LPARAM(&mut search as *mut TargetSearch as isize),
                );
            }
            if !search.found.is_empty() {
                for hwnd in search.found {
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                    }
                }
                closed_any = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(
            closed_any,
            "never found visible #32770 dialog owned by test process"
        );

        let res = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("picker did not exit after WM_CLOSE");
        handle.join().expect("join picker thread");
        assert!(matches!(res, Ok(None)), "expected Ok(None), got {res:?}");
    }
}
