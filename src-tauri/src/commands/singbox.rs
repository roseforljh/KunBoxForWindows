use crate::state::AppState;
use crate::types::{
    node_is_auto_selection_eligible, AppSettings, CommandResult, HealthEvent, HealthEventKind,
    HealthStatus, ProxyState, TrafficStats, NODE_AUTO_SELECTION_ELIGIBLE_META_KEY,
    NODE_METERED_PROTECTED_META_KEY, NODE_RUNTIME_META_KEYS,
};
use futures_util::stream::StreamExt;
use std::fs;
use std::fs::OpenOptions;
use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;

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

fn report_git_proxy_failure(state: &AppState, operation: &str, error: &str) {
    let message = format!("{}: {}", operation, error);
    append_startup_diagnostic(state, &message);
    log::warn!("{}", message);
}

fn restore_git_proxy(state: &AppState) {
    if let Err(err) = crate::commands::git_proxy::restore_after_disconnect(state) {
        report_git_proxy_failure(state, "Git proxy restore failed", &err);
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
const PLUGIN_DETOUR_CHAIN_PORT_BASE: u16 = 19_390;
const HEALTH_FAILED_BACKOFF_BASE_MS: i64 = 30_000;
const HEALTH_FAILED_BACKOFF_MAX_MS: i64 = 300_000;
const HEALTH_SELECTOR_SWITCH_COOLDOWN_MS: i64 = 60_000;
const HEALTH_BACKUP_PROBE_LIMIT: usize = 3;

fn local_clash_api_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("创建本地 Clash API 客户端失败")
}

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
    append_startup_diagnostic(
        state,
        "startup cleanup: killing stray sing-box.exe processes",
    );

    let output = Command::new("taskkill")
        .args(["/F", "/T", "/IM", "sing-box.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if output.status.success() {
        append_startup_diagnostic(
            state,
            "startup cleanup: stray sing-box.exe processes terminated",
        );
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        return Ok(());
    }

    let stdout = decode_windows_output(&output.stdout);
    let stderr = decode_windows_output(&output.stderr);
    let combined = if !stderr.is_empty() { stderr } else { stdout };
    let lower = combined.to_lowercase();

    if lower.contains("not found") || lower.contains("没有运行的任务") || lower.contains("没有找到")
    {
        append_startup_diagnostic(
            state,
            "startup cleanup: no stray sing-box.exe process found",
        );
        return Ok(());
    }

    append_startup_diagnostic(
        state,
        &format!("startup cleanup: taskkill failed: {}", combined),
    );
    Err(format!("清理残留 sing-box 进程失败: {}", combined))
}

fn inbound_listen_addr(settings: &AppSettings) -> &'static str {
    if settings.allow_lan {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    }
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

async fn find_available_tcp_port_avoiding(
    listen_addr: &str,
    avoid_ports: &[u16],
) -> Result<u16, String> {
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
            let (fallback, listener) =
                reserve_available_tcp_port_avoiding(listen_addr, &[settings.socks_port]).await?;
            append_startup_diagnostic(
                state,
                &format!(
                    "mixed-in port {} unavailable ({}), using fallback port {}",
                    old_port, err, fallback
                ),
            );
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
            let (fallback, listener) =
                reserve_available_tcp_port_avoiding(listen_addr, &[settings.local_port]).await?;
            append_startup_diagnostic(
                state,
                &format!(
                    "socks-in port {} unavailable ({}), using fallback port {}",
                    old_port, err, fallback
                ),
            );
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
    let default_port_available =
        std::net::TcpListener::bind(("127.0.0.1", DEFAULT_CLASH_API_PORT)).is_ok();
    let port = if !default_port_available
        || crate::commands::profiles::check_clash_api_running(DEFAULT_CLASH_API_PORT).await
    {
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
    use std::os::windows::process::CommandExt;
    use std::process::Command as StdCommand;

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

pub(crate) async fn singbox_start_impl(
    app: AppHandle,
    state: &AppState,
) -> Result<CommandResult, String> {
    let _latency_lifecycle = crate::commands::profiles::begin_latency_lifecycle(state).await;
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
    append_startup_diagnostic(state, "singbox_start invoked");
    cancel_health_monitor(state.health_cancel.clone()).await;
    let singbox_path = get_singbox_path(&app)?;
    append_startup_diagnostic(
        state,
        &format!("resolved sing-box path: {:?}", singbox_path),
    );

    #[cfg(windows)]
    {
        append_startup_diagnostic(
            state,
            "checking for stale proxy configuration before startup",
        );
        repair_stale_proxy_if_needed(state).await?;
        append_startup_diagnostic(state, "stale proxy repair check completed");
    }

    if !singbox_path.exists() {
        append_startup_diagnostic(state, "sing-box kernel missing on startup");
        return Ok(CommandResult::err(
            "未检测到 sing-box 内核，请先到【设置 → 内核】下载并安装后再启动 VPN。",
        ));
    }

    // Check if TUN mode is enabled and admin rights are required
    let settings = state.settings.lock().await.clone();
    let mut effective_settings = settings.clone();
    let mut startup_warning: Option<String> = None;
    append_startup_diagnostic(
        state,
        &format!(
            "startup connect settings: tun_enabled={}, system_proxy={}, local_port={}",
            settings.tun_enabled, settings.system_proxy, settings.local_port,
        ),
    );
    if settings.tun_enabled && !is_running_as_admin() {
        append_startup_diagnostic(
            state,
            "startup blocked because TUN mode requires admin privileges",
        );
        return Ok(CommandResult::err(
            "TUN 模式需要管理员权限。请右键点击应用图标，选择「以管理员身份运行」后重试。",
        ));
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
            let _ = app.emit(
                "singbox:log",
                serde_json::json!({
                    "timestamp": chrono::Utc::now().timestamp_millis(),
                    "level": "warn",
                    "tag": "sing-box",
                    "message": warning,
                }),
            );
        }
    }

    // TUN 已接管系统流量时再挂系统代理容易双路径冲突，启动时强制只保留 TUN。
    if effective_settings.tun_enabled && effective_settings.system_proxy {
        effective_settings.system_proxy = false;
        append_startup_diagnostic(
            state,
            "TUN mode active: auto-disabling system proxy for this session",
        );
        let message =
            "已启用 TUN 模式，本次连接自动关闭系统代理，避免双代理叠加导致证书或连接异常。";
        if startup_warning.is_none() {
            startup_warning = Some(message.to_string());
        }
        let _ = app.emit(
            "singbox:log",
            serde_json::json!({
                "timestamp": chrono::Utc::now().timestamp_millis(),
                "level": "warn",
                "tag": "sing-box",
                "message": message,
            }),
        );
    }

    if let Some(mut child) = state.singbox_process.lock().await.take() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
    stop_plugin_bridges(state).await;

    crate::commands::profiles::cleanup_temp_singbox(state).await;

    #[cfg(windows)]
    kill_stray_singbox_processes(state).await?;

    let (ports_changed, inbound_port_reservations) =
        resolve_available_inbound_ports(state, &mut effective_settings).await?;
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
        append_startup_diagnostic(
            state,
            &format!(
                "persisted auto-selected inbound ports: mixed-in={}, socks-in={}",
                effective_settings.local_port, effective_settings.socks_port
            ),
        );
    }

    // Generate config
    let clash_api_port = allocate_clash_api_port(state).await?;
    append_startup_diagnostic(
        state,
        &format!("selected clash api port: {}", clash_api_port),
    );
    let config_result = generate_config_with_settings(&state, &effective_settings).await?;
    if !config_result.success {
        return Ok(config_result);
    }

    if let Err(err) = start_plugin_bridges(&app, state).await {
        stop_plugin_bridges(state).await;
        return Ok(CommandResult::err(err));
    }

    let config_path = state.config_dir.join("config.json");

    let config_path_str = config_path
        .to_str()
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
        append_startup_diagnostic(
            state,
            &format!("sing-box preflight check failed: {}", detail),
        );
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
                    let _ = app_clone.emit(
                        "singbox:log",
                        serde_json::json!({
                            "timestamp": chrono::Utc::now().timestamp_millis(),
                            "level": "info",
                            "tag": "sing-box",
                            "message": line
                        }),
                    );
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
    let health_cancel = state.health_cancel.clone();
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
                            if !shutting_down
                                && !matches!(
                                    current_state,
                                    ProxyState::Idle | ProxyState::Disconnecting
                                )
                            {
                                *proxy_state.lock().await = ProxyState::Error;
                                *start_time_state.lock().await = None;
                                if let Some(cancel) = traffic_cancel.lock().await.take() {
                                    cancel.cancel();
                                }
                                cancel_health_monitor(health_cancel.clone()).await;
                                let _ = disable_system_proxy_for_state_on_crash(&wait_app).await;
                                let crash_state = wait_app.state::<AppState>();
                                restore_git_proxy(&crash_state);
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
                            if !shutting_down
                                && !matches!(
                                    current_state,
                                    ProxyState::Idle | ProxyState::Disconnecting
                                )
                            {
                                *proxy_state.lock().await = ProxyState::Error;
                                *start_time_state.lock().await = None;
                                if let Some(cancel) = traffic_cancel.lock().await.take() {
                                    cancel.cancel();
                                }
                                cancel_health_monitor(health_cancel.clone()).await;
                                let _ = disable_system_proxy_for_state_on_crash(&wait_app).await;
                                let crash_state = wait_app.state::<AppState>();
                                restore_git_proxy(&crash_state);
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
        if startup_detail
            .as_deref()
            .is_some_and(|detail| !detail.trim().is_empty())
        {
            if let Some(mut child) = state.singbox_process.lock().await.take() {
                let _ = child.kill().await;
                let _ = child.wait().await;
            }
            stop_plugin_bridges(state).await;
            *state.proxy_state.lock().await = ProxyState::Error;
            *state.start_time.lock().await = None;
            append_startup_diagnostic(
                state,
                &format!(
                    "fatal startup error detected before Clash API ready: {}",
                    startup_detail.clone().unwrap_or_default()
                ),
            );
            let _ = app.emit("singbox:state", "error");
            return Ok(CommandResult::err(format_startup_failure_message(
                startup_detail,
            )));
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
                if let Err(err) = test_selector_latency_internal(
                    &app_for_selector_test,
                    selector_tag.clone(),
                    None,
                )
                .await
                {
                    log::warn!(
                        "Auto selector latency test failed for '{}': {}",
                        selector_tag,
                        err
                    );
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
        start_traffic_polling(
            app_for_traffic,
            clash_api_port,
            traffic_stats,
            start_time_val,
            cancel_token,
        )
        .await;
    });

    if effective_settings.health_monitor_enabled {
        start_health_monitor(app.clone(), state, effective_settings.clone()).await;
    }

    // Enable system proxy
    if effective_settings.system_proxy {
        append_startup_diagnostic(
            state,
            &format!(
                "enabling system proxy on port {}",
                effective_settings.local_port
            ),
        );
        let _ = enable_system_proxy_for_state(state, effective_settings.local_port).await;
    } else {
        append_startup_diagnostic(
            state,
            "system proxy disabled in settings, skipping enable step",
        );
    }

    if let Err(err) = crate::commands::git_proxy::sync_for_connection(state, &effective_settings) {
        report_git_proxy_failure(state, "Git proxy synchronization failed", &err);
    }

    append_startup_diagnostic(state, "singbox_start finished successfully");
    Ok(match startup_warning {
        Some(warning) => CommandResult::ok_with_warning(warning),
        None => CommandResult::ok(),
    })
}

pub(crate) async fn singbox_stop_impl(
    app: AppHandle,
    state: &AppState,
) -> Result<CommandResult, String> {
    let _latency_lifecycle = crate::commands::profiles::begin_latency_lifecycle(state).await;
    let _lifecycle_guard = state.lifecycle_lock.lock().await;
    append_startup_diagnostic(state, "singbox_stop invoked");
    *state.shutdown_in_progress.lock().await = true;

    // Cancel traffic polling
    if let Some(cancel) = state.traffic_cancel.lock().await.take() {
        cancel.cancel();
    }
    cancel_health_monitor(state.health_cancel.clone()).await;

    *state.proxy_state.lock().await = ProxyState::Disconnecting;
    let _ = app.emit("singbox:state", "disconnecting");

    // Kill process from state
    if let Some(mut child) = state.singbox_process.lock().await.take() {
        match child.try_wait() {
            Ok(Some(status)) => {
                append_startup_diagnostic(
                    state,
                    &format!("singbox_stop: child already exited with status {}", status),
                );
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
                append_startup_diagnostic(
                    state,
                    "singbox_stop: child killed and waited successfully",
                );
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

    let cleanup_result =
        disable_system_proxy_for_state(state, ProxyCleanupMode::RestoreSnapshot).await;
    if cleanup_result.is_err() {
        let _ = disable_system_proxy_for_state(state, ProxyCleanupMode::ForceClear).await;
    }

    restore_git_proxy(state);

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
pub async fn singbox_start(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CommandResult, String> {
    singbox_start_impl(app, &state).await
}

#[tauri::command]
pub async fn singbox_stop(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CommandResult, String> {
    singbox_stop_impl(app, &state).await
}

#[tauri::command]
pub async fn singbox_restart(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<CommandResult, String> {
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
pub async fn singbox_switch_node(
    app: AppHandle,
    state: State<'_, AppState>,
    node_tag: String,
) -> Result<CommandResult, String> {
    let mut profiles_data = load_profiles_data_from_file(&state).await;
    let active_profile_id = match profiles_data.active_profile_id.clone() {
        Some(id) => id,
        None => return Ok(CommandResult::err("No active profile")),
    };
    let previous_active_node_tag = profiles_data.active_node_tag.clone();

    profiles_data.active_node_tag = Some(node_tag.clone());
    let profiles_content =
        serde_json::to_string_pretty(&profiles_data).map_err(|e| e.to_string())?;
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

    let previous_signature = node_bootstrap_signature(active_or_first_node(
        &raw_nodes,
        previous_active_node_tag.as_deref(),
    ));
    let target_signature =
        node_bootstrap_signature(active_or_first_node(&raw_nodes, Some(&node_tag)));

    if previous_signature != target_signature {
        return singbox_restart(app, state).await;
    }

    match switch_selector_to_node(
        &local_clash_api_client(),
        get_clash_api_port(&state).await,
        "PROXY",
        &node_tag,
    )
    .await
    {
        Ok(()) => Ok(CommandResult::ok()),
        Err(err) => Ok(CommandResult::err(err)),
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
    matches!(
        node_type,
        "shadowsocks"
            | "vmess"
            | "vless"
            | "trojan"
            | "hysteria"
            | "hysteria2"
            | "tuic"
            | "anytls"
            | "http"
            | "socks"
            | "wireguard"
            | "ssh"
            | "shadowtls"
            | "naive"
    )
}

fn sanitize_naive_tls(obj: &mut serde_json::Map<String, serde_json::Value>, server: &str) {
    let mut tls = obj
        .remove("tls")
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();

    let allowed_keys = [
        "enabled",
        "server_name",
        "certificate",
        "certificate_path",
        "ech",
    ];
    tls.retain(|key, _| allowed_keys.contains(&key.as_str()));
    tls.insert("enabled".to_string(), serde_json::Value::Bool(true));

    if !server.is_empty() && !tls.contains_key("server_name") {
        tls.insert(
            "server_name".to_string(),
            serde_json::Value::String(server.to_string()),
        );
    }

    obj.insert("tls".to_string(), serde_json::Value::Object(tls));
}

fn outbound_server_uses_domain(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    let server = obj
        .get("server")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']');

    !server.is_empty() && server.parse::<std::net::IpAddr>().is_err()
}

fn apply_outbound_domain_resolver(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    resolver_tag: &str,
) {
    if !outbound_server_uses_domain(obj) {
        return;
    }

    obj.entry("domain_strategy".to_string())
        .or_insert_with(|| serde_json::Value::String("ipv4_only".to_string()));

    if !obj.contains_key("domain_resolver") {
        obj.insert(
            "domain_resolver".to_string(),
            serde_json::json!({
                "server": resolver_tag,
                "strategy": "ipv4_only"
            }),
        );
    }
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
    apply_outbound_domain_resolver(obj, "dns-bootstrap");
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

fn config_file_has_outbound_type(
    config_path: &std::path::Path,
    outbound_type: &str,
) -> Result<bool, String> {
    let content = fs::read_to_string(config_path).map_err(|e| e.to_string())?;
    let config: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
    Ok(config_value_has_outbound_type(&config, outbound_type))
}

#[cfg(windows)]
fn support_file_available_for_executable(
    executable_path: &std::path::Path,
    filename: &str,
) -> bool {
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
        let node_type = obj
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        let server = obj
            .get("server")
            .and_then(|s| s.as_str())
            .unwrap_or("")
            .to_string();
        let port = obj.get("server_port").and_then(|p| p.as_u64()).unwrap_or(0);
        let tls_insecure = obj
            .get("skip-cert-verify")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        for key in [
            crate::commands::profiles::ECH_DNS_SERVER_META_KEY,
            NODE_AUTO_SELECTION_ELIGIBLE_META_KEY,
            NODE_METERED_PROTECTED_META_KEY,
        ] {
            obj.remove(key);
        }
        for key in NODE_RUNTIME_META_KEYS {
            obj.remove(key);
        }

        if node_type != "shadowsocks" && node_type != "shadowsocksr" {
            obj.remove("method");
        }

        if !obj.contains_key("tls") {
            let tls_server_name = obj
                .get("servername")
                .or_else(|| obj.get("sni"))
                .and_then(|value| value.as_str())
                .filter(|value| !value.is_empty())
                .map(|value| value.to_string())
                .unwrap_or_else(|| server.clone());
            match node_type.as_str() {
                "hysteria2" | "hysteria" | "tuic" | "naive" | "anytls" => {
                    obj.insert(
                        "tls".to_string(),
                        serde_json::json!({
                            "enabled": true,
                            "server_name": tls_server_name,
                            "insecure": tls_insecure
                        }),
                    );
                }
                "vless" | "vmess" | "trojan" => {
                    if port == 443 || port == 8443 || port == 2053 {
                        obj.insert(
                            "tls".to_string(),
                            serde_json::json!({
                                "enabled": true,
                                "server_name": tls_server_name,
                                "insecure": tls_insecure
                            }),
                        );
                    }
                }
                _ => {}
            }
        }

        if node_type == "anytls" {
            for key in ["sni", "servername", "skip-cert-verify", "udp"] {
                obj.remove(key);
            }
        }

        if node_type == "vless" && !obj.contains_key("packet_encoding") {
            obj.insert(
                "packet_encoding".to_string(),
                serde_json::Value::String("xudp".to_string()),
            );
        }

        if node_type == "naive" {
            sanitize_naive_outbound(obj, &server);
        }

        if is_proxy_type(&node_type) {
            apply_outbound_domain_resolver(obj, "dns-bootstrap");
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
    let mut processed = process_node(node);
    if !is_xray_bridge_node(&processed) {
        return processed;
    }

    let tag = processed
        .get("tag")
        .and_then(|value| value.as_str())
        .unwrap_or("xray-plugin")
        .to_string();
    let bridge_index = bridge_specs.len();
    let port = plugin_bridge_port(bridge_index);
    let detour = processed
        .get("detour")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string);
    let detour_chain_port = detour
        .as_ref()
        .map(|_| PLUGIN_DETOUR_CHAIN_PORT_BASE.saturating_add(bridge_index as u16));

    if let Some(obj) = processed.as_object_mut() {
        obj.remove("detour");
    }
    let mut spec = serde_json::json!({
        "core": "xray",
        "tag": tag.clone(),
        "listen": "127.0.0.1",
        "port": port,
        "node": processed
    });
    if let Some(chain_port) = detour_chain_port {
        spec["frontProxyChainPort"] = serde_json::json!(chain_port);
    }
    if let Some(detour) = detour {
        spec["frontProxyTag"] = serde_json::Value::String(detour);
    }
    bridge_specs.push(spec);

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
    Ok(resource_dir
        .join("resources")
        .join("libs")
        .join(XRAY_PLUGIN_FILENAME))
}

fn xray_tls_settings(tls: &serde_json::Value) -> serde_json::Value {
    let mut settings = serde_json::Map::new();

    if let Some(server_name) = tls.get("server_name").and_then(|value| value.as_str()) {
        settings.insert(
            "serverName".to_string(),
            serde_json::Value::String(server_name.to_string()),
        );
    }
    if let Some(insecure) = tls.get("insecure").and_then(|value| value.as_bool()) {
        settings.insert(
            "allowInsecure".to_string(),
            serde_json::Value::Bool(insecure),
        );
    }
    if let Some(alpn) = tls.get("alpn").and_then(|value| value.as_array()) {
        settings.insert("alpn".to_string(), serde_json::Value::Array(alpn.clone()));
    }
    if let Some(fingerprint) = tls
        .get("utls")
        .and_then(|value| value.get("fingerprint"))
        .and_then(|value| value.as_str())
    {
        settings.insert(
            "fingerprint".to_string(),
            serde_json::Value::String(fingerprint.to_string()),
        );
    }

    if let Some(reality) = tls.get("reality").and_then(|value| value.as_object()) {
        if let Some(public_key) = reality.get("public_key").and_then(|value| value.as_str()) {
            settings.insert(
                "publicKey".to_string(),
                serde_json::Value::String(public_key.to_string()),
            );
        }
        if let Some(short_id) = reality.get("short_id").and_then(|value| value.as_str()) {
            settings.insert(
                "shortId".to_string(),
                serde_json::Value::String(short_id.to_string()),
            );
        }
    }

    serde_json::Value::Object(settings)
}

fn vless_encryption(node: &serde_json::Value) -> &str {
    node.get("encryption")
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
    stream.insert(
        "network".to_string(),
        serde_json::Value::String(network.to_string()),
    );

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
        stream.insert(
            "xhttpSettings".to_string(),
            serde_json::Value::Object(xhttp),
        );
    }

    if let Some(tls) = node.get("tls").filter(|value| {
        value
            .get("enabled")
            .and_then(|enabled| enabled.as_bool())
            .unwrap_or(false)
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
        stream.insert(
            "security".to_string(),
            serde_json::Value::String(security.to_string()),
        );
        stream.insert(format!("{}Settings", security), xray_tls_settings(tls));
    } else {
        stream.insert(
            "security".to_string(),
            serde_json::Value::String("none".to_string()),
        );
    }

    serde_json::Value::Object(stream)
}

pub(crate) fn build_xray_plugin_config(
    node: &serde_json::Value,
    port: u16,
    front_proxy_chain_port: Option<u16>,
) -> Result<serde_json::Value, String> {
    let server = node
        .get("server")
        .and_then(|value| value.as_str())
        .ok_or("Xray plugin node missing server")?;
    let server_port = node
        .get("server_port")
        .and_then(|value| value.as_u64())
        .ok_or("Xray plugin node missing server_port")?;
    let uuid = node
        .get("uuid")
        .and_then(|value| value.as_str())
        .ok_or("Xray plugin node missing uuid")?;

    let encryption = vless_encryption(node);
    let mut user = serde_json::json!({
        "id": uuid,
        "encryption": encryption
    });
    if let Some(flow) = node
        .get("flow")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
    {
        user["flow"] = serde_json::Value::String(flow.to_string());
    }

    let mut remote_outbound = serde_json::json!({
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
    });
    let mut outbounds = vec![remote_outbound.clone()];
    if let Some(chain_port) = front_proxy_chain_port {
        remote_outbound["proxySettings"] = serde_json::json!({
            "tag": "kunbox-front-proxy-bridge",
            "transportLayer": true
        });
        outbounds[0] = remote_outbound;
        outbounds.push(serde_json::json!({
            "tag": "kunbox-front-proxy-bridge",
            "protocol": "socks",
            "settings": {
                "servers": [
                    {
                        "address": "127.0.0.1",
                        "port": chain_port
                    }
                ]
            }
        }));
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
        "outbounds": outbounds
    }))
}

async fn stop_plugin_bridges(state: &AppState) {
    let mut processes = state.plugin_processes.lock().await;
    for mut child in processes.drain(..) {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}

pub(crate) fn parse_plugin_bridge_port(
    spec: &serde_json::Value,
    field: &str,
) -> Result<Option<u16>, String> {
    let Some(value) = spec.get(field) else {
        return Ok(None);
    };
    let raw = value
        .as_u64()
        .ok_or_else(|| format!("插件桥接端口字段 {} 必须是整数", field))?;
    let port = u16::try_from(raw)
        .ok()
        .filter(|port| *port > 0)
        .ok_or_else(|| format!("插件桥接端口字段 {} 超出有效范围", field))?;
    Ok(Some(port))
}

async fn start_plugin_bridges(app: &AppHandle, state: &AppState) -> Result<(), String> {
    stop_plugin_bridges(state).await;

    let bridge_path = plugin_bridge_path(state);
    if !bridge_path.exists() {
        return Ok(());
    }

    let content = fs::read_to_string(&bridge_path).map_err(|e| e.to_string())?;
    let specs: Vec<serde_json::Value> =
        serde_json::from_str(&content).map_err(|e| e.to_string())?;
    if specs.is_empty() {
        return Ok(());
    }

    let xray_path = xray_plugin_path(app)?;
    if !xray_path.exists() {
        return Err("检测到 xhttp 节点，但未找到 Xray 插件核心。请将 xray.exe 放到应用数据目录的 libs 目录。".to_string());
    }

    let mut started = Vec::new();
    for (index, spec) in specs.iter().enumerate() {
        let core = spec
            .get("core")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        if core != "xray" {
            continue;
        }

        let port = parse_plugin_bridge_port(spec, "port")?
            .ok_or_else(|| "插件桥接缺少端口".to_string())?;
        let node = spec.get("node").ok_or("Plugin bridge missing node")?;
        let front_proxy_chain_port = parse_plugin_bridge_port(spec, "frontProxyChainPort")?;
        let config = build_xray_plugin_config(node, port, front_proxy_chain_port)?;
        let config_path = state.config_dir.join(format!("plugin-xray-{}.json", index));
        let config_str = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        fs::write(&config_path, config_str).map_err(|e| e.to_string())?;
        let config_path_str = config_path
            .to_str()
            .ok_or("Xray plugin config path contains invalid UTF-8")?;

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
    let (server_type, server_with_path, default_port) =
        if let Some(v) = value.strip_prefix("udp://") {
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

pub(crate) fn build_dns_bootstrap_server() -> serde_json::Value {
    build_dns_server("https://223.5.5.5/dns-query", "dns-bootstrap", "direct")
}

fn dns_server_uses_domain_address(address: &str) -> bool {
    let server = build_dns_server(address, "dns-probe", "direct");
    let Some(host) = server.get("server").and_then(|value| value.as_str()) else {
        return false;
    };
    let host = host.trim().trim_start_matches('[').trim_end_matches(']');

    !host.is_empty() && host.parse::<std::net::IpAddr>().is_err()
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

fn active_or_first_node<'a>(
    raw_nodes: &'a [serde_json::Value],
    active_node_tag: Option<&str>,
) -> Option<&'a serde_json::Value> {
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
            obj.insert(
                "action".to_string(),
                serde_json::Value::String("reject".to_string()),
            );
        } else {
            obj.insert(
                "outbound".to_string(),
                serde_json::Value::String(target.to_string()),
            );
        }
    }

    rule
}

fn plugin_bridge_remote_direct_rule(spec: &serde_json::Value) -> Option<serde_json::Value> {
    if spec.get("frontProxyChainPort").is_some() {
        return None;
    }
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
    if spec.get("frontProxyChainPort").is_some() {
        return None;
    }
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

pub(crate) fn parse_profile_scoped_node_ref(value: &str) -> Option<(&str, &str)> {
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

fn runtime_detour_tag(value: &str, owner_profile_id: &str, active_profile_id: &str) -> String {
    match parse_profile_scoped_node_ref(value) {
        Some((profile_id, node_tag)) if profile_id == active_profile_id => node_tag.to_string(),
        Some((profile_id, node_tag)) => format!("{}::{}", profile_id, node_tag),
        None if owner_profile_id == active_profile_id => value.to_string(),
        None => format!("{}::{}", owner_profile_id, value),
    }
}

fn prepare_profile_node_for_runtime(
    mut node: serde_json::Value,
    owner_profile_id: &str,
    active_profile_id: &str,
) -> serde_json::Value {
    if let Some(obj) = node.as_object_mut() {
        if let Some(detour) = obj
            .get("detour")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            obj.insert(
                "detour".to_string(),
                serde_json::Value::String(runtime_detour_tag(
                    detour,
                    owner_profile_id,
                    active_profile_id,
                )),
            );
        }
    }
    node
}

fn collect_node_detour_references(
    active_nodes: &[serde_json::Value],
    active_profile_id: &str,
    all_profiles: &[ProfileInfo],
) -> std::collections::HashSet<String> {
    let mut references = std::collections::HashSet::new();
    for node in active_nodes {
        let prepared =
            prepare_profile_node_for_runtime(node.clone(), active_profile_id, active_profile_id);
        if let Some(detour) = prepared.get("detour").and_then(serde_json::Value::as_str) {
            if parse_profile_scoped_node_ref(detour).is_some() {
                references.insert(detour.to_string());
            }
        }
    }

    loop {
        let mut changed = false;
        for node_ref in references.clone() {
            let Some((profile_id, node_tag)) = parse_profile_scoped_node_ref(&node_ref) else {
                continue;
            };
            let Some(profile) = all_profiles.iter().find(|profile| profile.id == profile_id) else {
                continue;
            };
            let Some(node) = profile
                .nodes
                .iter()
                .find(|node| node.get("tag").and_then(serde_json::Value::as_str) == Some(node_tag))
            else {
                continue;
            };
            let prepared =
                prepare_profile_node_for_runtime(node.clone(), profile_id, active_profile_id);
            if let Some(detour) = prepared.get("detour").and_then(serde_json::Value::as_str) {
                if parse_profile_scoped_node_ref(detour).is_some() {
                    changed |= references.insert(detour.to_string());
                }
            }
        }
        if !changed {
            break;
        }
    }
    references
}

fn collect_route_profile_and_node_references(
    rulesets: &[crate::types::RuleSet],
    custom_rules: &crate::types::CustomRules,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    let mut referenced_profile_ids = std::collections::HashSet::new();
    let mut referenced_profile_scoped_node_refs = std::collections::HashSet::new();

    for rs in rulesets.iter().filter(|r| r.enabled) {
        if let Some(ref value) = rs.outbound_value {
            match rs.outbound_mode.as_str() {
                "profile" | "配置" => {
                    referenced_profile_ids.insert(value.clone());
                }
                "node" | "节点" => {
                    if parse_profile_scoped_node_ref(value).is_some() {
                        referenced_profile_scoped_node_refs.insert(value.clone());
                    }
                }
                _ => {}
            }
        }
    }

    for rule in custom_rules.domain_rules.iter().filter(|r| r.enabled) {
        if let Some(ref value) = rule.outbound_value {
            match rule.outbound_mode.as_str() {
                "profile" => {
                    referenced_profile_ids.insert(value.clone());
                }
                "node" => {
                    if parse_profile_scoped_node_ref(value).is_some() {
                        referenced_profile_scoped_node_refs.insert(value.clone());
                    }
                }
                _ => {}
            }
        }
    }

    (referenced_profile_ids, referenced_profile_scoped_node_refs)
}

fn selector_tag_collides(
    candidate: &str,
    nodes: &[serde_json::Value],
    referenced_profile_scoped_node_refs: &std::collections::HashSet<String>,
    referenced_profile_ids: &std::collections::HashSet<String>,
) -> bool {
    nodes
        .iter()
        .any(|node| node.get("tag").and_then(|tag| tag.as_str()) == Some(candidate))
        || referenced_profile_scoped_node_refs.iter().any(|node_ref| {
            parse_profile_scoped_node_ref(node_ref)
                .map(|(_, node_tag)| node_tag == candidate)
                .unwrap_or(false)
        })
        || referenced_profile_ids
            .iter()
            .any(|profile_id| profile_selector_tag(profile_id) == candidate)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HealthTargetKind {
    Selector,
    FixedNode,
}

#[derive(Debug, Clone)]
struct HealthTarget {
    kind: HealthTargetKind,
    selector_tag: Option<String>,
    node_tag: Option<String>,
    rule_label: Option<String>,
    auto_failover: bool,
}

#[derive(Debug, Clone)]
struct NodeHealth {
    status: HealthStatus,
    last_latency_ms: Option<u32>,
    success_streak: u8,
    failure_streak: u8,
    last_checked_at: i64,
    next_probe_after: i64,
    cooldown_until: Option<i64>,
    last_error: Option<String>,
}

impl NodeHealth {
    fn new(_tag: String) -> Self {
        Self {
            status: HealthStatus::Unknown,
            last_latency_ms: None,
            success_streak: 0,
            failure_streak: 0,
            last_checked_at: 0,
            next_probe_after: 0,
            cooldown_until: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone)]
struct SelectorHealth {
    selector_tag: String,
    current_node: Option<String>,
    current_auto_selection_eligible: bool,
    backup_nodes: Vec<String>,
    last_switch_at: Option<i64>,
    switch_cooldown_until: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HealthAction {
    None,
    SwitchSelector {
        selector: String,
        from: String,
        to: String,
    },
    NotifyFixedNodeFailed {
        node: String,
        rule: Option<String>,
    },
    NotifyMainNodeNeedsManualSwitch {
        selector: String,
        from: String,
        to: String,
        reason: String,
    },
    NotifyNoBackup {
        selector: String,
    },
}

fn failed_probe_backoff_ms(failure_streak: u8) -> i64 {
    let exponent = failure_streak.saturating_sub(3).min(3) as u32;
    (HEALTH_FAILED_BACKOFF_BASE_MS * 2_i64.pow(exponent)).min(HEALTH_FAILED_BACKOFF_MAX_MS)
}

fn record_probe_success(health: &mut NodeHealth, latency_ms: u32, now_ms: i64) {
    health.success_streak = health.success_streak.saturating_add(1);
    health.failure_streak = 0;
    health.last_latency_ms = Some(latency_ms);
    health.last_checked_at = now_ms;
    health.next_probe_after = now_ms;
    health.last_error = None;
    health.status = if health.success_streak >= 2 {
        HealthStatus::Healthy
    } else {
        HealthStatus::Recovering
    };
}

fn record_probe_failure(health: &mut NodeHealth, error: String, now_ms: i64) {
    health.failure_streak = health.failure_streak.saturating_add(1);
    health.success_streak = 0;
    health.last_checked_at = now_ms;
    health.last_error = Some(error);

    if health.failure_streak >= 3 {
        health.status = HealthStatus::Failed;
        health.next_probe_after = now_ms + failed_probe_backoff_ms(health.failure_streak);
    } else {
        health.status = HealthStatus::Suspect;
        health.next_probe_after = now_ms;
    }
}

fn should_probe(health: &NodeHealth, now_ms: i64) -> bool {
    if health
        .cooldown_until
        .is_some_and(|cooldown_until| now_ms < cooldown_until)
    {
        return false;
    }

    now_ms >= health.next_probe_after
}

fn select_backup_probe_candidates(
    backup_nodes: &[String],
    node_health: &std::collections::HashMap<String, NodeHealth>,
    now_ms: i64,
    limit: usize,
) -> Vec<String> {
    let mut candidates = backup_nodes
        .iter()
        .enumerate()
        .filter_map(|(index, tag)| {
            let health = node_health.get(tag);
            if health.is_some_and(|health| !should_probe(health, now_ms)) {
                return None;
            }
            let priority = match health.map(|health| &health.status) {
                Some(HealthStatus::Recovering) => 0,
                None | Some(HealthStatus::Unknown) => 1,
                Some(HealthStatus::Suspect) => 2,
                Some(HealthStatus::Failed) => 3,
                Some(HealthStatus::Healthy) => 4,
            };
            let last_checked_at = health.map_or(0, |health| health.last_checked_at);
            Some((priority, last_checked_at, index, tag.clone()))
        })
        .collect::<Vec<_>>();
    candidates
        .sort_by_key(|(priority, last_checked_at, index, _)| (*priority, *last_checked_at, *index));
    candidates
        .into_iter()
        .take(limit)
        .map(|(_, _, _, tag)| tag)
        .collect()
}

fn decide_health_action(
    target: &HealthTarget,
    selector: Option<&SelectorHealth>,
    node_health: &std::collections::HashMap<String, NodeHealth>,
    now_ms: i64,
) -> HealthAction {
    if target.kind == HealthTargetKind::FixedNode {
        let Some(node_tag) = target.node_tag.as_deref() else {
            return HealthAction::None;
        };
        return match node_health.get(node_tag) {
            Some(health)
                if health.status == HealthStatus::Failed && health.last_checked_at == now_ms =>
            {
                HealthAction::NotifyFixedNodeFailed {
                    node: node_tag.to_string(),
                    rule: target.rule_label.clone(),
                }
            }
            _ => HealthAction::None,
        };
    }

    if !target.auto_failover {
        return HealthAction::None;
    }

    let Some(selector) = selector else {
        return HealthAction::None;
    };
    if selector
        .switch_cooldown_until
        .is_some_and(|cooldown_until| now_ms < cooldown_until)
    {
        return HealthAction::None;
    }

    let selector_tag = target
        .selector_tag
        .as_deref()
        .unwrap_or(selector.selector_tag.as_str());
    let Some(current_node) = selector.current_node.as_deref() else {
        return HealthAction::None;
    };
    if !selector.current_auto_selection_eligible && is_main_selector_tag(selector_tag) {
        return HealthAction::None;
    }
    let current_failed = node_health.get(current_node).is_some_and(|health| {
        health.status == HealthStatus::Failed && health.last_checked_at == now_ms
    });
    if selector.current_auto_selection_eligible && !current_failed {
        return HealthAction::None;
    }

    let best_backup = selector
        .backup_nodes
        .iter()
        .filter(|node| node.as_str() != current_node)
        .filter_map(|node| {
            let health = node_health.get(node)?;
            (health.status == HealthStatus::Healthy)
                .then_some((node, health.last_latency_ms.unwrap_or(u32::MAX)))
        })
        .min_by_key(|(_, latency)| *latency)
        .map(|(node, _)| node.clone());

    match best_backup {
        Some(to) => HealthAction::SwitchSelector {
            selector: selector_tag.to_string(),
            from: current_node.to_string(),
            to,
        },
        None if selector.current_auto_selection_eligible => HealthAction::NotifyNoBackup {
            selector: selector_tag.to_string(),
        },
        None => HealthAction::None,
    }
}

fn is_main_selector_tag(selector: &str) -> bool {
    matches!(selector, "PROXY" | "PROXY-kb")
}

fn gate_main_selector_health_action(
    action: HealthAction,
    settings: &AppSettings,
    previous_signature: (bool, Option<String>),
    target_signature: (bool, Option<String>),
) -> HealthAction {
    let HealthAction::SwitchSelector { selector, from, to } = action else {
        return action;
    };

    if !is_main_selector_tag(&selector) {
        return HealthAction::SwitchSelector { selector, from, to };
    }

    if !settings.main_node_auto_failover {
        return HealthAction::NotifyMainNodeNeedsManualSwitch {
            selector,
            from,
            to,
            reason: "主节点故障自动切换未开启".to_string(),
        };
    }

    if previous_signature != target_signature {
        return HealthAction::NotifyMainNodeNeedsManualSwitch {
            selector,
            from,
            to,
            reason: "目标节点需要不同的 DNS bootstrap，未自动切换".to_string(),
        };
    }

    HealthAction::SwitchSelector { selector, from, to }
}

fn health_event_for_action(action: &HealthAction) -> Option<HealthEvent> {
    match action {
        HealthAction::None => None,
        HealthAction::SwitchSelector { selector, from, to } => Some(HealthEvent {
            kind: HealthEventKind::SelectorFailedOver,
            selector: Some(selector.clone()),
            from: Some(from.clone()),
            to: Some(to.clone()),
            node: None,
            rule: None,
            message: format!("分流 {} 已从 {} 自动切换到 {}", selector, from, to),
        }),
        HealthAction::NotifyFixedNodeFailed { node, rule } => Some(HealthEvent {
            kind: HealthEventKind::FixedNodeFailed,
            selector: None,
            from: None,
            to: None,
            node: Some(node.clone()),
            rule: rule.clone(),
            message: format!(
                "分流节点不可用：{}。该规则绑定了固定节点，KunBox 未自动更换，请手动调整规则或节点。",
                node
            ),
        }),
        HealthAction::NotifyMainNodeNeedsManualSwitch { selector, from, to, reason } => Some(HealthEvent {
            kind: HealthEventKind::MainNodeNeedsManualSwitch,
            selector: Some(selector.clone()),
            from: Some(from.clone()),
            to: Some(to.clone()),
            node: Some(from.clone()),
            rule: None,
            message: format!("主节点 {} 不可用，未自动切换到 {}：{}", from, to, reason),
        }),
        HealthAction::NotifyNoBackup { selector } => Some(HealthEvent {
            kind: HealthEventKind::SelectorNoBackup,
            selector: Some(selector.clone()),
            from: None,
            to: None,
            node: None,
            rule: None,
            message: format!("分流 {} 当前节点不可用，暂无健康备用节点。", selector),
        }),
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
        obj.insert(
            "tag".to_string(),
            serde_json::Value::String(tag.to_string()),
        );
    }
    node
}

fn is_valid_profile_id(profile_id: &str) -> bool {
    !profile_id.is_empty()
        && profile_id.len() <= 64
        && profile_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn is_valid_ruleset_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 128
        && tag
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
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

fn allocate_unique_outbound_tag(
    base: &str,
    used_tags: &std::collections::HashSet<String>,
) -> String {
    if !used_tags.contains(base) {
        return base.to_string();
    }

    let mut suffix = 1u32;
    loop {
        let candidate = format!("{}-{}", base, suffix);
        if !used_tags.contains(&candidate) {
            return candidate;
        }
        suffix += 1;
    }
}

/// 加载所有配置文件的节点信息
fn load_all_profiles(
    state: &AppState,
    profiles_data: &crate::types::ProfilesData,
) -> Vec<ProfileInfo> {
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

async fn generate_config_with_settings(
    state: &AppState,
    settings: &crate::types::AppSettings,
) -> Result<CommandResult, String> {
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
    let mut raw_nodes: Vec<serde_json::Value> =
        serde_json::from_str(&nodes_content).map_err(|e| e.to_string())?;

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
                            if let Ok(repaired_raw_nodes) =
                                serde_json::from_str::<Vec<serde_json::Value>>(&repaired_content)
                            {
                                raw_nodes = repaired_raw_nodes;
                                log::info!(
                                    "Repaired legacy ECH subscription nodes for profile '{}'",
                                    profile.name
                                );
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(err) => {
                    log::warn!(
                        "Failed to refresh legacy ECH nodes for profile '{}': {}",
                        profile.name,
                        err
                    );
                }
            }
        }
    }

    let auto_eligible_active_tags: std::collections::HashSet<String> = raw_nodes
        .iter()
        .filter(|node| node_is_auto_selection_eligible(node))
        .filter_map(|node| node.get("tag").and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect();

    // 处理当前配置的节点，并过滤 sing-box 不支持的类型，避免单个无效节点拖垮整份配置。
    let nodes: Vec<serde_json::Value> = raw_nodes
        .iter()
        .map(|node| {
            prepare_profile_node_for_runtime(node.clone(), &active_profile_id, &active_profile_id)
        })
        .map(|node| process_node(&node))
        .filter(|node| {
            node.get("type")
                .and_then(|value| value.as_str())
                .is_some_and(is_proxy_type)
        })
        .collect();

    if nodes.is_empty() {
        return Ok(CommandResult::err("当前配置没有可用的受支持代理节点"));
    }

    let active_node_tag = profiles_data
        .active_node_tag
        .clone()
        .filter(|tag| {
            nodes
                .iter()
                .any(|node| node.get("tag").and_then(|value| value.as_str()) == Some(tag.as_str()))
        })
        .or_else(|| {
            nodes
                .first()
                .and_then(|n| n.get("tag").and_then(|t| t.as_str()).map(|s| s.to_string()))
        });

    let inferred_ech_dns_server =
        extract_ech_dns_server_override(&raw_nodes, active_node_tag.as_deref());
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
    let remote_dns_domain_resolver =
        if active_node_has_ech || dns_server_uses_domain_address(&effective_remote_dns) {
            Some("dns-bootstrap")
        } else {
            None
        };

    // 加载所有配置文件信息（用于跨配置分流）
    let all_profiles = load_all_profiles(state, &profiles_data);

    // 收集规则集引用的 profile ID 和 node tag
    let enabled_rulesets: Vec<_> = rulesets.iter().filter(|r| r.enabled).collect();
    let (referenced_profile_ids, mut referenced_profile_scoped_node_refs) =
        collect_route_profile_and_node_references(&rulesets, &custom_rules);
    let runtime_detour_nodes = raw_nodes.clone();
    referenced_profile_scoped_node_refs.extend(collect_node_detour_references(
        &runtime_detour_nodes,
        &active_profile_id,
        &all_profiles,
    ));

    // Pre-scan for tag collisions to avoid conflict with node tags named "PROXY" or "auto"
    let will_proxy_collide = selector_tag_collides(
        "PROXY",
        &nodes,
        &referenced_profile_scoped_node_refs,
        &referenced_profile_ids,
    );
    let will_auto_collide = selector_tag_collides(
        "auto",
        &nodes,
        &referenced_profile_scoped_node_refs,
        &referenced_profile_ids,
    );

    let proxy_tag = if will_proxy_collide {
        "PROXY-kb"
    } else {
        "PROXY"
    };
    let auto_tag = if will_auto_collide { "auto-kb" } else { "auto" };
    let remote_dns_detour = if active_node_has_ech {
        "direct"
    } else {
        proxy_tag
    };

    // Build config - 使用 sing-box 1.11+ 新格式
    let listen_addr = if settings.allow_lan {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    };

    let routing_mode = settings.routing_mode.as_str();

    // 构建 DNS 服务器列表（sing-box 1.12+ 新格式）
    let mut dns_servers = vec![
        build_dns_bootstrap_server(),
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
        if !direct_domains.is_empty()
            || !direct_domain_suffixes.is_empty()
            || !direct_domain_keywords.is_empty()
        {
            let mut dns_rule = serde_json::Map::new();
            if !direct_domains.is_empty() {
                dns_rule.insert("domain".to_string(), serde_json::json!(direct_domains));
            }
            if !direct_domain_suffixes.is_empty() {
                dns_rule.insert(
                    "domain_suffix".to_string(),
                    serde_json::json!(direct_domain_suffixes),
                );
            }
            if !direct_domain_keywords.is_empty() {
                dns_rule.insert(
                    "domain_keyword".to_string(),
                    serde_json::json!(direct_domain_keywords),
                );
            }
            dns_rule.insert("server".to_string(), serde_json::json!("dns-local"));
            dns_rules.push(serde_json::Value::Object(dns_rule));
            log::info!(
                "Added DNS rule for direct domains: {} domain, {} suffix, {} keyword",
                direct_domains.len(),
                direct_domain_suffixes.len(),
                direct_domain_keywords.len()
            );
        }

        // 生成 proxy 域名的 DNS 规则 → dns-remote
        if !proxy_domains.is_empty()
            || !proxy_domain_suffixes.is_empty()
            || !proxy_domain_keywords.is_empty()
        {
            let mut dns_rule = serde_json::Map::new();
            if !proxy_domains.is_empty() {
                dns_rule.insert("domain".to_string(), serde_json::json!(proxy_domains));
            }
            if !proxy_domain_suffixes.is_empty() {
                dns_rule.insert(
                    "domain_suffix".to_string(),
                    serde_json::json!(proxy_domain_suffixes),
                );
            }
            if !proxy_domain_keywords.is_empty() {
                dns_rule.insert(
                    "domain_keyword".to_string(),
                    serde_json::json!(proxy_domain_keywords),
                );
            }
            dns_rule.insert("server".to_string(), serde_json::json!("dns-remote"));
            dns_rules.push(serde_json::Value::Object(dns_rule));
            log::info!(
                "Added DNS rule for proxy domains: {} domain, {} suffix, {} keyword",
                proxy_domains.len(),
                proxy_domain_suffixes.len(),
                proxy_domain_keywords.len()
            );
        }
    }

    // ========== 根据规则集(ruleset)生成对应的 DNS 规则 ==========
    // 必须位于域名分流规则之后，保证域名分流优先匹配。
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
                "proxy" | "node" | "节点" | "profile" | "配置" => {
                    proxy_rulesets.push(rs.tag.clone())
                }
                _ => {} // block 不需要 DNS 规则
            }
        }

        if !direct_rulesets.is_empty() {
            dns_rules.push(serde_json::json!({
                "rule_set": direct_rulesets,
                "server": "dns-local"
            }));
            log::info!(
                "Added DNS rule for {} direct rulesets",
                direct_rulesets.len()
            );
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
        // TUN 网卡仅 IPv4：FakeIP 不分配 IPv6 段，AAAA 走系统原生解析/链路。
        let mut fakeip_server = serde_json::json!({
            "tag": "dns-fakeip",
            "type": "fakeip",
            "inet4_range": "198.18.0.0/15"
        });
        if !settings.tun_enabled {
            fakeip_server["inet6_range"] = serde_json::json!("fc00::/18");
        }
        dns_servers.push(fakeip_server);
        let mut fake_dns_rule = serde_json::json!({
            "query_type": if settings.tun_enabled {
                serde_json::json!(["A"])
            } else {
                serde_json::json!(["A", "AAAA"])
            },
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
        }),
    ];

    // 如果启用 TUN 模式，添加 TUN inbound
    if settings.tun_enabled {
        // Windows 上 TUN IPv6 路径半残：不要把 v6 塞进隧道。
        // 策略：IPv4 走 TUN（优先）；IPv6 留给系统原生网卡作回退。MTU 1500 避免巨帧黑洞。
        inbounds.push(serde_json::json!({
            "type": "tun",
            "tag": "tun-in",
            "interface_name": "kunbox-tun",
            "address": ["172.19.0.1/30"],
            "mtu": 1500,
            "auto_route": true,
            "strict_route": settings.tun_strict_route,
            "stack": settings.tun_stack
        }));
    }

    let dns_final = match routing_mode {
        "global-direct" => "dns-local",
        _ => "dns-remote",
    };
    // TUN 开启时 prefer_ipv4：优先 A/IPv4 走隧道；纯 v6 或 v4 不可用时仍可解析 AAAA，走系统原生 IPv6。
    let mut dns_config = serde_json::json!({
        "servers": dns_servers,
        "rules": dns_rules,
        "final": dns_final,
        "independent_cache": true,
        "reverse_mapping": true
    });
    if settings.tun_enabled {
        dns_config["strategy"] = serde_json::json!("prefer_ipv4");
    }

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
            "default_domain_resolver": {
                "server": "dns-bootstrap",
                "strategy": "ipv4_only"
            },
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
            outbounds.push(node_for_singbox_with_plugin_bridge(
                node,
                &mut plugin_bridge_specs,
            ));
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
            if let Some(node) = profile
                .nodes
                .iter()
                .find(|n| n.get("tag").and_then(|t| t.as_str()) == Some(node_tag))
            {
                let node_type = node.get("type").and_then(|t| t.as_str()).unwrap_or("");
                if is_proxy_type(node_type) {
                    let prepared_node = prepare_profile_node_for_runtime(
                        node.clone(),
                        profile_id,
                        &active_profile_id,
                    );
                    let scoped_node = with_outbound_tag(prepared_node, &outbound_tag);
                    outbounds.push(node_for_singbox_with_plugin_bridge(
                        &scoped_node,
                        &mut plugin_bridge_specs,
                    ));
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
            let mut profile_proxy_entries: Vec<(String, String, bool)> = Vec::new();
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
                            let prepared_node = prepare_profile_node_for_runtime(
                                node.clone(),
                                &profile.id,
                                &active_profile_id,
                            );
                            let outbound_node = if outbound_tag == tag {
                                prepared_node
                            } else {
                                with_outbound_tag(process_node(&prepared_node), &outbound_tag)
                            };
                            outbounds.push(node_for_singbox_with_plugin_bridge(
                                &outbound_node,
                                &mut plugin_bridge_specs,
                            ));
                            existing_tags.insert(outbound_tag.clone());
                        }
                        profile_proxy_entries.push((
                            tag.to_string(),
                            outbound_tag,
                            node_is_auto_selection_eligible(node),
                        ));
                    }
                }
            }
            let profile_selector_tags: Vec<String> = profile_proxy_entries
                .iter()
                .filter(|(_, _, eligible)| *eligible)
                .map(|(_, outbound_tag, _)| outbound_tag.clone())
                .collect();

            // 创建 selector 类型（由应用层管理延迟测试和切换）
            if !profile_selector_tags.is_empty() {
                let selector_default = profiles_data
                    .node_selections
                    .get(&profile.id)
                    .and_then(|saved_tag| {
                        profile_proxy_entries
                            .iter()
                            .find(|(raw_tag, outbound_tag, eligible)| {
                                *eligible && (raw_tag == saved_tag || outbound_tag == saved_tag)
                            })
                            .map(|(_, outbound_tag, _)| outbound_tag.clone())
                    })
                    .or_else(|| {
                        (profiles_data.active_profile_id.as_deref() == Some(profile.id.as_str()))
                            .then(|| profiles_data.active_node_tag.clone())
                            .flatten()
                            .and_then(|active_tag| {
                                profile_proxy_entries
                                    .iter()
                                    .find(|(raw_tag, outbound_tag, eligible)| {
                                        *eligible
                                            && (raw_tag == &active_tag
                                                || outbound_tag == &active_tag)
                                    })
                                    .map(|(_, outbound_tag, _)| outbound_tag.clone())
                            })
                    })
                    .or_else(|| profile_selector_tags.first().cloned());

                outbounds.push(serde_json::json!({
                    "type": "selector",
                    "tag": selector_tag,
                    "outbounds": profile_selector_tags,
                    "default": selector_default,
                    "interrupt_exist_connections": true
                }));
                existing_tags.insert(selector_tag.clone());
                profile_id_to_selector.insert(profile_id.clone(), selector_tag.clone());
                log::info!(
                    "Created profile selector: {} with {} nodes",
                    selector_tag,
                    profile_selector_tags.len()
                );
            }
        }
    }

    // 4. 添加 PROXY selector（主选择器）
    let default_tag = active_node_tag.clone();
    if !proxy_tags.is_empty() {
        let proxy_outbounds: Vec<String> = proxy_tags.iter().cloned().collect();
        outbounds.insert(
            0,
            serde_json::json!({
                "type": "selector",
                "tag": proxy_tag,
                "outbounds": proxy_outbounds.clone(),
                "default": default_tag,
                "interrupt_exist_connections": true
            }),
        );
        existing_tags.insert(proxy_tag.to_string());
    }

    // 5. 添加 auto urltest（只包含允许自动探测与切换的节点）
    let auto_proxy_tags: Vec<String> = proxy_tags
        .iter()
        .filter(|tag| auto_eligible_active_tags.contains(tag.as_str()))
        .cloned()
        .collect();
    if auto_proxy_tags.len() > 1 {
        outbounds.push(serde_json::json!({
            "type": "urltest",
            "tag": auto_tag,
            "outbounds": auto_proxy_tags,
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

    let mut used_inbound_tags: std::collections::HashSet<String> = config["inbounds"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|inbound| inbound.get("tag").and_then(|tag| tag.as_str()))
        .map(str::to_string)
        .collect();
    let mut plugin_chain_routes = Vec::new();
    for spec in &plugin_bridge_specs {
        let Some(port) = spec
            .get("frontProxyChainPort")
            .and_then(serde_json::Value::as_u64)
        else {
            continue;
        };
        let Some(outbound_tag) = spec
            .get("frontProxyTag")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let inbound_tag =
            allocate_unique_outbound_tag("kunbox-front-proxy-chain-in", &used_inbound_tags);
        used_inbound_tags.insert(inbound_tag.clone());
        if let Some(inbounds) = config["inbounds"].as_array_mut() {
            inbounds.push(serde_json::json!({
                "type": "mixed",
                "tag": inbound_tag.clone(),
                "listen": "127.0.0.1",
                "listen_port": port
            }));
        }
        plugin_chain_routes.push((inbound_tag, outbound_tag.to_string()));
    }

    if let Some(dns_rules) = config["dns"]["rules"].as_array_mut() {
        for spec in &plugin_bridge_specs {
            if let Some(rule) = plugin_bridge_remote_dns_rule(spec) {
                dns_rules.insert(0, rule);
            }
        }
    }

    // 收集所有可用的 outbound tags
    let available_outbound_tags: std::collections::HashSet<String> = outbounds
        .iter()
        .filter_map(|o| o.get("tag").and_then(|t| t.as_str()).map(|s| s.to_string()))
        .collect();

    // ========== 构建路由规则 ==========
    let mut rules: Vec<serde_json::Value> = vec![
        serde_json::json!({ "inbound": "mixed-in", "action": "sniff" }),
        serde_json::json!({ "inbound": "socks-in", "action": "sniff" }),
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
    for (inbound_tag, outbound_tag) in plugin_chain_routes.into_iter().rev() {
        rules.insert(
            0,
            serde_json::json!({
                "inbound": inbound_tag,
                "outbound": outbound_tag
            }),
        );
    }

    // 预先声明规则集引用和缓存目录（广告屏蔽和用户规则集都需要）
    let mut rule_set_refs = Vec::new();

    if settings.tun_enabled {
        rules.insert(
            1,
            serde_json::json!({ "inbound": "tun-in", "action": "sniff" }),
        );
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
                        if let Some(node_tag) =
                            resolve_node_route_outbound(node_ref, &available_outbound_tags)
                        {
                            node_tag
                        } else {
                            log::warn!(
                                "Node '{}' not found for domain rule '{}', falling back to {}",
                                node_ref,
                                rule.value,
                                proxy_tag
                            );
                            proxy_tag.to_string()
                        }
                    } else {
                        proxy_tag.to_string()
                    }
                }
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
                            log::warn!(
                                "Profile '{}' not found for domain rule '{}', falling back to {}",
                                profile_id,
                                rule.value,
                                proxy_tag
                            );
                            proxy_tag.to_string()
                        }
                    } else {
                        proxy_tag.to_string()
                    }
                }
                other => other.to_string(),
            };

            let rule_json = match rule.rule_type.as_str() {
                "domain" => apply_route_target(
                    serde_json::json!({
                        "domain": [&rule.value]
                    }),
                    &outbound,
                ),
                "domain_suffix" => apply_route_target(
                    serde_json::json!({
                        "domain_suffix": [&rule.value]
                    }),
                    &outbound,
                ),
                "domain_keyword" => apply_route_target(
                    serde_json::json!({
                        "domain_keyword": [&rule.value]
                    }),
                    &outbound,
                ),
                _ => apply_route_target(
                    serde_json::json!({
                        "domain_suffix": [&rule.value]
                    }),
                    &outbound,
                ),
            };
            rules.push(rule_json);
            log::info!(
                "Added domain rule: {} ({}) -> {}",
                rule.value,
                rule.rule_type,
                outbound
            );
        }
    }

    // 添加规则集路由规则，必须位于域名分流规则之后，保证域名分流优先匹配。

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
                        if let Some(node_tag) =
                            resolve_node_route_outbound(node_ref, &available_outbound_tags)
                        {
                            node_tag
                        } else {
                            log::warn!(
                                "Node '{}' not found for ruleset '{}', falling back to {}",
                                node_ref,
                                rs.tag,
                                proxy_tag
                            );
                            proxy_tag.to_string()
                        }
                    } else {
                        proxy_tag.to_string()
                    }
                }
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
                            log::warn!(
                                "Profile '{}' not found for ruleset '{}', falling back to {}",
                                profile_id,
                                rs.tag,
                                proxy_tag
                            );
                            proxy_tag.to_string()
                        }
                    } else {
                        proxy_tag.to_string()
                    }
                }
                other => other.to_string(),
            };

            let rule_json = apply_route_target(
                serde_json::json!({
                    "rule_set": [rs.tag]
                }),
                &outbound,
            );
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
    let plugin_bridge_str =
        serde_json::to_string_pretty(&plugin_bridge_specs).map_err(|e| e.to_string())?;
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
        let guard = SYSTEM_PROXY_SNAPSHOT
            .lock()
            .map_err(|_| "系统代理快照锁失败".to_string())?;
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

    let mut guard = SYSTEM_PROXY_SNAPSHOT
        .lock()
        .map_err(|_| "系统代理快照锁失败".to_string())?;
    if guard.is_none() {
        *guard = Some(snapshot);
    }
    Ok(())
}

#[cfg(windows)]
async fn snapshot_system_proxy_if_needed_for_state(state: &AppState) -> Result<(), String> {
    let already_snapshotted = {
        let guard = SYSTEM_PROXY_SNAPSHOT
            .lock()
            .map_err(|_| "系统代理快照锁失败".to_string())?;
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

    let mut guard = SYSTEM_PROXY_SNAPSHOT
        .lock()
        .map_err(|_| "系统代理快照锁失败".to_string())?;
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
    if stderr.contains("unable to find") || stderr.contains("无法找到") || stderr.contains("找不到")
    {
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
fn save_persisted_proxy_snapshot(
    state: &AppState,
    snapshot: &SystemProxySnapshot,
) -> Result<(), String> {
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
    let persisted: PersistedSystemProxySnapshot =
        serde_json::from_slice(&content).map_err(|e| e.to_string())?;
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
        let mut guard = SYSTEM_PROXY_SNAPSHOT
            .lock()
            .map_err(|_| "系统代理快照锁失败".to_string())?;
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
        set_registry_value(
            "ProxyEnable",
            "REG_DWORD",
            snapshot.proxy_enable.as_deref().unwrap_or("0"),
        )
        .await?;

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
async fn disable_system_proxy_for_state(
    state: &AppState,
    mode: ProxyCleanupMode,
) -> Result<(), String> {
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
async fn disable_system_proxy_for_state(
    _state: &AppState,
    mode: ProxyCleanupMode,
) -> Result<(), String> {
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
        append_startup_diagnostic(
            state,
            "detected stale proxy configuration, forcing restore/clear",
        );
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

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

    fn rule_matches_inbound(rule: &serde_json::Value, tag: &str) -> bool {
        match rule.get("inbound") {
            Some(value) if value.as_str() == Some(tag) => true,
            Some(value) => value
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(tag))),
            None => false,
        }
    }

    fn has_sniff_rule_for_inbound(rules: &[serde_json::Value], tag: &str) -> bool {
        rules.iter().any(|rule| {
            rule.get("action").and_then(|action| action.as_str()) == Some("sniff")
                && rule_matches_inbound(rule, tag)
        })
    }

    #[test]
    fn detects_local_proxy_server_values() {
        assert!(looks_like_local_proxy_server("127.0.0.1:7890"));
        assert!(looks_like_local_proxy_server(
            "http=127.0.0.1:7890;https=127.0.0.1:7890"
        ));
        assert!(!looks_like_local_proxy_server("10.0.0.1:7890"));
    }

    #[test]
    fn force_clear_mode_is_distinct() {
        assert_ne!(
            ProxyCleanupMode::RestoreSnapshot,
            ProxyCleanupMode::ForceClear
        );
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

        let (changed, reservations) = resolve_available_inbound_ports(&state, &mut settings)
            .await
            .unwrap();

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
        let json =
            r#"{"InterfaceAlias":"vgate0","InterfaceDescription":"Wintun Userspace Tunnel"}"#;
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
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("node-a")],
        );

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let rules = config["route"]["rules"].as_array().unwrap();
        let dns_hijack_rule = rules
            .iter()
            .find(|rule| {
                rule.get("action").and_then(|action| action.as_str()) == Some("hijack-dns")
            })
            .expect("dns hijack rule should be generated");

        assert_eq!(
            dns_hijack_rule.get("type").and_then(|value| value.as_str()),
            Some("logical")
        );
        assert_eq!(
            dns_hijack_rule.get("mode").and_then(|value| value.as_str()),
            Some("or")
        );

        let nested_rules = dns_hijack_rule
            .get("rules")
            .and_then(|value| value.as_array())
            .unwrap();
        assert!(nested_rules
            .iter()
            .any(|rule| rule.get("protocol").and_then(|value| value.as_str()) == Some("dns")));
        assert!(nested_rules
            .iter()
            .any(|rule| rule.get("port").and_then(|value| value.as_u64()) == Some(53)));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_respects_tun_strict_route_setting() {
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
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("node-a")],
        );

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let tun_inbound = config["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|inbound| inbound.get("tag").and_then(|tag| tag.as_str()) == Some("tun-in"))
            .expect("tun inbound should be generated");

        assert_eq!(
            tun_inbound
                .get("strict_route")
                .and_then(|value| value.as_bool()),
            Some(false)
        );

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn node_detour_loads_cross_profile_dependency() {
        let data_dir = unique_test_dir("node-detour-cross-profile");
        let state = AppState::new(data_dir.clone());
        let profiles_data = ProfilesData {
            profiles: vec![
                make_profile("profile-a", "Profile A"),
                make_profile("profile-b", "Profile B"),
            ],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("target".to_string()),
            node_selections: HashMap::new(),
        };
        let mut target = make_node("target");
        target["detour"] = serde_json::json!("profile-b::front");
        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-a.json"), &vec![target]);
        write_json_file(
            &state.configs_dir().join("profile-b.json"),
            &vec![make_node("front")],
        );

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let outbounds = config["outbounds"].as_array().unwrap();
        let target = outbounds
            .iter()
            .find(|outbound| outbound["tag"].as_str() == Some("target"))
            .unwrap();
        assert_eq!(target["detour"].as_str(), Some("profile-b::front"));
        assert!(outbounds
            .iter()
            .any(|outbound| outbound["tag"].as_str() == Some("profile-b::front")));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn node_policies_keep_manual_nodes_and_filter_automatic_candidates() {
        let data_dir = unique_test_dir("node-policy-filtering");
        let state = AppState::new(data_dir.clone());
        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-a", "Profile A")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("eligible-a".to_string()),
            node_selections: HashMap::new(),
        };
        let eligible_a = make_node("eligible-a");
        let mut disabled = make_node("disabled");
        disabled[NODE_AUTO_SELECTION_ELIGIBLE_META_KEY] = serde_json::json!(false);
        let eligible_b = make_node("eligible-b");
        let mut metered = make_node("metered");
        metered[NODE_METERED_PROTECTED_META_KEY] = serde_json::json!(true);
        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![eligible_a, disabled, eligible_b, metered],
        );

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let outbounds = config["outbounds"].as_array().unwrap();
        assert!(outbounds
            .iter()
            .any(|outbound| outbound["tag"].as_str() == Some("metered")));
        let main_selector = outbounds
            .iter()
            .find(|outbound| outbound["tag"].as_str() == Some("PROXY"))
            .unwrap();
        assert_eq!(
            main_selector["outbounds"],
            serde_json::json!(["eligible-a", "disabled", "eligible-b", "metered"])
        );
        let automatic = outbounds
            .iter()
            .find(|outbound| outbound["type"].as_str() == Some("urltest"))
            .unwrap();
        assert_eq!(
            automatic["outbounds"],
            serde_json::json!(["eligible-a", "eligible-b"])
        );
        assert!(outbounds.iter().all(|outbound| {
            outbound
                .get(NODE_AUTO_SELECTION_ELIGIBLE_META_KEY)
                .is_none()
                && outbound.get(NODE_METERED_PROTECTED_META_KEY).is_none()
        }));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_strips_frontend_runtime_fields_from_socks_outbounds() {
        let data_dir = unique_test_dir("socks-frontend-runtime-fields");
        let state = AppState::new(data_dir.clone());
        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-a", "Profile A")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("residential".to_string()),
            node_selections: HashMap::new(),
        };
        let nodes = vec![
            serde_json::json!({
                "tag": "residential",
                "type": "socks",
                "server": "proxy.example.com",
                "server_port": 1000,
                "version": "5",
                "username": "user",
                "password": "password",
                "latencyMs": 128,
                "latencyStatus": "success",
                "healthStatus": "healthy",
                "isTimeout": false,
                "isTesting": false,
                "sourceProfileId": "profile-a",
                "sourceProfileName": "Profile A"
            }),
            serde_json::json!({
                "tag": "idc",
                "type": "socks",
                "server": "203.0.113.10",
                "server_port": 1080,
                "version": "5",
                "username": "user",
                "password": "password",
                "latencyMs": null,
                "latencyStatus": "timeout",
                "isTimeout": true,
                "isTesting": true
            }),
        ];
        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(&state.configs_dir().join("profile-a.json"), &nodes);

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let outbounds = config["outbounds"].as_array().unwrap();
        for tag in ["residential", "idc"] {
            let outbound = outbounds
                .iter()
                .find(|outbound| outbound["tag"].as_str() == Some(tag))
                .unwrap();
            for key in NODE_RUNTIME_META_KEYS {
                assert!(outbound.get(key).is_none(), "unexpected field: {key}");
            }
        }

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn xhttp_node_detour_builds_chain_route() {
        let data_dir = unique_test_dir("xhttp-node-detour-chain");
        let state = AppState::new(data_dir.clone());
        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-a", "Profile A")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("xhttp-node".to_string()),
            node_selections: HashMap::new(),
        };
        let mut xhttp_node = make_xhttp_node("xhttp-node");
        xhttp_node["detour"] = serde_json::json!("front-node");
        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("front-node"), xhttp_node],
        );

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let chain_inbound = config["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|inbound| {
                inbound["tag"]
                    .as_str()
                    .is_some_and(|tag| tag.starts_with("kunbox-front-proxy-chain-in"))
            })
            .expect("expected node detour chain inbound");
        assert_eq!(chain_inbound["listen"].as_str(), Some("127.0.0.1"));
        let chain_tag = chain_inbound["tag"].as_str().unwrap();
        let chain_port = chain_inbound["listen_port"].as_u64().unwrap();
        assert!(config["route"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| {
                rule["inbound"].as_str() == Some(chain_tag)
                    && rule["outbound"].as_str() == Some("front-node")
            }));

        let plugin_specs: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(state.config_dir.join("plugin-bridges.json")).unwrap(),
        )
        .unwrap();
        let xhttp_spec = plugin_specs
            .as_array()
            .unwrap()
            .iter()
            .find(|spec| spec["tag"].as_str() == Some("xhttp-node"))
            .unwrap();
        assert_eq!(xhttp_spec["frontProxyTag"].as_str(), Some("front-node"));
        assert_eq!(xhttp_spec["frontProxyChainPort"].as_u64(), Some(chain_port));

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
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("node-a")],
        );

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        assert_eq!(
            config["experimental"]["cache_file"]["store_rdrc"].as_bool(),
            Some(true)
        );

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
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("node-a")],
        );
        write_json_file(
            &state.configs_dir().join("profile-b.json"),
            &vec![make_node("node-b")],
        );

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
    async fn collect_health_targets_separates_selectors_and_fixed_nodes() {
        let data_dir = unique_test_dir("health-targets");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![
                make_profile("profile-a", "Profile A"),
                make_profile("profile-b", "Profile B"),
            ],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("node-a".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);

        *state.custom_rules.lock().await = CustomRules {
            domain_rules: vec![
                DomainRule {
                    id: "profile-domain".to_string(),
                    name: "profile domain".to_string(),
                    rule_type: "domain".to_string(),
                    value: "profile.example.com".to_string(),
                    outbound_mode: "profile".to_string(),
                    outbound_value: Some("profile-b".to_string()),
                    enabled: true,
                },
                DomainRule {
                    id: "fixed-domain".to_string(),
                    name: "fixed domain".to_string(),
                    rule_type: "domain".to_string(),
                    value: "fixed.example.com".to_string(),
                    outbound_mode: "node".to_string(),
                    outbound_value: Some("fixed-node".to_string()),
                    enabled: true,
                },
            ],
        };
        *state.rulesets.lock().await = vec![
            make_ruleset("rs-profile", "profile", Some("profile-b")),
            make_ruleset("rs-fixed", "节点", Some("ruleset-node")),
        ];

        let targets = collect_health_targets(&state).await;
        let selector_tags: std::collections::HashSet<&str> = targets
            .iter()
            .filter(|target| target.kind == HealthTargetKind::Selector)
            .filter_map(|target| target.selector_tag.as_deref())
            .collect();
        assert!(selector_tags.contains("PROXY"));
        assert!(selector_tags.contains("P:profile-b"));

        let fixed_domain = targets
            .iter()
            .find(|target| target.node_tag.as_deref() == Some("fixed-node"))
            .expect("fixed domain node should be collected");
        assert_eq!(fixed_domain.kind, HealthTargetKind::FixedNode);
        assert_eq!(
            fixed_domain.rule_label.as_deref(),
            Some("fixed.example.com")
        );
        assert!(!fixed_domain.auto_failover);

        let fixed_ruleset = targets
            .iter()
            .find(|target| target.node_tag.as_deref() == Some("ruleset-node"))
            .expect("fixed ruleset node should be collected");
        assert_eq!(fixed_ruleset.kind, HealthTargetKind::FixedNode);
        assert_eq!(fixed_ruleset.rule_label.as_deref(), Some("rs-fixed"));
        assert!(!fixed_ruleset.auto_failover);

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn collect_health_targets_uses_renamed_main_selector_when_proxy_tag_collides() {
        let data_dir = unique_test_dir("health-targets-proxy-collision");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-a", "Profile A")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("PROXY".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("PROXY")],
        );

        let targets = collect_health_targets(&state).await;
        let selector_tags: std::collections::HashSet<&str> = targets
            .iter()
            .filter(|target| target.kind == HealthTargetKind::Selector)
            .filter_map(|target| target.selector_tag.as_deref())
            .collect();

        assert!(selector_tags.contains("PROXY-kb"));
        assert!(!selector_tags.contains("PROXY"));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn health_state_fails_after_three_consecutive_failures() {
        let mut health = NodeHealth::new("node-a".to_string());

        record_probe_failure(&mut health, "timeout".to_string(), 1_000);
        assert_eq!(health.status, HealthStatus::Suspect);
        assert_eq!(health.failure_streak, 1);

        record_probe_failure(&mut health, "timeout".to_string(), 2_000);
        assert_eq!(health.status, HealthStatus::Suspect);
        assert_eq!(health.failure_streak, 2);

        record_probe_failure(&mut health, "timeout".to_string(), 3_000);
        assert_eq!(health.status, HealthStatus::Failed);
        assert_eq!(health.failure_streak, 3);
        assert_eq!(health.last_error.as_deref(), Some("timeout"));
    }

    #[test]
    fn health_state_recovers_after_two_consecutive_successes() {
        let mut health = NodeHealth::new("node-a".to_string());

        record_probe_failure(&mut health, "timeout".to_string(), 1_000);
        record_probe_failure(&mut health, "timeout".to_string(), 2_000);
        record_probe_failure(&mut health, "timeout".to_string(), 3_000);
        assert_eq!(health.status, HealthStatus::Failed);

        record_probe_success(&mut health, 120, 4_000);
        assert_eq!(health.status, HealthStatus::Recovering);
        assert_eq!(health.success_streak, 1);

        record_probe_success(&mut health, 90, 5_000);
        assert_eq!(health.status, HealthStatus::Healthy);
        assert_eq!(health.success_streak, 2);
        assert_eq!(health.failure_streak, 0);
        assert_eq!(health.last_latency_ms, Some(90));
        assert!(health.last_error.is_none());
    }

    #[test]
    fn failed_node_uses_backoff_before_next_probe() {
        let mut health = NodeHealth::new("node-a".to_string());

        record_probe_failure(&mut health, "timeout".to_string(), 1_000);
        record_probe_failure(&mut health, "timeout".to_string(), 2_000);
        record_probe_failure(&mut health, "timeout".to_string(), 3_000);

        assert_eq!(health.status, HealthStatus::Failed);
        assert_eq!(health.next_probe_after, 33_000);
        assert!(!should_probe(&health, 32_999));
        assert!(should_probe(&health, 33_000));

        record_probe_failure(&mut health, "timeout".to_string(), 33_000);
        assert_eq!(health.next_probe_after, 93_000);
    }

    #[tokio::test]
    async fn selector_latency_probe_uses_configured_timeout() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let (request_tx, request_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 4096];
            let n = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..n]).to_string();
            let _ = request_tx.send(request);
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 12\r\n\r\n{\"delay\":42}",
                )
                .await
                .unwrap();
        });

        let result = probe_selector_node_latency(
            local_clash_api_client(),
            port,
            "node-a".to_string(),
            "https://example.com/probe".to_string(),
            1_234,
        )
        .await;

        assert_eq!(result, ("node-a".to_string(), Some(42)));
        let request = request_rx.await.unwrap();
        assert!(request.starts_with("GET /proxies/node-a/delay?"));
        assert!(request.contains("url=https%3A%2F%2Fexample.com%2Fprobe"));
        assert!(request.contains("timeout=1234"));
    }

    #[tokio::test]
    async fn real_proxy_path_probe_succeeds_for_204_through_proxy() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let (request_tx, request_rx) = tokio::sync::oneshot::channel();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 4096];
            let n = socket.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..n]).to_string();
            let _ = request_tx.send(request);
            socket
                .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });

        let latency = probe_real_proxy_path(port, "http://probe.test/generate_204", 1_000)
            .await
            .unwrap();

        let request = request_rx.await.unwrap();
        assert!(request.contains("http://probe.test/generate_204"));
        assert!(latency < 1_000);
    }

    #[tokio::test]
    async fn real_proxy_path_probe_fails_for_503_through_proxy() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 4096];
            let _ = socket.read(&mut buffer).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });

        let err = probe_real_proxy_path(port, "http://probe.test/generate_204", 1_000)
            .await
            .unwrap_err();

        assert!(err.contains("503"));
    }

    #[tokio::test]
    async fn switch_selector_to_node_reports_http_failure() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 4096];
            let _ = socket.read(&mut buffer).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n")
                .await
                .unwrap();
        });

        let client = local_clash_api_client();
        let err = switch_selector_to_node(&client, port, "P:profile-a", "backup-node")
            .await
            .unwrap_err();

        assert!(err.contains("500"));
    }

    #[tokio::test]
    async fn selector_switch_closes_only_connections_still_using_previous_node() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .unwrap();
        let port = listener.local_addr().unwrap().port();
        let (request_tx, mut request_rx) = tokio::sync::mpsc::unbounded_channel();

        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut buffer = [0_u8; 4096];
                let n = socket.read(&mut buffer).await.unwrap();
                let request = String::from_utf8_lossy(&buffer[..n]).to_string();
                let request_line = request.lines().next().unwrap_or_default().to_string();
                let _ = request_tx.send(request_line.clone());

                let (status, body) = if request_line.starts_with("GET /connections ") {
                    (
                        "200 OK",
                        r#"{"connections":[{"id":"stale-connection","chains":["old-node","P:profile-a"]},{"id":"detoured-stale-connection","chains":["backup-node","old-node","P:profile-a"]},{"id":"current-connection","chains":["front-node","backup-node","P:profile-a"]},{"id":"other-connection","chains":["other-node","P:profile-b"]}]}"#,
                    )
                } else {
                    ("204 No Content", "")
                };
                let response = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    body.len(),
                    body
                );
                socket.write_all(response.as_bytes()).await.unwrap();
            }
        });

        let client = local_clash_api_client();
        switch_selector_to_node(&client, port, "P:profile-a", "backup-node")
            .await
            .unwrap();

        let mut requests = Vec::new();
        for _ in 0..4 {
            requests.push(
                tokio::time::timeout(Duration::from_secs(1), request_rx.recv())
                    .await
                    .expect("selector 切换后必须清理旧连接")
                    .unwrap(),
            );
        }
        assert!(requests[0].starts_with("PUT /proxies/P%3Aprofile-a "));
        assert_eq!(requests[1], "GET /connections HTTP/1.1");
        assert_eq!(requests[2], "DELETE /connections/stale-connection HTTP/1.1");
        assert_eq!(
            requests[3],
            "DELETE /connections/detoured-stale-connection HTTP/1.1"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(100), request_rx.recv())
                .await
                .is_err()
        );
    }

    #[test]
    fn selector_failover_selects_lowest_latency_healthy_backup() {
        let target = HealthTarget {
            kind: HealthTargetKind::Selector,
            selector_tag: Some("P:profile-a".to_string()),
            node_tag: None,
            rule_label: None,
            auto_failover: true,
        };
        let selector = SelectorHealth {
            selector_tag: "P:profile-a".to_string(),
            current_node: Some("node-a".to_string()),
            current_auto_selection_eligible: true,
            backup_nodes: vec!["node-b".to_string(), "node-c".to_string()],
            last_switch_at: None,
            switch_cooldown_until: None,
        };
        let mut current = NodeHealth::new("node-a".to_string());
        current.status = HealthStatus::Failed;
        current.last_checked_at = 10_000;
        let mut slow_backup = NodeHealth::new("node-b".to_string());
        slow_backup.status = HealthStatus::Healthy;
        slow_backup.last_latency_ms = Some(180);
        let mut fast_backup = NodeHealth::new("node-c".to_string());
        fast_backup.status = HealthStatus::Healthy;
        fast_backup.last_latency_ms = Some(80);
        let node_health = HashMap::from([
            ("node-a".to_string(), current),
            ("node-b".to_string(), slow_backup),
            ("node-c".to_string(), fast_backup),
        ]);

        let action = decide_health_action(&target, Some(&selector), &node_health, 10_000);

        assert_eq!(
            action,
            HealthAction::SwitchSelector {
                selector: "P:profile-a".to_string(),
                from: "node-a".to_string(),
                to: "node-c".to_string(),
            }
        );
    }

    #[test]
    fn backup_probe_candidates_rotate_past_previously_checked_prefix() {
        let backup_nodes = (0..6)
            .map(|index| format!("node-{index}"))
            .collect::<Vec<_>>();
        let mut node_health = HashMap::new();
        for tag in backup_nodes.iter().take(3) {
            let mut health = NodeHealth::new(tag.clone());
            health.status = HealthStatus::Suspect;
            health.last_checked_at = 10_000;
            node_health.insert(tag.clone(), health);
        }

        assert_eq!(
            select_backup_probe_candidates(&backup_nodes, &node_health, 20_000, 3),
            vec![
                "node-3".to_string(),
                "node-4".to_string(),
                "node-5".to_string()
            ]
        );

        let mut recovering = NodeHealth::new("node-3".to_string());
        recovering.status = HealthStatus::Recovering;
        recovering.last_checked_at = 20_000;
        node_health.insert("node-3".to_string(), recovering);
        assert_eq!(
            select_backup_probe_candidates(&backup_nodes, &node_health, 30_000, 3)[0],
            "node-3"
        );
    }

    #[test]
    fn selector_failover_does_not_switch_during_cooldown() {
        let target = HealthTarget {
            kind: HealthTargetKind::Selector,
            selector_tag: Some("P:profile-a".to_string()),
            node_tag: None,
            rule_label: None,
            auto_failover: true,
        };
        let selector = SelectorHealth {
            selector_tag: "P:profile-a".to_string(),
            current_node: Some("node-a".to_string()),
            current_auto_selection_eligible: true,
            backup_nodes: vec!["node-b".to_string()],
            last_switch_at: Some(1_000),
            switch_cooldown_until: Some(60_000),
        };
        let mut current = NodeHealth::new("node-a".to_string());
        current.status = HealthStatus::Failed;
        current.last_checked_at = 30_000;
        let mut backup = NodeHealth::new("node-b".to_string());
        backup.status = HealthStatus::Healthy;
        backup.last_latency_ms = Some(60);
        let node_health = HashMap::from([
            ("node-a".to_string(), current),
            ("node-b".to_string(), backup),
        ]);

        let action = decide_health_action(&target, Some(&selector), &node_health, 30_000);

        assert_eq!(action, HealthAction::None);
    }

    #[test]
    fn selector_without_backup_returns_notify_no_backup() {
        let target = HealthTarget {
            kind: HealthTargetKind::Selector,
            selector_tag: Some("P:profile-a".to_string()),
            node_tag: None,
            rule_label: None,
            auto_failover: true,
        };
        let selector = SelectorHealth {
            selector_tag: "P:profile-a".to_string(),
            current_node: Some("node-a".to_string()),
            current_auto_selection_eligible: true,
            backup_nodes: vec!["node-b".to_string()],
            last_switch_at: None,
            switch_cooldown_until: None,
        };
        let mut current = NodeHealth::new("node-a".to_string());
        current.status = HealthStatus::Failed;
        current.last_checked_at = 10_000;
        let mut backup = NodeHealth::new("node-b".to_string());
        backup.status = HealthStatus::Failed;
        let node_health = HashMap::from([
            ("node-a".to_string(), current),
            ("node-b".to_string(), backup),
        ]);

        let action = decide_health_action(&target, Some(&selector), &node_health, 10_000);

        assert_eq!(
            action,
            HealthAction::NotifyNoBackup {
                selector: "P:profile-a".to_string(),
            }
        );
    }

    #[test]
    fn selector_with_ineligible_current_switches_to_healthy_eligible_backup() {
        let target = HealthTarget {
            kind: HealthTargetKind::Selector,
            selector_tag: Some("P:profile-a".to_string()),
            node_tag: None,
            rule_label: None,
            auto_failover: true,
        };
        let selector = SelectorHealth {
            selector_tag: "P:profile-a".to_string(),
            current_node: Some("metered-node".to_string()),
            current_auto_selection_eligible: false,
            backup_nodes: vec!["eligible-node".to_string()],
            last_switch_at: None,
            switch_cooldown_until: None,
        };
        let mut backup = NodeHealth::new("eligible-node".to_string());
        backup.status = HealthStatus::Healthy;
        backup.last_latency_ms = Some(70);
        let node_health = HashMap::from([("eligible-node".to_string(), backup)]);

        let action = decide_health_action(&target, Some(&selector), &node_health, 10_000);

        assert_eq!(
            action,
            HealthAction::SwitchSelector {
                selector: "P:profile-a".to_string(),
                from: "metered-node".to_string(),
                to: "eligible-node".to_string(),
            }
        );
    }

    #[test]
    fn main_selector_keeps_explicit_ineligible_node() {
        let target = HealthTarget {
            kind: HealthTargetKind::Selector,
            selector_tag: Some("PROXY".to_string()),
            node_tag: None,
            rule_label: None,
            auto_failover: true,
        };
        let selector = SelectorHealth {
            selector_tag: "PROXY".to_string(),
            current_node: Some("metered-node".to_string()),
            current_auto_selection_eligible: false,
            backup_nodes: vec!["eligible-node".to_string()],
            last_switch_at: None,
            switch_cooldown_until: None,
        };
        let mut backup = NodeHealth::new("eligible-node".to_string());
        backup.status = HealthStatus::Healthy;
        backup.last_latency_ms = Some(70);
        let node_health = HashMap::from([("eligible-node".to_string(), backup)]);

        assert_eq!(
            decide_health_action(&target, Some(&selector), &node_health, 10_000),
            HealthAction::None
        );
    }

    #[test]
    fn fixed_node_failure_never_returns_switch_action() {
        let target = HealthTarget {
            kind: HealthTargetKind::FixedNode,
            selector_tag: None,
            node_tag: Some("fixed-node".to_string()),
            rule_label: Some("fixed.example.com".to_string()),
            auto_failover: false,
        };
        let mut fixed = NodeHealth::new("fixed-node".to_string());
        fixed.status = HealthStatus::Failed;
        fixed.last_checked_at = 10_000;
        let node_health = HashMap::from([("fixed-node".to_string(), fixed)]);

        let action = decide_health_action(&target, None, &node_health, 10_000);

        assert_eq!(
            action,
            HealthAction::NotifyFixedNodeFailed {
                node: "fixed-node".to_string(),
                rule: Some("fixed.example.com".to_string()),
            }
        );
    }

    #[test]
    fn fixed_node_failure_is_not_reported_again_before_next_probe() {
        let target = HealthTarget {
            kind: HealthTargetKind::FixedNode,
            selector_tag: None,
            node_tag: Some("fixed-node".to_string()),
            rule_label: Some("fixed.example.com".to_string()),
            auto_failover: false,
        };
        let mut fixed = NodeHealth::new("fixed-node".to_string());
        fixed.status = HealthStatus::Failed;
        fixed.last_checked_at = 1_000;
        fixed.next_probe_after = 31_000;
        let node_health = HashMap::from([("fixed-node".to_string(), fixed)]);

        let action = decide_health_action(&target, None, &node_health, 10_000);

        assert_eq!(action, HealthAction::None);
    }

    #[tokio::test]
    async fn health_monitor_disabled_does_not_install_cancel_token() {
        let data_dir = unique_test_dir("health-monitor-disabled");
        let state = AppState::new(data_dir.clone());

        let cancel = prepare_health_monitor_cancel(&state, false).await;

        assert!(cancel.is_none());
        assert!(state.health_cancel.lock().await.is_none());

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn cancel_health_monitor_cancels_existing_token() {
        let data_dir = unique_test_dir("health-monitor-cancel");
        let state = AppState::new(data_dir.clone());
        let token = CancellationToken::new();
        *state.health_cancel.lock().await = Some(token.clone());

        cancel_health_monitor(state.health_cancel.clone()).await;

        assert!(token.is_cancelled());
        assert!(state.health_cancel.lock().await.is_none());

        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn main_selector_auto_failover_disabled_returns_manual_notify() {
        let mut settings = AppSettings::default();
        settings.main_node_auto_failover = false;
        let action = HealthAction::SwitchSelector {
            selector: "PROXY".to_string(),
            from: "node-a".to_string(),
            to: "node-b".to_string(),
        };

        let gated =
            gate_main_selector_health_action(action, &settings, (false, None), (false, None));

        assert_eq!(
            gated,
            HealthAction::NotifyMainNodeNeedsManualSwitch {
                selector: "PROXY".to_string(),
                from: "node-a".to_string(),
                to: "node-b".to_string(),
                reason: "主节点故障自动切换未开启".to_string(),
            }
        );
    }

    #[test]
    fn main_selector_signature_mismatch_returns_manual_notify() {
        let mut settings = AppSettings::default();
        settings.main_node_auto_failover = true;
        let action = HealthAction::SwitchSelector {
            selector: "PROXY".to_string(),
            from: "node-a".to_string(),
            to: "node-b".to_string(),
        };

        let gated = gate_main_selector_health_action(
            action,
            &settings,
            (false, None),
            (true, Some("https://dns.example/dns-query".to_string())),
        );

        assert_eq!(
            gated,
            HealthAction::NotifyMainNodeNeedsManualSwitch {
                selector: "PROXY".to_string(),
                from: "node-a".to_string(),
                to: "node-b".to_string(),
                reason: "目标节点需要不同的 DNS bootstrap，未自动切换".to_string(),
            }
        );
    }

    #[test]
    fn fixed_node_failure_action_builds_fixed_node_event() {
        let action = HealthAction::NotifyFixedNodeFailed {
            node: "fixed-node".to_string(),
            rule: Some("fixed.example.com".to_string()),
        };

        let event = health_event_for_action(&action).expect("fixed node event should be emitted");

        assert_eq!(event.kind, HealthEventKind::FixedNodeFailed);
        assert_eq!(event.node.as_deref(), Some("fixed-node"));
        assert_eq!(event.rule.as_deref(), Some("fixed.example.com"));
        assert_eq!(
            event.message,
            "分流节点不可用：fixed-node。该规则绑定了固定节点，KunBox 未自动更换，请手动调整规则或节点。"
        );
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
            .find(|outbound| {
                outbound.get("tag").and_then(|tag| tag.as_str()) == Some("P:profile-a")
            })
            .expect("expected profile selector");

        assert_eq!(
            selector.get("default").and_then(|value| value.as_str()),
            Some("node-b")
        );

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn profile_selector_defaults_to_auto_eligible_node_and_interrupts_old_connections() {
        let data_dir = unique_test_dir("profile-selector-auto-policy");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![
                make_profile("profile-a", "Alpha"),
                make_profile("profile-b", "Beta"),
                make_profile("profile-c", "Gamma"),
            ],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("main-node".to_string()),
            node_selections: HashMap::from([
                ("profile-b".to_string(), "metered-node".to_string()),
                ("profile-c".to_string(), "only-metered-node".to_string()),
            ]),
        };
        let mut metered = make_node("metered-node");
        metered[NODE_METERED_PROTECTED_META_KEY] = serde_json::json!(true);
        let mut auto_disabled = make_node("auto-disabled-node");
        auto_disabled[NODE_AUTO_SELECTION_ELIGIBLE_META_KEY] = serde_json::json!(false);

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("main-node")],
        );
        write_json_file(
            &state.configs_dir().join("profile-b.json"),
            &vec![metered, auto_disabled, make_node("eligible-node")],
        );
        let mut only_metered = make_node("only-metered-node");
        only_metered[NODE_METERED_PROTECTED_META_KEY] = serde_json::json!(true);
        write_json_file(
            &state.configs_dir().join("profile-c.json"),
            &vec![only_metered],
        );
        *state.rulesets.lock().await = vec![
            make_ruleset("rs-b", "profile", Some("profile-b")),
            make_ruleset("rs-c", "profile", Some("profile-c")),
        ];

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let outbounds = config["outbounds"].as_array().unwrap();
        let selector = outbounds
            .iter()
            .find(|outbound| outbound["tag"].as_str() == Some("P:profile-b"))
            .expect("expected profile selector");

        assert_eq!(
            selector["outbounds"],
            serde_json::json!(["profile-b::eligible-node"])
        );
        assert_eq!(
            selector["default"].as_str(),
            Some("profile-b::eligible-node")
        );
        assert_eq!(
            selector["interrupt_exist_connections"].as_bool(),
            Some(true)
        );
        assert!(outbounds
            .iter()
            .any(|outbound| { outbound["tag"].as_str() == Some("profile-b::metered-node") }));
        assert!(outbounds
            .iter()
            .any(|outbound| { outbound["tag"].as_str() == Some("profile-b::auto-disabled-node") }));
        assert!(outbounds
            .iter()
            .all(|outbound| { outbound["tag"].as_str() != Some("P:profile-c") }));
        assert!(outbounds
            .iter()
            .any(|outbound| { outbound["tag"].as_str() == Some("profile-c::only-metered-node") }));

        let main_selector = outbounds
            .iter()
            .find(|outbound| outbound["tag"].as_str() == Some("PROXY"))
            .expect("expected main selector");
        assert_eq!(
            main_selector["interrupt_exist_connections"].as_bool(),
            Some(true)
        );

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn profile_selector_switch_persists_scoped_selection_without_changing_main_node() {
        let data_dir = unique_test_dir("profile-selector-persistence");
        let state = AppState::new(data_dir.clone());
        let profiles_data = ProfilesData {
            profiles: vec![
                make_profile("profile-a", "Alpha"),
                make_profile("profile-b", "Beta"),
            ],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("main-node".to_string()),
            node_selections: HashMap::new(),
        };
        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("main-node"), make_node("profile-route-node")],
        );
        write_json_file(
            &state.configs_dir().join("profile-b.json"),
            &vec![make_node("selected-node")],
        );

        persist_selector_selection(&state, "P:profile-b", "profile-b::selected-node")
            .await
            .unwrap();
        persist_selector_selection(&state, "P:profile-a", "profile-route-node")
            .await
            .unwrap();

        let persisted: ProfilesData =
            serde_json::from_str(&fs::read_to_string(state.profiles_file()).unwrap()).unwrap();
        assert_eq!(persisted.active_node_tag.as_deref(), Some("main-node"));
        assert_eq!(
            persisted
                .node_selections
                .get("profile-b")
                .map(String::as_str),
            Some("selected-node")
        );
        assert_eq!(
            persisted
                .node_selections
                .get("profile-a")
                .map(String::as_str),
            Some("profile-route-node")
        );
        assert_eq!(
            state
                .profiles_data
                .lock()
                .await
                .node_selections
                .get("profile-b")
                .map(String::as_str),
            Some("selected-node")
        );

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn explicit_scoped_metered_node_remains_available_for_single_node_routing() {
        let data_dir = unique_test_dir("explicit-metered-node-routing");
        let state = AppState::new(data_dir.clone());
        let profiles_data = ProfilesData {
            profiles: vec![
                make_profile("profile-a", "Alpha"),
                make_profile("profile-b", "Beta"),
            ],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("main-node".to_string()),
            node_selections: HashMap::new(),
        };
        let mut metered = make_node("metered-node");
        metered[NODE_METERED_PROTECTED_META_KEY] = serde_json::json!(true);
        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("main-node")],
        );
        write_json_file(&state.configs_dir().join("profile-b.json"), &vec![metered]);
        *state.custom_rules.lock().await = CustomRules {
            domain_rules: vec![make_domain_rule(
                "metered.example.com",
                "profile-b::metered-node",
            )],
            ..CustomRules::default()
        };

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        assert!(config["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .any(|outbound| { outbound["tag"].as_str() == Some("profile-b::metered-node") }));
        assert!(config["route"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| { rule["outbound"].as_str() == Some("profile-b::metered-node") }));

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
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("shared-node")],
        );
        write_json_file(
            &state.configs_dir().join("profile-b.json"),
            &vec![make_node("shared-node")],
        );

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
            .find(|outbound| {
                outbound.get("tag").and_then(|tag| tag.as_str()) == Some("P:profile-b")
            })
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
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("shared-node")],
        );
        write_json_file(
            &state.configs_dir().join("profile-b.json"),
            &vec![make_node("shared-node")],
        );

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
                    .map(|domains| {
                        domains
                            .iter()
                            .any(|value| value.as_str() == Some("example.com"))
                    })
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
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("shared-node")],
        );
        write_json_file(
            &state.configs_dir().join("profile-b.json"),
            &vec![make_node("shared-node")],
        );
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
        let profile_ruleset_rule = rules
            .iter()
            .find(|rule| {
                rule.get("rule_set")
                    .and_then(|rule_set| rule_set.as_array())
                    .map(|rule_sets| {
                        rule_sets
                            .iter()
                            .any(|value| value.as_str() == Some("rs-profile"))
                    })
                    .unwrap_or(false)
            })
            .unwrap();
        let bare_ruleset_rule = rules
            .iter()
            .find(|rule| {
                rule.get("rule_set")
                    .and_then(|rule_set| rule_set.as_array())
                    .map(|rule_sets| {
                        rule_sets
                            .iter()
                            .any(|value| value.as_str() == Some("rs-bare"))
                    })
                    .unwrap_or(false)
            })
            .unwrap();

        assert_eq!(
            profile_ruleset_rule
                .get("outbound")
                .and_then(|value| value.as_str()),
            Some("profile-b::shared-node")
        );
        assert_eq!(
            bare_ruleset_rule
                .get("outbound")
                .and_then(|value| value.as_str()),
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

        assert_eq!(
            dns_remote.get("server").and_then(|value| value.as_str()),
            Some("dns.alidns.com")
        );
        assert_eq!(
            dns_remote.get("path").and_then(|value| value.as_str()),
            Some("/dns-query")
        );
        assert_eq!(
            dns_remote
                .get("domain_resolver")
                .and_then(|value| value.as_str()),
            Some("dns-bootstrap")
        );
        assert!(
            dns_remote.get("detour").is_none(),
            "ECH bootstrap DNS must not detour through proxy"
        );

        let dns_bootstrap = config["dns"]["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|server| {
                server.get("tag").and_then(|value| value.as_str()) == Some("dns-bootstrap")
            })
            .unwrap();

        assert_eq!(
            dns_bootstrap.get("type").and_then(|value| value.as_str()),
            Some("https")
        );
        assert_eq!(
            dns_bootstrap.get("server").and_then(|value| value.as_str()),
            Some("223.5.5.5")
        );
        assert_eq!(
            config["route"]["default_domain_resolver"]
                .get("server")
                .and_then(|value| value.as_str()),
            Some("dns-bootstrap")
        );

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

        assert_eq!(
            dns_remote.get("detour").and_then(|value| value.as_str()),
            Some("PROXY")
        );
        assert_eq!(
            dns_remote
                .get("domain_resolver")
                .and_then(|value| value.as_str()),
            Some("dns-bootstrap")
        );

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
        write_json_file(
            &state.configs_dir().join("profile-xhttp.json"),
            &vec![make_xhttp_node("XHTTP Node")],
        );

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let outbound = config["outbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node.get("tag").and_then(|value| value.as_str()) == Some("XHTTP Node"))
            .unwrap();

        assert_eq!(
            outbound.get("type").and_then(|value| value.as_str()),
            Some("socks")
        );
        assert_eq!(
            outbound.get("server").and_then(|value| value.as_str()),
            Some("127.0.0.1")
        );
        assert!(outbound
            .get("server_port")
            .and_then(|value| value.as_u64())
            .is_some());

        let plugin_specs: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(state.config_dir.join("plugin-bridges.json")).unwrap(),
        )
        .unwrap();
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
                    .map(|values| {
                        values
                            .iter()
                            .any(|value| value.as_str() == Some("35.194.192.123/32"))
                    })
                    .unwrap_or(false)
            })
            .expect("xray plugin remote server must bypass TUN proxy loop");

        assert_eq!(
            remote_rule.get("outbound").and_then(|value| value.as_str()),
            Some("direct")
        );

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_uses_non_strict_tun_route_by_default() {
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
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("node-a")],
        );

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let tun_inbound = config["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|inbound| inbound.get("tag").and_then(|tag| tag.as_str()) == Some("tun-in"))
            .expect("tun inbound should be generated");

        assert_eq!(
            tun_inbound
                .get("strict_route")
                .and_then(|value| value.as_bool()),
            Some(false)
        );

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_uses_prefer_ipv4_tun_and_dns_strategy() {
        let data_dir = unique_test_dir("tun-prefer-ipv4");
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
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("node-a")],
        );

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let tun_inbound = config["inbounds"]
            .as_array()
            .unwrap()
            .iter()
            .find(|inbound| inbound.get("tag").and_then(|tag| tag.as_str()) == Some("tun-in"))
            .expect("tun inbound should be generated");

        assert_eq!(
            tun_inbound
                .get("address")
                .and_then(|value| value.as_array()),
            Some(&vec![serde_json::json!("172.19.0.1/30")])
        );
        assert_eq!(
            tun_inbound.get("mtu").and_then(|value| value.as_u64()),
            Some(1500)
        );
        assert_eq!(
            config["dns"]
                .get("strategy")
                .and_then(|value| value.as_str()),
            Some("prefer_ipv4")
        );

        let fakeip = config["dns"]["servers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|server| server.get("tag").and_then(|tag| tag.as_str()) == Some("dns-fakeip"))
            .expect("fakeip server should exist when fake_dns is enabled");
        assert!(fakeip.get("inet6_range").is_none());

        let fake_dns_rule = config["dns"]["rules"]
            .as_array()
            .unwrap()
            .iter()
            .find(|rule| {
                rule.get("server").and_then(|server| server.as_str()) == Some("dns-fakeip")
            })
            .expect("fake dns rule should exist");
        assert_eq!(
            fake_dns_rule
                .get("query_type")
                .and_then(|value| value.as_array()),
            Some(&vec![serde_json::json!("A")])
        );

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
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("node-a")],
        );

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let dns_rules = config["dns"]["rules"].as_array().unwrap();
        let fake_dns_rule = dns_rules
            .iter()
            .find(|rule| {
                rule.get("server").and_then(|server| server.as_str()) == Some("dns-fakeip")
            })
            .expect("fake dns rule should be generated");

        let inbound = fake_dns_rule
            .get("inbound")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(inbound.len(), 1);
        assert_eq!(inbound[0].as_str(), Some("tun-in"));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_preserves_domain_routing_for_non_tun_inbounds() {
        let data_dir = unique_test_dir("non-tun-domain-routing");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-a", "Profile A")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("node-a".to_string()),
            node_selections: HashMap::new(),
        };

        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("node-a")],
        );
        *state.custom_rules.lock().await = CustomRules {
            domain_rules: vec![DomainRule {
                id: "rule-direct".to_string(),
                name: "direct example".to_string(),
                rule_type: "domain_suffix".to_string(),
                value: "example.com".to_string(),
                outbound_mode: "direct".to_string(),
                outbound_value: None,
                enabled: true,
            }],
        };

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let inbounds = config["inbounds"].as_array().unwrap();
        assert!(inbounds
            .iter()
            .all(|inbound| { inbound.get("tag").and_then(|tag| tag.as_str()) != Some("tun-in") }));

        assert_eq!(
            config["dns"]
                .get("reverse_mapping")
                .and_then(|value| value.as_bool()),
            Some(true)
        );

        let route_rules = config["route"]["rules"].as_array().unwrap();
        assert!(has_sniff_rule_for_inbound(route_rules, "mixed-in"));
        assert!(has_sniff_rule_for_inbound(route_rules, "socks-in"));

        assert!(route_rules.iter().any(|rule| {
            rule.get("domain_suffix")
                .and_then(|value| value.as_array())
                .is_some_and(|domains| {
                    domains
                        .iter()
                        .any(|domain| domain.as_str() == Some("example.com"))
                })
                && rule.get("outbound").and_then(|value| value.as_str()) == Some("direct")
        }));

        let _ = fs::remove_dir_all(data_dir);
    }

    #[tokio::test]
    async fn generate_config_places_domain_rules_before_rulesets() {
        let data_dir = unique_test_dir("domain-before-ruleset");
        let state = AppState::new(data_dir.clone());

        let profiles_data = ProfilesData {
            profiles: vec![make_profile("profile-a", "Profile A")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("node-a".to_string()),
            node_selections: HashMap::new(),
        };
        write_json_file(&state.profiles_file(), &profiles_data);
        write_json_file(
            &state.configs_dir().join("profile-a.json"),
            &vec![make_node("node-a")],
        );
        fs::create_dir_all(state.rulesets_cache_dir()).unwrap();
        fs::write(state.rulesets_cache_dir().join("rs-conflict.srs"), b"dummy").unwrap();

        *state.rulesets.lock().await = vec![make_ruleset("rs-conflict", "proxy", None)];
        *state.custom_rules.lock().await = CustomRules {
            domain_rules: vec![DomainRule {
                id: "domain-priority".to_string(),
                name: "priority.example".to_string(),
                rule_type: "domain_suffix".to_string(),
                value: "priority.example".to_string(),
                outbound_mode: "direct".to_string(),
                outbound_value: None,
                enabled: true,
            }],
        };

        let result = generate_config(&state).await.unwrap();
        assert!(result.success);

        let config = read_generated_config(&state);
        let route_rules = config["route"]["rules"].as_array().unwrap();
        let domain_route_index = route_rules
            .iter()
            .position(|rule| rule.get("domain_suffix").is_some())
            .unwrap();
        let ruleset_route_index = route_rules
            .iter()
            .position(|rule| rule.get("rule_set").is_some())
            .unwrap();
        assert!(domain_route_index < ruleset_route_index);

        let dns_rules = config["dns"]["rules"].as_array().unwrap();
        let domain_dns_index = dns_rules
            .iter()
            .position(|rule| rule.get("domain_suffix").is_some())
            .unwrap();
        let ruleset_dns_index = dns_rules
            .iter()
            .position(|rule| rule.get("rule_set").is_some())
            .unwrap();
        assert!(domain_dns_index < ruleset_dns_index);

        let _ = fs::remove_dir_all(data_dir);
    }

    #[test]
    fn build_xray_plugin_config_preserves_vless_xhttp_transport() {
        let node = make_xhttp_node("XHTTP Node");
        let config = build_xray_plugin_config(&node, 18080, None).unwrap();

        let inbound = config["inbounds"].as_array().unwrap().first().unwrap();
        assert_eq!(inbound["protocol"].as_str(), Some("socks"));
        assert_eq!(inbound["port"].as_u64(), Some(18080));

        let outbound = config["outbounds"].as_array().unwrap().first().unwrap();
        assert_eq!(outbound["protocol"].as_str(), Some("vless"));
        assert_eq!(
            outbound["settings"]["vnext"][0]["users"][0]["encryption"].as_str(),
            Some("mlkem768x25519plus.native.0rtt.test")
        );
        assert_eq!(
            outbound["settings"]["vnext"][0]["users"][0]["flow"].as_str(),
            Some("xtls-rprx-vision")
        );
        assert_eq!(
            outbound["streamSettings"]["network"].as_str(),
            Some("xhttp")
        );
        assert_eq!(
            outbound["streamSettings"]["xhttpSettings"]["path"].as_str(),
            Some("/proxy")
        );
        assert_eq!(
            outbound["streamSettings"]["xhttpSettings"]["host"].as_str(),
            Some("cdn.example.com")
        );
    }

    #[test]
    fn build_xray_plugin_config_routes_remote_outbound_through_front_proxy() {
        let node = make_xhttp_node("XHTTP Node");
        let config = build_xray_plugin_config(&node, 18080, Some(19090)).unwrap();
        let outbounds = config["outbounds"].as_array().unwrap();
        let remote = outbounds
            .iter()
            .find(|outbound| outbound["protocol"].as_str() == Some("vless"))
            .unwrap();
        let bridge = outbounds
            .iter()
            .find(|outbound| outbound["tag"].as_str() == Some("kunbox-front-proxy-bridge"))
            .unwrap();

        assert_eq!(
            remote["proxySettings"]["tag"].as_str(),
            Some("kunbox-front-proxy-bridge")
        );
        assert_eq!(
            remote["proxySettings"]["transportLayer"].as_bool(),
            Some(true)
        );
        assert_eq!(bridge["protocol"].as_str(), Some("socks"));
        assert_eq!(
            bridge["settings"]["servers"][0]["address"].as_str(),
            Some("127.0.0.1")
        );
        assert_eq!(
            bridge["settings"]["servers"][0]["port"].as_u64(),
            Some(19090)
        );
    }

    #[test]
    fn plugin_bridge_port_rejects_invalid_values() {
        assert_eq!(
            parse_plugin_bridge_port(&serde_json::json!({ "port": 65535 }), "port").unwrap(),
            Some(65535)
        );
        assert!(parse_plugin_bridge_port(&serde_json::json!({ "port": 0 }), "port").is_err());
        assert!(parse_plugin_bridge_port(&serde_json::json!({ "port": 65536 }), "port").is_err());
        assert!(parse_plugin_bridge_port(&serde_json::json!({ "port": "18080" }), "port").is_err());
        assert_eq!(
            parse_plugin_bridge_port(&serde_json::json!({}), "frontProxyChainPort").unwrap(),
            None
        );
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

        assert_eq!(
            outbound.get("type").and_then(|value| value.as_str()),
            Some("naive")
        );
        assert!(outbound.get("network").is_none());
        assert!(outbound.get("transport").is_none());
        assert_eq!(
            outbound.get("quic").and_then(|value| value.as_bool()),
            Some(false)
        );
        assert_eq!(
            outbound
                .get("domain_strategy")
                .and_then(|value| value.as_str()),
            Some("ipv4_only")
        );
        assert_eq!(
            outbound
                .get("domain_resolver")
                .and_then(|value| value.get("server"))
                .and_then(|value| value.as_str()),
            Some("dns-bootstrap")
        );
        assert_eq!(
            outbound
                .get("domain_resolver")
                .and_then(|value| value.get("strategy"))
                .and_then(|value| value.as_str()),
            Some("ipv4_only")
        );
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
        assert!(outbound
            .get("tls")
            .and_then(|value| value.get("insecure"))
            .is_none());
    }

    #[test]
    fn node_for_singbox_adds_tls_to_anytls() {
        let node = serde_json::json!({
            "type": "anytls",
            "tag": "AnyTLS",
            "server": "204.136.11.104",
            "server_port": 31424,
            "password": "secret",
            "sni": "anyway.example.com",
            "skip-cert-verify": true,
            "udp": true
        });
        let mut bridge_specs = Vec::new();

        let outbound = node_for_singbox_with_plugin_bridge(&node, &mut bridge_specs);

        assert_eq!(
            outbound.get("type").and_then(|value| value.as_str()),
            Some("anytls")
        );
        assert_eq!(
            outbound
                .get("tls")
                .and_then(|value| value.get("enabled"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            outbound
                .get("tls")
                .and_then(|value| value.get("server_name"))
                .and_then(|value| value.as_str()),
            Some("anyway.example.com")
        );
        assert_eq!(
            outbound
                .get("tls")
                .and_then(|value| value.get("insecure"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert!(outbound.get("sni").is_none());
        assert!(outbound.get("skip-cert-verify").is_none());
        assert!(outbound.get("udp").is_none());
    }

    #[test]
    fn node_for_singbox_adds_bootstrap_resolver_for_domain_proxy_server() {
        let node = serde_json::json!({
            "tag": "VMess",
            "type": "vmess",
            "server": "vmess.example.com",
            "server_port": 443,
            "uuid": "00000000-0000-0000-0000-000000000000"
        });
        let mut bridge_specs = Vec::new();

        let outbound = node_for_singbox_with_plugin_bridge(&node, &mut bridge_specs);

        assert_eq!(
            outbound
                .get("domain_resolver")
                .and_then(|value| value.get("server"))
                .and_then(|value| value.as_str()),
            Some("dns-bootstrap")
        );
        assert_eq!(
            outbound
                .get("domain_strategy")
                .and_then(|value| value.as_str()),
            Some("ipv4_only")
        );
        assert_eq!(
            outbound
                .get("domain_resolver")
                .and_then(|value| value.get("strategy"))
                .and_then(|value| value.as_str()),
            Some("ipv4_only")
        );
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

        let config = build_xray_plugin_config(&node, 18080, None).unwrap();
        let outbound = config["outbounds"].as_array().unwrap().first().unwrap();

        assert_eq!(
            outbound["settings"]["vnext"][0]["users"][0]["encryption"].as_str(),
            Some("mlkem768x25519plus.native.0rtt.legacy")
        );
        assert!(outbound["streamSettings"]["xhttpSettings"]["extra"]
            .get("encryption")
            .is_none());
        assert_eq!(
            outbound["streamSettings"]["xhttpSettings"]["extra"]["noGRPCHeader"].as_bool(),
            Some(true)
        );
    }

    #[tokio::test]
    async fn bounded_selector_probe_stream_yields_before_all_probes_finish() {
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let completed = Arc::new(AtomicUsize::new(0));
        let tags: Vec<String> = (0..12).map(|idx| format!("node-{idx}")).collect();

        let probes = bounded_selector_probe_stream(tags.clone(), 3, |tag| {
            let active = active.clone();
            let max_active = max_active.clone();
            let completed = completed.clone();
            async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                update_max(&max_active, current);
                let delay = if tag == "node-0" { 5 } else { 50 };
                tokio::time::sleep(Duration::from_millis(delay)).await;
                active.fetch_sub(1, Ordering::SeqCst);
                completed.fetch_add(1, Ordering::SeqCst);
                (tag, Some(10))
            }
        });
        futures_util::pin_mut!(probes);

        let first = probes.next().await.unwrap();
        assert_eq!(first.0, "node-0");
        assert!(completed.load(Ordering::SeqCst) < tags.len());
        assert!(active.load(Ordering::SeqCst) > 0);

        let mut results = vec![first];
        while let Some(result) = probes.next().await {
            results.push(result);
        }

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
                lower.contains("not found")
                    || lower.contains("没有运行的任务")
                    || lower.contains("没有找到")
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
    let client = local_clash_api_client();
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

async fn prepare_health_monitor_cancel(
    state: &AppState,
    enabled: bool,
) -> Option<CancellationToken> {
    if !enabled {
        return None;
    }

    let cancel = CancellationToken::new();
    let mut guard = state.health_cancel.lock().await;
    if let Some(previous) = guard.take() {
        previous.cancel();
    }
    *guard = Some(cancel.clone());
    Some(cancel)
}

async fn cancel_health_monitor(health_cancel: Arc<tokio::sync::Mutex<Option<CancellationToken>>>) {
    if let Some(cancel) = health_cancel.lock().await.take() {
        cancel.cancel();
    }
}

async fn start_health_monitor(app: AppHandle, state: &AppState, settings: AppSettings) {
    let Some(cancel) = prepare_health_monitor_cancel(state, settings.health_monitor_enabled).await
    else {
        return;
    };
    let state_for_monitor = state.clone();
    tokio::spawn(async move {
        run_health_monitor(app, state_for_monitor, cancel, settings).await;
    });
}

async fn run_health_monitor(
    app: AppHandle,
    state: AppState,
    cancel: CancellationToken,
    settings: AppSettings,
) {
    tokio::select! {
        _ = cancel.cancelled() => return,
        _ = tokio::time::sleep(std::time::Duration::from_secs(3)) => {}
    }

    let client = local_clash_api_client();
    let mut node_health = std::collections::HashMap::new();
    let mut selector_health = std::collections::HashMap::new();

    loop {
        if !matches!(*state.proxy_state.lock().await, ProxyState::Connected) {
            break;
        }

        run_health_monitor_once(
            &app,
            &state,
            &settings,
            &client,
            &mut node_health,
            &mut selector_health,
        )
        .await;

        let interval = settings.health_probe_interval_sec.max(5);
        tokio::select! {
            _ = cancel.cancelled() => break,
            _ = tokio::time::sleep(std::time::Duration::from_secs(interval)) => {}
        }
    }
}

async fn run_health_monitor_once(
    app: &AppHandle,
    state: &AppState,
    settings: &AppSettings,
    client: &reqwest::Client,
    node_health: &mut std::collections::HashMap<String, NodeHealth>,
    selector_health: &mut std::collections::HashMap<String, SelectorHealth>,
) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let clash_api_port = get_clash_api_port(state).await;
    let targets = collect_health_targets(state).await;
    let health_eligible_nodes = collect_health_eligible_node_tags(state).await;

    for target in targets {
        match target.kind {
            HealthTargetKind::Selector => {
                let Some(selector_tag) = target.selector_tag.as_deref() else {
                    continue;
                };
                let Some(mut selector) =
                    fetch_selector_health(client, clash_api_port, selector_tag).await
                else {
                    continue;
                };
                selector.current_auto_selection_eligible = selector
                    .current_node
                    .as_ref()
                    .is_some_and(|tag| health_eligible_nodes.contains(tag));
                selector
                    .backup_nodes
                    .retain(|tag| health_eligible_nodes.contains(tag));
                if let Some(previous) = selector_health.get(selector_tag) {
                    selector.last_switch_at = previous.last_switch_at;
                    selector.switch_cooldown_until = previous.switch_cooldown_until;
                }

                if selector.current_auto_selection_eligible {
                    let Some(current_node) = selector.current_node.clone() else {
                        continue;
                    };
                    let probe_url = health_probe_url_for_target(&target, settings);
                    let health = node_health
                        .entry(current_node.clone())
                        .or_insert_with(|| NodeHealth::new(current_node.clone()));
                    if should_probe(health, now_ms) {
                        let (_, latency) = probe_selector_node_latency(
                            client.clone(),
                            clash_api_port,
                            current_node,
                            probe_url,
                            settings.latency_test_timeout as u64,
                        )
                        .await;
                        match latency {
                            Some(value) => record_probe_success(health, value, now_ms),
                            None => record_probe_failure(
                                health,
                                "Clash API delay 探针失败".to_string(),
                                now_ms,
                            ),
                        }
                    }
                }

                let backup_probe_nodes = select_backup_probe_candidates(
                    &selector.backup_nodes,
                    node_health,
                    now_ms,
                    HEALTH_BACKUP_PROBE_LIMIT,
                );
                let backup_probe_results = bounded_selector_probe_stream(
                    backup_probe_nodes,
                    HEALTH_BACKUP_PROBE_LIMIT,
                    |backup_node| {
                        let client = client.clone();
                        let test_url = settings.latency_test_url.clone();
                        async move {
                            probe_selector_node_latency(
                                client,
                                clash_api_port,
                                backup_node,
                                test_url,
                                settings.latency_test_timeout as u64,
                            )
                            .await
                        }
                    },
                );
                futures_util::pin_mut!(backup_probe_results);
                while let Some((backup_node, latency)) = backup_probe_results.next().await {
                    let health = node_health
                        .entry(backup_node.clone())
                        .or_insert_with(|| NodeHealth::new(backup_node.clone()));
                    match latency {
                        Some(value) => record_probe_success(health, value, now_ms),
                        None => record_probe_failure(
                            health,
                            "Clash API delay 探针失败".to_string(),
                            now_ms,
                        ),
                    }
                }

                let action = decide_health_action(&target, Some(&selector), node_health, now_ms);
                let action = gate_health_action_for_state(action, state, settings).await;
                let action_executed =
                    execute_health_action(app, state, client, clash_api_port, &action).await;

                if action_executed {
                    if let HealthAction::SwitchSelector {
                        selector: switched_selector,
                        ..
                    } = action
                    {
                        if switched_selector == selector_tag {
                            selector.last_switch_at = Some(now_ms);
                            selector.switch_cooldown_until =
                                Some(now_ms + HEALTH_SELECTOR_SWITCH_COOLDOWN_MS);
                        }
                    }
                }
                selector_health.insert(selector_tag.to_string(), selector);
            }
            HealthTargetKind::FixedNode => {
                let Some(node_tag) = target.node_tag.clone() else {
                    continue;
                };
                if !health_eligible_nodes.contains(&node_tag) {
                    continue;
                }
                let probe_url = health_probe_url_for_target(&target, settings);
                let health = node_health
                    .entry(node_tag.clone())
                    .or_insert_with(|| NodeHealth::new(node_tag));
                if should_probe(health, now_ms) {
                    match probe_real_proxy_path(
                        settings.local_port,
                        &probe_url,
                        settings.latency_test_timeout as u64,
                    )
                    .await
                    {
                        Ok(latency) => record_probe_success(health, latency, now_ms),
                        Err(err) => record_probe_failure(health, err, now_ms),
                    }
                }
                let action = decide_health_action(&target, None, node_health, now_ms);
                execute_health_action(app, state, client, clash_api_port, &action).await;
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
        Ok(content) => {
            serde_json::from_str::<crate::types::ProfilesData>(&content).unwrap_or_default()
        }
        Err(err) => {
            log::warn!(
                "Failed to read profiles file for selector collection: {}",
                err
            );
            crate::types::ProfilesData::default()
        }
    }
}

async fn collect_health_eligible_node_tags(state: &AppState) -> std::collections::HashSet<String> {
    let profiles_data = load_profiles_data_from_file(state).await;
    let active_profile_id = profiles_data.active_profile_id.as_deref();
    let mut tags = std::collections::HashSet::new();
    for profile in profiles_data
        .profiles
        .iter()
        .filter(|profile| profile.enabled)
    {
        let Some(nodes_file) = profile_nodes_path(state, &profile.id).ok() else {
            continue;
        };
        let nodes = fs::read_to_string(nodes_file)
            .ok()
            .and_then(|content| serde_json::from_str::<Vec<serde_json::Value>>(&content).ok())
            .unwrap_or_default();
        for node in nodes
            .iter()
            .filter(|node| node_is_auto_selection_eligible(node))
        {
            let Some(tag) = node.get("tag").and_then(serde_json::Value::as_str) else {
                continue;
            };
            tags.insert(if active_profile_id == Some(profile.id.as_str()) {
                tag.to_string()
            } else {
                normalized_node_reference_tag(&format!("{}::{}", profile.id, tag))
            });
        }
    }
    tags
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

async fn resolve_health_main_selector_tag(state: &AppState) -> String {
    let profiles_data = load_profiles_data_from_file(state).await;
    let nodes = profiles_data
        .active_profile_id
        .as_deref()
        .and_then(|profile_id| profile_nodes_path(state, profile_id).ok())
        .and_then(|nodes_file| fs::read_to_string(nodes_file).ok())
        .and_then(|content| serde_json::from_str::<Vec<serde_json::Value>>(&content).ok())
        .map(|raw_nodes| {
            raw_nodes
                .iter()
                .map(process_node)
                .filter(|node| {
                    node.get("type")
                        .and_then(|value| value.as_str())
                        .is_some_and(is_proxy_type)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let rulesets = state.rulesets.lock().await.clone();
    let custom_rules = state.custom_rules.lock().await.clone();
    let (referenced_profile_ids, referenced_profile_scoped_node_refs) =
        collect_route_profile_and_node_references(&rulesets, &custom_rules);

    if selector_tag_collides(
        "PROXY",
        &nodes,
        &referenced_profile_scoped_node_refs,
        &referenced_profile_ids,
    ) {
        "PROXY-kb".to_string()
    } else {
        "PROXY".to_string()
    }
}

async fn collect_health_targets(state: &AppState) -> Vec<HealthTarget> {
    let mut targets = Vec::new();
    let mut seen_selectors = std::collections::HashSet::new();
    let mut seen_fixed = std::collections::HashSet::new();

    let main_selector_tag = resolve_health_main_selector_tag(state).await;
    if seen_selectors.insert(main_selector_tag.clone()) {
        targets.push(HealthTarget {
            kind: HealthTargetKind::Selector,
            selector_tag: Some(main_selector_tag),
            node_tag: None,
            rule_label: None,
            auto_failover: true,
        });
    }

    for selector_tag in collect_referenced_profile_selector_tags(state).await {
        if seen_selectors.insert(selector_tag.clone()) {
            targets.push(HealthTarget {
                kind: HealthTargetKind::Selector,
                selector_tag: Some(selector_tag),
                node_tag: None,
                rule_label: None,
                auto_failover: true,
            });
        }
    }

    let rulesets = state.rulesets.lock().await.clone();
    let custom_rules = state.custom_rules.lock().await.clone();

    for rule in custom_rules.domain_rules.iter().filter(|rule| rule.enabled) {
        if rule.outbound_mode != "node" {
            continue;
        }

        let Some(node_ref) = rule.outbound_value.as_deref() else {
            continue;
        };
        let node_tag = normalized_node_reference_tag(node_ref);
        let dedup_key = format!("domain:{}:{}", rule.value, node_tag);
        if seen_fixed.insert(dedup_key) {
            targets.push(HealthTarget {
                kind: HealthTargetKind::FixedNode,
                selector_tag: None,
                node_tag: Some(node_tag),
                rule_label: Some(rule.value.clone()),
                auto_failover: false,
            });
        }
    }

    for ruleset in rulesets.iter().filter(|ruleset| ruleset.enabled) {
        if !matches!(ruleset.outbound_mode.as_str(), "node" | "节点") {
            continue;
        }

        let Some(node_ref) = ruleset.outbound_value.as_deref() else {
            continue;
        };
        let node_tag = normalized_node_reference_tag(node_ref);
        let dedup_key = format!("ruleset:{}:{}", ruleset.tag, node_tag);
        if seen_fixed.insert(dedup_key) {
            targets.push(HealthTarget {
                kind: HealthTargetKind::FixedNode,
                selector_tag: None,
                node_tag: Some(node_tag),
                rule_label: Some(ruleset.tag.clone()),
                auto_failover: false,
            });
        }
    }

    targets
}

fn health_probe_url_for_target(target: &HealthTarget, settings: &AppSettings) -> String {
    if let Some(label) = target.rule_label.as_deref() {
        let label = label.trim();
        if !label.is_empty()
            && label.contains('.')
            && !label.contains('/')
            && !label.starts_with("rs-")
        {
            return format!("https://{}/", label.trim_start_matches('.'));
        }
    }

    settings.latency_test_url.clone()
}

async fn fetch_selector_health(
    client: &reqwest::Client,
    clash_api_port: u16,
    selector_tag: &str,
) -> Option<SelectorHealth> {
    let resp = client
        .get(format!(
            "http://127.0.0.1:{}/proxies/{}",
            clash_api_port,
            urlencoding::encode(selector_tag)
        ))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }

    let data = resp.json::<serde_json::Value>().await.ok()?;
    let current_node = data
        .get("now")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    let backup_nodes = data
        .get("all")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str())
                .filter(|node| Some(*node) != current_node.as_deref())
                .map(|node| node.to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Some(SelectorHealth {
        selector_tag: selector_tag.to_string(),
        current_node,
        current_auto_selection_eligible: true,
        backup_nodes,
        last_switch_at: None,
        switch_cooldown_until: None,
    })
}

async fn main_selector_node_signatures(
    state: &AppState,
    from: &str,
    to: &str,
) -> ((bool, Option<String>), (bool, Option<String>)) {
    let profiles_data = load_profiles_data_from_file(state).await;
    let Some(active_profile_id) = profiles_data.active_profile_id.as_deref() else {
        return ((false, None), (false, None));
    };
    let nodes_file = match profile_nodes_path(state, active_profile_id) {
        Ok(path) => path,
        Err(_) => return ((false, None), (false, None)),
    };
    let raw_nodes = fs::read_to_string(nodes_file)
        .ok()
        .and_then(|content| serde_json::from_str::<Vec<serde_json::Value>>(&content).ok())
        .unwrap_or_default();

    (
        node_bootstrap_signature(active_or_first_node(&raw_nodes, Some(from))),
        node_bootstrap_signature(active_or_first_node(&raw_nodes, Some(to))),
    )
}

async fn gate_health_action_for_state(
    action: HealthAction,
    state: &AppState,
    settings: &AppSettings,
) -> HealthAction {
    let HealthAction::SwitchSelector { selector, from, to } = action else {
        return action;
    };

    if !is_main_selector_tag(&selector) {
        return HealthAction::SwitchSelector { selector, from, to };
    }

    let (previous_signature, target_signature) =
        main_selector_node_signatures(state, &from, &to).await;
    gate_main_selector_health_action(
        HealthAction::SwitchSelector { selector, from, to },
        settings,
        previous_signature,
        target_signature,
    )
}

async fn persist_selector_selection(
    state: &AppState,
    selector_tag: &str,
    runtime_node_tag: &str,
) -> Result<(), String> {
    let mut cached_profiles = state.profiles_data.lock().await;
    let profiles_file = state.profiles_file();
    let profiles_content =
        fs::read_to_string(&profiles_file).map_err(|err| format!("读取配置选择失败: {}", err))?;
    let mut profiles_data: crate::types::ProfilesData = serde_json::from_str(&profiles_content)
        .map_err(|err| format!("解析配置选择失败: {}", err))?;

    if is_main_selector_tag(selector_tag) {
        let active_profile_id = profiles_data
            .active_profile_id
            .as_deref()
            .ok_or_else(|| "当前没有活动配置".to_string())?;
        let nodes_file = profile_nodes_path(state, active_profile_id)?;
        let nodes_content = fs::read_to_string(nodes_file)
            .map_err(|err| format!("读取活动配置节点失败: {}", err))?;
        let nodes: Vec<serde_json::Value> = serde_json::from_str(&nodes_content)
            .map_err(|err| format!("解析活动配置节点失败: {}", err))?;
        if !nodes.iter().any(|node| {
            node.get("tag").and_then(serde_json::Value::as_str) == Some(runtime_node_tag)
        }) {
            return Err(format!("活动配置中不存在节点 '{}'", runtime_node_tag));
        }
        profiles_data.active_node_tag = Some(runtime_node_tag.to_string());
    } else {
        let profile_id = selector_tag
            .strip_prefix("P:")
            .filter(|profile_id| is_valid_profile_id(profile_id))
            .ok_or_else(|| format!("无效的配置 selector '{}'", selector_tag))?;
        if !profiles_data
            .profiles
            .iter()
            .any(|profile| profile.id == profile_id)
        {
            return Err(format!("selector 对应的配置 '{}' 不存在", profile_id));
        }

        let nodes_file = profile_nodes_path(state, profile_id)?;
        let nodes_content = fs::read_to_string(nodes_file)
            .map_err(|err| format!("读取 selector 配置节点失败: {}", err))?;
        let nodes: Vec<serde_json::Value> = serde_json::from_str(&nodes_content)
            .map_err(|err| format!("解析 selector 配置节点失败: {}", err))?;
        let node_exists = |tag: &str| {
            nodes
                .iter()
                .any(|node| node.get("tag").and_then(serde_json::Value::as_str) == Some(tag))
        };
        let scoped_prefix = format!("{}::", profile_id);
        let scoped_raw_tag = runtime_node_tag.strip_prefix(&scoped_prefix);
        let profile_is_active = profiles_data.active_profile_id.as_deref() == Some(profile_id);
        let raw_node_tag = if profile_is_active && node_exists(runtime_node_tag) {
            runtime_node_tag
        } else if let Some(raw_tag) = scoped_raw_tag.filter(|tag| node_exists(tag)) {
            raw_tag
        } else if node_exists(runtime_node_tag) {
            runtime_node_tag
        } else {
            return Err(format!(
                "配置 '{}' 中不存在 selector 节点 '{}'",
                profile_id, runtime_node_tag
            ));
        };
        profiles_data
            .node_selections
            .insert(profile_id.to_string(), raw_node_tag.to_string());
    }

    let updated_content =
        serde_json::to_string_pretty(&profiles_data).map_err(|err| err.to_string())?;
    fs::write(profiles_file, updated_content).map_err(|err| err.to_string())?;
    *cached_profiles = profiles_data;
    Ok(())
}

async fn execute_health_action(
    app: &AppHandle,
    state: &AppState,
    client: &reqwest::Client,
    clash_api_port: u16,
    action: &HealthAction,
) -> bool {
    match action {
        HealthAction::None => return false,
        HealthAction::SwitchSelector { selector, to, .. } => {
            if let Err(err) = switch_selector_to_node(client, clash_api_port, selector, to).await {
                log::warn!(
                    "Health failover failed to switch selector '{}' to '{}': {}",
                    selector,
                    to,
                    err
                );
                return false;
            }

            if let Err(err) = persist_selector_selection(state, selector, to).await {
                log::warn!(
                    "Failed to persist health failover selector '{}' node '{}': {}",
                    selector,
                    to,
                    err
                );
            }
        }
        HealthAction::NotifyFixedNodeFailed { .. }
        | HealthAction::NotifyMainNodeNeedsManualSwitch { .. }
        | HealthAction::NotifyNoBackup { .. } => {}
    }

    if let Some(event) = health_event_for_action(action) {
        let _ = app.emit("singbox:health", event);
    }
    true
}

async fn switch_selector_to_node(
    client: &reqwest::Client,
    clash_api_port: u16,
    selector_tag: &str,
    node_tag: &str,
) -> Result<(), String> {
    let resp = client
        .put(format!(
            "http://127.0.0.1:{}/proxies/{}",
            clash_api_port,
            urlencoding::encode(selector_tag)
        ))
        .json(&serde_json::json!({ "name": node_tag }))
        .send()
        .await
        .map_err(|err| {
            format!(
                "请求 Clash API 切换 selector '{}' 到节点 '{}' 失败: {}",
                selector_tag, node_tag, err
            )
        })?;

    if !resp.status().is_success() {
        return Err(format!(
            "Clash API 切换 selector '{}' 到节点 '{}' 返回 {}",
            selector_tag,
            node_tag,
            resp.status()
        ));
    }

    match close_stale_selector_connections(client, clash_api_port, selector_tag, node_tag).await {
        Ok(closed) if closed > 0 => {
            log::info!(
                "Closed {} stale connections after switching selector '{}' to '{}'",
                closed,
                selector_tag,
                node_tag
            );
        }
        Err(err) => {
            log::warn!(
                "Selector '{}' switched to '{}', but stale connection cleanup failed: {}",
                selector_tag,
                node_tag,
                err
            );
        }
        _ => {}
    }

    Ok(())
}

async fn close_stale_selector_connections(
    client: &reqwest::Client,
    clash_api_port: u16,
    selector_tag: &str,
    selected_node_tag: &str,
) -> Result<usize, String> {
    let resp = client
        .get(format!("http://127.0.0.1:{}/connections", clash_api_port))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .map_err(|err| format!("获取活跃连接失败: {}", err))?;
    if !resp.status().is_success() {
        return Err(format!("获取活跃连接返回 {}", resp.status()));
    }

    let data: serde_json::Value = resp
        .json()
        .await
        .map_err(|err| format!("解析活跃连接失败: {}", err))?;
    let stale_connection_ids = data
        .get("connections")
        .and_then(|connections| connections.as_array())
        .into_iter()
        .flatten()
        .filter(|connection| {
            connection
                .get("chains")
                .and_then(|chains| chains.as_array())
                .is_some_and(|chains| {
                    let selector_index = chains
                        .iter()
                        .rposition(|tag| tag.as_str() == Some(selector_tag));
                    selector_index.is_some_and(|selector_index| {
                        selector_index
                            .checked_sub(1)
                            .and_then(|index| chains.get(index))
                            .and_then(|tag| tag.as_str())
                            != Some(selected_node_tag)
                    })
                })
        })
        .filter_map(|connection| {
            connection
                .get("id")
                .and_then(|id| id.as_str())
                .filter(|id| !id.is_empty())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();

    let mut closed = 0;
    let mut failures = Vec::new();
    for connection_id in stale_connection_ids {
        match client
            .delete(format!(
                "http://127.0.0.1:{}/connections/{}",
                clash_api_port,
                urlencoding::encode(&connection_id)
            ))
            .timeout(std::time::Duration::from_secs(2))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => closed += 1,
            Ok(resp) => failures.push(format!("{} 返回 {}", connection_id, resp.status())),
            Err(err) => failures.push(format!("{} 删除失败: {}", connection_id, err)),
        }
    }

    if failures.is_empty() {
        Ok(closed)
    } else {
        Err(failures.join("；"))
    }
}

async fn probe_real_proxy_path(local_port: u16, url: &str, timeout_ms: u64) -> Result<u32, String> {
    let timeout_ms = if timeout_ms == 0 { 5_000 } else { timeout_ms };
    let proxy = reqwest::Proxy::all(format!("http://127.0.0.1:{local_port}"))
        .map_err(|err| format!("真实路径探针代理配置失败: {}", err))?;
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()
        .map_err(|err| format!("真实路径探针客户端创建失败: {}", err))?;
    let started_at = std::time::Instant::now();
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|err| format!("真实路径探针请求失败: {}", err))?;
    let status = resp.status();

    if status.is_success() || status.is_redirection() {
        let elapsed_ms = started_at.elapsed().as_millis().min(u32::MAX as u128) as u32;
        Ok(elapsed_ms)
    } else {
        Err(format!("真实路径探针 HTTP 状态异常: {}", status.as_u16()))
    }
}

async fn probe_selector_node_latency(
    client: reqwest::Client,
    clash_api_port: u16,
    tag: String,
    test_url: String,
    timeout_ms: u64,
) -> (String, Option<u32>) {
    let timeout_query = timeout_ms.to_string();
    let result = client
        .get(format!(
            "http://127.0.0.1:{}/proxies/{}/delay",
            clash_api_port,
            urlencoding::encode(&tag)
        ))
        .query(&[
            ("url", test_url.as_str()),
            ("timeout", timeout_query.as_str()),
        ])
        .timeout(std::time::Duration::from_millis(
            timeout_ms.saturating_add(1000),
        ))
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

fn bounded_selector_probe_stream<I, F, Fut>(
    items: I,
    concurrency_limit: usize,
    probe: F,
) -> impl futures_util::Stream<Item = (String, Option<u32>)>
where
    I: IntoIterator<Item = String>,
    F: FnMut(String) -> Fut,
    Fut: Future<Output = (String, Option<u32>)>,
{
    futures_util::stream::iter(items)
        .map(probe)
        .buffer_unordered(concurrency_limit.max(1))
}

async fn test_selector_latency_internal(
    app: &AppHandle,
    selector_tag: String,
    test_url: Option<String>,
) -> Result<serde_json::Value, String> {
    let state = app.state::<AppState>();
    let clash_api_port = get_clash_api_port(&state).await;
    let client = local_clash_api_client();
    let settings = state.settings.lock().await.clone();
    let timeout_ms = settings.latency_test_timeout as u64;
    let test_url = test_url.unwrap_or_else(|| settings.latency_test_url.clone());

    let resp = client
        .get(format!(
            "http://127.0.0.1:{}/proxies/{}",
            clash_api_port,
            urlencoding::encode(&selector_tag)
        ))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("Failed to get selector info: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Selector '{}' not found", selector_tag));
    }

    let selector_info: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut node_tags: Vec<String> = selector_info
        .get("all")
        .and_then(|a| a.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();
    let auto_eligible_nodes = collect_health_eligible_node_tags(&state).await;
    node_tags.retain(|tag| auto_eligible_nodes.contains(tag));

    if node_tags.is_empty() {
        return Ok(serde_json::json!({
            "success": true,
            "message": "No auto-selection eligible nodes to test"
        }));
    }

    log::info!(
        "Testing {} nodes for selector '{}'",
        node_tags.len(),
        selector_tag
    );

    let probe_results = bounded_selector_probe_stream(
        node_tags.clone(),
        SELECTOR_LATENCY_CONCURRENCY_LIMIT,
        |tag| {
            let client = client.clone();
            let test_url = test_url.clone();
            async move {
                probe_selector_node_latency(client, clash_api_port, tag, test_url, timeout_ms).await
            }
        },
    );
    futures_util::pin_mut!(probe_results);

    let mut first_switch_done = false;
    let mut best_node: Option<(String, u32)> = None;
    let mut valid_count: usize = 0;
    let first_switch_threshold = std::cmp::min(5usize, node_tags.len());
    let mut results = Vec::with_capacity(node_tags.len());

    while let Some((tag, delay)) = probe_results.next().await {
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
                log::info!(
                    "First phase done, switching '{}' to '{}' ({}ms)",
                    selector_tag,
                    best_tag,
                    best_delay
                );
                match switch_selector_to_node(&client, clash_api_port, &selector_tag, best_tag)
                    .await
                {
                    Ok(()) => {
                        if let Err(err) =
                            persist_selector_selection(&state, &selector_tag, best_tag).await
                        {
                            log::warn!(
                                "First phase selector selection persistence failed: {}",
                                err
                            );
                        }
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
                    Err(err) => {
                        log::warn!("First phase selector switch failed: {}", err);
                    }
                }
            }
            first_switch_done = true;
        }

        results.push((tag, delay));
    }

    if let Some((best_tag, best_delay)) = &best_node {
        log::info!(
            "Final switch '{}' to '{}' ({}ms)",
            selector_tag,
            best_tag,
            best_delay
        );
        match switch_selector_to_node(&client, clash_api_port, &selector_tag, best_tag).await {
            Ok(()) => {
                if let Err(err) = persist_selector_selection(&state, &selector_tag, best_tag).await
                {
                    log::warn!("Final selector selection persistence failed: {}", err);
                }
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
            Err(err) => {
                log::warn!("Final selector switch failed: {}", err);
            }
        }
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
    test_url: Option<String>,
) -> Result<serde_json::Value, String> {
    test_selector_latency_internal(&app, selector_tag, test_url).await
}
