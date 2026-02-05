use tauri::{AppHandle, Manager, State};
use std::fs;
use std::sync::Arc;
use tokio::sync::Mutex;
use uuid::Uuid;
use crate::state::AppState;
use crate::types::{Profile, ProfilesData, ProxyState, SingBoxOutbound};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

// Temporary sing-box for latency testing
static TEMP_SINGBOX_PROCESS: once_cell::sync::Lazy<Arc<Mutex<Option<tokio::process::Child>>>> = 
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));
const TEMP_SINGBOX_PORT: u16 = 19090;

fn load_profiles_data(state: &AppState) -> ProfilesData {
    let file = state.profiles_file();
    if file.exists() {
        if let Ok(content) = fs::read_to_string(&file) {
            if let Ok(data) = serde_json::from_str(&content) {
                return data;
            }
        }
    }
    ProfilesData::default()
}

fn save_profiles_data(state: &AppState, data: &ProfilesData) -> Result<(), String> {
    fs::create_dir_all(&state.data_dir).map_err(|e| e.to_string())?;
    let content = serde_json::to_string_pretty(data).map_err(|e| e.to_string())?;
    fs::write(state.profiles_file(), content).map_err(|e| e.to_string())?;
    Ok(())
}

fn load_profile_nodes(state: &AppState, profile_id: &str) -> Vec<SingBoxOutbound> {
    let file = state.configs_dir().join(format!("{}.json", profile_id));
    if file.exists() {
        if let Ok(content) = fs::read_to_string(&file) {
            if let Ok(nodes) = serde_json::from_str(&content) {
                return nodes;
            }
        }
    }
    Vec::new()
}

fn load_profile_nodes_raw(state: &AppState, profile_id: &str) -> Vec<serde_json::Value> {
    let file = state.configs_dir().join(format!("{}.json", profile_id));
    if file.exists() {
        if let Ok(content) = fs::read_to_string(&file) {
            if let Ok(nodes) = serde_json::from_str(&content) {
                return nodes;
            }
        }
    }
    Vec::new()
}

fn save_profile_nodes(state: &AppState, profile_id: &str, nodes: &[SingBoxOutbound]) -> Result<(), String> {
    fs::create_dir_all(state.configs_dir()).map_err(|e| e.to_string())?;
    let file = state.configs_dir().join(format!("{}.json", profile_id));
    let content = serde_json::to_string_pretty(nodes).map_err(|e| e.to_string())?;
    fs::write(file, content).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn profile_list(state: State<'_, AppState>) -> Result<Vec<Profile>, String> {
    let data = load_profiles_data(&state);
    *state.profiles_data.lock().await = data.clone();
    Ok(data.profiles)
}

#[tauri::command]
pub async fn profile_add(
    state: State<'_, AppState>,
    url: String,
    name: Option<String>,
    auto_update_interval: Option<u32>,
    dns_pre_resolve: Option<bool>,
    dns_server: Option<String>,
) -> Result<Profile, String> {
    let nodes = fetch_subscription(&url).await?;
    
    let profile = Profile {
        id: Uuid::new_v4().to_string(),
        name: name.unwrap_or_else(|| extract_hostname(&url)),
        url,
        last_update: Some(chrono::Utc::now().timestamp_millis() as u64),
        node_count: nodes.len() as u32,
        enabled: true,
        auto_update_interval: auto_update_interval.unwrap_or(0),
        dns_pre_resolve: dns_pre_resolve.unwrap_or(false),
        dns_server,
    };

    save_profile_nodes(&state, &profile.id, &nodes)?;

    let mut data = load_profiles_data(&state);
    if data.active_profile_id.is_none() {
        data.active_profile_id = Some(profile.id.clone());
        data.active_node_tag = nodes.first().and_then(|n| n.tag.clone());
    }
    data.profiles.push(profile.clone());
    save_profiles_data(&state, &data)?;
    *state.profiles_data.lock().await = data;

    Ok(profile)
}

#[tauri::command]
pub async fn profile_update(state: State<'_, AppState>, id: String) -> Result<Profile, String> {
    let mut data = load_profiles_data(&state);
    let profile_idx = data.profiles.iter().position(|p| p.id == id)
        .ok_or("Profile not found")?;

    let url = data.profiles[profile_idx].url.clone();
    let nodes = fetch_subscription(&url).await?;
    
    data.profiles[profile_idx].last_update = Some(chrono::Utc::now().timestamp_millis() as u64);
    data.profiles[profile_idx].node_count = nodes.len() as u32;
    
    save_profile_nodes(&state, &id, &nodes)?;
    save_profiles_data(&state, &data)?;
    
    let profile = data.profiles[profile_idx].clone();
    *state.profiles_data.lock().await = data;
    Ok(profile)
}

#[tauri::command]
pub async fn profile_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut data = load_profiles_data(&state);
    data.profiles.retain(|p| p.id != id);
    
    let config_file = state.configs_dir().join(format!("{}.json", id));
    let _ = fs::remove_file(config_file);

    if data.active_profile_id.as_ref() == Some(&id) {
        data.active_profile_id = data.profiles.first().map(|p| p.id.clone());
        data.active_node_tag = None;
    }

    save_profiles_data(&state, &data)?;
    *state.profiles_data.lock().await = data;
    Ok(())
}

