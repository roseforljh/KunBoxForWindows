use crate::state::AppState;
use crate::types::{
    node_is_metered_protected, AppSettings, NodeLatencyResult, NodeLatencyStatus, ProxyState,
};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::catalog::{load_profile_nodes, load_profile_nodes_raw, load_profiles_data};
use super::subscription::ECH_DNS_SERVER_META_KEY;

mod config;
mod probe;
mod runtime;

#[cfg(test)]
mod tests;

use config::*;
use probe::*;
pub(crate) use runtime::check_clash_api_running;
use runtime::*;

#[cfg(windows)]
#[allow(unused_imports)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

static TEMP_SINGBOX_PROCESS: once_cell::sync::Lazy<Arc<Mutex<Option<tokio::process::Child>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));
static TEMP_XRAY_PROCESSES: once_cell::sync::Lazy<Arc<Mutex<Vec<tokio::process::Child>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(Vec::new())));
static TEMP_SINGBOX_TAG_MAP: once_cell::sync::Lazy<
    Arc<Mutex<std::collections::HashMap<String, Vec<String>>>>,
> = once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(std::collections::HashMap::new())));
static TEMP_SINGBOX_PROXY_PORT_MAP: once_cell::sync::Lazy<
    Arc<Mutex<std::collections::HashMap<String, Vec<u16>>>>,
> = once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(std::collections::HashMap::new())));
static TEMP_SINGBOX_ACTIVE_TESTS: once_cell::sync::Lazy<Arc<Mutex<usize>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(0)));
static TEMP_SINGBOX_OWNER_BATCH_ID: once_cell::sync::Lazy<Arc<Mutex<Option<u64>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));
static LATENCY_TEST_CANCEL_TOKEN: once_cell::sync::Lazy<Arc<Mutex<CancellationToken>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(CancellationToken::new())));
static ACTIVE_LATENCY_BATCH_ID: once_cell::sync::Lazy<Arc<Mutex<Option<u64>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));
static CANCELED_LATENCY_BATCHES: once_cell::sync::Lazy<Arc<Mutex<HashSet<u64>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(HashSet::new())));
const TEMP_SINGBOX_PORT: u16 = 19090;
const TEMP_XRAY_BRIDGE_PORT_BASE: u16 = 19180;
const TEMP_PROXY_INBOUND_PORT_BASE: u16 = 19280;
const TEMP_LATENCY_FALLBACK_REMOTE_DNS: &str = "223.5.5.5";

