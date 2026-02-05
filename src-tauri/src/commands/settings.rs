use tauri::State;
use std::fs;
use crate::state::AppState;
use crate::types::AppSettings;

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
        if let Some(v) = obj.get("exitOnClose").and_then(|v| v.as_bool()) { current.exit_on_close = v; }
        if let Some(v) = obj.get("theme").and_then(|v| v.as_str()) { current.theme = v.to_string(); }
    }
    
    fs::create_dir_all(&state.data_dir).map_err(|e| e.to_string())?;
    let content = serde_json::to_string_pretty(&current).map_err(|e| e.to_string())?;
    fs::write(state.settings_file(), content).map_err(|e| e.to_string())?;
    *state.settings.lock().await = current;
    Ok(())
}
