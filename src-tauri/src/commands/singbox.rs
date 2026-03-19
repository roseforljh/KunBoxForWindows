use tauri::{AppHandle, Emitter, Manager, State};
use std::fs;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::process::Command;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;
use crate::state::AppState;
use crate::types::{CommandResult, ProxyState, TrafficStats};

#[cfg(windows)]
#[allow(unused_imports)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

#[cfg(windows)]
fn is_running_as_admin() -> bool {
    use std::process::Command as StdCommand;
    use std::os::windows::process::CommandExt;

    let output = StdCommand::new("net")
        .args(["session"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    match output {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn is_running_as_admin() -> bool {
    false
}

pub(crate) async fn singbox_start_impl(app: AppHandle, state: &AppState) -> Result<CommandResult, String> {
    let singbox_path = get_singbox_path(&app)?;
    
    if !singbox_path.exists() {
        return Ok(CommandResult::err("未检测到 sing-box 内核，请先到【设置 → 内核】下载并安装后再启动 VPN。"));
    }

    // Check if TUN mode is enabled and admin rights are required
    let settings = state.settings.lock().await;
    if settings.tun_enabled && !is_running_as_admin() {
        return Ok(CommandResult::err("TUN 模式需要管理员权限。请右键点击应用图标，选择「以管理员身份运行」后重试。"));
    }
    drop(settings);

    // Stop any existing managed process in our state before starting a new one
    if let Some(mut child) = state.singbox_process.lock().await.take() {
        let _ = child.kill().await;
    }

    // Generate config
    let config_result = generate_config(&state).await?;
    if !config_result.success {
        return Ok(config_result);
    }

    let config_path = state.config_dir.join("config.json");

    let config_path_str = config_path.to_str()
        .ok_or_else(|| "Config path contains invalid UTF-8 characters".to_string())?;

    // Preflight check: show clear error to UI before trying to run
    #[cfg(windows)]
    let check_output = Command::new(&singbox_path)
        .args(["check", "-c", config_path_str])
        .current_dir(&state.config_dir)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    #[cfg(not(windows))]
    let check_output = Command::new(&singbox_path)
        .args(["check", "-c", config_path_str])
        .current_dir(&state.config_dir)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !check_output.status.success() {
        let stderr = String::from_utf8_lossy(&check_output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&check_output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        let message = if detail.is_empty() {
            "内核配置检查失败，请检查节点与DNS设置".to_string()
        } else {
            format!("内核配置检查失败: {}", detail)
        };
        return Ok(CommandResult::err(message));
    }

    // Update state
    *state.proxy_state.lock().await = ProxyState::Connecting;
    let _ = app.emit("singbox:state", "connecting");

    // Start sing-box process

    #[cfg(windows)]
    let mut child = Command::new(&singbox_path)
        .args(["run", "-c", config_path_str])
        .current_dir(&state.config_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;

    #[cfg(not(windows))]
    let mut child = Command::new(&singbox_path)
        .args(["run", "-c", config_path_str])
        .current_dir(&state.config_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| e.to_string())?;

    // Capture stderr for logging
    if let Some(stderr) = child.stderr.take() {
        let app_clone = app.clone();
        let settings_ref = state.settings.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let enable_runtime_logs = settings_ref.lock().await.enable_runtime_logs;
                if enable_runtime_logs {
                    let _ = app_clone.emit("singbox:log", serde_json::json!({
                        "timestamp": chrono::Utc::now().timestamp_millis(),
                        "level": "info",
                        "tag": "sing-box",
                        "message": line
                    }));
                }
            }
        });
    }

    let start_time_val = chrono::Utc::now().timestamp_millis() as u64;
    *state.start_time.lock().await = Some(start_time_val);

    // Store child process handle in state so it persists beyond this function
    // (kill_on_drop(true) would kill the process when `child` is dropped otherwise)
    *state.singbox_process.lock().await = Some(child);

    // Spawn a background task that polls the process exit status without taking
    // the handle out of state.  This way singbox_stop_impl can still .take()
    // and .kill() the child at any time.
    let wait_app = app.clone();
    let proxy_state = state.proxy_state.clone();
    let start_time_state = state.start_time.clone();
    let process_slot = state.singbox_process.clone();
    let traffic_cancel = state.traffic_cancel.clone();
    tokio::spawn(async move {
        // Poll the child process by periodically checking if it has exited.
        // We cannot call child.wait() because that requires &mut ownership,
        // and taking the child out of the slot would break singbox_stop_impl.
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;

            let mut guard = process_slot.lock().await;
            match guard.as_mut() {
                None => {
                    // Process handle was taken by singbox_stop_impl — normal shutdown
                    break;
                }
                Some(child) => {
                    // try_wait returns Ok(Some(status)) if exited, Ok(None) if still running
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            // Process exited on its own — take handle out and react
                            let _ = guard.take();
                            drop(guard);

                            let current_state = proxy_state.lock().await.clone();
                            if !matches!(current_state, ProxyState::Idle | ProxyState::Disconnecting) {
                                *proxy_state.lock().await = ProxyState::Error;
                                *start_time_state.lock().await = None;
                                if let Some(cancel) = traffic_cancel.lock().await.take() {
                                    cancel.cancel();
                                }
                                let _ = disable_system_proxy_internal().await;
                                let _ = wait_app.emit("singbox:state", "error");
                                let _ = wait_app.emit("singbox:log", serde_json::json!({
                                    "timestamp": chrono::Utc::now().timestamp_millis(),
                                    "level": "error",
                                    "tag": "sing-box",
                                    "message": format!("sing-box exited unexpectedly: {}", status)
                                }));
                            }
                            break;
                        }
                        Ok(None) => {
                            // Still running, continue polling
                        }
                        Err(_) => {
                            // Error checking status, assume crashed
                            let _ = guard.take();
                            drop(guard);

                            let current_state = proxy_state.lock().await.clone();
                            if !matches!(current_state, ProxyState::Idle | ProxyState::Disconnecting) {
                                *proxy_state.lock().await = ProxyState::Error;
                                *start_time_state.lock().await = None;
                                if let Some(cancel) = traffic_cancel.lock().await.take() {
                                    cancel.cancel();
                                }
                                let _ = disable_system_proxy_internal().await;
                                let _ = wait_app.emit("singbox:state", "error");
                            }
                            break;
                        }
                    }
                }
            }
        }
    });

    *state.proxy_state.lock().await = ProxyState::Connected;
    
    let _ = app.emit("singbox:state", "connected");

    // 连接成功后，后台自动测试 profile selector 延迟并切换
    let selector_tags = collect_referenced_profile_selector_tags(&state).await;
    if !selector_tags.is_empty() {
        let app_for_selector_test = app.clone();
        tokio::spawn(async move {
            // 给 Clash API 一点准备时间
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            for selector_tag in selector_tags {
                if let Err(err) = test_selector_latency_internal(&app_for_selector_test, selector_tag.clone(), None).await {
                    log::warn!("Auto selector latency test failed for '{}': {}", selector_tag, err);
                }
            }
        });
    }

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

pub(crate) async fn singbox_stop_impl(app: AppHandle, state: &AppState) -> Result<CommandResult, String> {
    // Cancel traffic polling
    if let Some(cancel) = state.traffic_cancel.lock().await.take() {
        cancel.cancel();
    }
    
    *state.proxy_state.lock().await = ProxyState::Disconnecting;
    let _ = app.emit("singbox:state", "disconnecting");

    // Kill process from state
    if let Some(mut child) = state.singbox_process.lock().await.take() {
        let _ = child.kill().await;
    }

    // Managed process has already been stopped above if present

    // Disable system proxy
    let _ = disable_system_proxy_internal().await;

    *state.proxy_state.lock().await = ProxyState::Idle;
    *state.start_time.lock().await = None;
    let _ = app.emit("singbox:state", "idle");

    Ok(CommandResult::ok())
}

#[tauri::command]
pub async fn singbox_start(app: AppHandle, state: State<'_, AppState>) -> Result<CommandResult, String> {
    singbox_start_impl(app, &state).await
}

#[tauri::command]
pub async fn singbox_stop(app: AppHandle, state: State<'_, AppState>) -> Result<CommandResult, String> {
    singbox_stop_impl(app, &state).await
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
        "http" | "socks" | "wireguard" | "ssh" | "shadowtls" |
        "naive"
    )
}