#[derive(Debug, PartialEq, Eq)]
enum LatencyTestBackend {
    Main,
    Temp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LatencyProbeError {
    Timeout,
    ProxyFailed,
    Failed,
}

fn select_latency_test_backend(
    proxy_state: &ProxyState,
    main_api_ready: bool,
) -> LatencyTestBackend {
    if main_api_ready || matches!(proxy_state, ProxyState::Connected | ProxyState::Connecting) {
        return LatencyTestBackend::Main;
    }

    LatencyTestBackend::Temp
}

fn temp_singbox_dir(state: &AppState) -> std::path::PathBuf {
    state.data_dir.join("temp_test")
}

fn normalize_latency_test_settings(settings: &AppSettings) -> (String, u32) {
    let test_url = if settings.latency_test_url.trim().is_empty() {
        "https://www.gstatic.com/generate_204".to_string()
    } else {
        settings.latency_test_url.trim().to_string()
    };
    let timeout_ms = settings.latency_test_timeout.max(1);
    (test_url, timeout_ms)
}

fn temp_latency_remote_dns(settings: &AppSettings) -> String {
    let remote_dns = settings.remote_dns.trim();
    if remote_dns.is_empty()
        || remote_dns.eq_ignore_ascii_case("fakeip")
        || remote_dns.eq_ignore_ascii_case("local")
    {
        return TEMP_LATENCY_FALLBACK_REMOTE_DNS.to_string();
    }

    remote_dns.to_string()
}

async fn current_latency_test_settings(state: &AppState) -> (String, u32) {
    let settings = state.settings.lock().await.clone();
    normalize_latency_test_settings(&settings)
}

fn map_latency_probe_result(
    result: Result<i64, LatencyProbeError>,
    failure_status: NodeLatencyStatus,
) -> NodeLatencyResult {
    match result {
        Ok(latency) if latency > 0 => NodeLatencyResult::success(latency),
        Ok(_) | Err(LatencyProbeError::Timeout) => NodeLatencyResult::timeout(),
        Err(LatencyProbeError::ProxyFailed) => NodeLatencyResult::proxy_failed(),
        Err(LatencyProbeError::Failed) => match failure_status {
            NodeLatencyStatus::ControllerUnavailable => NodeLatencyResult::controller_unavailable(),
            NodeLatencyStatus::LocalTestFailed => NodeLatencyResult::local_test_failed(),
            NodeLatencyStatus::Timeout => NodeLatencyResult::timeout(),
            NodeLatencyStatus::ProxyFailed => NodeLatencyResult::proxy_failed(),
            NodeLatencyStatus::Success => NodeLatencyResult::local_test_failed(),
        },
    }
}

fn should_try_proxy_latency_fallback(
    clash_result: &Result<i64, LatencyProbeError>,
    proxy_port: Option<u16>,
) -> bool {
    proxy_port.is_some() && !matches!(clash_result, Ok(latency) if *latency > 0)
}

async fn test_latency_via_temp_backend(
    app: &AppHandle,
    state: &AppState,
    tag: &str,
    run_id: Option<u64>,
    test_url: &str,
    timeout_ms: u32,
    cancel_token: CancellationToken,
    allow_main_process_alive: bool,
) -> NodeLatencyResult {
    append_latency_diagnostic(
        state,
        &format!(
            "latency test requested for tag='{}', test_url='{}', timeout_ms={}",
            tag, test_url, timeout_ms
        ),
    );
    if is_latency_test_batch_cancelled(run_id).await {
        return NodeLatencyResult::local_test_failed();
    }

    if !acquire_temp_singbox_test_slot(run_id).await {
        log::debug!(
            "Temp sing-box slot owner changed before testing '{}', skipping stale request",
            tag
        );
        return NodeLatencyResult::local_test_failed();
    }

    let started = start_temp_singbox(app, state, &cancel_token, allow_main_process_alive).await;
    if !started {
        append_latency_diagnostic(
            state,
            &format!("temp sing-box failed to start for tag='{}'", tag),
        );
        log::warn!("Temp sing-box not available for latency test: {}", tag);
        release_temp_singbox_test_slot(state, run_id).await;
        return NodeLatencyResult::local_test_failed();
    }

    if is_latency_test_batch_cancelled(run_id).await {
        release_temp_singbox_test_slot(state, run_id).await;
        return NodeLatencyResult::local_test_failed();
    }

    let temp_tag = match if run_id.is_some() {
        take_temp_singbox_tag_for_batch(run_id, tag).await
    } else {
        first_temp_singbox_tag(tag).await
    } {
        Some(temp_tag) => temp_tag,
        None => {
            append_latency_diagnostic(
                state,
                &format!("temp proxy tag mapping missing for tag='{}'", tag),
            );
            log::warn!("Temp proxy tag mapping missing for '{}'", tag);
            release_temp_singbox_test_slot(state, run_id).await;
            return NodeLatencyResult::local_test_failed();
        }
    };
    let proxy_port = if run_id.is_some() {
        TEMP_SINGBOX_PROXY_PORT_MAP
            .lock()
            .await
            .get_mut(tag)
            .and_then(|ports| {
                if ports.is_empty() {
                    None
                } else {
                    Some(ports.remove(0))
                }
            })
    } else {
        first_temp_proxy_port(tag).await
    };

    let clash_result = test_latency_via_clash_api_cancellable(
        &temp_tag,
        TEMP_SINGBOX_PORT,
        test_url,
        timeout_ms,
        cancel_token.clone(),
    )
    .await;
    let result = if should_try_proxy_latency_fallback(&clash_result, proxy_port) {
        append_latency_diagnostic(
            state,
            &format!(
                "clash api latency failed for tag='{}', temp_tag='{}', fallback proxy_port={:?}: {:?}",
                tag, temp_tag, proxy_port, clash_result
            ),
        );
        match proxy_port {
            Some(proxy_port) => {
                test_latency_via_http_proxy_cancellable(
                    proxy_port,
                    test_url,
                    timeout_ms,
                    cancel_token.clone(),
                )
                .await
            }
            None => clash_result,
        }
    } else {
        clash_result
    };

    append_latency_diagnostic(
        state,
        &format!(
            "latency raw result for tag='{}', temp_tag='{}': {:?}",
            tag, temp_tag, result
        ),
    );
    release_temp_singbox_test_slot(state, run_id).await;
    map_latency_probe_result(result, NodeLatencyStatus::LocalTestFailed)
}

fn remove_temp_singbox_dir(path: &std::path::Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn append_latency_diagnostic(state: &AppState, message: &str) {
    let _ = fs::create_dir_all(&state.data_dir);
    let path = state.data_dir.join("latency-diagnostics.log");
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(path) {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{}] {}", ts, message);
    }
}

fn append_temp_latency_logs(state: &AppState) {
    let temp_dir = temp_singbox_dir(state);
    if !temp_dir.exists() {
        return;
    }

    append_latency_diagnostic(
        state,
        &format!("collecting temp latency logs from {:?}", temp_dir),
    );
    let log_names = [
        "temp-singbox.out.log",
        "temp-singbox.err.log",
        "plugin_bridges.json",
        "config.json",
    ];

    for name in log_names {
        let path = temp_dir.join(name);
        if let Ok(content) = fs::read_to_string(&path) {
            append_latency_diagnostic(
                state,
                &format!(
                    "===== {} =====\n{}\n===== end {} =====",
                    name, content, name
                ),
            );
        }
    }

    if let Ok(entries) = fs::read_dir(&temp_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !file_name.starts_with("plugin-xray-temp-") {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&path) {
                append_latency_diagnostic(
                    state,
                    &format!(
                        "===== {} =====\n{}\n===== end {} =====",
                        file_name, content, file_name
                    ),
                );
            }
        }
    }
}