#[tauri::command]
pub async fn profile_set_active(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut data = load_profiles_data(&state);
    if !data.profiles.iter().any(|p| p.id == id) {
        return Err("Profile not found".to_string());
    }
    
    data.active_profile_id = Some(id.clone());
    let nodes = load_profile_nodes(&state, &id);
    data.active_node_tag = nodes.first().and_then(|n| n.tag.clone());
    
    save_profiles_data(&state, &data)?;
    *state.profiles_data.lock().await = data;
    Ok(())
}

#[tauri::command]
pub async fn profile_edit(
    state: State<'_, AppState>,
    id: String,
    name: String,
    url: String,
    auto_update_interval: Option<u32>,
    dns_pre_resolve: Option<bool>,
    dns_server: Option<String>,
) -> Result<Profile, String> {
    let mut data = load_profiles_data(&state);
    let profile_idx = data.profiles.iter().position(|p| p.id == id)
        .ok_or("Profile not found")?;

    data.profiles[profile_idx].name = name;
    data.profiles[profile_idx].url = url;
    if let Some(interval) = auto_update_interval {
        data.profiles[profile_idx].auto_update_interval = interval;
    }
    if let Some(dns) = dns_pre_resolve {
        data.profiles[profile_idx].dns_pre_resolve = dns;
    }
    if let Some(server) = dns_server {
        data.profiles[profile_idx].dns_server = Some(server);
    }

    save_profiles_data(&state, &data)?;
    let profile = data.profiles[profile_idx].clone();
    *state.profiles_data.lock().await = data;
    Ok(profile)
}

#[tauri::command]
pub async fn profile_set_enabled(state: State<'_, AppState>, id: String, enabled: bool) -> Result<(), String> {
    let mut data = load_profiles_data(&state);
    let profile = data.profiles.iter_mut().find(|p| p.id == id)
        .ok_or("Profile not found")?;
    profile.enabled = enabled;
    save_profiles_data(&state, &data)?;
    *state.profiles_data.lock().await = data;
    Ok(())
}

