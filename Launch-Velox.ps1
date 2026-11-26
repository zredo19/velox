$exe = Join-Path $PSScriptRoot "src-tauri\target\release\velox.exe"
if (-not (Test-Path $exe)) {
    $exe = Join-Path $PSScriptRoot "src-tauri\target\debug\velox.exe"
}
if (-not (Test-Path $exe)) {
    Write-Error "Velox binary not found. Run 'pnpm build' and 'cd src-tauri; cargo build --release' first."
    exit 1
}
Start-Process -FilePath $exe -WorkingDirectory (Split-Path $exe)