async fn cleanup_temp_singbox_process() {
    let mut xray_processes = TEMP_XRAY_PROCESSES.lock().await;
    for mut child in xray_processes.drain(..) {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }

    let mut process = TEMP_SINGBOX_PROCESS.lock().await;
    if let Some(mut child) = process.take() {
        let _ = child.kill().await;
        let _ = child.wait().await;
    }
}

async fn clear_temp_singbox_tag_map() {
    TEMP_SINGBOX_TAG_MAP.lock().await.clear();
}

async fn clear_temp_singbox_proxy_port_map() {
    TEMP_SINGBOX_PROXY_PORT_MAP.lock().await.clear();
}

async fn acquire_temp_singbox_test_slot(run_id: Option<u64>) -> bool {
    let mut owner = TEMP_SINGBOX_OWNER_BATCH_ID.lock().await;
    let mut active = TEMP_SINGBOX_ACTIVE_TESTS.lock().await;

    if *active == 0 {
        *owner = run_id;
    } else if run_id.is_none() {
        return false;
    }

    if *owner != run_id {
        return false;
    }

    *active += 1;
    true
}

async fn release_temp_singbox_test_slot(state: &AppState, run_id: Option<u64>) {
    let should_cleanup = {
        let mut owner = TEMP_SINGBOX_OWNER_BATCH_ID.lock().await;
        let mut active = TEMP_SINGBOX_ACTIVE_TESTS.lock().await;
        if *owner != run_id {
            return;
        }
        if *active > 0 {
            *active -= 1;
        }
        if *active == 0 {
            *owner = None;
            true
        } else {
            false
        }
    };

    if should_cleanup {
        cleanup_temp_singbox(state).await;
    }
}

async fn read_temp_singbox_tag_map() -> std::collections::HashMap<String, Vec<String>> {
    TEMP_SINGBOX_TAG_MAP.lock().await.clone()
}

async fn read_temp_proxy_port_map() -> std::collections::HashMap<String, Vec<u16>> {
    TEMP_SINGBOX_PROXY_PORT_MAP.lock().await.clone()
}

async fn take_temp_singbox_tag_for_batch(
    run_id: Option<u64>,
    original_tag: &str,
) -> Option<String> {
    if *TEMP_SINGBOX_OWNER_BATCH_ID.lock().await != run_id {
        return None;
    }
    let mut tag_map = TEMP_SINGBOX_TAG_MAP.lock().await;
    take_temp_singbox_tag(&mut tag_map, original_tag)
}

async fn current_latency_test_cancel_token() -> CancellationToken {
    LATENCY_TEST_CANCEL_TOKEN.lock().await.clone()
}

