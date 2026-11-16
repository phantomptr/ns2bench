fn main() {
    // Windows gives the main thread 1 MB (macOS and Linux 8 MB). The window, its commands and
    // startup run there: give it the same 8 MB so a deep call can't overflow on Windows only.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&["update_action", "cli_action", "component_action", "agent_action", "open_logs", "tray_configure"])
    )).expect("Tauri build");
}
