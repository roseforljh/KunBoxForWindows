use tauri::{AppHandle, Emitter, Manager, State};
use std::fs;
use std::process::Stdio;
use std::sync::Arc;
use tokio::process::Command;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;
use crate::state::AppState;
use crate::types::{CommandResult, ProxyState, TrafficStats};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[tauri::command]
pub async fn singbox_start(app: AppHandle, state: State<'_, AppState>) -> Result<CommandResult, String> {
    let singbox_path = get_singbox_path(&app)?;
    
    if !singbox_path.exists() {
        return Ok(CommandResult::err("sing-box.exe not found. Please install kernel first."));
    }

    // Generate config
    let config_result = generate_config(&state).await?;
    if !config_result.success {
        return Ok(config_result);
    }

    let config_path = state.config_dir.join("config.json");
    
    // Update state
    *state.proxy_state.lock().await = ProxyState::Connecting;
    let _ = app.emit("singbox:state", "connecting");

    // Start sing-box process
    #[cfg(windows)]
    let mut child = Command::new(&singbox_path)
        .args(["run", "-c", config_path.to_str().unwrap()])
        .current_dir(&state.config_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;

    #[cfg(not(windows))]
    let mut child = Command::new(&singbox_path)
        .args(["run", "-c", config_path.to_str().unwrap()])
        .current_dir(&state.config_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;

    // Capture stderr for logging
    if let Some(stderr) = child.stderr.take() {
        let app_clone = app.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let _ = app_clone.emit("singbox:log", serde_json::json!({
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                    "level": "info",
                    "tag": "sing-box",
                    "message": line
                }));
            }
        });
    }

    *state.singbox_process.lock().await = Some(child);
    *state.proxy_state.lock().await = ProxyState::Connected;
    let start_time_val = chrono::Utc::now().timestamp_millis() as u64;
    *state.start_time.lock().await = Some(start_time_val);
    
    let _ = app.emit("singbox:state", "connected");

    // Start traffic polling
    let cancel_token = CancellationToken::new();
    *state.traffic_cancel.lock().await = Some(cancel_token.clone());
    
    let app_for_traffic = app.clone();
    let traffic_stats = state.traffic_stats.clone();
    tokio::spawn(async move {
        start_traffic_polling(app_for_traffic, traffic_stats, start_time_val, cancel_token).await;
    });

    // Enable system proxy
    let settings = state.settings.lock().await;
    if settings.system_proxy {
        let _ = enable_system_proxy_internal(settings.local_port).await;
    }

    Ok(CommandResult::ok())
}

#[tauri::command]
pub async fn singbox_stop(app: AppHandle, state: State<'_, AppState>) -> Result<CommandResult, String> {
    // Cancel traffic polling
    if let Some(cancel) = state.traffic_cancel.lock().await.take() {
        cancel.cancel();
    }
    
    *state.proxy_state.lock().await = ProxyState::Disconnecting;
    let _ = app.emit("singbox:state", "disconnecting");

    // Kill process
    if let Some(mut child) = state.singbox_process.lock().await.take() {
        let _ = child.kill().await;
    }

    // Disable system proxy
    let _ = disable_system_proxy_internal().await;

    *state.proxy_state.lock().await = ProxyState::Idle;
    *state.start_time.lock().await = None;
    let _ = app.emit("singbox:state", "idle");

    Ok(CommandResult::ok())
}

#[tauri::command]
pub async fn singbox_restart(app: AppHandle, state: State<'_, AppState>) -> Result<CommandResult, String> {
    singbox_stop(app.clone(), state.clone()).await?;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    singbox_start(app, state).await
}

#[tauri::command]
pub async fn singbox_get_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let proxy_state = state.proxy_state.lock().await.clone();
    let start_time = state.start_time.lock().await.clone();
    
    Ok(serde_json::json!({
        "state": proxy_state,
        "startTime": start_time
    }))
}