async fn cancel_and_reset_latency_test_token() {
    let mut slot = LATENCY_TEST_CANCEL_TOKEN.lock().await;
    slot.cancel();
    *slot = CancellationToken::new();
}

async fn begin_latency_test_batch(run_id: u64) {
    cancel_and_reset_latency_test_token().await;
    *ACTIVE_LATENCY_BATCH_ID.lock().await = Some(run_id);
    CANCELED_LATENCY_BATCHES.lock().await.remove(&run_id);
}

async fn mark_latency_test_batch_cancelled(run_id: Option<u64>) {
    if let Some(run_id) = run_id {
        CANCELED_LATENCY_BATCHES.lock().await.insert(run_id);
    }

    let should_cancel_active = {
        let active_run_id = *ACTIVE_LATENCY_BATCH_ID.lock().await;
        run_id.is_none() || active_run_id == run_id
    };

    if should_cancel_active {
        cancel_and_reset_latency_test_token().await;
        *ACTIVE_LATENCY_BATCH_ID.lock().await = None;
    }
}

async fn is_latency_test_batch_cancelled(run_id: Option<u64>) -> bool {
    match run_id {
        Some(run_id) => CANCELED_LATENCY_BATCHES.lock().await.contains(&run_id),
        None => false,
    }
}

async fn first_temp_singbox_tag(original_tag: &str) -> Option<String> {
    TEMP_SINGBOX_TAG_MAP
        .lock()
        .await
        .get(original_tag)
        .and_then(|tags| tags.first().cloned())
}

async fn first_temp_proxy_port(original_tag: &str) -> Option<u16> {
    TEMP_SINGBOX_PROXY_PORT_MAP
        .lock()
        .await
        .get(original_tag)
        .and_then(|ports| ports.first().copied())
}

fn take_temp_proxy_port(
    port_map: &mut std::collections::HashMap<String, Vec<u16>>,
    original_tag: &str,
) -> Option<u16> {
    let should_remove = match port_map.get(original_tag) {
        Some(ports) => ports.len() <= 1,
        None => return None,
    };

    if should_remove {
        return port_map
            .remove(original_tag)
            .and_then(|mut ports| ports.drain(..1).next());
    }

    port_map.get_mut(original_tag).map(|ports| ports.remove(0))
}

fn take_temp_singbox_tag(
    tag_map: &mut std::collections::HashMap<String, Vec<String>>,
    original_tag: &str,
) -> Option<String> {
    let should_remove = match tag_map.get(original_tag) {
        Some(aliases) => aliases.len() <= 1,
        None => return None,
    };

    if should_remove {
        return tag_map
            .remove(original_tag)
            .and_then(|mut aliases| aliases.drain(..1).next());
    }

    tag_map
        .get_mut(original_tag)
        .map(|aliases| aliases.remove(0))
}

fn make_temp_latency_tag(index: usize) -> String {
    format!("latency-{:04}", index)
}

fn make_temp_latency_inbound_tag(index: usize) -> String {
    format!("latency-in-{:04}", index)
}

fn temp_xray_bridge_port(index: usize) -> u16 {
    TEMP_XRAY_BRIDGE_PORT_BASE.saturating_add(index as u16)
}

fn temp_proxy_inbound_port(index: usize) -> u16 {
    TEMP_PROXY_INBOUND_PORT_BASE.saturating_add(index as u16)
}

fn is_ip_literal(value: &str) -> bool {
    let trimmed = value.trim().trim_start_matches('[').trim_end_matches(']');
    trimmed.parse::<std::net::IpAddr>().is_ok()
}

fn apply_temp_latency_domain_resolver(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    resolver_tag: &str,
) {
    if obj.contains_key("domain_resolver") {
        return;
    }
    let server = obj
        .get("server")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    if server.trim().is_empty() || is_ip_literal(server) {
        return;
    }

    obj.insert(
        "domain_resolver".to_string(),
        serde_json::json!({
            "server": resolver_tag,
            "strategy": "ipv4_only"
        }),
    );
}

fn temp_dns_domain_resolver(remote_dns: &str, active_node_has_ech: bool) -> Option<&'static str> {
    if active_node_has_ech || dns_server_uses_domain_address(remote_dns) {
        Some("dns-bootstrap")
    } else {
        None
    }
}

