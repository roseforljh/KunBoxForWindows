use tauri::State;
use std::fs;
use crate::state::AppState;
use crate::types::AppSettings;

#[cfg(windows)]
use std::process::Command;

/// Set or remove Windows startup registry entry
#[cfg(windows)]
fn set_windows_startup(enable: bool) -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_path_str = exe_path.to_string_lossy();
    
    if enable {
        // Add to registry Run key
        Command::new("reg")
            .args([
                "add",
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                "/v", "KunBox",
                "/t", "REG_SZ",
                "/d", &exe_path_str,
                "/f"
            ])
            .output()
            .map_err(|e| e.to_string())?;
    } else {
        // Remove from registry Run key
        Command::new("reg")
            .args([
                "delete",
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                "/v", "KunBox",
                "/f"
            ])
            .output()
            .ok(); // Ignore error if key doesn't exist
    }
    Ok(())
}

#[cfg(not(windows))]
fn set_windows_startup(_enable: bool) -> Result<(), String> {
    Ok(())
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    let file = state.settings_file();
    if file.exists() {
        let content = fs::read_to_string(&file).map_err(|e| e.to_string())?;
        let settings: AppSettings = serde_json::from_str(&content).unwrap_or_default();
        *state.settings.lock().await = settings.clone();
        Ok(settings)
    } else {
        let settings = state.settings.lock().await.clone();
        Ok(settings)
    }
}

#[tauri::command]
pub async fn set_settings(state: State<'_, AppState>, settings: serde_json::Value) -> Result<(), String> {
    // Get current settings
    let mut current = state.settings.lock().await.clone();
    let old_start_with_windows = current.start_with_windows;
    
    // Merge with incoming partial settings
    if let Some(obj) = settings.as_object() {
        if let Some(v) = obj.get("localPort").and_then(|v| v.as_u64()) { current.local_port = v as u16; }
        if let Some(v) = obj.get("socksPort").and_then(|v| v.as_u64()) { current.socks_port = v as u16; }
        if let Some(v) = obj.get("allowLan").and_then(|v| v.as_bool()) { current.allow_lan = v; }
        if let Some(v) = obj.get("systemProxy").and_then(|v| v.as_bool()) { current.system_proxy = v; }
        if let Some(v) = obj.get("tunEnabled").and_then(|v| v.as_bool()) { current.tun_enabled = v; }
        if let Some(v) = obj.get("tunStack").and_then(|v| v.as_str()) { current.tun_stack = v.to_string(); }
        if let Some(v) = obj.get("localDns").and_then(|v| v.as_str()) { current.local_dns = v.to_string(); }
        if let Some(v) = obj.get("remoteDns").and_then(|v| v.as_str()) { current.remote_dns = v.to_string(); }
        if let Some(v) = obj.get("fakeDns").and_then(|v| v.as_bool()) { current.fake_dns = v; }
        if let Some(v) = obj.get("blockAds").and_then(|v| v.as_bool()) { current.block_ads = v; }
        if let Some(v) = obj.get("bypassLan").and_then(|v| v.as_bool()) { current.bypass_lan = v; }
        if let Some(v) = obj.get("routingMode").and_then(|v| v.as_str()) { current.routing_mode = v.to_string(); }
        if let Some(v) = obj.get("defaultRule").and_then(|v| v.as_str()) { current.default_rule = v.to_string(); }
        if let Some(v) = obj.get("latencyTestUrl").and_then(|v| v.as_str()) { current.latency_test_url = v.to_string(); }
        if let Some(v) = obj.get("latencyTestTimeout").and_then(|v| v.as_u64()) { current.latency_test_timeout = v as u32; }
        if let Some(v) = obj.get("autoConnect").and_then(|v| v.as_bool()) { current.auto_connect = v; }
        if let Some(v) = obj.get("minimizeToTray").and_then(|v| v.as_bool()) { current.minimize_to_tray = v; }
        if let Some(v) = obj.get("startWithWindows").and_then(|v| v.as_bool()) { current.start_with_windows = v; }
        if let Some(v) = obj.get("startMinimized").and_then(|v| v.as_bool()) { current.start_minimized = v; }
        if let Some(v) = obj.get("silentStart").and_then(|v| v.as_bool()) { current.silent_start = v; }
        if let Some(v) = obj.get("exitOnClose").and_then(|v| v.as_bool()) { current.exit_on_close = v; }
        if let Some(v) = obj.get("theme").and_then(|v| v.as_str()) { current.theme = v.to_string(); }
        if let Some(v) = obj.get("requireAdmin").and_then(|v| v.as_bool()) { current.require_admin = v; }
        if let Some(v) = obj.get("enableRuntimeLogs").and_then(|v| v.as_bool()) { current.enable_runtime_logs = v; }
    }
    
    // Handle Windows startup setting change
    if current.start_with_windows != old_start_with_windows {
        if let Err(e) = set_windows_startup(current.start_with_windows) {
            log::error!("Failed to set Windows startup: {}", e);
        }
    }
    
    fs::create_dir_all(&state.data_dir).map_err(|e| e.to_string())?;
    let content = serde_json::to_string_pretty(&current).map_err(|e| e.to_string())?;
    fs::write(state.settings_file(), content).map_err(|e| e.to_string())?;
    *state.settings.lock().await = current;
    Ok(())
}
