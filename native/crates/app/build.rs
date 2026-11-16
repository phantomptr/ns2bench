fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&["update_action", "cli_action", "component_action", "agent_action", "open_logs", "tray_configure"])
    )).expect("Tauri build");
}