fn dns_server_uses_domain_address(remote_dns: &str) -> bool {
    let server = crate::commands::singbox::build_dns_server(remote_dns, "dns-probe", "direct");
    let Some(host) = server.get("server").and_then(|value| value.as_str()) else {
        return false;
    };

    !host.trim().is_empty() && !is_ip_literal(host)
}

pub(crate) async fn cleanup_temp_singbox(state: &AppState) {
    cleanup_temp_singbox_process().await;
    append_temp_latency_logs(state);
    clear_temp_singbox_tag_map().await;
    clear_temp_singbox_proxy_port_map().await;
    *TEMP_SINGBOX_ACTIVE_TESTS.lock().await = 0;
    *TEMP_SINGBOX_OWNER_BATCH_ID.lock().await = None;
    let temp_dir = temp_singbox_dir(state);
    if let Err(err) = remove_temp_singbox_dir(&temp_dir) {
        log::warn!("Failed to clean temp sing-box dir {:?}: {}", temp_dir, err);
    }
    let temp_test_dir = state.data_dir.join("temp_test");
    if temp_test_dir.exists() {
        if let Err(err) = fs::remove_dir_all(&temp_test_dir) {
            log::warn!("Failed to clean temp_test dir {:?}: {}", temp_test_dir, err);
        }
    }
}

#[tauri::command]
pub async fn node_begin_latency_tests(
    state: State<'_, AppState>,
    run_id: u64,
) -> Result<(), String> {
    cleanup_temp_singbox(&state).await;
    begin_latency_test_batch(run_id).await;
    Ok(())
}

#[tauri::command]
pub async fn node_test_latency(
    app: AppHandle,
    state: State<'_, AppState>,
    tag: String,
    run_id: Option<u64>,
) -> Result<NodeLatencyResult, String> {
    if is_latency_test_batch_cancelled(run_id).await {
        return Ok(NodeLatencyResult::local_test_failed());
    }

    let cancel_token = if run_id.is_some() {
        current_latency_test_cancel_token().await
    } else {
        CancellationToken::new()
    };

    let (test_url, timeout_ms) = current_latency_test_settings(&state).await;

    let proxy_state = {
        let proxy_state = state.proxy_state.lock().await;
        (*proxy_state).clone()
    };
    let main_api_port = *state.clash_api_port.lock().await;
    let main_api_ready = check_clash_api_running(main_api_port).await;

    match select_latency_test_backend(&proxy_state, main_api_ready) {
        LatencyTestBackend::Main => {
            let result = test_latency_via_clash_api_cancellable(
                &tag,
                main_api_port,
                &test_url,
                timeout_ms,
                cancel_token.clone(),
            )
            .await;
            let mapped = map_latency_probe_result(result, NodeLatencyStatus::ControllerUnavailable);
            if mapped.status == NodeLatencyStatus::Success {
                return Ok(mapped);
            }
            log::warn!("Main Clash API latency probe did not produce success for '{}', falling back to temp backend", tag);
            let temp_result = test_latency_via_temp_backend(
                &app,
                &state,
                &tag,
                run_id,
                &test_url,
                timeout_ms,
                cancel_token,
                true,
            )
            .await;
            return Ok(temp_result);
        }
        LatencyTestBackend::Temp => {
            let temp_result = test_latency_via_temp_backend(
                &app,
                &state,
                &tag,
                run_id,
                &test_url,
                timeout_ms,
                cancel_token,
                false,
            )
            .await;
            return Ok(temp_result);
        }
    }
}