#[tauri::command]
pub async fn singbox_switch_node(state: State<'_, AppState>, node_tag: String) -> Result<CommandResult, String> {
    let proxy_state = state.proxy_state.lock().await.clone();
    if !matches!(proxy_state, ProxyState::Connected) {
        return Ok(CommandResult::err("VPN not running"));
    }

    let client = reqwest::Client::new();
    let res = client
        .put("http://127.0.0.1:9090/proxies/PROXY")
        .json(&serde_json::json!({ "name": node_tag }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if res.status().is_success() {
        Ok(CommandResult::ok())
    } else {
        Ok(CommandResult::err(format!("API returned {}", res.status())))
    }
}

#[tauri::command]
pub async fn singbox_enable_system_proxy(port: Option<u16>) -> Result<CommandResult, String> {
    let port = port.unwrap_or(7890);
    enable_system_proxy_internal(port).await?;
    Ok(CommandResult::ok())
}

#[tauri::command]
pub async fn singbox_disable_system_proxy() -> Result<CommandResult, String> {
    disable_system_proxy_internal().await?;
    Ok(CommandResult::ok())
}

/// 判断节点类型是否是代理类型
fn is_proxy_type(node_type: &str) -> bool {
    matches!(node_type, 
        "shadowsocks" | "vmess" | "vless" | "trojan" | 
        "hysteria" | "hysteria2" | "tuic" | "anytls" |
        "http" | "socks" | "wireguard" | "ssh" | "shadowtls"
    )
}

/// 处理节点配置，确保格式正确
fn process_node(node: &serde_json::Value) -> serde_json::Value {
    let mut node = node.clone();
    if let Some(obj) = node.as_object_mut() {
        let node_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let server = obj.get("server").and_then(|s| s.as_str()).unwrap_or("").to_string();
        let port = obj.get("server_port").and_then(|p| p.as_u64()).unwrap_or(0);
        
        // vless/vmess/trojan 不需要 method 字段
        if node_type != "shadowsocks" && node_type != "shadowsocksr" {
            obj.remove("method");
        }
        
        // 为需要 TLS 的节点添加配置（如果没有的话）
        if !obj.contains_key("tls") {
            match node_type.as_str() {
                "hysteria2" | "hysteria" | "tuic" => {
                    obj.insert("tls".to_string(), serde_json::json!({
                        "enabled": true,
                        "server_name": server,
                        "insecure": true
                    }));
                }
                "vless" | "vmess" | "trojan" => {
                    if port == 443 || port == 8443 || port == 2053 {
                        obj.insert("tls".to_string(), serde_json::json!({
                            "enabled": true,
                            "server_name": server,
                            "insecure": true
                        }));
                    }
                }
                _ => {}
            }
        }
        
        // vless 需要 packet_encoding
        if node_type == "vless" && !obj.contains_key("packet_encoding") {
            obj.insert("packet_encoding".to_string(), serde_json::Value::String("xudp".to_string()));
        }
    }
    node
}

/// 配置文件信息（用于跨配置分流）
struct ProfileInfo {
    id: String,
    name: String,
    nodes: Vec<serde_json::Value>,
}

/// 加载所有配置文件的节点信息
fn load_all_profiles(state: &AppState, profiles_data: &crate::types::ProfilesData) -> Vec<ProfileInfo> {
    let configs_dir = state.configs_dir();
    let mut result = Vec::new();
    
    for profile in &profiles_data.profiles {
        let nodes_file = configs_dir.join(format!("{}.json", profile.id));
        if nodes_file.exists() {
            if let Ok(content) = fs::read_to_string(&nodes_file) {
                if let Ok(nodes) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
                    result.push(ProfileInfo {
                        id: profile.id.clone(),
                        name: profile.name.clone(),
                        nodes,
                    });
                }
            }
        }
    }
    
    result
}

async fn generate_config(state: &AppState) -> Result<CommandResult, String> {
    let profiles_data = state.profiles_data.lock().await;
    let settings = state.settings.lock().await;
    let rulesets = state.rulesets.lock().await;

    let active_profile_id = match &profiles_data.active_profile_id {
        Some(id) => id.clone(),
        None => return Ok(CommandResult::err("No active profile")),
    };

    let nodes_file = state.configs_dir().join(format!("{}.json", active_profile_id));
    if !nodes_file.exists() {
        return Ok(CommandResult::err("No nodes in active profile"));
    }

    let nodes_content = fs::read_to_string(&nodes_file).map_err(|e| e.to_string())?;
    let raw_nodes: Vec<serde_json::Value> = serde_json::from_str(&nodes_content).map_err(|e| e.to_string())?;

    if raw_nodes.is_empty() {
        return Ok(CommandResult::err("No nodes in active profile"));
    }

    // 处理当前配置的节点
    let nodes: Vec<serde_json::Value> = raw_nodes.iter().map(process_node).collect();

    let active_node_tag = profiles_data.active_node_tag.clone()
        .or_else(|| nodes.first().and_then(|n| n.get("tag").and_then(|t| t.as_str()).map(|s| s.to_string())));

    // 加载所有配置文件信息（用于跨配置分流）
    let all_profiles = load_all_profiles(state, &profiles_data);

    // 收集规则集引用的 profile ID 和 node tag
    let enabled_rulesets: Vec<_> = rulesets.iter().filter(|r| r.enabled).collect();
    let mut referenced_profile_ids = std::collections::HashSet::new();
    let mut referenced_node_tags = std::collections::HashSet::new();
    
    for rs in &enabled_rulesets {
        if let Some(ref value) = rs.outbound_value {
            match rs.outbound_mode.as_str() {
                "profile" | "配置" => { referenced_profile_ids.insert(value.clone()); }
                "node" | "节点" => { referenced_node_tags.insert(value.clone()); }
                _ => {}
            }
        }
    }

    // Build config - 使用 sing-box 1.11+ 新格式
    let listen_addr = if settings.allow_lan { "0.0.0.0" } else { "127.0.0.1" };
    
    let mut config = serde_json::json!({
        "log": {
            "disabled": false,
            "level": "info",
            "timestamp": true
        },
        "experimental": {
            "clash_api": {
                "external_controller": "127.0.0.1:9090",
                "default_mode": "rule"
            },
            "cache_file": {
                "enabled": true,
                "path": "cache.db"
            }
        },
        "dns": {
            "servers": [
                {
                    "tag": "dns-local",
                    "address": settings.local_dns,
                    "detour": "direct"
                },
                {
                    "tag": "dns-remote",
                    "address": settings.remote_dns,
                    "detour": "PROXY"
                }
            ],
            "rules": [
                {
                    "outbound": "any",
                    "server": "dns-local"
                }
            ],
            "final": "dns-remote",
            "independent_cache": true
        },
        "inbounds": [
            {
                "type": "mixed",
                "tag": "mixed-in",
                "listen": listen_addr,
                "listen_port": settings.local_port,
                "sniff": true,
                "sniff_override_destination": true
            },
            {
                "type": "socks",
                "tag": "socks-in",
                "listen": listen_addr,
                "listen_port": settings.socks_port
            }
        ],
        "route": {
            "auto_detect_interface": true,
            "final": if settings.default_rule == "proxy" { "PROXY" } else { &settings.default_rule }
        }
    });

    // ========== 构建 outbounds ==========
    let mut outbounds: Vec<serde_json::Value> = Vec::new();
    let mut proxy_tags: Vec<String> = Vec::new();
    let mut existing_tags = std::collections::HashSet::new();

    // 1. 添加当前配置的节点
    for node in &nodes {
        let node_type = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if is_proxy_type(node_type) {
            outbounds.push(node.clone());
            if let Some(tag) = node.get("tag").and_then(|t| t.as_str()) {
                proxy_tags.push(tag.to_string());
                existing_tags.insert(tag.to_string());
            }
        }
    }

    // 2. 处理跨配置节点引用（node 模式）
    for node_tag in &referenced_node_tags {
        if existing_tags.contains(node_tag) {
            continue;
        }
        // 在其他配置中查找该节点
        for profile in &all_profiles {
            if let Some(node) = profile.nodes.iter().find(|n| {
                n.get("tag").and_then(|t| t.as_str()) == Some(node_tag.as_str())
            }) {
                let node_type = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if is_proxy_type(node_type) {
                    outbounds.push(process_node(node));
                    proxy_tags.push(node_tag.clone());
                    existing_tags.insert(node_tag.clone());
                    log::info!("Added cross-profile node: {} from profile {}", node_tag, profile.name);
                    break;
                }
            }
        }
    }

    // 3. 处理配置分流（profile 模式）- 创建 urltest selector
    let mut profile_id_to_selector = std::collections::HashMap::new();
    
    for profile_id in &referenced_profile_ids {
        if let Some(profile) = all_profiles.iter().find(|p| &p.id == profile_id) {
            let selector_tag = format!("P:{}", profile.name);
            if existing_tags.contains(&selector_tag) {
                continue;
            }

            // 收集该配置的所有代理节点
            let mut profile_proxy_tags: Vec<String> = Vec::new();
            for node in &profile.nodes {
                let node_type = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if is_proxy_type(node_type) {
                    if let Some(tag) = node.get("tag").and_then(|t| t.as_str()) {
                        // 如果节点不存在，添加到 outbounds
                        if !existing_tags.contains(tag) {
                            outbounds.push(process_node(node));
                            existing_tags.insert(tag.to_string());
                        }
                        profile_proxy_tags.push(tag.to_string());
                    }
                }
            }

            // 创建 urltest 类型的 selector（自动选择最低延迟节点）
            if !profile_proxy_tags.is_empty() {
                outbounds.push(serde_json::json!({
                    "type": "urltest",
                    "tag": selector_tag,
                    "outbounds": profile_proxy_tags,
                    "url": settings.latency_test_url,
                    "interval": "30m",
                    "tolerance": 50,
                    "interrupt_exist_connections": true
                }));
                existing_tags.insert(selector_tag.clone());
                profile_id_to_selector.insert(profile_id.clone(), selector_tag.clone());
                log::info!("Created profile selector: {} with {} nodes", selector_tag, profile_proxy_tags.len());
            }
        }
    }

    // 4. 添加 PROXY selector（主选择器）
    let default_tag = active_node_tag.clone();
    if !proxy_tags.is_empty() {
        outbounds.insert(0, serde_json::json!({
            "type": "selector",
            "tag": "PROXY",
            "outbounds": proxy_tags,
            "default": default_tag,
            "interrupt_exist_connections": false
        }));
    }

    // 5. 添加 auto urltest（如果有多个节点）
    if proxy_tags.len() > 1 {
        outbounds.push(serde_json::json!({
            "type": "urltest",
            "tag": "auto",
            "outbounds": proxy_tags,
            "url": settings.latency_test_url,
            "interval": "300s",
            "tolerance": 50
        }));
    }

    // 6. 添加基础出站
    outbounds.push(serde_json::json!({ "type": "direct", "tag": "direct" }));
    outbounds.push(serde_json::json!({ "type": "block", "tag": "block" }));

    config["outbounds"] = serde_json::Value::Array(outbounds.clone());

    // 收集所有可用的 outbound tags
    let available_outbound_tags: std::collections::HashSet<String> = outbounds.iter()
        .filter_map(|o| o.get("tag").and_then(|t| t.as_str()).map(|s| s.to_string()))
        .collect();

    // ========== 构建路由规则 ==========
    let mut rules: Vec<serde_json::Value> = vec![
        serde_json::json!({ "protocol": "dns", "action": "hijack-dns" }),
    ];

    if settings.bypass_lan {
        rules.push(serde_json::json!({ "ip_is_private": true, "outbound": "direct" }));
    }

    // 添加规则集路由规则
    let mut rule_set_refs = Vec::new();
    let rulesets_cache_dir = state.rulesets_cache_dir();

    for rs in &enabled_rulesets {
        // 检查本地缓存文件是否存在
        let local_path = rulesets_cache_dir.join(format!("{}.srs", rs.tag));
        
        if !local_path.exists() {
            log::warn!("Ruleset cache not found, skipping: {}", rs.tag);
            continue;
        }

        // 添加规则集引用
        rule_set_refs.push(serde_json::json!({
            "tag": rs.tag,
            "type": "local",
            "format": rs.format,
            "path": local_path.to_string_lossy()
        }));

        // 映射 outbound_mode 到正确的出站名称
        let outbound = match rs.outbound_mode.as_str() {
            "proxy" => "PROXY".to_string(),
            "direct" => "direct".to_string(),
            "block" => "block".to_string(),
            // node 模式：验证节点是否存在
            "node" | "节点" => {
                if let Some(ref node_tag) = rs.outbound_value {
                    if available_outbound_tags.contains(node_tag) {
                        node_tag.clone()
                    } else {
                        log::warn!("Node '{}' not found for ruleset '{}', falling back to PROXY", node_tag, rs.tag);
                        "PROXY".to_string()
                    }
                } else {
                    "PROXY".to_string()
                }
            },
            // profile 模式：使用配置的 urltest selector
            "profile" | "配置" => {
                if let Some(ref profile_id) = rs.outbound_value {
                    if let Some(selector_tag) = profile_id_to_selector.get(profile_id) {
                        if available_outbound_tags.contains(selector_tag) {
                            selector_tag.clone()
                        } else {
                            log::warn!("Profile selector '{}' not found for ruleset '{}', falling back to PROXY", selector_tag, rs.tag);
                            "PROXY".to_string()
                        }
                    } else {
                        log::warn!("Profile '{}' not found for ruleset '{}', falling back to PROXY", profile_id, rs.tag);
                        "PROXY".to_string()
                    }
                } else {
                    "PROXY".to_string()
                }
            },
            other => other.to_string()
        };

        rules.push(serde_json::json!({
            "rule_set": [rs.tag],
            "outbound": outbound
        }));
    }

    if !rule_set_refs.is_empty() {
        config["route"]["rule_set"] = serde_json::Value::Array(rule_set_refs);
    }

    config["route"]["rules"] = serde_json::Value::Array(rules);

    // Write config
    fs::create_dir_all(&state.config_dir).map_err(|e| e.to_string())?;
    let config_path = state.config_dir.join("config.json");
    let config_str = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    fs::write(&config_path, config_str).map_err(|e| e.to_string())?;

    Ok(CommandResult::ok())
}

fn get_singbox_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let resource_path = app.path().resource_dir().map_err(|e| e.to_string())?;
    Ok(resource_path.join("resources/libs/sing-box.exe"))
}

