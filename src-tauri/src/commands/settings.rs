use tauri::State;
use std::fs;
use crate::state::AppState;
use crate::types::AppSettings;

#[cfg(windows)]
use std::process::Command;

#[cfg(windows)]
fn ensure_u16_in_range(value: u64, field: &str) -> Result<u16, String> {
    if value == 0 || value > u16::MAX as u64 {
        return Err(format!("{} 超出有效范围", field));
    }
    Ok(value as u16)
}

#[cfg(windows)]
fn ensure_u32_in_range(value: u64, min: u32, max: u32, field: &str) -> Result<u32, String> {
    if value < min as u64 || value > max as u64 {
        return Err(format!("{} 超出有效范围", field));
    }
    Ok(value as u32)
}

/// Set or remove Windows startup registry entry
#[cfg(windows)]
fn set_windows_startup(enable: bool) -> Result<(), String> {
    let exe_path = std::env::current_exe().map_err(|e| e.to_string())?;
    let exe_path_str = exe_path.to_string_lossy();
    
    if enable {
        let output = Command::new("reg")
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
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
    } else {
        let output = Command::new("reg")
            .args([
                "delete",
                "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Run",
                "/v", "KunBox",
                "/f"
            ])
            .output()
            .map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
        }
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
        let settings: AppSettings = serde_json::from_str(&content)
            .map_err(|e| format!("settings.json 格式错误: {}", e))?;
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
        if let Some(v) = obj.get("localPort").and_then(|v| v.as_u64()) { current.local_port = ensure_u16_in_range(v, "localPort")?; }
        if let Some(v) = obj.get("socksPort").and_then(|v| v.as_u64()) { current.socks_port = ensure_u16_in_range(v, "socksPort")?; }
        if let Some(v) = obj.get("allowLan").and_then(|v| v.as_bool()) { current.allow_lan = v; }
        if let Some(v) = obj.get("systemProxy").and_then(|v| v.as_bool()) { current.system_proxy = v; }
        if let Some(v) = obj.get("tunEnabled").and_then(|v| v.as_bool()) { current.tun_enabled = v; }
        if let Some(v) = obj.get("tunStack").and_then(|v| v.as_str()) { current.tun_stack = v.to_string(); }
        if let Some(v) = obj.get("localDns").and_then(|v| v.as_str()) { current.local_dns = v.to_string(); }
        if let Some(v) = obj.get("remoteDns").and_then(|v| v.as_str()) { current.remote_dns = v.to_string(); }
        if let Some(v) = obj.get("fakeDns").and_then(|v| v.as_bool()) { current.fake_dns = v; }
        if let Some(v) = obj.get("bypassLan").and_then(|v| v.as_bool()) { current.bypass_lan = v; }
        if let Some(v) = obj.get("routingMode").and_then(|v| v.as_str()) { current.routing_mode = v.to_string(); }
        if let Some(v) = obj.get("defaultRule").and_then(|v| v.as_str()) { current.default_rule = v.to_string(); }
        if let Some(v) = obj.get("latencyTestUrl").and_then(|v| v.as_str()) { current.latency_test_url = v.to_string(); }
        if let Some(v) = obj.get("latencyTestTimeout").and_then(|v| v.as_u64()) {
            current.latency_test_timeout = ensure_u32_in_range(v, 1000, 30000, "latencyTestTimeout")?;
        }
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
        set_windows_startup(current.start_with_windows)?;
    }
    
    fs::create_dir_all(&state.data_dir).map_err(|e| e.to_string())?;
    let content = serde_json::to_string_pretty(&current).map_err(|e| e.to_string())?;
    fs::write(state.settings_file(), content).map_err(|e| e.to_string())?;
    *state.settings.lock().await = current;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_u16_range() {
        assert_eq!(ensure_u16_in_range(7890, "port").unwrap(), 7890);
        assert!(ensure_u16_in_range(0, "port").is_err());
        assert!(ensure_u16_in_range(70000, "port").is_err());
    }

    #[test]
    fn validates_u32_range() {
        assert_eq!(ensure_u32_in_range(5000, 1000, 30000, "timeout").unwrap(), 5000);
        assert!(ensure_u32_in_range(999, 1000, 30000, "timeout").is_err());
        assert!(ensure_u32_in_range(30001, 1000, 30000, "timeout").is_err());
    }
}
