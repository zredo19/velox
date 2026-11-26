fn main() {
    tauri_build::build();

    // Tauri links its ComCtl32 v6 manifest archive (libresource.a) only to bin targets,
    // so Windows GNU unit/integration test harnesses fail at load time (0xc0000139)
    // when comctl32!TaskDialogIndirect is resolved. Link the archive for test builds too.
    #[cfg(target_os = "windows")]
    if let Ok(out_dir) = std::env::var("OUT_DIR") {
        let res = std::path::Path::new(&out_dir).join("libresource.a");
        if res.is_file() {
            println!("cargo:rustc-link-arg={}", res.display());
        }
    }
}