/// 处理节点配置，确保格式正确
fn process_node(node: &serde_json::Value) -> serde_json::Value {
    let mut node = node.clone();
    if let Some(obj) = node.as_object_mut() {
        let node_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let server = obj.get("server").and_then(|s| s.as_str()).unwrap_or("").to_string();
        let port = obj.get("server_port").and_then(|p| p.as_u64()).unwrap_or(0);

        if node_type != "shadowsocks" && node_type != "shadowsocksr" {
            obj.remove("method");
        }

        if !obj.contains_key("tls") {
            match node_type.as_str() {
                "hysteria2" | "hysteria" | "tuic" | "naive" => {
                    obj.insert("tls".to_string(), serde_json::json!({
                        "enabled": true,
                        "server_name": server,
                        "insecure": false
                    }));
                }
                "vless" | "vmess" | "trojan" => {
                    if port == 443 || port == 8443 || port == 2053 {
                        obj.insert("tls".to_string(), serde_json::json!({
                            "enabled": true,
                            "server_name": server,
                            "insecure": false
                        }));
                    }
                }
                _ => {}
            }
        }

        if node_type == "vless" && !obj.contains_key("packet_encoding") {
            obj.insert("packet_encoding".to_string(), serde_json::Value::String("xudp".to_string()));
        }
    }
    node
}

