use tauri::{AppHandle, Emitter, Manager, State};
use std::fs;
use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::fs::OpenOptions;
use std::io::Write;
use futures_util::stream::{FuturesUnordered, StreamExt};
use tokio::process::Command;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::sync::CancellationToken;
use crate::state::AppState;
use crate::types::{AppSettings, CommandResult, ProxyState, TrafficStats};

#[cfg(windows)]
fn decode_windows_output(bytes: &[u8]) -> String {
    let (decoded, _, had_errors) = encoding_rs::GBK.decode(bytes);
    let text = decoded.trim().to_string();
    if !had_errors && !text.is_empty() {
        return text;
    }
    String::from_utf8_lossy(bytes).trim().to_string()
}

#[cfg(windows)]
fn append_startup_diagnostic(state: &AppState, message: &str) {
    let _ = fs::create_dir_all(&state.data_dir);
    let path = state.data_dir.join("startup-diagnostics.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{}] {}", ts, message);
    }
}

#[cfg(windows)]
fn extract_singbox_fatal_error(line: &str) -> Option<String> {
    let stripped = line.replace('\u{1b}', "");
    if let Some(index) = stripped.find("FATAL") {
        let detail = stripped[index + "FATAL".len()..].trim();
        if !detail.is_empty() {
            return Some(detail.to_string());
        }
    }
    None
}

#[cfg(windows)]
fn format_startup_failure_message(detail: Option<String>) -> String {
    detail
        .filter(|text| !text.trim().is_empty())
        .map(|text| format!("内核启动失败: {}", text))
        .unwrap_or_else(|| "主核心 Clash API 启动超时，请重试。".to_string())
}

#[cfg(windows)]
#[allow(unused_imports)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const SELECTOR_LATENCY_CONCURRENCY_LIMIT: usize = 8;
const DEFAULT_CLASH_API_PORT: u16 = 9090;
const KUNBOX_TUN_ALIAS: &str = "kunbox-tun";
const PLUGIN_BRIDGES_FILE: &str = "plugin-bridges.json";
const XRAY_PLUGIN_FILENAME: &str = "xray.exe";

#[cfg(windows)]
#[derive(Debug, serde::Deserialize)]
struct NetAdapterRecord {
    #[serde(rename = "InterfaceAlias")]
    interface_alias: Option<String>,
    #[serde(rename = "InterfaceDescription")]
    interface_description: Option<String>,
}

#[cfg(windows)]
fn parse_foreign_wintun_aliases(json: &str, own_alias: &str) -> Vec<String> {
    if json.trim().is_empty() {
        return Vec::new();
    }

    let value: serde_json::Value = match serde_json::from_str(json) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };

    let records: Vec<NetAdapterRecord> = match value {
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter_map(|item| serde_json::from_value::<NetAdapterRecord>(item).ok())
            .collect(),
        serde_json::Value::Object(_) => serde_json::from_value::<NetAdapterRecord>(value)
            .map(|record| vec![record])
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    records
        .into_iter()
        .filter_map(|record| {
            let alias = record.interface_alias?.trim().to_string();
            if alias.is_empty() || alias.eq_ignore_ascii_case(own_alias) {
                return None;
            }
            let description = record.interface_description.unwrap_or_default();
            description
                .to_ascii_lowercase()
                .contains("wintun")
                .then_some(alias)
        })
        .collect()
}

#[cfg(windows)]
async fn detect_foreign_wintun_aliases() -> Result<Vec<String>, String> {
    let script = format!(
        "Get-NetAdapter | Where-Object {{ $_.Status -eq 'Up' -and $_.InterfaceDescription -like '*Wintun*' -and $_.InterfaceAlias -ne '{alias}' }} | Select-Object InterfaceAlias, InterfaceDescription | ConvertTo-Json -Compress",
        alias = KUNBOX_TUN_ALIAS,
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = decode_windows_output(&output.stdout);
    Ok(parse_foreign_wintun_aliases(&stdout, KUNBOX_TUN_ALIAS))
}

#[cfg(windows)]
async fn kill_stray_singbox_processes(state: &AppState) -> Result<(), String> {
    append_startup_diagnostic(state, "startup cleanup: killing stray sing-box.exe processes");

    let output = Command::new("taskkill")
        .args(["/F", "/T", "/IM", "sing-box.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        append_startup_diagnostic(state, "startup cleanup: stray sing-box.exe processes terminated");
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        return Ok(());
    }

    let stdout = decode_windows_output(&output.stdout);
    let stderr = decode_windows_output(&output.stderr);
    let combined = if !stderr.is_empty() { stderr } else { stdout };
    let lower = combined.to_lowercase();

    if lower.contains("not found") || lower.contains("没有运行的任务") || lower.contains("没有找到") {
        append_startup_diagnostic(state, "startup cleanup: no stray sing-box.exe process found");
        return Ok(());
    }

    append_startup_diagnostic(state, &format!("startup cleanup: taskkill failed: {}", combined));
    Err(format!("清理残留 sing-box 进程失败: {}", combined))
}

fn inbound_listen_addr(settings: &AppSettings) -> &'static str {
    if settings.allow_lan { "0.0.0.0" } else { "127.0.0.1" }
}

fn reserve_tcp_port(listen_addr: &str, port: u16) -> Result<std::net::TcpListener, std::io::Error> {
    std::net::TcpListener::bind((listen_addr, port))
}

async fn reserve_available_tcp_port_avoiding(
    listen_addr: &str,
    avoid_ports: &[u16],
) -> Result<(u16, std::net::TcpListener), String> {
    for _ in 0..64 {
        let listener = std::net::TcpListener::bind((listen_addr, 0)).map_err(|e| e.to_string())?;
        let port = listener.local_addr().map_err(|e| e.to_string())?.port();
        if !avoid_ports.contains(&port) {
            return Ok((port, listener));
        }
    }
    Err("无法分配可用本地端口".to_string())
}

async fn find_available_tcp_port_avoiding(listen_addr: &str, avoid_ports: &[u16]) -> Result<u16, String> {
    let (port, listener) = reserve_available_tcp_port_avoiding(listen_addr, avoid_ports).await?;
    drop(listener);
    Ok(port)
}

async fn find_available_tcp_port() -> Result<u16, String> {
    find_available_tcp_port_avoiding("127.0.0.1", &[]).await
}

async fn resolve_available_inbound_ports(
    state: &AppState,
    settings: &mut AppSettings,
) -> Result<(bool, Vec<std::net::TcpListener>), String> {
    let listen_addr = inbound_listen_addr(settings);
    let mut changed = false;
    let mut reservations = Vec::new();

    match reserve_tcp_port(listen_addr, settings.local_port) {
        Ok(listener) => reservations.push(listener),
        Err(err) => {
            let old_port = settings.local_port;
            let (fallback, listener) = reserve_available_tcp_port_avoiding(listen_addr, &[settings.socks_port]).await?;
            append_startup_diagnostic(state, &format!(
                "mixed-in port {} unavailable ({}), using fallback port {}",
                old_port, err, fallback
            ));
            settings.local_port = fallback;
            reservations.push(listener);
            changed = true;
        }
    }

    let socks_reservation = if settings.socks_port == settings.local_port {
        Err("same as mixed-in port".to_string())
    } else {
        reserve_tcp_port(listen_addr, settings.socks_port).map_err(|err| err.to_string())
    };

    match socks_reservation {
        Ok(listener) => reservations.push(listener),
        Err(err) => {
            let old_port = settings.socks_port;
            let (fallback, listener) = reserve_available_tcp_port_avoiding(listen_addr, &[settings.local_port]).await?;
            append_startup_diagnostic(state, &format!(
                "socks-in port {} unavailable ({}), using fallback port {}",
                old_port, err, fallback
            ));
            settings.socks_port = fallback;
            reservations.push(listener);
            changed = true;
        }
    }

    Ok((changed, reservations))
}

fn write_settings_file(state: &AppState, settings: &AppSettings) -> Result<(), String> {
    fs::create_dir_all(&state.data_dir).map_err(|e| e.to_string())?;
    let content = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(state.settings_file(), content).map_err(|e| e.to_string())
}

async fn allocate_clash_api_port(state: &AppState) -> Result<u16, String> {
    let default_port_available = std::net::TcpListener::bind(("127.0.0.1", DEFAULT_CLASH_API_PORT)).is_ok();
    let port = if !default_port_available || crate::commands::profiles::check_clash_api_running(DEFAULT_CLASH_API_PORT).await {
        find_available_tcp_port().await?
    } else {
        DEFAULT_CLASH_API_PORT
    };
    *state.clash_api_port.lock().await = port;
    Ok(port)
}

async fn get_clash_api_port(state: &AppState) -> u16 {
    *state.clash_api_port.lock().await
}

fn build_foreign_wintun_warning(aliases: &[String]) -> String {
    format!(
        "检测到外部隧道适配器仍在运行：{}。KunBox 已自动切换为本次非 TUN 本地代理模式，已保存设置不会被修改。",
        aliases.join(", ")
    )
}

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
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
    append_startup_diagnostic(state, "singbox_start invoked");
    let singbox_path = get_singbox_path(&app)?;
    append_startup_diagnostic(state, &format!("resolved sing-box path: {:?}", singbox_path));

    #[cfg(windows)]
    {
        append_startup_diagnostic(state, "checking for stale proxy configuration before startup");
        repair_stale_proxy_if_needed(state).await?;
        append_startup_diagnostic(state, "stale proxy repair check completed");
    }
    
    if !singbox_path.exists() {
        append_startup_diagnostic(state, "sing-box kernel missing on startup");
        return Ok(CommandResult::err("未检测到 sing-box 内核，请先到【设置 → 内核】下载并安装后再启动 VPN。"));
    }

    // Check if TUN mode is enabled and admin rights are required
    let settings = state.settings.lock().await.clone();
    let mut effective_settings = settings.clone();
    let mut startup_warning: Option<String> = None;
    append_startup_diagnostic(
        state,
        &format!(
            "startup connect settings: tun_enabled={}, system_proxy={}, local_port={}",
            settings.tun_enabled,
            settings.system_proxy,
            settings.local_port,
        ),
    );
    if settings.tun_enabled && !is_running_as_admin() {
        append_startup_diagnostic(state, "startup blocked because TUN mode requires admin privileges");
        return Ok(CommandResult::err("TUN 模式需要管理员权限。请右键点击应用图标，选择「以管理员身份运行」后重试。"));
    }

    #[cfg(windows)]
    if settings.tun_enabled {
        let foreign_wintun_aliases = detect_foreign_wintun_aliases().await?;
        if !foreign_wintun_aliases.is_empty() {
            let warning = build_foreign_wintun_warning(&foreign_wintun_aliases);
            effective_settings.tun_enabled = false;
            effective_settings.system_proxy = true;
            startup_warning = Some(warning.clone());
            append_startup_diagnostic(
                state,
                &format!("startup degraded to non-TUN mode because foreign wintun adapters are active: {}", foreign_wintun_aliases.join(", ")),
            );
            let _ = app.emit("singbox:log", serde_json::json!({
                "timestamp": chrono::Utc::now().timestamp_millis(),
                "level": "warn",
                "tag": "sing-box",
                "message": warning,
            }));
        }
    }

    if let Some(mut child) = state.singbox_process.lock().await.take() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    stop_plugin_bridges(state).await;

    crate::commands::profiles::cleanup_temp_singbox(state).await;

    #[cfg(windows)]
    kill_stray_singbox_processes(state).await?;

    let (ports_changed, inbound_port_reservations) = resolve_available_inbound_ports(state, &mut effective_settings).await?;
    if ports_changed {
        let persisted_settings = {
            let locked = state.settings.lock().await;
            let mut next = locked.clone();
            next.local_port = effective_settings.local_port;
            next.socks_port = effective_settings.socks_port;
            next
        };
        write_settings_file(state, &persisted_settings)?;
        *state.settings.lock().await = persisted_settings;
        let port_warning = format!(
            "本地代理端口被系统保留或占用，已自动切换为 HTTP {} / SOCKS {}。",
            effective_settings.local_port, effective_settings.socks_port
        );
        startup_warning = Some(match startup_warning {
            Some(existing) => format!("{}\n{}", existing, port_warning),
            None => port_warning,
        });
        append_startup_diagnostic(state, &format!(
            "persisted auto-selected inbound ports: mixed-in={}, socks-in={}",
            effective_settings.local_port, effective_settings.socks_port
        ));
    }

    // Generate config
    let clash_api_port = allocate_clash_api_port(state).await?;
    append_startup_diagnostic(state, &format!("selected clash api port: {}", clash_api_port));
    let config_result = generate_config_with_settings(&state, &effective_settings).await?;
    if !config_result.success {
        return Ok(config_result);
    }

    if let Err(err) = start_plugin_bridges(&app, state).await {
        stop_plugin_bridges(state).await;
        return Ok(CommandResult::err(err));
    }

    let config_path = state.config_dir.join("config.json");

    let config_path_str = config_path.to_str()
        .ok_or_else(|| "Config path contains invalid UTF-8 characters".to_string())?;

    #[cfg(windows)]
    if config_file_has_outbound_type(&config_path, "naive")?
        && !support_file_available_for_executable(&singbox_path, "libcronet.dll")
    {
        stop_plugin_bridges(state).await;
        let message = "当前配置包含 naive 节点，但未找到 libcronet.dll。请到【设置 → 内核】重新下载 sing-box 1.13+ 官方 Windows 内核，或将 libcronet.dll 放到 sing-box.exe 同目录后重试。";
        append_startup_diagnostic(state, message);
        return Ok(CommandResult::err(message));
    }

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
        stop_plugin_bridges(state).await;
        let stderr = decode_windows_output(&check_output.stderr);
        let stdout = decode_windows_output(&check_output.stdout);
        let detail = if !stderr.is_empty() { stderr } else { stdout };
        append_startup_diagnostic(state, &format!("sing-box preflight check failed: {}", detail));
        let message = if detail.is_empty() {
            "内核配置检查失败，请检查节点与DNS设置".to_string()
        } else {
            format!("内核配置检查失败: {}", detail)
        };
        *state.shutdown_in_progress.lock().await = false;
        return Ok(CommandResult::err(message));
    }

    // Update state
    *state.proxy_state.lock().await = ProxyState::Connecting;
    let _ = app.emit("singbox:state", "connecting");

    // Start sing-box process

    drop(inbound_port_reservations);

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

    #[cfg(windows)]
    write_proxy_session_marker(state, ProxySessionFlag::Active)?;
    append_startup_diagnostic(state, "sing-box process spawned successfully");

    let startup_error_message = Arc::new(tokio::sync::Mutex::new(None::<String>));

    // Capture stderr for logging
    if let Some(stderr) = child.stderr.take() {
        let app_clone = app.clone();
        let settings_ref = state.settings.clone();
        let startup_error_message = startup_error_message.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                #[cfg(windows)]
                if let Some(detail) = extract_singbox_fatal_error(&line) {
                    let mut guard = startup_error_message.lock().await;
                    if guard.is_none() {
                        *guard = Some(detail);
                    }
                }
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
    let shutdown_in_progress = state.shutdown_in_progress.clone();
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
                            let shutting_down = *shutdown_in_progress.lock().await;
                            if !shutting_down && !matches!(current_state, ProxyState::Idle | ProxyState::Disconnecting) {
                                *proxy_state.lock().await = ProxyState::Error;
                                *start_time_state.lock().await = None;
                                if let Some(cancel) = traffic_cancel.lock().await.take() {
                                    cancel.cancel();
                                }
                                let _ = disable_system_proxy_for_state_on_crash(&wait_app).await;
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
                            let shutting_down = *shutdown_in_progress.lock().await;
                            if !shutting_down && !matches!(current_state, ProxyState::Idle | ProxyState::Disconnecting) {
                                *proxy_state.lock().await = ProxyState::Error;
                                *start_time_state.lock().await = None;
                                if let Some(cancel) = traffic_cancel.lock().await.take() {
                                    cancel.cancel();
                                }
                                let _ = disable_system_proxy_for_state_on_crash(&wait_app).await;
                                let _ = wait_app.emit("singbox:state", "error");
                            }
                            break;
                        }
                    }
                }
            }
        }
    });

    let mut clash_api_ready = false;
    for _ in 0..20 {
        let startup_detail = startup_error_message.lock().await.clone();
        if startup_detail.as_deref().is_some_and(|detail| !detail.trim().is_empty()) {
            if let Some(mut child) = state.singbox_process.lock().await.take() {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
        stop_plugin_bridges(state).await;
            *state.proxy_state.lock().await = ProxyState::Error;
            *state.start_time.lock().await = None;
            append_startup_diagnostic(state, &format!("fatal startup error detected before Clash API ready: {}", startup_detail.clone().unwrap_or_default()));
            let _ = app.emit("singbox:state", "error");
            return Ok(CommandResult::err(format_startup_failure_message(startup_detail)));
        }

        if crate::commands::profiles::check_clash_api_running(clash_api_port).await {
            clash_api_ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }

    if !clash_api_ready {
        if let Some(mut child) = state.singbox_process.lock().await.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
            stop_plugin_bridges(state).await;
        *state.proxy_state.lock().await = ProxyState::Error;
        *state.start_time.lock().await = None;
        append_startup_diagnostic(state, "main Clash API not ready in time");
        let _ = app.emit("singbox:state", "error");
        let startup_detail = startup_error_message.lock().await.clone();
        let message = format_startup_failure_message(startup_detail);
        return Ok(CommandResult::err(message));
    }

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
        start_traffic_polling(app_for_traffic, clash_api_port, traffic_stats, start_time_val, cancel_token).await;
    });

    // Enable system proxy
    if effective_settings.system_proxy {
        append_startup_diagnostic(state, &format!("enabling system proxy on port {}", effective_settings.local_port));
        let _ = enable_system_proxy_for_state(state, effective_settings.local_port).await;
    } else {
        append_startup_diagnostic(state, "system proxy disabled in settings, skipping enable step");
    }

    append_startup_diagnostic(state, "singbox_start finished successfully");
    Ok(match startup_warning {
        Some(warning) => CommandResult::ok_with_warning(warning),
        None => CommandResult::ok(),
    })
}