#[tauri::command]
pub async fn node_list(state: State<'_, AppState>) -> Result<Vec<SingBoxOutbound>, String> {
    let data = load_profiles_data(&state);
    if let Some(id) = data.active_profile_id {
        Ok(load_profile_nodes(&state, &id))
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
pub async fn node_set_active(state: State<'_, AppState>, tag: String) -> Result<(), String> {
    let mut data = load_profiles_data(&state);
    data.active_node_tag = Some(tag);
    save_profiles_data(&state, &data)?;
    *state.profiles_data.lock().await = data;
    Ok(())
}

#[tauri::command]
pub async fn node_delete(state: State<'_, AppState>, tag: String) -> Result<(), String> {
    let mut data = load_profiles_data(&state);
    let profile_id = data.active_profile_id.clone().ok_or("No active profile")?;
    
    let mut nodes = load_profile_nodes(&state, &profile_id);
    let original_len = nodes.len();
    nodes.retain(|n| n.tag.as_ref() != Some(&tag));
    
    if nodes.len() == original_len {
        return Err("Node not found".to_string());
    }

    save_profile_nodes(&state, &profile_id, &nodes)?;

    if let Some(profile) = data.profiles.iter_mut().find(|p| p.id == profile_id) {
        profile.node_count = nodes.len() as u32;
    }

    if data.active_node_tag.as_ref() == Some(&tag) {
        data.active_node_tag = nodes.first().and_then(|n| n.tag.clone());
    }

    save_profiles_data(&state, &data)?;
    *state.profiles_data.lock().await = data;
    Ok(())
}

async fn fetch_subscription(url: &str) -> Result<Vec<SingBoxOutbound>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    let content = response.text().await.map_err(|e| e.to_string())?;
    
    parse_subscription_content(&content)
}

fn parse_subscription_content(content: &str) -> Result<Vec<SingBoxOutbound>, String> {
    // Try JSON first
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(content) {
        if let Some(proxies) = json.get("proxies").and_then(|p| p.as_array()) {
            return parse_clash_proxies(proxies);
        }
        if let Some(outbounds) = json.get("outbounds").and_then(|o| o.as_array()) {
            return parse_singbox_outbounds(outbounds);
        }
    }

    // Try YAML (Clash format)
    if let Ok(yaml) = serde_yaml::from_str::<serde_json::Value>(content) {
        if let Some(proxies) = yaml.get("proxies").and_then(|p| p.as_array()) {
            return parse_clash_proxies(proxies);
        }
    }

    // Try base64 decode
    if let Ok(decoded) = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, content.trim()) {
        if let Ok(decoded_str) = String::from_utf8(decoded) {
            let nodes: Vec<SingBoxOutbound> = decoded_str
                .lines()
                .filter_map(|line| parse_node_link(line.trim()))
                .collect();
            if !nodes.is_empty() {
                return Ok(nodes);
            }
        }
    }

    // Try line-by-line parsing
    let nodes: Vec<SingBoxOutbound> = content
        .lines()
        .filter_map(|line| parse_node_link(line.trim()))
        .collect();
    
    if nodes.is_empty() {
        Err("No valid nodes found".to_string())
    } else {
        Ok(nodes)
    }
}