fn build_dns_server(address: &str, tag: &str, detour: &str) -> serde_json::Value {
    let value = address.trim();
    if value.eq_ignore_ascii_case("local") {
        return serde_json::json!({ "tag": tag, "type": "local" });
    }
    if value.eq_ignore_ascii_case("fakeip") {
        return serde_json::json!({ "tag": tag, "type": "fakeip" });
    }

    // 解析协议类型、服务器地址（可能含路径）、默认端口、可选路径
    let (server_type, server_with_path, default_port) = if let Some(v) = value.strip_prefix("udp://") {
        ("udp", v.to_string(), 53u16)
    } else if let Some(v) = value.strip_prefix("tcp://") {
        ("tcp", v.to_string(), 53)
    } else if let Some(v) = value.strip_prefix("tls://") {
        ("tls", v.to_string(), 853)
    } else if let Some(v) = value.strip_prefix("https://") {
        ("https", v.to_string(), 443)
    } else if let Some(v) = value.strip_prefix("h3://") {
        ("h3", v.to_string(), 443)
    } else if let Some(v) = value.strip_prefix("quic://") {
        ("quic", v.to_string(), 853)
    } else if value.contains("://") {
        ("udp", value.to_string(), 53)
    } else if value.contains("/dns-query") || value.contains("/resolve") {
        ("https", value.to_string(), 443)
    } else {
        ("udp", value.to_string(), 53)
    };

    // 从 server_with_path 中分离 host:port 和路径（如 /dns-query）
    // 例如 "1.1.1.1/dns-query" → host="1.1.1.1", path="/dns-query"
    // 例如 "dns.google:443/dns-query" → host="dns.google", port=443, path="/dns-query"
    let (server_no_path, path) = if let Some(slash_pos) = server_with_path.find('/') {
        let path_str = &server_with_path[slash_pos..];
        let host_part = &server_with_path[..slash_pos];
        (host_part.to_string(), Some(path_str.to_string()))
    } else {
        (server_with_path.clone(), None)
    };

    // 从 server_no_path 中提取 host 和可选端口
    let (host, port) = if let Some(bracket_end) = server_no_path.find(']') {
        // IPv6 地址 [::1]:port
        if let Some(colon_pos) = server_no_path[bracket_end..].find(':') {
            let port_str = &server_no_path[bracket_end + colon_pos + 1..];
            let port = port_str.parse::<u16>().unwrap_or(default_port);
            (server_no_path[..bracket_end + 1].to_string(), port)
        } else {
            (server_no_path.clone(), default_port)
        }
    } else if let Some(colon_pos) = server_no_path.rfind(':') {
        // host:port（排除纯 IPv6 地址如 ::1）
        let after_colon = &server_no_path[colon_pos + 1..];
        if let Ok(port) = after_colon.parse::<u16>() {
            (server_no_path[..colon_pos].to_string(), port)
        } else {
            (server_no_path.clone(), default_port)
        }
    } else {
        (server_no_path.clone(), default_port)
    };

    let mut server_obj = serde_json::json!({
        "tag": tag,
        "type": server_type,
        "server": host,
        "server_port": port
    });

    // DoH/H3 需要 path 字段（sing-box 1.12+ 新格式）
    // server 字段只含主机名/IP，路径通过 path 字段传递
    if matches!(server_type, "https" | "h3") {
        if let Some(ref p) = path {
            server_obj["path"] = serde_json::Value::String(p.clone());
        }
        // 不设 path 时 sing-box 默认用 /dns-query
    }

    if detour != "direct" {
        server_obj["detour"] = serde_json::Value::String(detour.to_string());
    }

    server_obj
}

fn apply_route_target(mut rule: serde_json::Value, target: &str) -> serde_json::Value {
    if let Some(obj) = rule.as_object_mut() {
        if target == "block" {
            obj.insert("action".to_string(), serde_json::Value::String("reject".to_string()));
        } else {
            obj.insert("outbound".to_string(), serde_json::Value::String(target.to_string()));
        }
    }

    rule
}