pub(crate) async fn singbox_stop_impl(app: AppHandle, state: &AppState) -> Result<CommandResult, String> {
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
    append_startup_diagnostic(state, "singbox_stop invoked");
    *state.shutdown_in_progress.lock().await = true;

    // Cancel traffic polling
    if let Some(cancel) = state.traffic_cancel.lock().await.take() {
        cancel.cancel();
    }
    
    *state.proxy_state.lock().await = ProxyState::Disconnecting;
    let _ = app.emit("singbox:state", "disconnecting");

    // Kill process from state
    if let Some(mut child) = state.singbox_process.lock().await.take() {
        match child.try_wait() {
            Ok(Some(status)) => {
                append_startup_diagnostic(state, &format!("singbox_stop: child already exited with status {}", status));
            }
            Ok(None) => {
                child.kill().await.map_err(|e| {
                    append_startup_diagnostic(state, &format!("singbox_stop: kill failed: {}", e));
                    e.to_string()
                })?;
                child.wait().await.map_err(|e| {
                    append_startup_diagnostic(state, &format!("singbox_stop: wait failed: {}", e));
                    e.to_string()
                })?;
                append_startup_diagnostic(state, "singbox_stop: child killed and waited successfully");
            }
            Err(e) => {
                append_startup_diagnostic(state, &format!("singbox_stop: try_wait failed: {}", e));
                return Err(e.to_string());
            }
        }
    } else {
        append_startup_diagnostic(state, "singbox_stop: no managed child handle present");
    }
    stop_plugin_bridges(state).await;

    // Managed process has already been stopped above if present

    // Disable system proxy
    #[cfg(windows)]
    let _ = write_proxy_session_marker(state, ProxySessionFlag::Cleaning);

    let cleanup_result = disable_system_proxy_for_state(state, ProxyCleanupMode::RestoreSnapshot).await;
    if cleanup_result.is_err() {
        let _ = disable_system_proxy_for_state(state, ProxyCleanupMode::ForceClear).await;
    }

    #[cfg(windows)]
    let _ = clear_proxy_session_marker(state);

    *state.proxy_state.lock().await = ProxyState::Idle;
    *state.start_time.lock().await = None;

    crate::commands::profiles::cleanup_temp_singbox(state).await;

    *state.shutdown_in_progress.lock().await = false;
    let _ = app.emit("singbox:state", "idle");
    append_startup_diagnostic(state, "singbox_stop finished successfully");

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
pub async fn singbox_switch_node(app: AppHandle, state: State<'_, AppState>, node_tag: String) -> Result<CommandResult, String> {
    let mut profiles_data = load_profiles_data_from_file(&state).await;
    let active_profile_id = match profiles_data.active_profile_id.clone() {
        Some(id) => id,
        None => return Ok(CommandResult::err("No active profile")),
    };
    let previous_active_node_tag = profiles_data.active_node_tag.clone();

    profiles_data.active_node_tag = Some(node_tag.clone());
    let profiles_content = serde_json::to_string_pretty(&profiles_data).map_err(|e| e.to_string())?;
    fs::write(state.profiles_file(), profiles_content).map_err(|e| e.to_string())?;
    *state.profiles_data.lock().await = profiles_data;

    let proxy_state = state.proxy_state.lock().await.clone();
    if !matches!(proxy_state, ProxyState::Connected) {
        return Ok(CommandResult::ok());
    }

    let nodes_file = profile_nodes_path(&state, &active_profile_id)?;
    let raw_nodes: Vec<serde_json::Value> = if nodes_file.exists() {
        let content = fs::read_to_string(&nodes_file).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())?
    } else {
        Vec::new()
    };

    let previous_signature = node_bootstrap_signature(active_or_first_node(&raw_nodes, previous_active_node_tag.as_deref()));
    let target_signature = node_bootstrap_signature(active_or_first_node(&raw_nodes, Some(&node_tag)));

    if previous_signature != target_signature {
        return singbox_restart(app, state).await;
    }

    let client = reqwest::Client::new();
    let res = client
        .put(format!("http://127.0.0.1:{}/proxies/PROXY", get_clash_api_port(&state).await))
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
    disable_system_proxy_internal(ProxyCleanupMode::RestoreSnapshot).await?;
    Ok(CommandResult::ok())
}

/// 判断节点类型是否是代理类型
pub(crate) fn is_proxy_type(node_type: &str) -> bool {
    matches!(node_type,
        "shadowsocks" | "vmess" | "vless" | "trojan" |
        "hysteria" | "hysteria2" | "tuic" | "anytls" |
        "http" | "socks" | "wireguard" | "ssh" | "shadowtls" |
        "naive"
    )
}

fn sanitize_naive_tls(obj: &mut serde_json::Map<String, serde_json::Value>, server: &str) {
    let mut tls = obj
        .remove("tls")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();

    let allowed_keys = ["enabled", "server_name", "certificate", "certificate_path", "ech"];
    tls.retain(|key, _| allowed_keys.contains(&key.as_str()));
    tls.insert("enabled".to_string(), serde_json::Value::Bool(true));

    if !server.is_empty() && !tls.contains_key("server_name") {
        tls.insert("server_name".to_string(), serde_json::Value::String(server.to_string()));
    }

    obj.insert("tls".to_string(), serde_json::Value::Object(tls));
}

fn sanitize_naive_outbound(obj: &mut serde_json::Map<String, serde_json::Value>, server: &str) {
    for key in [
        "network",
        "transport",
        "ws-opts",
        "grpc-opts",
        "h2-opts",
        "http-opts",
        "skip-cert-verify",
        "servername",
        "sni",
        "alpn",
        "client-fingerprint",
        "reality-opts",
        "method",
        "security",
        "packet_encoding",
        "flow",
        "uuid",
        "alter_id",
    ] {
        obj.remove(key);
    }
    sanitize_naive_tls(obj, server);
    obj.entry("quic".to_string())
        .or_insert_with(|| serde_json::Value::Bool(false));
}

fn config_value_has_outbound_type(config: &serde_json::Value, outbound_type: &str) -> bool {
    config
        .get("outbounds")
        .and_then(|value| value.as_array())
        .is_some_and(|outbounds| {
            outbounds.iter().any(|outbound| {
                outbound
                    .get("type")
                    .and_then(|value| value.as_str())
                    .is_some_and(|value| value.eq_ignore_ascii_case(outbound_type))
            })
        })
}