fn parse_clash_proxies(proxies: &[serde_json::Value]) -> Result<Vec<SingBoxOutbound>, String> {
    let nodes: Vec<SingBoxOutbound> = proxies
        .iter()
        .filter_map(|p| {
            let tag = p.get("name")?.as_str()?.to_string();
            let proxy_type = map_clash_type(p.get("type")?.as_str()?);
            let server = p.get("server")?.as_str()?.to_string();
            let port = p.get("port")?.as_u64()? as u16;

            let mut extra = serde_json::Map::new();
            
            // Basic fields
            if let Some(pwd) = p.get("password").and_then(|v| v.as_str()) {
                extra.insert("password".to_string(), serde_json::Value::String(pwd.to_string()));
            }
            if let Some(uuid) = p.get("uuid").and_then(|v| v.as_str()) {
                extra.insert("uuid".to_string(), serde_json::Value::String(uuid.to_string()));
            }
            if let Some(flow) = p.get("flow").and_then(|v| v.as_str()) {
                extra.insert("flow".to_string(), serde_json::Value::String(flow.to_string()));
            }
            
            // Method only for shadowsocks
            if proxy_type == "shadowsocks" || proxy_type == "shadowsocksr" {
                if let Some(method) = p.get("method").or(p.get("cipher")).and_then(|v| v.as_str()) {
                    extra.insert("method".to_string(), serde_json::Value::String(method.to_string()));
                }
            }
            
            // VMess specific
            if proxy_type == "vmess" {
                extra.insert("security".to_string(), serde_json::Value::String(
                    p.get("cipher").and_then(|v| v.as_str()).unwrap_or("auto").to_string()
                ));
                if let Some(aid) = p.get("alterId").and_then(|v| v.as_u64()) {
                    extra.insert("alter_id".to_string(), serde_json::Value::Number(aid.into()));
                }
            }
            
            // VLESS specific
            if proxy_type == "vless" {
                extra.insert("packet_encoding".to_string(), serde_json::Value::String("xudp".to_string()));
            }
            
            // TLS configuration
            let network = p.get("network").and_then(|v| v.as_str()).unwrap_or("tcp");
            let tls_enabled = p.get("tls").and_then(|v| v.as_bool()).unwrap_or(false);
            let servername = p.get("servername").or(p.get("sni")).and_then(|v| v.as_str());
            let skip_cert = p.get("skip-cert-verify").and_then(|v| v.as_bool()).unwrap_or(false);
            
            if tls_enabled || network == "ws" || network == "grpc" || network == "h2" || proxy_type == "hysteria2" || proxy_type == "hysteria" || proxy_type == "tuic" {
                let mut tls = serde_json::Map::new();
                tls.insert("enabled".to_string(), serde_json::Value::Bool(true));
                tls.insert("server_name".to_string(), serde_json::Value::String(
                    servername.unwrap_or(&server).to_string()
                ));
                tls.insert("insecure".to_string(), serde_json::Value::Bool(skip_cert));
                
                // ALPN
                if let Some(alpn) = p.get("alpn").and_then(|v| v.as_array()) {
                    tls.insert("alpn".to_string(), serde_json::Value::Array(alpn.clone()));
                }
                
                // Client fingerprint (uTLS)
                if let Some(fp) = p.get("client-fingerprint").and_then(|v| v.as_str()) {
                    tls.insert("utls".to_string(), serde_json::json!({
                        "enabled": true,
                        "fingerprint": fp
                    }));
                }
                
                // Reality
                if let Some(reality_opts) = p.get("reality-opts").and_then(|v| v.as_object()) {
                    let mut reality = serde_json::Map::new();
                    reality.insert("enabled".to_string(), serde_json::Value::Bool(true));
                    if let Some(pk) = reality_opts.get("public-key").and_then(|v| v.as_str()) {
                        reality.insert("public_key".to_string(), serde_json::Value::String(pk.to_string()));
                    }
                    if let Some(sid) = reality_opts.get("short-id").and_then(|v| v.as_str()) {
                        reality.insert("short_id".to_string(), serde_json::Value::String(sid.to_string()));
                    }
                    tls.insert("reality".to_string(), serde_json::Value::Object(reality));
                }
                
                extra.insert("tls".to_string(), serde_json::Value::Object(tls));
            }
            
            // Transport configuration
            match network {
                "ws" => {
                    let ws_opts = p.get("ws-opts").and_then(|v| v.as_object());
                    let mut transport = serde_json::Map::new();
                    transport.insert("type".to_string(), serde_json::Value::String("ws".to_string()));
                    
                    let mut path = ws_opts.and_then(|o| o.get("path")).and_then(|v| v.as_str())
                        .unwrap_or("/").to_string();
                    
                    // Parse early data from path (e.g., /path?ed=2560)
                    if let Some(ed_pos) = path.find("?ed=") {
                        let ed_str = &path[ed_pos + 4..];
                        if let Ok(ed) = ed_str.parse::<u32>() {
                            transport.insert("max_early_data".to_string(), serde_json::Value::Number(ed.into()));
                            transport.insert("early_data_header_name".to_string(), 
                                serde_json::Value::String("Sec-WebSocket-Protocol".to_string()));
                        }
                        path = path[..ed_pos].to_string();
                    }
                    
                    transport.insert("path".to_string(), serde_json::Value::String(path));
                    
                    if let Some(headers) = ws_opts.and_then(|o| o.get("headers")).and_then(|v| v.as_object()) {
                        transport.insert("headers".to_string(), serde_json::Value::Object(headers.clone()));
                    }
                    
                    extra.insert("transport".to_string(), serde_json::Value::Object(transport));
                }
                "grpc" => {
                    let grpc_opts = p.get("grpc-opts").and_then(|v| v.as_object());
                    let mut transport = serde_json::Map::new();
                    transport.insert("type".to_string(), serde_json::Value::String("grpc".to_string()));
                    if let Some(sn) = grpc_opts.and_then(|o| o.get("grpc-service-name")).and_then(|v| v.as_str()) {
                        transport.insert("service_name".to_string(), serde_json::Value::String(sn.to_string()));
                    }
                    extra.insert("transport".to_string(), serde_json::Value::Object(transport));
                }
                "h2" => {
                    let h2_opts = p.get("h2-opts").and_then(|v| v.as_object());
                    let mut transport = serde_json::Map::new();
                    transport.insert("type".to_string(), serde_json::Value::String("http".to_string()));
                    if let Some(path) = h2_opts.and_then(|o| o.get("path")).and_then(|v| v.as_array()) {
                        let path_str = path.iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(",");
                        transport.insert("path".to_string(), serde_json::Value::String(path_str));
                    }
                    if let Some(host) = h2_opts.and_then(|o| o.get("host")) {
                        transport.insert("host".to_string(), host.clone());
                    }
                    extra.insert("transport".to_string(), serde_json::Value::Object(transport));
                }
                _ => {}
            }

            Some(SingBoxOutbound {
                tag: Some(tag),
                outbound_type: Some(proxy_type),
                server: Some(server),
                server_port: Some(port),
                extra: extra.into_iter().collect(),
            })
        })
        .collect();
    
    Ok(nodes)
}