async fn enable_system_proxy_internal(port: u16) -> Result<(), String> {
    let proxy = format!("127.0.0.1:{}", port);
    
    #[cfg(windows)]
    {
        Command::new("reg")
            .args(["add", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings", "/v", "ProxyEnable", "/t", "REG_DWORD", "/d", "1", "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .await
            .map_err(|e| e.to_string())?;

        Command::new("reg")
            .args(["add", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings", "/v", "ProxyServer", "/t", "REG_SZ", "/d", &proxy, "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .await
            .map_err(|e| e.to_string())?;
    }

    #[cfg(not(windows))]
    {
        let _ = proxy;
    }

    Ok(())
}

async fn disable_system_proxy_internal() -> Result<(), String> {
    #[cfg(windows)]
    Command::new("reg")
        .args(["add", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings", "/v", "ProxyEnable", "/t", "REG_DWORD", "/d", "0", "/f"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    Ok(())
}

async fn start_traffic_polling(
    app: AppHandle,
    traffic_stats: Arc<tokio::sync::Mutex<TrafficStats>>,
    start_time: u64,
    cancel: CancellationToken,
) {
    let client = reqwest::Client::new();
    let mut last_upload: u64 = 0;
    let mut last_download: u64 = 0;
    
    // Wait a bit for sing-box to be ready
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                log::info!("Traffic polling cancelled");
                break;
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                // Fetch connections from Clash API to get total traffic
                match client.get("http://127.0.0.1:9090/connections")
                    .timeout(std::time::Duration::from_secs(2))
                    .send()
                    .await 
                {
                    Ok(resp) => {
                        if let Ok(data) = resp.json::<serde_json::Value>().await {
                            let upload_total = data.get("uploadTotal").and_then(|v| v.as_u64()).unwrap_or(0);
                            let download_total = data.get("downloadTotal").and_then(|v| v.as_u64()).unwrap_or(0);
                            
                            // Calculate speed from difference
                            let upload_speed = if upload_total > last_upload { upload_total - last_upload } else { 0 };
                            let download_speed = if download_total > last_download { download_total - last_download } else { 0 };
                            
                            last_upload = upload_total;
                            last_download = download_total;
                            
                            let duration = chrono::Utc::now().timestamp_millis() as u64 - start_time;
                            
                            let stats = TrafficStats {
                                upload_speed,
                                download_speed,
                                upload_total,
                                download_total,
                                duration,
                            };
                            
                            *traffic_stats.lock().await = stats.clone();
                            let _ = app.emit("singbox:traffic", &stats);
                        }
                    }
                    Err(e) => {
                        log::warn!("Traffic polling error: {}", e);
                    }
                }
            }
        }
    }
}