fn config_file_has_outbound_type(config_path: &std::path::Path, outbound_type: &str) -> Result<bool, String> {
    let content = fs::read_to_string(config_path).map_err(|e| e.to_string())?;
    let config: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(config_value_has_outbound_type(&config, outbound_type))
}

#[cfg(windows)]
fn support_file_available_for_executable(executable_path: &std::path::Path, filename: &str) -> bool {
    if executable_path
        .parent()
        .is_some_and(|dir| dir.join(filename).exists())
    {
        return true;
    }

    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|path| path.join(filename).exists()))
        .unwrap_or(false)
}

/// 处理节点配置，确保格式正确
fn process_node(node: &serde_json::Value) -> serde_json::Value {
    let mut node = node.clone();
    if let Some(obj) = node.as_object_mut() {
        let node_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
        let server = obj.get("server").and_then(|s| s.as_str()).unwrap_or("").to_string();
        let port = obj.get("server_port").and_then(|p| p.as_u64()).unwrap_or(0);

        obj.remove(crate::commands::profiles::ECH_DNS_SERVER_META_KEY);

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

        if node_type == "naive" {
            sanitize_naive_outbound(obj, &server);
        }
    }
    node
}

fn is_xray_bridge_node(node: &serde_json::Value) -> bool {
    node.get("type").and_then(|value| value.as_str()) == Some("vless")
        && node
            .get("transport")
            .and_then(|value| value.get("type"))
            .and_then(|value| value.as_str())
            .is_some_and(|transport_type| transport_type.eq_ignore_ascii_case("xhttp"))
}

fn plugin_bridge_port(index: usize) -> u16 {
    18080 + index as u16
}

fn plugin_bridge_path(state: &AppState) -> PathBuf {
    state.config_dir.join(PLUGIN_BRIDGES_FILE)
}

pub(crate) fn node_for_singbox_with_plugin_bridge(
    node: &serde_json::Value,
    bridge_specs: &mut Vec<serde_json::Value>,
) -> serde_json::Value {
    let processed = process_node(node);
    if !is_xray_bridge_node(&processed) {
        return processed;
    }

    let tag = processed
        .get("tag")
        .and_then(|value| value.as_str())
        .unwrap_or("xray-plugin");
    let port = plugin_bridge_port(bridge_specs.len());

    bridge_specs.push(serde_json::json!({
        "core": "xray",
        "tag": tag,
        "listen": "127.0.0.1",
        "port": port,
        "node": processed
    }));

    serde_json::json!({
        "type": "socks",
        "tag": tag,
        "server": "127.0.0.1",
        "server_port": port,
        "version": "5"
    })
}

pub(crate) fn xray_plugin_path(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let data_plugin = app_data_dir.join("libs").join(XRAY_PLUGIN_FILENAME);
    if data_plugin.exists() {
        return Ok(data_plugin);
    }

    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    Ok(resource_dir.join("resources").join("libs").join(XRAY_PLUGIN_FILENAME))
}

fn xray_tls_settings(tls: &serde_json::Value) -> serde_json::Value {
    let mut settings = serde_json::Map::new();

    if let Some(server_name) = tls.get("server_name").and_then(|value| value.as_str()) {
        settings.insert("serverName".to_string(), serde_json::Value::String(server_name.to_string()));
    }
    if let Some(insecure) = tls.get("insecure").and_then(|value| value.as_bool()) {
        settings.insert("allowInsecure".to_string(), serde_json::Value::Bool(insecure));
    }
    if let Some(alpn) = tls.get("alpn").and_then(|value| value.as_array()) {
        settings.insert("alpn".to_string(), serde_json::Value::Array(alpn.clone()));
    }
    if let Some(fingerprint) = tls
        .get("utls")
        .and_then(|value| value.get("fingerprint"))
        .and_then(|value| value.as_str())
    {
        settings.insert("fingerprint".to_string(), serde_json::Value::String(fingerprint.to_string()));
    }

    if let Some(reality) = tls.get("reality").and_then(|value| value.as_object()) {
        if let Some(public_key) = reality.get("public_key").and_then(|value| value.as_str()) {
            settings.insert("publicKey".to_string(), serde_json::Value::String(public_key.to_string()));
        }
        if let Some(short_id) = reality.get("short_id").and_then(|value| value.as_str()) {
            settings.insert("shortId".to_string(), serde_json::Value::String(short_id.to_string()));
        }
    }

    serde_json::Value::Object(settings)
}

fn vless_encryption(node: &serde_json::Value) -> &str {
    node
        .get("encryption")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            node.get("transport")
                .and_then(|value| value.get("extra"))
                .and_then(|value| value.get("encryption"))
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or("none")
}

fn xray_stream_settings(node: &serde_json::Value) -> serde_json::Value {
    let mut stream = serde_json::Map::new();

    let transport = node.get("transport").and_then(|value| value.as_object());
    let network = transport
        .and_then(|value| value.get("type"))
        .and_then(|value| value.as_str())
        .unwrap_or("tcp");
    stream.insert("network".to_string(), serde_json::Value::String(network.to_string()));

    if network.eq_ignore_ascii_case("xhttp") {
        let mut xhttp = serde_json::Map::new();
        if let Some(transport) = transport {
            for (key, value) in transport {
                if key == "type" {
                    continue;
                }
                if key == "extra" {
                    if let Some(extra) = value.as_object() {
                        let mut cleaned = extra.clone();
                        cleaned.remove("encryption");
                        if !cleaned.is_empty() {
                            xhttp.insert(key.clone(), serde_json::Value::Object(cleaned));
                        }
                    } else {
                        xhttp.insert(key.clone(), value.clone());
                    }
                } else {
                    xhttp.insert(key.clone(), value.clone());
                }
            }
        }
        stream.insert("xhttpSettings".to_string(), serde_json::Value::Object(xhttp));
    }

    if let Some(tls) = node.get("tls").filter(|value| {
        value.get("enabled").and_then(|enabled| enabled.as_bool()).unwrap_or(false)
    }) {
        let security = if tls
            .get("reality")
            .and_then(|value| value.get("enabled"))
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        {
            "reality"
        } else {
            "tls"
        };
        stream.insert("security".to_string(), serde_json::Value::String(security.to_string()));
        stream.insert(format!("{}Settings", security), xray_tls_settings(tls));
    } else {
        stream.insert("security".to_string(), serde_json::Value::String("none".to_string()));
    }

    serde_json::Value::Object(stream)
}

pub(crate) fn build_xray_plugin_config(node: &serde_json::Value, port: u16) -> Result<serde_json::Value, String> {
    let server = node.get("server").and_then(|value| value.as_str()).ok_or("Xray plugin node missing server")?;
    let server_port = node.get("server_port").and_then(|value| value.as_u64()).ok_or("Xray plugin node missing server_port")?;
    let uuid = node.get("uuid").and_then(|value| value.as_str()).ok_or("Xray plugin node missing uuid")?;

    let encryption = vless_encryption(node);
    let mut user = serde_json::json!({
        "id": uuid,
        "encryption": encryption
    });
    if let Some(flow) = node.get("flow").and_then(|value| value.as_str()).filter(|value| !value.is_empty()) {
        user["flow"] = serde_json::Value::String(flow.to_string());
    }

    Ok(serde_json::json!({
        "log": {
            "loglevel": "warning"
        },
        "inbounds": [
            {
                "listen": "127.0.0.1",
                "port": port,
                "protocol": "socks",
                "settings": {
                    "udp": true,
                    "auth": "noauth"
                }
            }
        ],
        "outbounds": [
            {
                "protocol": "vless",
                "settings": {
                    "vnext": [
                        {
                            "address": server,
                            "port": server_port,
                            "users": [user]
                        }
                    ]
                },
                "streamSettings": xray_stream_settings(node)
            }
        ]
    }))
}

async fn stop_plugin_bridges(state: &AppState) {
    let mut processes = state.plugin_processes.lock().await;
    for mut child in processes.drain(..) {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}

async fn start_plugin_bridges(app: &AppHandle, state: &AppState) -> Result<(), String> {
    stop_plugin_bridges(state).await;

    let bridge_path = plugin_bridge_path(state);
    if !bridge_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&bridge_path).map_err(|e| e.to_string())?;
    let specs: Vec<serde_json::Value> = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    if specs.is_empty() {
        return Ok(());
    }

    let xray_path = xray_plugin_path(app)?;
    if !xray_path.exists() {
        return Err("检测到 xhttp 节点，但未找到 Xray 插件核心。请将 xray.exe 放到应用数据目录的 libs 目录。".to_string());
    }

    let mut started = Vec::new();
    for (index, spec) in specs.iter().enumerate() {
        let core = spec.get("core").and_then(|value| value.as_str()).unwrap_or("");
        if core != "xray" {
            continue;
        }

        let port = spec.get("port").and_then(|value| value.as_u64()).ok_or("Plugin bridge missing port")? as u16;
        let node = spec.get("node").ok_or("Plugin bridge missing node")?;
        let config = build_xray_plugin_config(node, port)?;
        let config_path = state.config_dir.join(format!("plugin-xray-{}.json", index));
        let config_str = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        fs::write(&config_path, config_str).map_err(|e| e.to_string())?;
        let config_path_str = config_path.to_str().ok_or("Xray plugin config path contains invalid UTF-8")?;

        #[cfg(windows)]
        let child = Command::new(&xray_path)
            .args(["run", "-config", config_path_str])
            .current_dir(&state.config_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| e.to_string())?;

        #[cfg(not(windows))]
        let child = Command::new(&xray_path)
            .args(["run", "-config", config_path_str])
            .current_dir(&state.config_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| e.to_string())?;

        started.push(child);
    }

    *state.plugin_processes.lock().await = started;
    Ok(())
}

pub(crate) fn build_dns_server(address: &str, tag: &str, detour: &str) -> serde_json::Value {
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

pub(crate) fn build_dns_server_with_resolver(
    address: &str,
    tag: &str,
    detour: &str,
    domain_resolver: Option<&str>,
) -> serde_json::Value {
    let mut server_obj = build_dns_server(address, tag, detour);
    if let Some(resolver) = domain_resolver {
        server_obj["domain_resolver"] = serde_json::Value::String(resolver.to_string());
    }
    server_obj
}

fn node_has_ech(node: &serde_json::Value) -> bool {
    let Some(ech) = node.get("tls").and_then(|value| value.get("ech")) else {
        return false;
    };

    ech.get("enabled").and_then(|value| value.as_bool()) == Some(true)
        || ech.get("query_server_name").is_some()
        || ech.get("config").is_some()
}

fn node_needs_ech_subscription_repair(node: &serde_json::Value) -> bool {
    let Some(ech) = node.get("tls").and_then(|value| value.get("ech")) else {
        return false;
    };

    if node
        .get(crate::commands::profiles::ECH_DNS_SERVER_META_KEY)
        .and_then(|value| value.as_str())
        .is_some()
    {
        return false;
    }

    node_has_ech(node) && ech.get("config").is_none()
}

fn extract_ech_dns_server_override(
    raw_nodes: &[serde_json::Value],
    active_node_tag: Option<&str>,
) -> Option<String> {
    if let Some(active_tag) = active_node_tag {
        if let Some(value) = raw_nodes
            .iter()
            .find(|node| node.get("tag").and_then(|value| value.as_str()) == Some(active_tag))
            .and_then(|node| node.get(crate::commands::profiles::ECH_DNS_SERVER_META_KEY))
            .and_then(|value| value.as_str())
        {
            return Some(value.to_string());
        }
    }

    let mut values = raw_nodes
        .iter()
        .filter_map(|node| node.get(crate::commands::profiles::ECH_DNS_SERVER_META_KEY))
        .filter_map(|value| value.as_str())
        .collect::<Vec<_>>();
    values.sort_unstable();
    values.dedup();
    (values.len() == 1).then(|| values[0].to_string())
}

fn active_or_first_node<'a>(raw_nodes: &'a [serde_json::Value], active_node_tag: Option<&str>) -> Option<&'a serde_json::Value> {
    active_node_tag
        .and_then(|active_tag| {
            raw_nodes
                .iter()
                .find(|node| node.get("tag").and_then(|value| value.as_str()) == Some(active_tag))
        })
        .or_else(|| raw_nodes.first())
}