fn parse_singbox_outbounds(outbounds: &[serde_json::Value]) -> Result<Vec<SingBoxOutbound>, String> {
    let nodes: Vec<SingBoxOutbound> = outbounds
        .iter()
        .filter_map(|o| {
            let outbound_type = o.get("type")?.as_str()?;
            if ["direct", "block", "dns", "selector", "urltest"].contains(&outbound_type) {
                return None;
            }
            serde_json::from_value(o.clone()).ok()
        })
        .collect();
    Ok(nodes)
}

fn parse_node_link(link: &str) -> Option<SingBoxOutbound> {
    if link.starts_with("ss://") {
        // Parse Shadowsocks link
        let rest = link.strip_prefix("ss://")?;
        let (encoded, tag) = rest.split_once('#').unwrap_or((rest, "SS"));
        let tag = urlencoding::decode(tag).ok()?.to_string();
        
        // Try decode base64 part
        let parts: Vec<&str> = encoded.split('@').collect();
        if parts.len() != 2 {
            return None;
        }
        
        let decoded = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, parts[0]).ok()?;
        let decoded_str = String::from_utf8(decoded).ok()?;
        let (method, password) = decoded_str.split_once(':')?;
        
        let host_port: Vec<&str> = parts[1].split(':').collect();
        if host_port.len() != 2 {
            return None;
        }
        
        let mut extra = std::collections::HashMap::new();
        extra.insert("method".to_string(), serde_json::Value::String(method.to_string()));
        extra.insert("password".to_string(), serde_json::Value::String(password.to_string()));
        
        Some(SingBoxOutbound {
            tag: Some(tag),
            outbound_type: Some("shadowsocks".to_string()),
            server: Some(host_port[0].to_string()),
            server_port: host_port[1].parse().ok(),
            extra,
        })
    } else {
        None
    }
}

fn map_clash_type(t: &str) -> String {
    match t.to_lowercase().as_str() {
        "ss" => "shadowsocks",
        "ssr" => "shadowsocksr",
        "vmess" => "vmess",
        "vless" => "vless",
        "trojan" => "trojan",
        "hysteria" => "hysteria",
        "hysteria2" => "hysteria2",
        "tuic" => "tuic",
        "http" => "http",
        "socks5" => "socks",
        other => other,
    }.to_string()
}

fn extract_hostname(url: &str) -> String {
    url::Url::parse(url)
        .map(|u| u.host_str().unwrap_or("Unknown").to_string())
        .unwrap_or_else(|_| "Unknown".to_string())
}

#[tauri::command]
pub async fn node_test_latency(app: AppHandle, state: State<'_, AppState>, tag: String) -> Result<i64, String> {
    // Check if main VPN is running
    let is_vpn_running = {
        let proxy_state = state.proxy_state.lock().await;
        matches!(*proxy_state, ProxyState::Connected)
    };
    
    if is_vpn_running {
        // Use main sing-box Clash API
        test_latency_via_clash_api(&tag, 9090).await
    } else {
        // Start temp sing-box if needed
        let started = start_temp_singbox(&app, &state).await;
        if !started {
            return Ok(-1);
        }
        
        // Wait for sing-box to be ready
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        
        test_latency_via_clash_api(&tag, TEMP_SINGBOX_PORT).await
    }
}