fn is_valid_profile_id(profile_id: &str) -> bool {
    !profile_id.is_empty()
        && profile_id.len() <= 64
        && profile_id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn is_valid_ruleset_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 128
        && tag.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn profile_nodes_path(state: &AppState, profile_id: &str) -> Result<PathBuf, String> {
    if !is_valid_profile_id(profile_id) {
        return Err("Invalid profile id".to_string());
    }

    Ok(state.configs_dir().join(format!("{}.json", profile_id)))
}

fn ruleset_cache_path(state: &AppState, tag: &str) -> Result<PathBuf, String> {
    if !is_valid_ruleset_tag(tag) {
        return Err("Invalid ruleset tag".to_string());
    }

    Ok(state.rulesets_cache_dir().join(format!("{}.srs", tag)))
}

/// 配置文件信息（用于跨配置分流）
struct ProfileInfo {
    id: String,
    name: String,
    nodes: Vec<serde_json::Value>,
}

/// 加载所有配置文件的节点信息
fn load_all_profiles(state: &AppState, profiles_data: &crate::types::ProfilesData) -> Vec<ProfileInfo> {
    let mut result = Vec::new();
    
    for profile in &profiles_data.profiles {
        let nodes_file = match profile_nodes_path(state, &profile.id) {
            Ok(path) => path,
            Err(_) => continue,
        };
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
    // Always reload profiles data from file to ensure we have the latest
    let profiles_file = state.profiles_file();
    let profiles_data: crate::types::ProfilesData = if profiles_file.exists() {
        let content = fs::read_to_string(&profiles_file).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        return Ok(CommandResult::err("No profiles file found"));
    };
    
    let settings = state.settings.lock().await;
    let rulesets = state.rulesets.lock().await;
    let custom_rules = state.custom_rules.lock().await;

    let active_profile_id = match &profiles_data.active_profile_id {
        Some(id) => id.clone(),
        None => return Ok(CommandResult::err("No active profile")),
    };

    let nodes_file = profile_nodes_path(state, &active_profile_id)?;
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

    // 收集自定义规则引用的 profile 和 node
    for rule in custom_rules.domain_rules.iter().filter(|r| r.enabled) {
        if let Some(ref value) = rule.outbound_value {
            match rule.outbound_mode.as_str() {
                "profile" => { referenced_profile_ids.insert(value.clone()); }
                "node" => { 
                    // Parse "profileId::nodeTag" format
                    let node_tag = if value.contains("::") {
                        value.split("::").nth(1).unwrap_or(value).to_string()
                    } else {
                        value.clone()
                    };
                    referenced_node_tags.insert(node_tag);
                }
                _ => {}
            }
        }
    }

    // Build config - 使用 sing-box 1.11+ 新格式
    let listen_addr = if settings.allow_lan { "0.0.0.0" } else { "127.0.0.1" };
    
    let routing_mode = settings.routing_mode.as_str();

    // 构建 DNS 服务器列表（sing-box 1.12+ 新格式）
    let mut dns_servers = vec![
        build_dns_server(&settings.local_dns, "dns-local", "direct"),
        build_dns_server(&settings.remote_dns, "dns-remote", "PROXY"),
    ];

    // 构建 DNS 规则
    let mut dns_rules: Vec<serde_json::Value> = Vec::new();

    // ========== 根据域名分流规则生成对应的 DNS 规则 ==========
    // 核心原理：sing-box 中 DNS 查询和路由是分开处理的。
    // 如果一个域名的路由规则设为 "direct"，但 DNS 查询走的是远程代理 DNS，
    // 就会导致 DNS 解析失败或返回错误的 IP，直连规则就不会生效。
    // 因此必须为每个域名路由规则生成对应的 DNS 规则：
    //   - direct 出站 → 使用 dns-local（本地 DNS 解析）
    //   - proxy/其他出站 → 使用 dns-remote（远程代理 DNS 解析）
    //   - block 出站 → 不需要 DNS 规则（直接拒绝）
    if routing_mode == "rule" {
        // 收集 direct 域名和 proxy 域名，按类型分组后批量添加 DNS 规则
        let mut direct_domains: Vec<String> = Vec::new();
        let mut direct_domain_suffixes: Vec<String> = Vec::new();
        let mut direct_domain_keywords: Vec<String> = Vec::new();
        let mut proxy_domains: Vec<String> = Vec::new();
        let mut proxy_domain_suffixes: Vec<String> = Vec::new();
        let mut proxy_domain_keywords: Vec<String> = Vec::new();

        for rule in custom_rules.domain_rules.iter().filter(|r| r.enabled) {
            let is_direct = rule.outbound_mode == "direct";
            let is_proxy = matches!(rule.outbound_mode.as_str(), "proxy" | "node" | "profile");
            // block 规则不需要 DNS 规则

            if is_direct {
                match rule.rule_type.as_str() {
                    "domain" => direct_domains.push(rule.value.clone()),
                    "domain_suffix" => direct_domain_suffixes.push(rule.value.clone()),
                    "domain_keyword" => direct_domain_keywords.push(rule.value.clone()),
                    _ => direct_domain_suffixes.push(rule.value.clone()),
                }
            } else if is_proxy {
                match rule.rule_type.as_str() {
                    "domain" => proxy_domains.push(rule.value.clone()),
                    "domain_suffix" => proxy_domain_suffixes.push(rule.value.clone()),
                    "domain_keyword" => proxy_domain_keywords.push(rule.value.clone()),
                    _ => proxy_domain_suffixes.push(rule.value.clone()),
                }
            }
        }

        // 生成 direct 域名的 DNS 规则 → dns-local
        if !direct_domains.is_empty() || !direct_domain_suffixes.is_empty() || !direct_domain_keywords.is_empty() {
            let mut dns_rule = serde_json::Map::new();
            if !direct_domains.is_empty() {
                dns_rule.insert("domain".to_string(), serde_json::json!(direct_domains));
            }
            if !direct_domain_suffixes.is_empty() {
                dns_rule.insert("domain_suffix".to_string(), serde_json::json!(direct_domain_suffixes));
            }
            if !direct_domain_keywords.is_empty() {
                dns_rule.insert("domain_keyword".to_string(), serde_json::json!(direct_domain_keywords));
            }
            dns_rule.insert("server".to_string(), serde_json::json!("dns-local"));
            dns_rules.push(serde_json::Value::Object(dns_rule));
            log::info!("Added DNS rule for direct domains: {} domain, {} suffix, {} keyword",
                direct_domains.len(), direct_domain_suffixes.len(), direct_domain_keywords.len());
        }

        // 生成 proxy 域名的 DNS 规则 → dns-remote
        if !proxy_domains.is_empty() || !proxy_domain_suffixes.is_empty() || !proxy_domain_keywords.is_empty() {
            let mut dns_rule = serde_json::Map::new();
            if !proxy_domains.is_empty() {
                dns_rule.insert("domain".to_string(), serde_json::json!(proxy_domains));
            }
            if !proxy_domain_suffixes.is_empty() {
                dns_rule.insert("domain_suffix".to_string(), serde_json::json!(proxy_domain_suffixes));
            }
            if !proxy_domain_keywords.is_empty() {
                dns_rule.insert("domain_keyword".to_string(), serde_json::json!(proxy_domain_keywords));
            }
            dns_rule.insert("server".to_string(), serde_json::json!("dns-remote"));
            dns_rules.push(serde_json::Value::Object(dns_rule));
            log::info!("Added DNS rule for proxy domains: {} domain, {} suffix, {} keyword",
                proxy_domains.len(), proxy_domain_suffixes.len(), proxy_domain_keywords.len());
        }
    }

    // ========== 根据规则集(ruleset)生成对应的 DNS 规则 ==========
    // 与域名规则同理：规则集中设为 direct 的域名类规则集也需要用 dns-local 解析
    if routing_mode == "rule" {
        let mut direct_rulesets: Vec<String> = Vec::new();
        let mut proxy_rulesets: Vec<String> = Vec::new();

        for rs in &enabled_rulesets {
            // 只为有本地缓存的规则集生成 DNS 规则
            let local_path = match ruleset_cache_path(state, &rs.tag) {
                Ok(path) => path,
                Err(_) => continue,
            };
            if !local_path.exists() {
                continue;
            }

            match rs.outbound_mode.as_str() {
                "direct" => direct_rulesets.push(rs.tag.clone()),
                "proxy" | "node" | "节点" | "profile" | "配置" => proxy_rulesets.push(rs.tag.clone()),
                _ => {} // block 不需要 DNS 规则
            }
        }

        if !direct_rulesets.is_empty() {
            dns_rules.push(serde_json::json!({
                "rule_set": direct_rulesets,
                "server": "dns-local"
            }));
            log::info!("Added DNS rule for {} direct rulesets", direct_rulesets.len());
        }

        if !proxy_rulesets.is_empty() {
            dns_rules.push(serde_json::json!({
                "rule_set": proxy_rulesets,
                "server": "dns-remote"
            }));
            log::info!("Added DNS rule for {} proxy rulesets", proxy_rulesets.len());
        }
    }

    // FakeDNS 规则（放在域名 DNS 规则之后，确保域名规则优先匹配）
    if settings.fake_dns {
        dns_servers.push(serde_json::json!({
            "tag": "dns-fakeip",
            "type": "fakeip",
            "inet4_range": "198.18.0.0/15",
            "inet6_range": "fc00::/18"
        }));
        dns_rules.push(serde_json::json!({
            "query_type": ["A", "AAAA"],
            "server": "dns-fakeip"
        }));
    }
    
    // 构建 inbounds
    let mut inbounds = vec![
        serde_json::json!({
            "type": "mixed",
            "tag": "mixed-in",
            "listen": listen_addr,
            "listen_port": settings.local_port
        }),
        serde_json::json!({
            "type": "socks",
            "tag": "socks-in",
            "listen": listen_addr,
            "listen_port": settings.socks_port
        })
    ];
    
    // 如果启用 TUN 模式，添加 TUN inbound
    if settings.tun_enabled {
        inbounds.push(serde_json::json!({
            "type": "tun",
            "tag": "tun-in",
            "interface_name": "kunbox-tun",
            "address": ["172.19.0.1/30", "fdfe:dcba:9876::1/126"],
            "mtu": 9000,
            "auto_route": true,
            "strict_route": true,
            "stack": settings.tun_stack
        }));
    }
    
    let dns_final = match routing_mode {
        "global-direct" => "dns-local",
        _ => "dns-remote",
    };
    let dns_config = serde_json::json!({
        "servers": dns_servers,
        "rules": dns_rules,
        "final": dns_final,
        "independent_cache": true
    });

    // 避免 1.13+ 硬错误，始终不生成已弃用 outbound DNS rule item

    let route_final = match routing_mode {
        "global-proxy" => "PROXY",
        "global-direct" => "direct",
        _ => match settings.default_rule.as_str() {
            "proxy" => "PROXY",
            "block" => "direct",
            other => other,
        },
    };

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
        "dns": dns_config,
        "inbounds": inbounds,
        "route": {
            "auto_detect_interface": true,
            "default_domain_resolver": "dns-local",
            "final": route_final
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

            // 创建 selector 类型（由应用层管理延迟测试和切换）
            if !profile_proxy_tags.is_empty() {
                outbounds.push(serde_json::json!({
                    "type": "selector",
                    "tag": selector_tag,
                    "outbounds": profile_proxy_tags,
                    "default": profile_proxy_tags.first(),
                    "interrupt_exist_connections": false
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
            "interval": "10m",
            "idle_timeout": "30m",
            "tolerance": 50
        }));
    }

    // 6. 添加基础出站
    outbounds.push(serde_json::json!({ "type": "direct", "tag": "direct" }));
    config["outbounds"] = serde_json::Value::Array(outbounds.clone());

    // 收集所有可用的 outbound tags
    let available_outbound_tags: std::collections::HashSet<String> = outbounds.iter()
        .filter_map(|o| o.get("tag").and_then(|t| t.as_str()).map(|s| s.to_string()))
        .collect();

    // ========== 构建路由规则 ==========
    let mut rules: Vec<serde_json::Value> = vec![
        serde_json::json!({ "inbound": "mixed-in", "action": "sniff" }),
        serde_json::json!({ "protocol": "dns", "action": "hijack-dns" }),
    ];

    // 预先声明规则集引用和缓存目录（广告屏蔽和用户规则集都需要）
    let mut rule_set_refs = Vec::new();

    if settings.tun_enabled {
        rules.insert(1, serde_json::json!({ "inbound": "tun-in", "action": "sniff" }));
    }

    if settings.bypass_lan {
        rules.push(serde_json::json!({ "ip_is_private": true, "outbound": "direct" }));
    }

    // ========== 添加自定义域名分流规则 ==========
    if routing_mode == "rule" {
        for rule in custom_rules.domain_rules.iter().filter(|r| r.enabled) {
            let outbound = match rule.outbound_mode.as_str() {
                "proxy" => "PROXY".to_string(),
                "direct" => "direct".to_string(),
                "block" => "block".to_string(),
                "node" => {
                    if let Some(ref node_ref) = rule.outbound_value {
                        // Parse "profileId::nodeTag" format
                        let node_tag = if node_ref.contains("::") {
                            node_ref.split("::").nth(1).unwrap_or(node_ref).to_string()
                        } else {
                            node_ref.clone()
                        };
                        if available_outbound_tags.contains(&node_tag) {
                            node_tag
                        } else {
                            log::warn!("Node '{}' not found for domain rule '{}', falling back to PROXY", node_tag, rule.value);
                            "PROXY".to_string()
                        }
                    } else {
                        "PROXY".to_string()
                    }
                },
                "profile" => {
                    if let Some(ref profile_id) = rule.outbound_value {
                        if let Some(selector_tag) = profile_id_to_selector.get(profile_id) {
                            if available_outbound_tags.contains(selector_tag) {
                                selector_tag.clone()
                            } else {
                                log::warn!("Profile selector '{}' not found for domain rule '{}', falling back to PROXY", selector_tag, rule.value);
                                "PROXY".to_string()
                            }
                        } else {
                            log::warn!("Profile '{}' not found for domain rule '{}', falling back to PROXY", profile_id, rule.value);
                            "PROXY".to_string()
                        }
                    } else {
                        "PROXY".to_string()
                    }
                },
                other => other.to_string()
            };

            let rule_json = match rule.rule_type.as_str() {
                "domain" => apply_route_target(serde_json::json!({
                    "domain": [&rule.value]
                }), &outbound),
                "domain_suffix" => apply_route_target(serde_json::json!({
                    "domain_suffix": [&rule.value]
                }), &outbound),
                "domain_keyword" => apply_route_target(serde_json::json!({
                    "domain_keyword": [&rule.value]
                }), &outbound),
                _ => apply_route_target(serde_json::json!({
                    "domain_suffix": [&rule.value]
                }), &outbound)
            };
            rules.push(rule_json);
            log::info!("Added domain rule: {} ({}) -> {}", rule.value, rule.rule_type, outbound);
        }
    }

    // 添加规则集路由规则

    if routing_mode == "rule" {
        for rs in &enabled_rulesets {
        // 检查本地缓存文件是否存在
        let local_path = match ruleset_cache_path(state, &rs.tag) {
            Ok(path) => path,
            Err(_) => {
                log::warn!("Invalid ruleset tag '{}', skipping", rs.tag);
                continue;
            }
        };
        
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

        let rule_json = apply_route_target(serde_json::json!({
            "rule_set": [rs.tag]
        }), &outbound);
        rules.push(rule_json);
        }
    }

    if routing_mode == "rule" && settings.default_rule == "block" {
        rules.push(serde_json::json!({ "action": "reject" }));
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

fn get_singbox_path(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let data_kernel = app_data_dir.join("libs").join("sing-box.exe");
    if data_kernel.exists() {
        return Ok(data_kernel);
    }

    let resource_path = app.path().resource_dir().map_err(|e| e.to_string())?;
    Ok(resource_path.join("resources/libs/sing-box.exe"))
}

struct SystemProxySnapshot {
    proxy_enable: Option<String>,
    proxy_server: Option<String>,
    proxy_override: Option<String>,
    auto_config_url: Option<String>,
}

static SYSTEM_PROXY_SNAPSHOT: once_cell::sync::Lazy<std::sync::Mutex<Option<SystemProxySnapshot>>> =
    once_cell::sync::Lazy::new(|| std::sync::Mutex::new(None));

#[cfg(windows)]
async fn query_registry_value(name: &str) -> Result<Option<String>, String> {
    let output = Command::new("reg")
        .args([
            "query",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v",
            name,
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Ok(None);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = stdout
        .lines()
        .find(|line| line.contains(name))
        .and_then(|line| line.split_whitespace().last())
        .map(|s| s.to_string());

    Ok(value)
}

#[cfg(windows)]
async fn snapshot_system_proxy_if_needed() -> Result<(), String> {
    let already_snapshotted = {
        let guard = SYSTEM_PROXY_SNAPSHOT.lock().map_err(|_| "系统代理快照锁失败".to_string())?;
        guard.is_some()
    };

    if already_snapshotted {
        return Ok(());
    }

    let snapshot = SystemProxySnapshot {
        proxy_enable: query_registry_value("ProxyEnable").await?,
        proxy_server: query_registry_value("ProxyServer").await?,
        proxy_override: query_registry_value("ProxyOverride").await?,
        auto_config_url: query_registry_value("AutoConfigURL").await?,
    };

    let mut guard = SYSTEM_PROXY_SNAPSHOT.lock().map_err(|_| "系统代理快照锁失败".to_string())?;
    if guard.is_none() {
        *guard = Some(snapshot);
    }
    Ok(())
}

#[cfg(windows)]
async fn set_registry_value(name: &str, value_type: &str, value: &str) -> Result<(), String> {
    let output = Command::new("reg")
        .args([
            "add",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v",
            name,
            "/t",
            value_type,
            "/d",
            value,
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    Ok(())
}

#[cfg(windows)]
async fn restore_system_proxy_snapshot() -> Result<(), String> {
    let snapshot = {
        let mut guard = SYSTEM_PROXY_SNAPSHOT.lock().map_err(|_| "系统代理快照锁失败".to_string())?;
        guard.take()
    };

    if let Some(snapshot) = snapshot {
        let enable_value = snapshot.proxy_enable.as_deref().unwrap_or("0");
        set_registry_value("ProxyEnable", "REG_DWORD", enable_value).await?;

        if let Some(proxy_server) = snapshot.proxy_server {
            set_registry_value("ProxyServer", "REG_SZ", &proxy_server).await?;
        }
        if let Some(proxy_override) = snapshot.proxy_override {
            set_registry_value("ProxyOverride", "REG_SZ", &proxy_override).await?;
        }
        if let Some(auto_config_url) = snapshot.auto_config_url {
            set_registry_value("AutoConfigURL", "REG_SZ", &auto_config_url).await?;
        }
    } else {
        set_registry_value("ProxyEnable", "REG_DWORD", "0").await?;
    }

    Ok(())
}

async fn enable_system_proxy_internal(port: u16) -> Result<(), String> {
    let proxy = format!("127.0.0.1:{}", port);
    
    #[cfg(windows)]
    {
        snapshot_system_proxy_if_needed().await?;
        set_registry_value("ProxyEnable", "REG_DWORD", "1").await?;
        set_registry_value("ProxyServer", "REG_SZ", &proxy).await?;
    }

    #[cfg(not(windows))]
    {
        let _ = proxy;
    }

    Ok(())
}

async fn disable_system_proxy_internal() -> Result<(), String> {
    #[cfg(windows)]
    restore_system_proxy_snapshot().await?;

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
    let mut error_streak: u32 = 0;

    // Wait a bit for sing-box to be ready
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    loop {
        let poll_interval = if error_streak >= 3 {
            std::time::Duration::from_secs(2)
        } else {
            std::time::Duration::from_secs(1)
        };

        tokio::select! {
            _ = cancel.cancelled() => {
                log::info!("Traffic polling cancelled");
                break;
            }
            _ = tokio::time::sleep(poll_interval) => {
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
                            error_streak = 0;

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
                        error_streak = error_streak.saturating_add(1);
                        if error_streak == 1 || error_streak % 10 == 0 {
                            log::warn!("Traffic polling error (streak={}): {}", error_streak, e);
                        }
                    }
                }
            }
        }
    }
}

async fn load_profiles_data_from_file(state: &AppState) -> crate::types::ProfilesData {
    let profiles_file = state.profiles_file();
    if !profiles_file.exists() {
        return crate::types::ProfilesData::default();
    }

    match fs::read_to_string(&profiles_file) {
        Ok(content) => serde_json::from_str::<crate::types::ProfilesData>(&content).unwrap_or_default(),
        Err(err) => {
            log::warn!("Failed to read profiles file for selector collection: {}", err);
            crate::types::ProfilesData::default()
        }
    }
}

async fn collect_referenced_profile_selector_tags(state: &AppState) -> Vec<String> {
    let rulesets = state.rulesets.lock().await.clone();

    let mut referenced_profile_ids = std::collections::HashSet::new();

    for rs in rulesets.iter().filter(|r| r.enabled) {
        if let Some(value) = &rs.outbound_value {
            if matches!(rs.outbound_mode.as_str(), "profile" | "配置") {
                referenced_profile_ids.insert(value.clone());
            }
        }
    }

    if referenced_profile_ids.is_empty() {
        return Vec::new();
    }

    let profiles_data = load_profiles_data_from_file(state).await;
    let profile_name_map: std::collections::HashMap<String, String> = profiles_data
        .profiles
        .iter()
        .map(|p| (p.id.clone(), p.name.clone()))
        .collect();

    let mut selector_tags: Vec<String> = referenced_profile_ids
        .into_iter()
        .filter_map(|profile_id| profile_name_map.get(&profile_id).map(|name| format!("P:{}", name)))
        .collect();

    selector_tags.sort();
    selector_tags.dedup();
    selector_tags
}

async fn switch_selector_to_node(
    client: &reqwest::Client,
    selector_tag: &str,
    node_tag: &str,
) {
    if let Err(err) = client
        .put(format!("http://127.0.0.1:9090/proxies/{}", urlencoding::encode(selector_tag)))
        .json(&serde_json::json!({ "name": node_tag }))
        .send()
        .await
    {
        log::warn!(
            "Failed to switch selector '{}' to node '{}': {}",
            selector_tag,
            node_tag,
            err
        );
    }
}

async fn test_selector_latency_internal(
    app: &AppHandle,
    selector_tag: String,
    test_url: Option<String>,
) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let test_url = test_url.unwrap_or_else(|| "https://www.gstatic.com/generate_204".to_string());

    let resp = client
        .get(format!("http://127.0.0.1:9090/proxies/{}", urlencoding::encode(&selector_tag)))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Failed to get selector info: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Selector '{}' not found", selector_tag));
    }

    let selector_info: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let node_tags: Vec<String> = selector_info
        .get("all")
        .and_then(|a| a.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();

    if node_tags.is_empty() {
        return Ok(serde_json::json!({ "success": true, "message": "No nodes to test" }));
    }

    log::info!("Testing {} nodes for selector '{}'", node_tags.len(), selector_tag);

    let mut futures = FuturesUnordered::new();
    for tag in node_tags.clone() {
        let client = client.clone();
        let test_url = test_url.clone();
        futures.push(async move {
            let result = client
                .get(format!("http://127.0.0.1:9090/proxies/{}/delay", urlencoding::encode(&tag)))
                .query(&[("url", &test_url), ("timeout", &"5000".to_string())])
                .timeout(std::time::Duration::from_secs(6))
                .send()
                .await;

            let delay = match result {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(data) = resp.json::<serde_json::Value>().await {
                        match data.get("delay").and_then(|d| d.as_u64()) {
                            Some(value) if value > 0 => Some(value as u32),
                            _ => None,
                        }
                    } else {
                        None
                    }
                }
                _ => None,
            };

            (tag, delay)
        });
    }

    let mut results: Vec<(String, Option<u32>)> = Vec::new();
    let mut first_switch_done = false;
    let mut best_node: Option<(String, u32)> = None;
    let mut valid_count: usize = 0;
    let first_switch_threshold = std::cmp::min(5usize, node_tags.len());

    while let Some((tag, delay)) = futures.next().await {
        results.push((tag.clone(), delay));

        if let Some(d) = delay {
            valid_count += 1;
            match &best_node {
                None => best_node = Some((tag.clone(), d)),
                Some((_, best_delay)) if d < *best_delay => best_node = Some((tag.clone(), d)),
                _ => {}
            }
        }

        if !first_switch_done && valid_count >= first_switch_threshold {
            if let Some((best_tag, best_delay)) = &best_node {
                log::info!("First phase done, switching '{}' to '{}' ({}ms)", selector_tag, best_tag, best_delay);
                switch_selector_to_node(&client, &selector_tag, best_tag).await;
                let _ = app.emit(
                    "singbox:selector-switch",
                    serde_json::json!({
                        "selector": selector_tag,
                        "node": best_tag,
                        "delay": best_delay,
                        "stage": "first"
                    }),
                );
            }
            first_switch_done = true;
        }
    }

    if let Some((best_tag, best_delay)) = &best_node {
        log::info!("Final switch '{}' to '{}' ({}ms)", selector_tag, best_tag, best_delay);
        switch_selector_to_node(&client, &selector_tag, best_tag).await;
        let _ = app.emit(
            "singbox:selector-switch",
            serde_json::json!({
                "selector": selector_tag,
                "node": best_tag,
                "delay": best_delay,
                "stage": "final"
            }),
        );
    }

    let tested_count = results.iter().filter(|(_, d)| d.is_some()).count();
    let timeout_count = results.iter().filter(|(_, d)| d.is_none()).count();

    Ok(serde_json::json!({
        "success": true,
        "selector": selector_tag,
        "total": results.len(),
        "tested": tested_count,
        "timeout": timeout_count,
        "bestNode": best_node.as_ref().map(|(t, _)| t),
        "bestDelay": best_node.as_ref().map(|(_, d)| d)
    }))
}

/// 测试指定 selector 的所有节点延迟，并智能切换
/// 逻辑：前5个有效结果（不足5则按总节点数）时选最低延迟，全部测完再选一次
#[tauri::command]
pub async fn singbox_test_selector_latency(
    app: AppHandle,
    selector_tag: String,
    test_url: Option<String>
) -> Result<serde_json::Value, String> {
    test_selector_latency_internal(&app, selector_tag, test_url).await
}