fn node_bootstrap_signature(node: Option<&serde_json::Value>) -> (bool, Option<String>) {
    let Some(node) = node else {
        return (false, None);
    };

    let uses_ech = node_has_ech(node);
    let resolver = node
        .get(crate::commands::profiles::ECH_DNS_SERVER_META_KEY)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    (uses_ech, resolver)
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

fn plugin_bridge_remote_direct_rule(spec: &serde_json::Value) -> Option<serde_json::Value> {
    let server = spec
        .get("node")
        .and_then(|node| node.get("server"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    if let Ok(ip) = server.parse::<std::net::IpAddr>() {
        let cidr = match ip {
            std::net::IpAddr::V4(_) => format!("{server}/32"),
            std::net::IpAddr::V6(_) => format!("{server}/128"),
        };
        return Some(serde_json::json!({
            "ip_cidr": [cidr],
            "outbound": "direct"
        }));
    }

    Some(serde_json::json!({
        "domain": [server],
        "outbound": "direct"
    }))
}

fn plugin_bridge_remote_dns_rule(spec: &serde_json::Value) -> Option<serde_json::Value> {
    let server = spec
        .get("node")
        .and_then(|node| node.get("server"))
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    if server.parse::<std::net::IpAddr>().is_ok() {
        return None;
    }

    Some(serde_json::json!({
        "domain": [server],
        "server": "dns-local"
    }))
}

fn profile_selector_tag(profile_id: &str) -> String {
    format!("P:{}", profile_id)
}

fn parse_profile_scoped_node_ref(value: &str) -> Option<(&str, &str)> {
    let (profile_id, node_tag) = value.split_once("::")?;
    if profile_id.is_empty() || node_tag.is_empty() {
        return None;
    }
    Some((profile_id, node_tag))
}

fn normalized_node_reference_tag(node_ref: &str) -> String {
    match parse_profile_scoped_node_ref(node_ref) {
        Some((profile_id, node_tag)) => format!("{}::{}", profile_id, node_tag),
        None => node_ref.to_string(),
    }
}

fn resolve_node_route_outbound(
    node_ref: &str,
    available_outbound_tags: &std::collections::HashSet<String>,
) -> Option<String> {
    let outbound_tag = normalized_node_reference_tag(node_ref);
    available_outbound_tags
        .contains(&outbound_tag)
        .then_some(outbound_tag)
}

fn with_outbound_tag(mut node: serde_json::Value, tag: &str) -> serde_json::Value {
    if let Some(obj) = node.as_object_mut() {
        obj.insert("tag".to_string(), serde_json::Value::String(tag.to_string()));
    }
    node
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

#[cfg(test)]
async fn generate_config(state: &AppState) -> Result<CommandResult, String> {
    let settings = state.settings.lock().await.clone();
    generate_config_with_settings(state, &settings).await
}

async fn generate_config_with_settings(state: &AppState, settings: &crate::types::AppSettings) -> Result<CommandResult, String> {
    // Always reload profiles data from file to ensure we have the latest
    let profiles_file = state.profiles_file();
    let profiles_data: crate::types::ProfilesData = if profiles_file.exists() {
        let content = fs::read_to_string(&profiles_file).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        return Ok(CommandResult::err("No profiles file found"));
    };
    
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
    let mut raw_nodes: Vec<serde_json::Value> = serde_json::from_str(&nodes_content).map_err(|e| e.to_string())?;

    if raw_nodes.is_empty() {
        return Ok(CommandResult::err("No nodes in active profile"));
    }

    let active_profile = profiles_data
        .profiles
        .iter()
        .find(|profile| profile.id == active_profile_id);

    if let Some(profile) = active_profile {
        let needs_ech_repair = !profile.url.trim().is_empty()
            && raw_nodes.iter().any(node_needs_ech_subscription_repair);

        if needs_ech_repair {
            match crate::commands::profiles::fetch_subscription(&profile.url).await {
                Ok(repaired_nodes) if !repaired_nodes.is_empty() => {
                    if let Ok(repaired_content) = serde_json::to_string_pretty(&repaired_nodes) {
                        if fs::write(&nodes_file, &repaired_content).is_ok() {
                            if let Ok(repaired_raw_nodes) = serde_json::from_str::<Vec<serde_json::Value>>(&repaired_content) {
                                raw_nodes = repaired_raw_nodes;
                                log::info!("Repaired legacy ECH subscription nodes for profile '{}'", profile.name);
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(err) => {
                    log::warn!("Failed to refresh legacy ECH nodes for profile '{}': {}", profile.name, err);
                }
            }
        }
    }

    // 处理当前配置的节点，并过滤 sing-box 不支持的类型，避免单个无效节点拖垮整份配置。
    let nodes: Vec<serde_json::Value> = raw_nodes
        .iter()
        .map(process_node)
        .filter(|node| {
            node.get("type")
                .and_then(|value| value.as_str())
                .is_some_and(is_proxy_type)
        })
        .collect();

    if nodes.is_empty() {
        return Ok(CommandResult::err("当前配置没有可用的受支持代理节点"));
    }

    let active_node_tag = profiles_data.active_node_tag.clone()
        .filter(|tag| {
            nodes
                .iter()
                .any(|node| node.get("tag").and_then(|value| value.as_str()) == Some(tag.as_str()))
        })
        .or_else(|| nodes.first().and_then(|n| n.get("tag").and_then(|t| t.as_str()).map(|s| s.to_string())));

    let inferred_ech_dns_server = extract_ech_dns_server_override(&raw_nodes, active_node_tag.as_deref());
    let active_node_has_ech = active_or_first_node(&raw_nodes, active_node_tag.as_deref())
        .map(node_has_ech)
        .unwrap_or(false);
    let effective_remote_dns = if active_node_has_ech {
        inferred_ech_dns_server
            .clone()
            .or_else(|| active_profile.and_then(|profile| profile.dns_server.clone()))
            .unwrap_or_else(|| settings.remote_dns.clone())
    } else {
        active_profile
            .and_then(|profile| profile.dns_server.clone())
            .unwrap_or_else(|| settings.remote_dns.clone())
    };
    let remote_dns_domain_resolver = active_node_has_ech.then_some("dns-local");

    // 加载所有配置文件信息（用于跨配置分流）
    let all_profiles = load_all_profiles(state, &profiles_data);

    // 收集规则集引用的 profile ID 和 node tag
    let enabled_rulesets: Vec<_> = rulesets.iter().filter(|r| r.enabled).collect();
    let mut referenced_profile_ids = std::collections::HashSet::new();
    let mut referenced_profile_scoped_node_refs = std::collections::HashSet::new();
    
    for rs in &enabled_rulesets {
        if let Some(ref value) = rs.outbound_value {
            match rs.outbound_mode.as_str() {
                "profile" | "配置" => { referenced_profile_ids.insert(value.clone()); }
                "node" | "节点" => {
                    if parse_profile_scoped_node_ref(value).is_some() {
                        referenced_profile_scoped_node_refs.insert(value.clone());
                    }
                }
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
                    if parse_profile_scoped_node_ref(value).is_some() {
                        referenced_profile_scoped_node_refs.insert(value.clone());
                    };
                }
                _ => {}
            }
        }
    }

    // Pre-scan for tag collisions to avoid conflict with node tags named "PROXY" or "auto"
    let will_proxy_collide = nodes.iter().any(|n| n.get("tag").and_then(|t| t.as_str()) == Some("PROXY"))
        || referenced_profile_scoped_node_refs.iter().any(|r| {
            parse_profile_scoped_node_ref(r)
                .map(|(_, node_tag)| node_tag == "PROXY")
                .unwrap_or(false)
        })
        || referenced_profile_ids.iter().any(|id| profile_selector_tag(id) == "PROXY");
    let will_auto_collide = nodes.iter().any(|n| n.get("tag").and_then(|t| t.as_str()) == Some("auto"))
        || referenced_profile_scoped_node_refs.iter().any(|r| {
            parse_profile_scoped_node_ref(r)
                .map(|(_, node_tag)| node_tag == "auto")
                .unwrap_or(false)
        })
        || referenced_profile_ids.iter().any(|id| profile_selector_tag(id) == "auto");

    let proxy_tag = if will_proxy_collide { "PROXY-kb" } else { "PROXY" };
    let auto_tag = if will_auto_collide { "auto-kb" } else { "auto" };
    let remote_dns_detour = if active_node_has_ech { "direct" } else { proxy_tag };

    // Build config - 使用 sing-box 1.11+ 新格式
    let listen_addr = if settings.allow_lan { "0.0.0.0" } else { "127.0.0.1" };
    
    let routing_mode = settings.routing_mode.as_str();

    // 构建 DNS 服务器列表（sing-box 1.12+ 新格式）
    let mut dns_servers = vec![
        build_dns_server(&settings.local_dns, "dns-local", "direct"),
        build_dns_server_with_resolver(
            &effective_remote_dns,
            "dns-remote",
            remote_dns_detour,
            remote_dns_domain_resolver,
        ),
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
        let mut fake_dns_rule = serde_json::json!({
            "query_type": ["A", "AAAA"],
            "server": "dns-fakeip"
        });
        if settings.tun_enabled {
            fake_dns_rule["inbound"] = serde_json::json!(["tun-in"]);
        }
        dns_rules.push(fake_dns_rule);
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
        "global-proxy" => proxy_tag,
        "global-direct" => "direct",
        _ => match settings.default_rule.as_str() {
            "proxy" => proxy_tag,
            "block" => "direct",
            other => other,
        },
    };

    let clash_api_port = get_clash_api_port(state).await;
    let mut config = serde_json::json!({
        "log": {
            "disabled": false,
            "level": "info",
            "timestamp": true
        },
        "experimental": {
            "clash_api": {
                "external_controller": format!("127.0.0.1:{}", clash_api_port),
                "default_mode": "rule"
            },
            "cache_file": {
                "enabled": true,
                "path": "cache.db",
                "store_rdrc": true
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
    let mut plugin_bridge_specs: Vec<serde_json::Value> = Vec::new();

    // 1. 添加当前配置的节点
    for node in &nodes {
        let node_type = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if is_proxy_type(node_type) {
            outbounds.push(node_for_singbox_with_plugin_bridge(node, &mut plugin_bridge_specs));
            if let Some(tag) = node.get("tag").and_then(|t| t.as_str()) {
                proxy_tags.push(tag.to_string());
                existing_tags.insert(tag.to_string());
            }
        }
    }

    for node_ref in &referenced_profile_scoped_node_refs {
        let Some((profile_id, node_tag)) = parse_profile_scoped_node_ref(node_ref) else {
            continue;
        };
        let outbound_tag = normalized_node_reference_tag(node_ref);
        if existing_tags.contains(&outbound_tag) {
            continue;
        }

        if let Some(profile) = all_profiles.iter().find(|p| p.id == profile_id) {
            if let Some(node) = profile.nodes.iter().find(|n| {
                n.get("tag").and_then(|t| t.as_str()) == Some(node_tag)
            }) {
                let node_type = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if is_proxy_type(node_type) {
                    let scoped_node = with_outbound_tag(process_node(node), &outbound_tag);
                    outbounds.push(node_for_singbox_with_plugin_bridge(&scoped_node, &mut plugin_bridge_specs));
                    existing_tags.insert(outbound_tag.clone());
                    log::info!(
                        "Added profile-scoped node: {} from profile {}",
                        outbound_tag,
                        profile.name
                    );
                }
            }
        }
    }

    // 3. 处理配置分流（profile 模式）- 创建 urltest selector
    let mut profile_id_to_selector = std::collections::HashMap::new();
    
    for profile_id in &referenced_profile_ids {
        if let Some(profile) = all_profiles.iter().find(|p| &p.id == profile_id) {
            let selector_tag = profile_selector_tag(&profile.id);
            if existing_tags.contains(&selector_tag) {
                continue;
            }

            // 收集该配置的所有代理节点
            let mut profile_proxy_entries: Vec<(String, String)> = Vec::new();
            for node in &profile.nodes {
                let node_type = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if is_proxy_type(node_type) {
                    if let Some(tag) = node.get("tag").and_then(|t| t.as_str()) {
                        let outbound_tag = if profile.id == active_profile_id {
                            tag.to_string()
                        } else {
                            normalized_node_reference_tag(&format!("{}::{}", profile.id, tag))
                        };

                        // 如果节点不存在，添加到 outbounds
                        if !existing_tags.contains(&outbound_tag) {
                            if outbound_tag == tag {
                                outbounds.push(node_for_singbox_with_plugin_bridge(node, &mut plugin_bridge_specs));
                            } else {
                                let scoped_node = with_outbound_tag(process_node(node), &outbound_tag);
                                outbounds.push(node_for_singbox_with_plugin_bridge(&scoped_node, &mut plugin_bridge_specs));
                            }
                            existing_tags.insert(outbound_tag.clone());
                        }
                        profile_proxy_entries.push((tag.to_string(), outbound_tag));
                    }
                }
            }
            let profile_proxy_tags: Vec<String> = profile_proxy_entries
                .iter()
                .map(|(_, outbound_tag)| outbound_tag.clone())
                .collect();

            // 创建 selector 类型（由应用层管理延迟测试和切换）
            if !profile_proxy_tags.is_empty() {
                let selector_default = profiles_data
                    .node_selections
                    .get(&profile.id)
                    .and_then(|saved_tag| {
                        profile_proxy_entries
                            .iter()
                            .find(|(raw_tag, outbound_tag)| raw_tag == saved_tag || outbound_tag == saved_tag)
                            .map(|(_, outbound_tag)| outbound_tag.clone())
                    })
                    .or_else(|| {
                        (profiles_data.active_profile_id.as_deref() == Some(profile.id.as_str()))
                            .then(|| profiles_data.active_node_tag.clone())
                            .flatten()
                            .and_then(|active_tag| {
                                profile_proxy_entries
                                    .iter()
                                    .find(|(raw_tag, outbound_tag)| raw_tag == &active_tag || outbound_tag == &active_tag)
                                    .map(|(_, outbound_tag)| outbound_tag.clone())
                            })
                    })
                    .or_else(|| profile_proxy_tags.first().cloned());

                outbounds.push(serde_json::json!({
                    "type": "selector",
                    "tag": selector_tag,
                    "outbounds": profile_proxy_tags,
                    "default": selector_default,
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
        let proxy_outbounds: Vec<String> = proxy_tags.iter().cloned().collect();
        outbounds.insert(0, serde_json::json!({
            "type": "selector",
            "tag": proxy_tag,
            "outbounds": proxy_outbounds.clone(),
            "default": default_tag,
            "interrupt_exist_connections": false
        }));
        existing_tags.insert(proxy_tag.to_string());
    }

    // 5. 添加 auto urltest（如果有多个节点）
    if proxy_tags.len() > 1 {
        outbounds.push(serde_json::json!({
            "type": "urltest",
            "tag": auto_tag,
            "outbounds": proxy_tags,
            "url": settings.latency_test_url,
            "interval": "10m",
            "idle_timeout": "30m",
            "tolerance": 50
        }));
        existing_tags.insert(auto_tag.to_string());
    }

    // 6. 添加基础出站
    outbounds.push(serde_json::json!({ "type": "direct", "tag": "direct" }));
    config["outbounds"] = serde_json::Value::Array(outbounds.clone());

    if let Some(dns_rules) = config["dns"]["rules"].as_array_mut() {
        for spec in &plugin_bridge_specs {
            if let Some(rule) = plugin_bridge_remote_dns_rule(spec) {
                dns_rules.insert(0, rule);
            }
        }
    }

    // 收集所有可用的 outbound tags
    let available_outbound_tags: std::collections::HashSet<String> = outbounds.iter()
        .filter_map(|o| o.get("tag").and_then(|t| t.as_str()).map(|s| s.to_string()))
        .collect();

    // ========== 构建路由规则 ==========
    let mut rules: Vec<serde_json::Value> = vec![
        serde_json::json!({ "inbound": "mixed-in", "action": "sniff" }),
        serde_json::json!({
            "type": "logical",
            "mode": "or",
            "rules": [
                { "protocol": "dns" },
                { "port": 53 }
            ],
            "action": "hijack-dns"
        }),
    ];

    // 预先声明规则集引用和缓存目录（广告屏蔽和用户规则集都需要）
    let mut rule_set_refs = Vec::new();

    if settings.tun_enabled {
        rules.insert(1, serde_json::json!({ "inbound": "tun-in", "action": "sniff" }));
    }

    if settings.bypass_lan {
        rules.push(serde_json::json!({ "ip_is_private": true, "outbound": "direct" }));
    }

    for spec in &plugin_bridge_specs {
        if let Some(rule) = plugin_bridge_remote_direct_rule(spec) {
            rules.push(rule);
        }
    }

    // ========== 添加自定义域名分流规则 ==========
    if routing_mode == "rule" {
        for rule in custom_rules.domain_rules.iter().filter(|r| r.enabled) {
            let outbound = match rule.outbound_mode.as_str() {
                "proxy" => proxy_tag.to_string(),
                "direct" => "direct".to_string(),
                "block" => "block".to_string(),
                "node" => {
                    if let Some(ref node_ref) = rule.outbound_value {
                        if let Some(node_tag) = resolve_node_route_outbound(node_ref, &available_outbound_tags) {
                            node_tag
                        } else {
                            log::warn!("Node '{}' not found for domain rule '{}', falling back to {}", node_ref, rule.value, proxy_tag);
                            proxy_tag.to_string()
                        }
                    } else {
                        proxy_tag.to_string()
                    }
                },
                "profile" => {
                    if let Some(ref profile_id) = rule.outbound_value {
                        if let Some(selector_tag) = profile_id_to_selector.get(profile_id) {
                            if available_outbound_tags.contains(selector_tag) {
                                selector_tag.clone()
                            } else {
                                log::warn!("Profile selector '{}' not found for domain rule '{}', falling back to {}", selector_tag, rule.value, proxy_tag);
                                proxy_tag.to_string()
                            }
                        } else {
                            log::warn!("Profile '{}' not found for domain rule '{}', falling back to {}", profile_id, rule.value, proxy_tag);
                            proxy_tag.to_string()
                        }
                    } else {
                        proxy_tag.to_string()
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
            "proxy" => proxy_tag.to_string(),
            "direct" => "direct".to_string(),
            "block" => "block".to_string(),
            // node 模式：验证节点是否存在
            "node" | "节点" => {
                if let Some(ref node_ref) = rs.outbound_value {
                    if let Some(node_tag) = resolve_node_route_outbound(node_ref, &available_outbound_tags) {
                        node_tag
                    } else {
                        log::warn!("Node '{}' not found for ruleset '{}', falling back to {}", node_ref, rs.tag, proxy_tag);
                        proxy_tag.to_string()
                    }
                } else {
                    proxy_tag.to_string()
                }
            },
            // profile 模式：使用配置的 urltest selector
            "profile" | "配置" => {
                if let Some(ref profile_id) = rs.outbound_value {
                    if let Some(selector_tag) = profile_id_to_selector.get(profile_id) {
                        if available_outbound_tags.contains(selector_tag) {
                            selector_tag.clone()
                        } else {
                            log::warn!("Profile selector '{}' not found for ruleset '{}', falling back to {}", selector_tag, rs.tag, proxy_tag);
                            proxy_tag.to_string()
                        }
                    } else {
                        log::warn!("Profile '{}' not found for ruleset '{}', falling back to {}", profile_id, rs.tag, proxy_tag);
                        proxy_tag.to_string()
                    }
                } else {
                    proxy_tag.to_string()
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
    let plugin_bridge_str = serde_json::to_string_pretty(&plugin_bridge_specs).map_err(|e| e.to_string())?;
    fs::write(plugin_bridge_path(state), plugin_bridge_str).map_err(|e| e.to_string())?;

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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedSystemProxySnapshot {
    proxy_enable: Option<String>,
    proxy_server: Option<String>,
    proxy_override: Option<String>,
    auto_config_url: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxyCleanupMode {
    RestoreSnapshot,
    ForceClear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProxySessionFlag {
    Active,
    Cleaning,
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

    let stdout = decode_windows_output(&output.stdout);
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
async fn snapshot_system_proxy_if_needed_for_state(state: &AppState) -> Result<(), String> {
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

    save_persisted_proxy_snapshot(state, &snapshot)?;

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
        return Err(decode_windows_output(&output.stderr));
    }

    Ok(())
}

#[cfg(windows)]
async fn delete_registry_value(name: &str) -> Result<(), String> {
    let output = Command::new("reg")
        .args([
            "delete",
            "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings",
            "/v",
            name,
            "/f",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = decode_windows_output(&output.stderr).to_lowercase();
    if stderr.contains("unable to find") || stderr.contains("无法找到") || stderr.contains("找不到") {
        return Ok(());
    }

    Err(decode_windows_output(&output.stderr))
}

#[cfg(windows)]
fn proxy_session_marker_path(state: &AppState) -> PathBuf {
    state.data_dir.join("proxy_session.flag")
}

#[cfg(windows)]
fn persisted_proxy_snapshot_path(state: &AppState) -> PathBuf {
    state.data_dir.join("proxy_snapshot.json")
}

#[cfg(windows)]
fn write_proxy_session_marker(state: &AppState, flag: ProxySessionFlag) -> Result<(), String> {
    let value = match flag {
        ProxySessionFlag::Active => "active",
        ProxySessionFlag::Cleaning => "cleaning",
    };
    fs::write(proxy_session_marker_path(state), value).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn clear_proxy_session_marker(state: &AppState) -> Result<(), String> {
    let path = proxy_session_marker_path(state);
    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(windows)]
fn read_proxy_session_marker(state: &AppState) -> Result<Option<String>, String> {
    let path = proxy_session_marker_path(state);
    if !path.exists() {
        return Ok(None);
    }

    let value = fs::read_to_string(path).map_err(|e| e.to_string())?;
    Ok(Some(value.trim().to_string()))
}

#[cfg(windows)]
fn save_persisted_proxy_snapshot(state: &AppState, snapshot: &SystemProxySnapshot) -> Result<(), String> {
    let persisted = PersistedSystemProxySnapshot {
        proxy_enable: snapshot.proxy_enable.clone(),
        proxy_server: snapshot.proxy_server.clone(),
        proxy_override: snapshot.proxy_override.clone(),
        auto_config_url: snapshot.auto_config_url.clone(),
    };

    let content = serde_json::to_vec(&persisted).map_err(|e| e.to_string())?;
    fs::write(persisted_proxy_snapshot_path(state), content).map_err(|e| e.to_string())
}

#[cfg(windows)]
fn load_persisted_proxy_snapshot(state: &AppState) -> Result<Option<SystemProxySnapshot>, String> {
    let path = persisted_proxy_snapshot_path(state);
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read(path).map_err(|e| e.to_string())?;
    let persisted: PersistedSystemProxySnapshot = serde_json::from_slice(&content).map_err(|e| e.to_string())?;
    Ok(Some(SystemProxySnapshot {
        proxy_enable: persisted.proxy_enable,
        proxy_server: persisted.proxy_server,
        proxy_override: persisted.proxy_override,
        auto_config_url: persisted.auto_config_url,
    }))
}

#[cfg(windows)]
fn clear_persisted_proxy_snapshot(state: &AppState) -> Result<(), String> {
    let path = persisted_proxy_snapshot_path(state);
    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(windows)]
fn looks_like_local_proxy_server(value: &str) -> bool {
    let value = value.trim().to_ascii_lowercase();
    value.starts_with("127.0.0.1:")
        || value.starts_with("localhost:")
        || value.contains("127.0.0.1:")
        || value.contains("localhost:")
}

#[cfg(windows)]
async fn force_clear_system_proxy() -> Result<(), String> {
    set_registry_value("ProxyEnable", "REG_DWORD", "0").await?;
    delete_registry_value("ProxyServer").await?;
    delete_registry_value("ProxyOverride").await?;
    delete_registry_value("AutoConfigURL").await?;
    Ok(())
}

#[cfg(windows)]
async fn restore_system_proxy_snapshot(mode: ProxyCleanupMode) -> Result<(), String> {
    let snapshot = {
        let mut guard = SYSTEM_PROXY_SNAPSHOT.lock().map_err(|_| "系统代理快照锁失败".to_string())?;
        guard.take()
    };

    if mode == ProxyCleanupMode::ForceClear {
        return force_clear_system_proxy().await;
    }

    if let Some(snapshot) = snapshot {
        let enable_value = snapshot.proxy_enable.as_deref().unwrap_or("0");
        set_registry_value("ProxyEnable", "REG_DWORD", enable_value).await?;

        if let Some(proxy_server) = snapshot.proxy_server {
            set_registry_value("ProxyServer", "REG_SZ", &proxy_server).await?;
        } else {
            delete_registry_value("ProxyServer").await?;
        }
        if let Some(proxy_override) = snapshot.proxy_override {
            set_registry_value("ProxyOverride", "REG_SZ", &proxy_override).await?;
        } else {
            delete_registry_value("ProxyOverride").await?;
        }
        if let Some(auto_config_url) = snapshot.auto_config_url {
            set_registry_value("AutoConfigURL", "REG_SZ", &auto_config_url).await?;
        } else {
            delete_registry_value("AutoConfigURL").await?;
        }
    } else {
        force_clear_system_proxy().await?;
    }

    Ok(())
}

#[cfg(windows)]
async fn restore_persisted_or_clear(state: &AppState) -> Result<(), String> {
    if let Some(snapshot) = load_persisted_proxy_snapshot(state)? {
        set_registry_value("ProxyEnable", "REG_DWORD", snapshot.proxy_enable.as_deref().unwrap_or("0")).await?;

        if let Some(proxy_server) = snapshot.proxy_server {
            set_registry_value("ProxyServer", "REG_SZ", &proxy_server).await?;
        } else {
            delete_registry_value("ProxyServer").await?;
        }

        if let Some(proxy_override) = snapshot.proxy_override {
            set_registry_value("ProxyOverride", "REG_SZ", &proxy_override).await?;
        } else {
            delete_registry_value("ProxyOverride").await?;
        }

        if let Some(auto_config_url) = snapshot.auto_config_url {
            set_registry_value("AutoConfigURL", "REG_SZ", &auto_config_url).await?;
        } else {
            delete_registry_value("AutoConfigURL").await?;
        }

        clear_persisted_proxy_snapshot(state)?;
        return Ok(());
    }

    force_clear_system_proxy().await
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

#[cfg(windows)]
async fn enable_system_proxy_for_state(state: &AppState, port: u16) -> Result<(), String> {
    let proxy = format!("127.0.0.1:{}", port);
    snapshot_system_proxy_if_needed_for_state(state).await?;
    set_registry_value("ProxyEnable", "REG_DWORD", "1").await?;
    set_registry_value("ProxyServer", "REG_SZ", &proxy).await?;
    Ok(())
}

#[cfg(not(windows))]
async fn enable_system_proxy_for_state(_state: &AppState, port: u16) -> Result<(), String> {
    enable_system_proxy_internal(port).await
}

#[cfg(windows)]
async fn disable_system_proxy_for_state(state: &AppState, mode: ProxyCleanupMode) -> Result<(), String> {
    match mode {
        ProxyCleanupMode::RestoreSnapshot => restore_persisted_or_clear(state).await,
        ProxyCleanupMode::ForceClear => {
            let result = force_clear_system_proxy().await;
            clear_persisted_proxy_snapshot(state)?;
            result
        }
    }
}

#[cfg(not(windows))]
async fn disable_system_proxy_for_state(_state: &AppState, mode: ProxyCleanupMode) -> Result<(), String> {
    disable_system_proxy_internal(mode).await
}

#[cfg(windows)]
async fn disable_system_proxy_for_state_on_crash(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    disable_system_proxy_for_state(&state, ProxyCleanupMode::RestoreSnapshot).await
}

#[cfg(not(windows))]
async fn disable_system_proxy_for_state_on_crash(_app: &AppHandle) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
async fn repair_stale_proxy_if_needed(state: &AppState) -> Result<(), String> {
    let marker = read_proxy_session_marker(state)?;
    let proxy_enable = query_registry_value("ProxyEnable").await?;
    let proxy_server = query_registry_value("ProxyServer").await?;
    let auto_config_url = query_registry_value("AutoConfigURL").await?;

    append_startup_diagnostic(
        state,
        &format!(
            "stale proxy probe: marker={:?}, proxy_enable={:?}, proxy_server={:?}, auto_config_url={:?}",
            marker,
            proxy_enable,
            proxy_server,
            auto_config_url,
        ),
    );

    let has_local_proxy_server = proxy_server
        .as_deref()
        .map(looks_like_local_proxy_server)
        .unwrap_or(false);

    let should_repair = marker.as_deref() == Some("active")
        || marker.as_deref() == Some("cleaning")
        || proxy_enable.as_deref() == Some("0") && has_local_proxy_server
        || auto_config_url
            .as_deref()
            .map(|value| value.contains("127.0.0.1") || value.contains("localhost"))
            .unwrap_or(false);

    if should_repair {
        append_startup_diagnostic(state, "detected stale proxy configuration, forcing restore/clear");
        log::warn!("Detected stale proxy configuration from previous session, forcing cleanup");
        restore_persisted_or_clear(state).await?;
        clear_proxy_session_marker(state)?;
        append_startup_diagnostic(state, "stale proxy cleanup completed");
    } else {
        append_startup_diagnostic(state, "no stale proxy configuration detected");
    }

    Ok(())
}

#[cfg(not(windows))]
async fn repair_stale_proxy_if_needed(_state: &AppState) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use crate::types::{CustomRules, DomainRule, Profile, ProfilesData, RuleSet};
    use std::collections::HashMap;
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn update_max(target: &AtomicUsize, candidate: usize) {
        let mut current = target.load(Ordering::SeqCst);
        while candidate > current {
            match target.compare_exchange(current, candidate, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kunbox-singbox-{}-{}", name, suffix))
    }

    fn make_profile(id: &str, name: &str) -> Profile {
        Profile {
            id: id.to_string(),
            name: name.to_string(),
            url: String::new(),
            last_update: None,
            node_count: 1,
            enabled: true,
            auto_update_interval: 0,
            dns_pre_resolve: false,
            dns_server: None,
        }
    }

    fn make_node(tag: &str) -> serde_json::Value {
        serde_json::json!({
            "tag": tag,
            "type": "vmess",
            "server": "example.com",
            "server_port": 443,
            "uuid": "00000000-0000-0000-0000-000000000000"
        })
    }

    fn make_xhttp_node(tag: &str) -> serde_json::Value {
        serde_json::json!({
            "tag": tag,
            "type": "vless",
            "server": "edge.example.com",
            "server_port": 443,
            "uuid": "00000000-0000-0000-0000-000000000000",
            "encryption": "mlkem768x25519plus.native.0rtt.test",
            "flow": "xtls-rprx-vision",
            "packet_encoding": "xudp",
            "tls": {
                "enabled": true,
                "server_name": "edge.example.com",
                "utls": {
                    "enabled": true,
                    "fingerprint": "chrome"
                }
            },
            "transport": {
                "type": "xhttp",
                "path": "/proxy",
                "host": "cdn.example.com",
                "mode": "auto"
            }
        })
    }

    fn make_ruleset(id: &str, outbound_mode: &str, outbound_value: Option<&str>) -> RuleSet {
        RuleSet {
            id: id.to_string(),
            tag: id.to_string(),
            name: id.to_string(),
            rule_type: "remote".to_string(),
            format: "binary".to_string(),
            url: None,
            outbound_mode: outbound_mode.to_string(),
            outbound_value: outbound_value.map(|value| value.to_string()),
            enabled: true,
            is_built_in: false,
        }
    }

    fn make_domain_rule(value: &str, outbound_value: &str) -> DomainRule {
        DomainRule {
            id: "rule-1".to_string(),
            name: value.to_string(),
            rule_type: "domain".to_string(),
            value: value.to_string(),
            outbound_mode: "node".to_string(),
            outbound_value: Some(outbound_value.to_string()),
            enabled: true,
        }
    }

    fn write_json_file(path: &Path, value: &impl serde::Serialize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    }

    fn read_generated_config(state: &AppState) -> serde_json::Value {
        let config_path = state.config_dir.join("config.json");
        let content = fs::read_to_string(config_path).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    #[test]
    fn detects_local_proxy_server_values() {
        assert!(looks_like_local_proxy_server("127.0.0.1:7890"));
        assert!(looks_like_local_proxy_server("http=127.0.0.1:7890;https=127.0.0.1:7890"));
        assert!(!looks_like_local_proxy_server("10.0.0.1:7890"));
    }

    #[test]
    fn force_clear_mode_is_distinct() {
        assert_ne!(ProxyCleanupMode::RestoreSnapshot, ProxyCleanupMode::ForceClear);
    }

    #[tokio::test]
    async fn resolve_available_inbound_ports_replaces_unavailable_ports() {
        let data_dir = unique_test_dir("inbound-port-fallback");
        let state = AppState::new(data_dir.clone());
        let blocked = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let blocked_port = blocked.local_addr().unwrap().port();

        let mut settings = AppSettings::default();
        settings.local_port = blocked_port;
        settings.socks_port = find_available_tcp_port_avoiding("127.0.0.1", &[blocked_port])
            .await
            .unwrap();

        let (changed, reservations) = resolve_available_inbound_ports(&state, &mut settings).await.unwrap();

        assert!(changed);
        assert_ne!(settings.local_port, blocked_port);
        assert_ne!(settings.local_port, settings.socks_port);
        assert_eq!(reservations.len(), 2);
        assert!(TcpListener::bind(("127.0.0.1", settings.local_port)).is_err());
        assert!(TcpListener::bind(("127.0.0.1", settings.socks_port)).is_err());
        drop(reservations);
        assert!(TcpListener::bind(("127.0.0.1", settings.local_port)).is_ok());

        drop(blocked);
        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn parse_foreign_wintun_aliases_ignores_kunbox_tun_and_keeps_foreign_aliases() {
        let json = r#"[
            {"InterfaceAlias":"kunbox-tun","InterfaceDescription":"Wintun Userspace Tunnel"},
            {"InterfaceAlias":"vgate0","InterfaceDescription":"Rust Wintun Tunnel Tunnel"},
            {"InterfaceAlias":"Ethernet","InterfaceDescription":"Realtek PCIe GbE Family Controller"}
        ]"#;

        let aliases = parse_foreign_wintun_aliases(json, KUNBOX_TUN_ALIAS);
        assert_eq!(aliases, vec!["vgate0".to_string()]);
    }

    #[test]
    fn parse_foreign_wintun_aliases_handles_single_object_payload() {
        let json = r#"{"InterfaceAlias":"vgate0","InterfaceDescription":"Wintun Userspace Tunnel"}"#;
        let aliases = parse_foreign_wintun_aliases(json, KUNBOX_TUN_ALIAS);
        assert_eq!(aliases, vec!["vgate0".to_string()]);
    }

    #[tokio::test]
    async fn generate_config_hijacks_dns_by_protocol_or_port() {
        let data_dir = unique_test_dir("dns-hijack-port");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-a", "Profile A")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("node-a".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-a.json"), &vec![make_node("node-a")]);

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let rules = config["route"]["rules"].as_array().unwrap();
        let dns_hijack_rule = rules
            .iter()
            .find(|rule| rule.get("action").and_then(|action| action.as_str()) == Some("hijack-dns"))
            .expect("dns hijack rule should be generated");

        assert_eq!(dns_hijack_rule.get("type").and_then(|value| value.as_str()), Some("logical"));
        assert_eq!(dns_hijack_rule.get("mode").and_then(|value| value.as_str()), Some("or"));

        let nested_rules = dns_hijack_rule.get("rules").and_then(|value| value.as_array()).unwrap();
        assert!(nested_rules.iter().any(|rule| rule.get("protocol").and_then(|value| value.as_str()) == Some("dns")));
        assert!(nested_rules.iter().any(|rule| rule.get("port").and_then(|value| value.as_u64()) == Some(53)));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_forces_strict_tun_route() {
        let data_dir = unique_test_dir("tun-strict-route");
        let state = AppState::new(data_dir.clone());

        {
            let mut settings = state.settings.lock().await;
            settings.tun_enabled = true;
            settings.tun_strict_route = false;
        }

        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-a", "Profile A")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("node-a".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-a.json"), &vec![make_node("node-a")]);

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let tun_inbound = config["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|inbound| inbound.get("tag").and_then(|tag| tag.as_str()) == Some("tun-in"))
            .expect("tun inbound should be generated");

        assert_eq!(tun_inbound.get("strict_route").and_then(|value| value.as_bool()), Some(true));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_stores_rejected_dns_response_cache() {
        let data_dir = unique_test_dir("dns-rdrc-cache");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-a", "Profile A")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("node-a".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-a.json"), &vec![make_node("node-a")]);

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        assert_eq!(config["experimental"]["cache_file"]["store_rdrc"].as_bool(), Some(true));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_uses_profile_ids_for_selector_tags() {
        let data_dir = unique_test_dir("duplicate-profile-selector");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![
                make_profile("profile-a", "Same Name"),
                make_profile("profile-b", "Same Name"),
            ],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("node-a".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-a.json"), &vec![make_node("node-a")]);
        write_json_file(&state.configs_dir().join("profile-b.json"), &vec![make_node("node-b")]);

        *state.rulesets.lock().await = vec![
            make_ruleset("rs-a", "profile", Some("profile-a")),
            make_ruleset("rs-b", "profile", Some("profile-b")),
        ];

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let outbounds = config["outbounds"].as_array().unwrap();
        let tags: std::collections::HashSet<&str> = outbounds
            .iter()
            .filter_map(|outbound| outbound.get("tag").and_then(|tag| tag.as_str()))
            .collect();

        assert!(tags.contains("P:profile-a"));
        assert!(tags.contains("P:profile-b"));
        assert!(!tags.contains("P:Same Name"));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_uses_saved_profile_node_selection_for_profile_selector_default() {
        let data_dir = unique_test_dir("profile-selector-default");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-a", "Profile A")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("node-b".to_string()),
            node_selections: HashMap::from([("profile-a".to_string(), "node-b".to_string())]),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("node-a"), make_node("node-b")],
        );

        *state.rulesets.lock().await = vec![make_ruleset("rs-a", "profile", Some("profile-a"))];

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let outbounds = config["outbounds"].as_array().unwrap();
        let selector = outbounds
            .iter()
            .find(|outbound| outbound.get("tag").and_then(|tag| tag.as_str()) == Some("P:profile-a"))
            .expect("expected profile selector");

        assert_eq!(selector.get("default").and_then(|value| value.as_str()), Some("node-b"));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_scopes_duplicate_profile_selector_nodes() {
        let data_dir = unique_test_dir("profile-selector-duplicate-node-tags");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![
                make_profile("profile-a", "Alpha"),
                make_profile("profile-b", "Beta"),
            ],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("shared-node".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-a.json"), &vec![make_node("shared-node")]);
        write_json_file(&state.configs_dir().join("profile-b.json"), &vec![make_node("shared-node")]);

        *state.rulesets.lock().await = vec![make_ruleset("rs-b", "profile", Some("profile-b"))];

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let outbounds = config["outbounds"].as_array().unwrap();
        let tags: std::collections::HashSet<&str> = outbounds
            .iter()
            .filter_map(|outbound| outbound.get("tag").and_then(|tag| tag.as_str()))
            .collect();

        assert!(tags.contains("shared-node"));
        assert!(tags.contains("profile-b::shared-node"));

        let selector = outbounds
            .iter()
            .find(|outbound| outbound.get("tag").and_then(|tag| tag.as_str()) == Some("P:profile-b"))
            .expect("expected profile-b selector");
        let selector_outbounds: Vec<&str> = selector
            .get("outbounds")
            .and_then(|value| value.as_array())
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str())
            .collect();

        assert_eq!(selector_outbounds, vec!["profile-b::shared-node"]);
        assert_eq!(
            selector.get("default").and_then(|value| value.as_str()),
            Some("profile-b::shared-node")
        );

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_preserves_profile_scoped_node_identity_for_domain_rules() {
        let data_dir = unique_test_dir("profile-scoped-node-routing");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![
                make_profile("profile-a", "Alpha"),
                make_profile("profile-b", "Beta"),
            ],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("shared-node".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-a.json"), &vec![make_node("shared-node")]);
        write_json_file(&state.configs_dir().join("profile-b.json"), &vec![make_node("shared-node")]);

        *state.custom_rules.lock().await = CustomRules {
            domain_rules: vec![make_domain_rule("example.com", "profile-b::shared-node")],
        };

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let outbounds = config["outbounds"].as_array().unwrap();
        let tags: std::collections::HashSet<&str> = outbounds
            .iter()
            .filter_map(|outbound| outbound.get("tag").and_then(|tag| tag.as_str()))
            .collect();

        assert!(tags.contains("shared-node"));
        assert!(tags.contains("profile-b::shared-node"));

        let domain_rule = config["route"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|rule| {
                rule.get("domain")
                    .and_then(|domain| domain.as_array())
                    .map(|domains| domains.iter().any(|value| value.as_str() == Some("example.com")))
                    .unwrap_or(false)
            })
            .unwrap();

        assert_eq!(
            domain_rule.get("outbound").and_then(|value| value.as_str()),
            Some("profile-b::shared-node")
        );

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_supports_profile_scoped_and_bare_ruleset_node_routes() {
        let data_dir = unique_test_dir("ruleset-profile-scoped-node-routing");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![
                make_profile("profile-a", "Alpha"),
                make_profile("profile-b", "Beta"),
            ],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("shared-node".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-a.json"), &vec![make_node("shared-node")]);
        write_json_file(&state.configs_dir().join("profile-b.json"), &vec![make_node("shared-node")]);
        fs::create_dir_all(state.rulesets_cache_dir()).unwrap();
        fs::write(state.rulesets_cache_dir().join("rs-profile.srs"), b"dummy").unwrap();
        fs::write(state.rulesets_cache_dir().join("rs-bare.srs"), b"dummy").unwrap();

        *state.rulesets.lock().await = vec![
            make_ruleset("rs-profile", "node", Some("profile-b::shared-node")),
            make_ruleset("rs-bare", "node", Some("shared-node")),
        ];

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let outbounds = config["outbounds"].as_array().unwrap();
        let tags: std::collections::HashSet<&str> = outbounds
            .iter()
            .filter_map(|outbound| outbound.get("tag").and_then(|tag| tag.as_str()))
            .collect();

        assert!(tags.contains("shared-node"));
        assert!(tags.contains("profile-b::shared-node"));

        let rules = config["route"]["rules"].as_array().unwrap();
        let profile_ruleset_rule = rules.iter().find(|rule| {
            rule.get("rule_set")
                .and_then(|rule_set| rule_set.as_array())
                .map(|rule_sets| rule_sets.iter().any(|value| value.as_str() == Some("rs-profile")))
                .unwrap_or(false)
        }).unwrap();
        let bare_ruleset_rule = rules.iter().find(|rule| {
            rule.get("rule_set")
                .and_then(|rule_set| rule_set.as_array())
                .map(|rule_sets| rule_sets.iter().any(|value| value.as_str() == Some("rs-bare")))
                .unwrap_or(false)
        }).unwrap();

        assert_eq!(
            profile_ruleset_rule.get("outbound").and_then(|value| value.as_str()),
            Some("profile-b::shared-node")
        );
        assert_eq!(
            bare_ruleset_rule.get("outbound").and_then(|value| value.as_str()),
            Some("shared-node")
        );

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_uses_direct_dns_bootstrap_for_ech_nodes() {
        let data_dir = unique_test_dir("ech-bootstrap");
        let state = AppState::new(data_dir.clone());

        let mut profile = make_profile("profile-ech", "ECH Profile");
        profile.dns_server = Some("https://dns.alidns.com/dns-query".to_string());

        let profiles_data = ProfilesData {
            profiles: vec![profile],
            active_profile_id: Some("profile-ech".to_string()),
            active_node_tag: Some("ECH Node".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(
            &state.configs_dir().join("profile-ech.json"),
            &vec![serde_json::json!({
                "tag": "ECH Node",
                "type": "vless",
                "server": "104.19.41.41",
                "server_port": 443,
                "uuid": "68d55b3f-c4f1-481a-8bfb-e483004f2c15",
                "packet_encoding": "xudp",
                "tls": {
                    "enabled": true,
                    "server_name": "cm.5945946.xyz",
                    "ech": {
                        "enabled": true,
                        "query_server_name": "cloudflare-ech.com"
                    }
                },
                crate::commands::profiles::ECH_DNS_SERVER_META_KEY: "https://dns.alidns.com/dns-query"
            })],
        );

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let dns_remote = config["dns"]["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|server| server.get("tag").and_then(|value| value.as_str()) == Some("dns-remote"))
            .unwrap();

        assert_eq!(dns_remote.get("server").and_then(|value| value.as_str()), Some("dns.alidns.com"));
        assert_eq!(dns_remote.get("path").and_then(|value| value.as_str()), Some("/dns-query"));
        assert_eq!(dns_remote.get("domain_resolver").and_then(|value| value.as_str()), Some("dns-local"));
        assert!(dns_remote.get("detour").is_none(), "ECH bootstrap DNS must not detour through proxy");

        let outbound = config["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node.get("tag").and_then(|value| value.as_str()) == Some("ECH Node"))
            .unwrap();

        assert_eq!(
            outbound.get(crate::commands::profiles::ECH_DNS_SERVER_META_KEY),
            None,
            "internal ECH DNS metadata must not leak into final sing-box config"
        );

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_keeps_proxy_dns_for_non_ech_active_node() {
        let data_dir = unique_test_dir("ech-non-active");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-mixed", "Mixed Profile")],
            active_profile_id: Some("profile-mixed".to_string()),
            active_node_tag: Some("Plain Node".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(
            &state.configs_dir().join("profile-mixed.json"),
            &vec![
                serde_json::json!({
                    "tag": "Plain Node",
                    "type": "vmess",
                    "server": "example.com",
                    "server_port": 443,
                    "uuid": "00000000-0000-0000-0000-000000000000"
                }),
                serde_json::json!({
                    "tag": "ECH Node",
                    "type": "vless",
                    "server": "104.19.41.41",
                    "server_port": 443,
                    "uuid": "68d55b3f-c4f1-481a-8bfb-e483004f2c15",
                    crate::commands::profiles::ECH_DNS_SERVER_META_KEY: "https://dns.alidns.com/dns-query",
                    "tls": {
                        "enabled": true,
                        "server_name": "cm.5945946.xyz",
                        "ech": {
                            "enabled": true,
                            "query_server_name": "cloudflare-ech.com"
                        }
                    }
                }),
            ],
        );

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let dns_remote = config["dns"]["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|server| server.get("tag").and_then(|value| value.as_str()) == Some("dns-remote"))
            .unwrap();

        assert_eq!(dns_remote.get("detour").and_then(|value| value.as_str()), Some("PROXY"));
        assert!(dns_remote.get("domain_resolver").is_none());

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_bridges_xhttp_nodes_through_local_xray_plugin() {
        let data_dir = unique_test_dir("xhttp-plugin-bridge");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-xhttp", "XHTTP Profile")],
            active_profile_id: Some("profile-xhttp".to_string()),
            active_node_tag: Some("XHTTP Node".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-xhttp.json"), &vec![make_xhttp_node("XHTTP Node")]);

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let outbound = config["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node.get("tag").and_then(|value| value.as_str()) == Some("XHTTP Node"))
            .unwrap();

        assert_eq!(outbound.get("type").and_then(|value| value.as_str()), Some("socks"));
        assert_eq!(outbound.get("server").and_then(|value| value.as_str()), Some("127.0.0.1"));
        assert!(outbound.get("server_port").and_then(|value| value.as_u64()).is_some());

        let plugin_specs: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(state.config_dir.join("plugin-bridges.json")).unwrap()
        ).unwrap();
        let spec = plugin_specs.as_array().unwrap().first().unwrap();

        assert_eq!(spec["tag"].as_str(), Some("XHTTP Node"));
        assert_eq!(spec["core"].as_str(), Some("xray"));
        assert_eq!(spec["node"]["transport"]["type"].as_str(), Some("xhttp"));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_routes_xray_plugin_remote_direct_in_tun_mode() {
        let data_dir = unique_test_dir("xhttp-plugin-tun-direct");
        let state = AppState::new(data_dir.clone());

        state.settings.lock().await.tun_enabled = true;

        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-xhttp", "XHTTP Profile")],
            active_profile_id: Some("profile-xhttp".to_string()),
            active_node_tag: Some("XHTTP Node".to_string()),
            node_selections: HashMap::new(),
        };

        let mut node = make_xhttp_node("XHTTP Node");
        node["server"] = serde_json::Value::String("35.194.192.123".to_string());
        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-xhttp.json"), &vec![node]);

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let rules = config["route"]["rules"].as_array().unwrap();
        let remote_rule = rules
            .iter()
            .find(|rule| {
                rule.get("ip_cidr")
                    .and_then(|values| values.as_array())
                    .map(|values| values.iter().any(|value| value.as_str() == Some("35.194.192.123/32")))
                    .unwrap_or(false)
            })
            .expect("xray plugin remote server must bypass TUN proxy loop");

        assert_eq!(remote_rule.get("outbound").and_then(|value| value.as_str()), Some("direct"));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_uses_strict_tun_route_by_default() {
        let data_dir = unique_test_dir("tun-non-strict-route");
        let state = AppState::new(data_dir.clone());

        state.settings.lock().await.tun_enabled = true;

        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-a", "Profile A")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("node-a".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-a.json"), &vec![make_node("node-a")]);

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let tun_inbound = config["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|inbound| inbound.get("tag").and_then(|tag| tag.as_str()) == Some("tun-in"))
            .expect("tun inbound should be generated");

        assert_eq!(tun_inbound.get("strict_route").and_then(|value| value.as_bool()), Some(true));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_scopes_fakedns_to_tun_inbound_when_tun_is_enabled() {
        let data_dir = unique_test_dir("tun-fakedns-scope");
        let state = AppState::new(data_dir.clone());

        {
            let mut settings = state.settings.lock().await;
            settings.tun_enabled = true;
            settings.fake_dns = true;
        }

        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-a", "Profile A")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("node-a".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-a.json"), &vec![make_node("node-a")]);

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let dns_rules = config["dns"]["rules"].as_array().unwrap();
        let fake_dns_rule = dns_rules
            .iter()
            .find(|rule| rule.get("server").and_then(|server| server.as_str()) == Some("dns-fakeip"))
            .expect("fake dns rule should be generated");

        let inbound = fake_dns_rule.get("inbound").and_then(|v| v.as_array()).unwrap();
        assert_eq!(inbound.len(), 1);
        assert_eq!(inbound[0].as_str(), Some("tun-in"));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn build_xray_plugin_config_preserves_vless_xhttp_transport() {
        let node = make_xhttp_node("XHTTP Node");
        let config = build_xray_plugin_config(&node, 18080).unwrap();

        let inbound = config["inbounds"].as_array().unwrap().first().unwrap();
        assert_eq!(inbound["protocol"].as_str(), Some("socks"));
        assert_eq!(inbound["port"].as_u64(), Some(18080));

        let outbound = config["outbounds"].as_array().unwrap().first().unwrap();
        assert_eq!(outbound["protocol"].as_str(), Some("vless"));
        assert_eq!(
            outbound["settings"]["vnext"][0]["users"][0]["encryption"].as_str(),
            Some("mlkem768x25519plus.native.0rtt.test")
        );
        assert_eq!(outbound["settings"]["vnext"][0]["users"][0]["flow"].as_str(), Some("xtls-rprx-vision"));
        assert_eq!(outbound["streamSettings"]["network"].as_str(), Some("xhttp"));
        assert_eq!(outbound["streamSettings"]["xhttpSettings"]["path"].as_str(), Some("/proxy"));
        assert_eq!(outbound["streamSettings"]["xhttpSettings"]["host"].as_str(), Some("cdn.example.com"));
    }

    #[test]
    fn node_for_singbox_removes_naive_unsupported_fields_and_keeps_tls() {
        let node = serde_json::json!({
            "type": "naive",
            "tag": "Naive H2",
            "server": "naive.example.com",
            "server_port": 443,
            "username": "user",
            "password": "pass",
            "network": "h2"
        });
        let mut bridge_specs = Vec::new();

        let outbound = node_for_singbox_with_plugin_bridge(&node, &mut bridge_specs);

        assert_eq!(outbound.get("type").and_then(|value| value.as_str()), Some("naive"));
        assert!(outbound.get("network").is_none());
        assert!(outbound.get("transport").is_none());
        assert_eq!(outbound.get("quic").and_then(|value| value.as_bool()), Some(false));
        assert_eq!(
            outbound
                .get("tls")
                .and_then(|value| value.get("server_name"))
                .and_then(|value| value.as_str()),
            Some("naive.example.com")
        );
        assert_eq!(
            outbound
                .get("tls")
                .and_then(|value| value.get("enabled"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert!(outbound.get("tls").and_then(|value| value.get("insecure")).is_none());
    }

    #[test]
    fn config_value_has_outbound_type_detects_naive_nodes() {
        let config = serde_json::json!({
            "outbounds": [
                { "type": "selector", "tag": "PROXY", "outbounds": ["Naive"] },
                { "type": "naive", "tag": "Naive" },
                { "type": "direct", "tag": "direct" }
            ]
        });

        assert!(config_value_has_outbound_type(&config, "naive"));
        assert!(!config_value_has_outbound_type(&config, "trojan"));
    }

    #[test]
    fn build_xray_plugin_config_hoists_legacy_xhttp_extra_encryption() {
        let mut node = make_xhttp_node("Legacy XHTTP Node");
        node.as_object_mut().unwrap().remove("encryption");
        node["transport"]["extra"] = serde_json::json!({
            "encryption": "mlkem768x25519plus.native.0rtt.legacy",
            "noGRPCHeader": true
        });

        let config = build_xray_plugin_config(&node, 18080).unwrap();
        let outbound = config["outbounds"].as_array().unwrap().first().unwrap();

        assert_eq!(
            outbound["settings"]["vnext"][0]["users"][0]["encryption"].as_str(),
            Some("mlkem768x25519plus.native.0rtt.legacy")
        );
        assert!(outbound["streamSettings"]["xhttpSettings"]["extra"].get("encryption").is_none());
        assert_eq!(
            outbound["streamSettings"]["xhttpSettings"]["extra"]["noGRPCHeader"].as_bool(),
            Some(true)
        );
    }

    #[tokio::test]
    async fn bounded_selector_probe_helper_caps_concurrency() {
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let tags: Vec<String> = (0..12).map(|idx| format!("node-{idx}")).collect();

        let results = run_bounded_selector_probes(tags.clone(), 3, |tag| {
            let active = active.clone();
            let max_active = max_active.clone();
            async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                update_max(&max_active, current);
                tokio::time::sleep(Duration::from_millis(10)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                (tag, Some(10))
            }
        })
        .await;

        assert_eq!(results.len(), tags.len());
        assert!(max_active.load(Ordering::SeqCst) <= 3);
    }

    #[test]
    fn recognizes_taskkill_missing_process_output() {
        let samples = [
            "ERROR: The process \"sing-box.exe\" not found.",
            "错误: 没有找到进程 \"sing-box.exe\"。",
            "错误: 没有运行的任务与指定标准匹配。",
        ];

        for sample in samples {
            let lower = sample.to_lowercase();
            assert!(
                lower.contains("not found") || lower.contains("没有运行的任务") || lower.contains("没有找到")
            );
        }
    }
}

async fn disable_system_proxy_internal(mode: ProxyCleanupMode) -> Result<(), String> {
    #[cfg(windows)]
    restore_system_proxy_snapshot(mode).await?;

    Ok(())
}

async fn start_traffic_polling(
    app: AppHandle,
    clash_api_port: u16,
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
                match client.get(format!("http://127.0.0.1:{}/connections", clash_api_port))
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
    let custom_rules = state.custom_rules.lock().await.clone();

    let mut referenced_profile_ids = std::collections::HashSet::new();

    for rs in rulesets.iter().filter(|r| r.enabled) {
        if let Some(value) = &rs.outbound_value {
            if matches!(rs.outbound_mode.as_str(), "profile" | "配置") {
                referenced_profile_ids.insert(value.clone());
            }
        }
    }

    for rule in custom_rules.domain_rules.iter().filter(|r| r.enabled) {
        if let Some(value) = &rule.outbound_value {
            if rule.outbound_mode == "profile" {
                referenced_profile_ids.insert(value.clone());
            }
        }
    }

    if referenced_profile_ids.is_empty() {
        return Vec::new();
    }

    let profiles_data = load_profiles_data_from_file(state).await;
    let existing_profile_ids: std::collections::HashSet<String> = profiles_data
        .profiles
        .iter()
        .map(|p| p.id.clone())
        .collect();

    let mut selector_tags: Vec<String> = referenced_profile_ids
        .into_iter()
        .filter(|profile_id| existing_profile_ids.contains(profile_id))
        .map(|profile_id| profile_selector_tag(&profile_id))
        .collect();

    selector_tags.sort();
    selector_tags.dedup();
    selector_tags
}

async fn switch_selector_to_node(
    client: &reqwest::Client,
    clash_api_port: u16,
    selector_tag: &str,
    node_tag: &str,
) {
    if let Err(err) = client
        .put(format!("http://127.0.0.1:{}/proxies/{}", clash_api_port, urlencoding::encode(selector_tag)))
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

async fn probe_selector_node_latency(
    client: reqwest::Client,
    clash_api_port: u16,
    tag: String,
    test_url: String,
) -> (String, Option<u32>) {
    let result = client
        .get(format!("http://127.0.0.1:{}/proxies/{}/delay", clash_api_port, urlencoding::encode(&tag)))
        .query(&[("url", test_url.as_str()), ("timeout", "5000")])
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
}

async fn run_bounded_selector_probes<I, F, Fut>(
    items: I,
    concurrency_limit: usize,
    mut probe: F,
) -> Vec<(String, Option<u32>)>
where
    I: IntoIterator<Item = String>,
    F: FnMut(String) -> Fut,
    Fut: Future<Output = (String, Option<u32>)>,
{
    let mut remaining = items.into_iter();
    let mut in_flight = FuturesUnordered::new();
    let concurrency_limit = concurrency_limit.max(1);

    for _ in 0..concurrency_limit {
        let Some(item) = remaining.next() else {
            break;
        };
        in_flight.push(probe(item));
    }

    let mut results = Vec::new();
    while let Some(result) = in_flight.next().await {
        results.push(result);
        if let Some(item) = remaining.next() {
            in_flight.push(probe(item));
        }
    }

    results
}

async fn test_selector_latency_internal(
    app: &AppHandle,
    selector_tag: String,
    test_url: Option<String>,
) -> Result<serde_json::Value, String> {
    let state = app.state::<AppState>();
    let clash_api_port = get_clash_api_port(&state).await;
    let client = reqwest::Client::new();
    let test_url = test_url.unwrap_or_else(|| "https://www.gstatic.com/generate_204".to_string());

    let resp = client
        .get(format!("http://127.0.0.1:{}/proxies/{}", clash_api_port, urlencoding::encode(&selector_tag)))
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

    let results = run_bounded_selector_probes(
        node_tags.clone(),
        SELECTOR_LATENCY_CONCURRENCY_LIMIT,
        |tag| {
            let client = client.clone();
            let test_url = test_url.clone();
            async move { probe_selector_node_latency(client, clash_api_port, tag, test_url).await }
        },
    )
    .await;

    let mut first_switch_done = false;
    let mut best_node: Option<(String, u32)> = None;
    let mut valid_count: usize = 0;
    let first_switch_threshold = std::cmp::min(5usize, node_tags.len());

    for (tag, delay) in &results {
        if let Some(d) = delay {
            valid_count += 1;
            match &best_node {
                None => best_node = Some((tag.clone(), *d)),
                Some((_, best_delay)) if *d < *best_delay => best_node = Some((tag.clone(), *d)),
                _ => {}
            }
        }

        if !first_switch_done && valid_count >= first_switch_threshold {
            if let Some((best_tag, best_delay)) = &best_node {
                log::info!("First phase done, switching '{}' to '{}' ({}ms)", selector_tag, best_tag, best_delay);
                switch_selector_to_node(&client, clash_api_port, &selector_tag, best_tag).await;
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
        switch_selector_to_node(&client, clash_api_port, &selector_tag, best_tag).await;
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