#[tauri::command]
pub async fn node_test_all(app: AppHandle, state: State<'_, AppState>) -> Result<std::collections::HashMap<String, i64>, String> {
    let data = load_profiles_data(&state);
    let profile_id = match data.active_profile_id {
        Some(id) => id,
        None => return Ok(std::collections::HashMap::new()),
    };
    
    let nodes = load_profile_nodes(&state, &profile_id);
    
    // Check if main VPN is running
    let is_vpn_running = {
        let proxy_state = state.proxy_state.lock().await;
        matches!(*proxy_state, ProxyState::Connected)
    };
    
    let port = if is_vpn_running {
        9090
    } else {
        // Start temp sing-box if needed
        let started = start_temp_singbox(&app, &state).await;
        if !started {
            return Ok(std::collections::HashMap::new());
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        TEMP_SINGBOX_PORT
    };
    
    let mut results = std::collections::HashMap::new();
    
    // Test in chunks for concurrency
    let chunk_size = 5;
    for chunk in nodes.chunks(chunk_size) {
        let futures: Vec<_> = chunk.iter()
            .filter_map(|node| node.tag.clone())
            .map(|tag| {
                let tag_clone = tag.clone();
                async move {
                    let latency = test_latency_via_clash_api(&tag_clone, port).await.unwrap_or(-1);
                    (tag_clone, latency)
                }
            })
            .collect();
        
        let chunk_results = futures::future::join_all(futures).await;
        for (tag, latency) in chunk_results {
            results.insert(tag, latency);
        }
    }
    
    Ok(results)
}

async fn test_latency_via_clash_api(proxy_name: &str, port: u16) -> Result<i64, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    
    let test_url = "https://www.gstatic.com/generate_204";
    let encoded_name = urlencoding::encode(proxy_name);
    let url = format!(
        "http://127.0.0.1:{}/proxies/{}/delay?url={}&timeout=10000",
        port,
        encoded_name,
        urlencoding::encode(test_url)
    );
    
    let response = client.get(&url).send().await.map_err(|e| e.to_string())?;
    
    if response.status().is_success() {
        let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
        if let Some(delay) = json.get("delay").and_then(|d| d.as_i64()) {
            return Ok(delay);
        }
    }
    
    Ok(-1)
}

async fn start_temp_singbox(app: &AppHandle, state: &AppState) -> bool {
    // Check if already running
    {
        let mut process = TEMP_SINGBOX_PROCESS.lock().await;
        if let Some(ref mut child) = *process {
            // Check if process is still running
            match child.try_wait() {
                Ok(None) => {
                    // Still running, check if API is responsive
                    if check_clash_api_running(TEMP_SINGBOX_PORT).await {
                        return true;
                    }
                }
                _ => {
                    // Process exited
                    *process = None;
                }
            }
        }
    }
    
    // Get kernel path
    let kernel_path = match app.path().resource_dir() {
        Ok(dir) => dir.join("resources").join("libs").join("sing-box.exe"),
        Err(_) => return false,
    };
    
    if !kernel_path.exists() {
        log::warn!("Kernel not found for latency testing: {:?}", kernel_path);
        return false;
    }
    
    // Load nodes and generate temp config
    let data = load_profiles_data(state);
    let profile_id = match data.active_profile_id {
        Some(id) => id,
        None => return false,
    };
    
    // 直接读取原始 JSON 节点
    let nodes_raw = load_profile_nodes_raw(state, &profile_id);
    if nodes_raw.is_empty() {
        log::warn!("No nodes found for latency testing");
        return false;
    }
    
    // Create temp config
    let temp_dir = state.data_dir.join("temp_test");
    if let Err(e) = fs::create_dir_all(&temp_dir) {
        log::error!("Failed to create temp dir: {}", e);
        return false;
    }
    
    let config = generate_temp_config_raw(&nodes_raw, TEMP_SINGBOX_PORT);
    let config_path = temp_dir.join("config.json");
    
    let config_str = serde_json::to_string_pretty(&config).unwrap_or_default();
    log::info!("Temp config: {}", config_str);
    
    if let Err(e) = fs::write(&config_path, &config_str) {
        log::error!("Failed to write temp config: {}", e);
        return false;
    }
    
    // Start temp sing-box
    #[cfg(windows)]
    let result = tokio::process::Command::new(&kernel_path)
        .args(["run", "-c", config_path.to_str().unwrap()])
        .current_dir(&temp_dir)
        .creation_flags(CREATE_NO_WINDOW)
        .kill_on_drop(true)
        .spawn();

    #[cfg(not(windows))]
    let result = tokio::process::Command::new(&kernel_path)
        .args(["run", "-c", config_path.to_str().unwrap()])
        .current_dir(&temp_dir)
        .kill_on_drop(true)
        .spawn();
    
    match result {
        Ok(child) => {
            let mut process = TEMP_SINGBOX_PROCESS.lock().await;
            *process = Some(child);
            log::info!("Started temp sing-box on port {}", TEMP_SINGBOX_PORT);
            true
        }
        Err(e) => {
            log::error!("Failed to start temp sing-box: {}", e);
            false
        }
    }
}