#[tauri::command]
pub async fn node_test_all(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<std::collections::HashMap<String, i64>, String> {
    let cancel_token = current_latency_test_cancel_token().await;
    let (test_url, timeout_ms) = current_latency_test_settings(&state).await;

    let data = load_profiles_data(&state);
    let profile_id = match data.active_profile_id {
        Some(id) => id,
        None => return Ok(std::collections::HashMap::new()),
    };

    let nodes = load_profile_nodes(&state, &profile_id);

    let proxy_state = {
        let proxy_state = state.proxy_state.lock().await;
        (*proxy_state).clone()
    };
    let main_api_port = *state.clash_api_port.lock().await;
    let main_api_ready = check_clash_api_running(main_api_port).await;

    let mut ports: Vec<u16> = Vec::new();
    let mut temp_used = false;
    let prefer_temp_backend =
        matches!(proxy_state, ProxyState::Connected | ProxyState::Connecting) || !main_api_ready;

    if prefer_temp_backend {
        let temp_ready = start_temp_singbox(&app, &state, &cancel_token, true).await;
        if temp_ready {
            ports.push(TEMP_SINGBOX_PORT);
            temp_used = true;
        }
    } else {
        match select_latency_test_backend(&proxy_state, main_api_ready) {
            LatencyTestBackend::Main => {
                ports.push(main_api_port);
            }
            LatencyTestBackend::Temp => {
                let temp_ready = start_temp_singbox(&app, &state, &cancel_token, false).await;
                if temp_ready {
                    ports.push(TEMP_SINGBOX_PORT);
                    temp_used = true;
                }
            }
        }
    }

    if ports.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let mut results = std::collections::HashMap::new();
    let mut temp_tag_map = if temp_used {
        Some(read_temp_singbox_tag_map().await)
    } else {
        None
    };
    let mut temp_proxy_port_map = if temp_used {
        Some(read_temp_proxy_port_map().await)
    } else {
        None
    };
    // Test in chunks for concurrency
    let chunk_size = 5;
    for chunk in nodes.chunks(chunk_size) {
        if cancel_token.is_cancelled() {
            break;
        }

        let futures: Vec<_> = chunk
            .iter()
            .filter_map(|node| node.tag.clone())
            .map(|tag| {
                let tag_clone = tag.clone();
                let cancel_token = cancel_token.clone();
                let test_url = test_url.clone();
                let lookup_tag = if let Some(tag_map) = temp_tag_map.as_mut() {
                    take_temp_singbox_tag(tag_map, &tag).unwrap_or_else(|| tag.clone())
                } else {
                    tag.clone()
                };
                let proxy_port = temp_proxy_port_map
                    .as_mut()
                    .and_then(|port_map| take_temp_proxy_port(port_map, &tag));
                let ports_clone = ports.clone();
                async move {
                    let mut latency = -1;
                    for p in ports_clone {
                        if cancel_token.is_cancelled() {
                            break;
                        }

                        let clash_result = test_latency_via_clash_api_cancellable(
                            &lookup_tag,
                            p,
                            &test_url,
                            timeout_ms,
                            cancel_token.clone(),
                        )
                        .await;
                        let probe_result =
                            if should_try_proxy_latency_fallback(&clash_result, proxy_port) {
                                match proxy_port {
                                    Some(proxy_port) => {
                                        test_latency_via_http_proxy_cancellable(
                                            proxy_port,
                                            &test_url,
                                            timeout_ms,
                                            cancel_token.clone(),
                                        )
                                        .await
                                    }
                                    None => clash_result,
                                }
                            } else {
                                clash_result
                            };

                        match probe_result {
                            Ok(v) if v > 0 => {
                                latency = v;
                                break;
                            }
                            Ok(_) | Err(LatencyProbeError::Timeout) => {}
                            Err(LatencyProbeError::ProxyFailed) => {
                                log::debug!(
                                    "Proxy latency test failed on port {} for '{}' via '{}'",
                                    p,
                                    tag_clone,
                                    lookup_tag
                                );
                            }
                            Err(LatencyProbeError::Failed) => {
                                log::debug!(
                                    "Latency test failed on port {} for '{}' via '{}'",
                                    p,
                                    tag_clone,
                                    lookup_tag
                                );
                            }
                        }
                    }
                    (tag_clone, latency)
                }
            })
            .collect();

        let chunk_results = futures::future::join_all(futures).await;
        for (tag, latency) in chunk_results {
            results.insert(tag, latency);
        }
    }

    if temp_used {
        cleanup_temp_singbox(&state).await;
    }

    Ok(results)
}

#[tauri::command]
pub async fn node_cancel_latency_tests(
    state: State<'_, AppState>,
    run_id: Option<u64>,
) -> Result<(), String> {
    let should_cleanup_temp = {
        let active_run_id = *ACTIVE_LATENCY_BATCH_ID.lock().await;
        run_id.is_none() || active_run_id == run_id
    };

    mark_latency_test_batch_cancelled(run_id).await;
    let still_safe_to_cleanup = {
        let active_run_id = *ACTIVE_LATENCY_BATCH_ID.lock().await;
        run_id.is_none() || active_run_id.is_none() || active_run_id == run_id
    };
    if should_cleanup_temp && still_safe_to_cleanup {
        cleanup_temp_singbox(&state).await;
    }
    Ok(())
}