async fn check_clash_api_running(port: u16) -> bool {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build();
    
    if let Ok(client) = client {
        let url = format!("http://127.0.0.1:{}/", port);
        if let Ok(resp) = client.get(&url).send().await {
            return resp.status().is_success();
        }
    }
    false
}

fn generate_temp_config(nodes: &[SingBoxOutbound], api_port: u16) -> serde_json::Value {
    // 直接序列化节点并添加 direct 出站
    let mut outbounds: Vec<serde_json::Value> = nodes.iter()
        .filter_map(|node| serde_json::to_value(node).ok())
        .collect();
    
    // 添加 direct 出站
    outbounds.push(serde_json::json!({ "type": "direct", "tag": "direct" }));
    
    serde_json::json!({
        "log": {
            "disabled": false,
            "level": "info",
            "timestamp": true
        },
        "experimental": {
            "clash_api": {
                "external_controller": format!("127.0.0.1:{}", api_port),
                "default_mode": "rule"
            }
        },
        "inbounds": [],
        "outbounds": outbounds,
        "route": {
            "final": "direct",
            "auto_detect_interface": true
        }
    })
}

fn generate_temp_config_raw(nodes: &[serde_json::Value], api_port: u16) -> serde_json::Value {
    // 处理节点，移除不合法字段并添加必要配置
    let mut outbounds: Vec<serde_json::Value> = nodes.iter()
        .map(|node| {
            let mut node = node.clone();
            if let Some(obj) = node.as_object_mut() {
                let node_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let server = obj.get("server").and_then(|s| s.as_str()).unwrap_or("").to_string();
                let port = obj.get("server_port").and_then(|p| p.as_u64()).unwrap_or(0);
                
                // vless/vmess/trojan 不需要 method 字段
                if node_type != "shadowsocks" && node_type != "shadowsocksr" {
                    obj.remove("method");
                }
                
                // 为需要 TLS 的节点添加配置
                if !obj.contains_key("tls") {
                    match node_type.as_str() {
                        "hysteria2" | "hysteria" | "tuic" => {
                            // 这些协议必须使用 TLS
                            obj.insert("tls".to_string(), serde_json::json!({
                                "enabled": true,
                                "server_name": server,
                                "insecure": true
                            }));
                        }
                        "vless" | "vmess" | "trojan" => {
                            // 443 端口通常需要 TLS
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
        })
        .collect();
    
    // 添加 direct 出站
    outbounds.push(serde_json::json!({ "type": "direct", "tag": "direct" }));
    
    serde_json::json!({
        "log": {
            "disabled": false,
            "level": "info",
            "timestamp": true
        },
        "experimental": {
            "clash_api": {
                "external_controller": format!("127.0.0.1:{}", api_port),
                "default_mode": "rule"
            }
        },
        "inbounds": [],
        "outbounds": outbounds,
        "route": {
            "final": "direct",
            "auto_detect_interface": true
        }
    })
}

#[tauri::command]
pub async fn node_add(
    state: State<'_, AppState>,
    link: String,
    profile_id: Option<String>,
) -> Result<SingBoxOutbound, String> {
    let node = parse_node_link(&link).ok_or("Invalid node link")?;
    
    let mut data = load_profiles_data(&state);
    let target_id = profile_id.or(data.active_profile_id.clone()).ok_or("No target profile")?;
    
    if !data.profiles.iter().any(|p| p.id == target_id) {
        return Err("Profile not found".to_string());
    }
    
    let mut nodes = load_profile_nodes(&state, &target_id);
    nodes.push(node.clone());
    save_profile_nodes(&state, &target_id, &nodes)?;
    
    if let Some(profile) = data.profiles.iter_mut().find(|p| p.id == target_id) {
        profile.node_count = nodes.len() as u32;
    }
    
    save_profiles_data(&state, &data)?;
    *state.profiles_data.lock().await = data;
    
    Ok(node)
}

#[tauri::command]
pub async fn node_export(state: State<'_, AppState>, tag: String) -> Result<String, String> {
    let data = load_profiles_data(&state);
    let profile_id = data.active_profile_id.ok_or("No active profile")?;
    
    let nodes = load_profile_nodes(&state, &profile_id);
    let node = nodes.iter().find(|n| n.tag.as_ref() == Some(&tag)).ok_or("Node not found")?;
    
    export_node_to_link(node)
}

fn export_node_to_link(node: &SingBoxOutbound) -> Result<String, String> {
    let default_tag = "Node".to_string();
    let default_server = String::new();
    
    let tag = urlencoding::encode(node.tag.as_ref().unwrap_or(&default_tag));
    let node_type = node.outbound_type.as_ref().map(|s| s.as_str()).unwrap_or("");
    let server = node.server.as_ref().unwrap_or(&default_server);
    let port = node.server_port.unwrap_or(0);
    
    match node_type.to_lowercase().as_str() {
        "shadowsocks" => {
            let method = node.extra.get("method").and_then(|v| v.as_str()).unwrap_or("aes-256-gcm");
            let password = node.extra.get("password").and_then(|v| v.as_str()).unwrap_or("");
            
            let user_info = base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                format!("{}:{}", method, password)
            );
            Ok(format!("ss://{}@{}:{}#{}", user_info, server, port, tag))
        }
        "vmess" => {
            let uuid = node.extra.get("uuid").and_then(|v| v.as_str()).unwrap_or("");
            
            let config = serde_json::json!({
                "v": "2",
                "ps": node.tag,
                "add": server,
                "port": port,
                "id": uuid,
                "aid": 0,
                "net": "tcp",
                "type": "none",
                "tls": ""
            });
            
            let encoded = base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                config.to_string()
            );
            Ok(format!("vmess://{}", encoded))
        }
        "vless" => {
            let uuid = node.extra.get("uuid").and_then(|v| v.as_str()).unwrap_or("");
            let flow = node.extra.get("flow").and_then(|v| v.as_str()).unwrap_or("");
            
            Ok(format!("vless://{}@{}:{}?flow={}&type=tcp#{}", uuid, server, port, flow, tag))
        }
        "trojan" => {
            let password = node.extra.get("password").and_then(|v| v.as_str()).unwrap_or("");
            
            Ok(format!("trojan://{}@{}:{}#{}", password, server, port, tag))
        }
        "hysteria2" => {
            let password = node.extra.get("password").and_then(|v| v.as_str()).unwrap_or("");
            
            Ok(format!("hysteria2://{}@{}:{}#{}", password, server, port, tag))
        }
        _ => {
            Ok(serde_json::to_string_pretty(node).map_err(|e| e.to_string())?)
        }
    }
}

#[tauri::command]
pub async fn profile_import_content(
    state: State<'_, AppState>,
    name: String,
    content: String,
    auto_update_interval: Option<u32>,
    dns_pre_resolve: Option<bool>,
    dns_server: Option<String>,
) -> Result<Profile, String> {
    let nodes = parse_subscription_content(&content)?;
    
    if nodes.is_empty() {
        return Err("No valid nodes found in content".to_string());
    }
    
    let profile = Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        url: String::new(),
        last_update: Some(chrono::Utc::now().timestamp_millis() as u64),
        node_count: nodes.len() as u32,
        enabled: true,
        auto_update_interval: auto_update_interval.unwrap_or(0),
        dns_pre_resolve: dns_pre_resolve.unwrap_or(false),
        dns_server,
    };

    save_profile_nodes(&state, &profile.id, &nodes)?;

    let mut data = load_profiles_data(&state);
    if data.active_profile_id.is_none() {
        data.active_profile_id = Some(profile.id.clone());
        data.active_node_tag = nodes.first().and_then(|n| n.tag.clone());
    }
    data.profiles.push(profile.clone());
    save_profiles_data(&state, &data)?;
    *state.profiles_data.lock().await = data;

    Ok(profile)
}
