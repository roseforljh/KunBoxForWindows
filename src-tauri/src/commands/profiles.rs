use tauri::{AppHandle, Manager, State};
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use crate::state::AppState;
use crate::types::{AppSettings, NodeLatencyResult, NodeLatencyStatus, Profile, ProfilesData, ProxyState, SingBoxOutbound};

#[cfg(windows)]
#[allow(unused_imports)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

// Temporary sing-box for latency testing
static TEMP_SINGBOX_PROCESS: once_cell::sync::Lazy<Arc<Mutex<Option<tokio::process::Child>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(None)));
static TEMP_XRAY_PROCESSES: once_cell::sync::Lazy<Arc<Mutex<Vec<tokio::process::Child>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(Vec::new())));
static TEMP_SINGBOX_TAG_MAP: once_cell::sync::Lazy<Arc<Mutex<std::collections::HashMap<String, Vec<String>>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(std::collections::HashMap::new())));
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
pub(crate) const ECH_DNS_SERVER_META_KEY: &str = "x_kunbox_ech_dns_server";
const TEMP_SINGBOX_PORT: u16 = 19090;
const TEMP_XRAY_BRIDGE_PORT_BASE: u16 = 19180;

#[derive(Debug, PartialEq, Eq)]
enum LatencyTestBackend {
    Main,
    Temp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LatencyProbeError {
    Timeout,
    Failed,
}

fn select_latency_test_backend(proxy_state: &ProxyState, main_api_ready: bool) -> LatencyTestBackend {
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
        Err(LatencyProbeError::Failed) => match failure_status {
            NodeLatencyStatus::ControllerUnavailable => NodeLatencyResult::controller_unavailable(),
            NodeLatencyStatus::LocalTestFailed => NodeLatencyResult::local_test_failed(),
            NodeLatencyStatus::Timeout => NodeLatencyResult::timeout(),
            NodeLatencyStatus::Success => NodeLatencyResult::local_test_failed(),
        },
    }
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
    append_latency_diagnostic(state, &format!("latency test requested for tag='{}', test_url='{}', timeout_ms={}", tag, test_url, timeout_ms));
    if is_latency_test_batch_cancelled(run_id).await {
        return NodeLatencyResult::local_test_failed();
    }

    if !acquire_temp_singbox_test_slot(run_id).await {
        log::debug!("Temp sing-box slot owner changed before testing '{}', skipping stale request", tag);
        return NodeLatencyResult::local_test_failed();
    }

    let started = start_temp_singbox(app, state, &cancel_token, allow_main_process_alive).await;
    if !started {
        append_latency_diagnostic(state, &format!("temp sing-box failed to start for tag='{}'", tag));
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
            append_latency_diagnostic(state, &format!("temp tag mapping missing for tag='{}'", tag));
            log::warn!("Temp Clash API tag mapping missing for '{}'", tag);
            release_temp_singbox_test_slot(state, run_id).await;
            return NodeLatencyResult::local_test_failed();
        }
    };

    let result = test_latency_via_clash_api_cancellable(
        &temp_tag,
        TEMP_SINGBOX_PORT,
        test_url,
        timeout_ms,
        cancel_token,
    ).await;
    append_latency_diagnostic(state, &format!("latency raw result for tag='{}', temp_tag='{}': {:?}", tag, temp_tag, result));
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

    append_latency_diagnostic(state, &format!("collecting temp latency logs from {:?}", temp_dir));
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
                &format!("===== {} =====\n{}\n===== end {} =====", name, content, name),
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
                    &format!("===== {} =====\n{}\n===== end {} =====", file_name, content, file_name),
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

async fn take_temp_singbox_tag_for_batch(run_id: Option<u64>, original_tag: &str) -> Option<String> {
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
        .and_then(|aliases| aliases.first().cloned())
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
        return tag_map.remove(original_tag).and_then(|mut aliases| aliases.drain(..1).next());
    }

    tag_map.get_mut(original_tag).map(|aliases| aliases.remove(0))
}

fn make_temp_latency_tag(index: usize) -> String {
    format!("latency-{:04}", index)
}

fn temp_xray_bridge_port(index: usize) -> u16 {
    TEMP_XRAY_BRIDGE_PORT_BASE.saturating_add(index as u16)
}

pub(crate) async fn cleanup_temp_singbox(state: &AppState) {
    cleanup_temp_singbox_process().await;
    append_temp_latency_logs(state);
    clear_temp_singbox_tag_map().await;
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

fn load_profiles_data(state: &AppState) -> ProfilesData {
    let file = state.profiles_file();
    if file.exists() {
        if let Ok(content) = fs::read_to_string(&file) {
            if let Ok(data) = serde_json::from_str(&content) {
                return data;
            }
            log::warn!("Failed to parse profiles file {:?}", file);
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

fn is_valid_profile_id(profile_id: &str) -> bool {
    !profile_id.is_empty()
        && profile_id.len() <= 64
        && profile_id.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn profile_nodes_path(state: &AppState, profile_id: &str) -> Result<std::path::PathBuf, String> {
    if !is_valid_profile_id(profile_id) {
        return Err("Invalid profile id".to_string());
    }

    Ok(state.configs_dir().join(format!("{}.json", profile_id)))
}

fn load_profile_nodes(state: &AppState, profile_id: &str) -> Vec<SingBoxOutbound> {
    let file = match profile_nodes_path(state, profile_id) {
        Ok(path) => path,
        Err(_) => return Vec::new(),
    };
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
    let file = match profile_nodes_path(state, profile_id) {
        Ok(path) => path,
        Err(_) => return Vec::new(),
    };
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
    let file = profile_nodes_path(state, profile_id)?;
    let content = serde_json::to_string_pretty(nodes).map_err(|e| e.to_string())?;
    fs::write(file, content).map_err(|e| e.to_string())?;
    Ok(())
}

fn reconcile_profile_node_selection(data: &mut ProfilesData, profile_id: &str, nodes: &[SingBoxOutbound]) {
    let node_exists = |tag: &str| nodes.iter().any(|node| node.tag.as_deref() == Some(tag));
    let fallback = nodes.first().and_then(|node| node.tag.clone());

    let next_selection = data
        .node_selections
        .get(profile_id)
        .cloned()
        .filter(|tag| node_exists(tag))
        .or_else(|| {
            (data.active_profile_id.as_deref() == Some(profile_id))
                .then(|| data.active_node_tag.clone())
                .flatten()
                .filter(|tag| node_exists(tag))
        })
        .or(fallback);

    if let Some(tag) = &next_selection {
        data.node_selections.insert(profile_id.to_string(), tag.clone());
    } else {
        data.node_selections.remove(profile_id);
    }

    if data.active_profile_id.as_deref() == Some(profile_id) {
        data.active_node_tag = next_selection;
    }
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
    reconcile_profile_node_selection(&mut data, &id, &nodes);
    save_profiles_data(&state, &data)?;

    let profile = data.profiles[profile_idx].clone();
    *state.profiles_data.lock().await = data;
    Ok(profile)
}

#[tauri::command]
pub async fn profile_delete(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut data = load_profiles_data(&state);
    data.profiles.retain(|p| p.id != id);

    let config_file = profile_nodes_path(&state, &id)?;
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
pub async fn profile_get_active(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let data = load_profiles_data(&state);
    Ok(data.active_profile_id)
}

#[tauri::command]
pub async fn profile_set_active(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let mut data = load_profiles_data(&state);
    if !data.profiles.iter().any(|p| p.id == id) {
        return Err("Profile not found".to_string());
    }

    // Save current profile's node selection before switching
    if let (Some(old_id), Some(old_tag)) = (&data.active_profile_id, &data.active_node_tag) {
        data.node_selections.insert(old_id.clone(), old_tag.clone());
    }

    data.active_profile_id = Some(id.clone());
    let nodes = load_profile_nodes(&state, &id);

    // Restore saved node selection if the node still exists, otherwise fallback to first node
    data.active_node_tag = data.node_selections.get(&id)
        .and_then(|saved_tag| {
            nodes.iter().any(|n| n.tag.as_deref() == Some(saved_tag.as_str()))
                .then(|| saved_tag.clone())
        })
        .or_else(|| nodes.first().and_then(|n| n.tag.clone()));

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
        if !dns {
            data.profiles[profile_idx].dns_server = None;
        }
    }
    if let Some(server) = dns_server {
        let server = server.trim().to_string();
        data.profiles[profile_idx].dns_server = if server.is_empty() { None } else { Some(server) };
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
pub async fn node_get_active(state: State<'_, AppState>) -> Result<Option<String>, String> {
    let data = load_profiles_data(&state);
    Ok(data.active_node_tag)
}

#[tauri::command]
pub async fn node_set_active(state: State<'_, AppState>, tag: String) -> Result<(), String> {
    let mut data = load_profiles_data(&state);
    data.active_node_tag = Some(tag.clone());
    // Also save to per-profile node selections
    if let Some(profile_id) = &data.active_profile_id {
        data.node_selections.insert(profile_id.clone(), tag);
    }
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

    reconcile_profile_node_selection(&mut data, &profile_id, &nodes);

    save_profiles_data(&state, &data)?;
    *state.profiles_data.lock().await = data;
    Ok(())
}

/// Node with source profile information
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NodeWithProfile {
    #[serde(flatten)]
    pub node: SingBoxOutbound,
    #[serde(rename = "sourceProfileId")]
    pub source_profile_id: String,
    #[serde(rename = "sourceProfileName")]
    pub source_profile_name: String,
}

#[tauri::command]
pub async fn node_list_all(state: State<'_, AppState>) -> Result<Vec<NodeWithProfile>, String> {
    let data = load_profiles_data(&state);
    let mut all_nodes = Vec::new();

    for profile in &data.profiles {
        if !profile.enabled {
            continue;
        }
        let nodes = load_profile_nodes(&state, &profile.id);
        for node in nodes {
            all_nodes.push(NodeWithProfile {
                node,
                source_profile_id: profile.id.clone(),
                source_profile_name: profile.name.clone(),
            });
        }
    }

    Ok(all_nodes)
}

fn normalize_duplicate_node_tags(nodes: Vec<SingBoxOutbound>) -> Vec<SingBoxOutbound> {
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    nodes
        .into_iter()
        .map(|mut node| {
            let original_tag = node.tag.clone().unwrap_or_else(|| "Node".to_string());
            let counter = seen.entry(original_tag.clone()).or_insert(0);
            *counter += 1;
            node.tag = Some(if *counter == 1 {
                original_tag
            } else {
                format!("{} #{}", original_tag, *counter)
            });
            node
        })
        .collect()
}

pub(crate) async fn fetch_subscription(url: &str) -> Result<Vec<SingBoxOutbound>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let response = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("订阅请求失败: HTTP {}", response.status()));
    }

    if let Some(content_length) = response.content_length() {
        if content_length > 10 * 1024 * 1024 {
            return Err("订阅内容过大，已拒绝加载".to_string());
        }
    }

    let content = response.text().await.map_err(|e| e.to_string())?;
    if content.len() > 10 * 1024 * 1024 {
        return Err("订阅内容过大，已拒绝加载".to_string());
    }

    parse_subscription_content(&content).map(normalize_duplicate_node_tags)
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
    if let Some(decoded) = decode_base64_compat(content.trim()) {
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

fn decode_base64_compat(input: &str) -> Option<Vec<u8>> {
    let input = input.trim();
    for engine in [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::STANDARD_NO_PAD,
        &base64::engine::general_purpose::URL_SAFE,
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(decoded) = base64::Engine::decode(engine, input) {
            return Some(decoded);
        }
    }
    None
}

fn parse_u16_port(value: u64) -> Option<u16> {
    u16::try_from(value).ok()
}

fn parse_port_value(value: &serde_json::Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(parse_u16_port)
        .or_else(|| value.as_str().and_then(|s| s.parse::<u16>().ok()))
}

fn parse_host_port(host_port: &str) -> Option<(String, u16)> {
    let host_port = host_port
        .split_once('?')
        .map(|(value, _)| value)
        .unwrap_or(host_port);
    let url = url::Url::parse(&format!("tcp://{}", host_port)).ok()?;
    Some((url.host_str()?.to_string(), url.port()?))
}

fn parse_bool_param(value: &str) -> bool {
    value == "1" || value.eq_ignore_ascii_case("true") || value.eq_ignore_ascii_case("yes")
}

fn parse_clash_proxies(proxies: &[serde_json::Value]) -> Result<Vec<SingBoxOutbound>, String> {
    let nodes: Vec<SingBoxOutbound> = proxies
        .iter()
        .filter_map(|p| {
            let tag = p.get("name")?.as_str()?.to_string();
            let proxy_type = map_clash_type(p.get("type")?.as_str()?);
            if !crate::commands::singbox::is_proxy_type(&proxy_type) {
                return None;
            }
            let server = p.get("server")?.as_str()?.to_string();
            let port = parse_u16_port(p.get("port")?.as_u64()?)?;

            let mut extra = serde_json::Map::new();

            // Basic fields
            if let Some(username) = p.get("username").and_then(|v| v.as_str()) {
                extra.insert("username".to_string(), serde_json::Value::String(username.to_string()));
            }
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
                if let Some(encryption) = p
                    .get("extra")
                    .and_then(|value| value.get("encryption"))
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                {
                    extra.insert("encryption".to_string(), serde_json::Value::String(encryption.to_string()));
                }
            }

            // TLS configuration
            let network = p.get("network").and_then(|v| v.as_str()).unwrap_or("tcp");
            let tls_enabled = p.get("tls").and_then(|v| v.as_bool()).unwrap_or(false);
            let servername = p.get("servername").or(p.get("sni")).and_then(|v| v.as_str());
            let skip_cert = p.get("skip-cert-verify").and_then(|v| v.as_bool()).unwrap_or(false);

            if proxy_type == "naive" {
                extra.insert("tls".to_string(), serde_json::json!({
                    "enabled": true,
                    "server_name": servername.unwrap_or(&server),
                    "insecure": skip_cert
                }));
            } else if tls_enabled || network == "ws" || network == "grpc" || network == "h2" || proxy_type == "hysteria2" || proxy_type == "hysteria" || proxy_type == "tuic" {
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
                "xhttp" => {
                    let xhttp_opts = p.get("xhttp-opts").and_then(|v| v.as_object());
                    let mut transport = serde_json::Map::new();
                    transport.insert("type".to_string(), serde_json::Value::String("xhttp".to_string()));

                    if let Some(path) = xhttp_opts.and_then(|o| o.get("path")).and_then(|v| v.as_str()) {
                        transport.insert("path".to_string(), serde_json::Value::String(path.to_string()));
                    }
                    if let Some(mode) = xhttp_opts.and_then(|o| o.get("mode")).and_then(|v| v.as_str()) {
                        transport.insert("mode".to_string(), serde_json::Value::String(mode.to_string()));
                    }
                    if let Some(extra_field) = xhttp_opts.and_then(|o| o.get("extra")) {
                        if let Some(extra_obj) = extra_field.as_object() {
                            let mut transport_extra = extra_obj.clone();
                            transport_extra.remove("encryption");
                            if !transport_extra.is_empty() {
                                transport.insert("extra".to_string(), serde_json::Value::Object(transport_extra));
                            }
                        } else {
                            transport.insert("extra".to_string(), extra_field.clone());
                        }
                    }
                    if let Some(headers) = xhttp_opts.and_then(|o| o.get("headers")).and_then(|v| v.as_object()) {
                        transport.insert("headers".to_string(), serde_json::Value::Object(headers.clone()));
                    }

                    extra.insert("transport".to_string(), serde_json::Value::Object(transport));
                }
                _ => {}
            }

            if proxy_type == "naive" {
                if let Some(udp_over_tcp) = p.get("udp-over-tcp").and_then(|v| v.as_bool()) {
                    extra.insert("udp_over_tcp".to_string(), serde_json::Value::Bool(udp_over_tcp));
                }
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
            if !crate::commands::singbox::is_proxy_type(outbound_type) {
                return None;
            }
            serde_json::from_value(o.clone()).ok()
        })
        .collect();
    Ok(nodes)
}

fn parse_node_link(link: &str) -> Option<SingBoxOutbound> {
    if link.starts_with("ss://") {
        parse_ss_link(link)
    } else if link.starts_with("vless://") {
        parse_vless_link(link)
    } else if link.starts_with("vmess://") {
        parse_vmess_link(link)
    } else if link.starts_with("trojan://") {
        parse_trojan_link(link)
    } else if link.starts_with("hysteria2://") || link.starts_with("hy2://") {
        parse_hysteria2_link(link)
    } else if link.starts_with("hysteria://") {
        parse_hysteria_link(link)
    } else if link.starts_with("tuic://") {
        parse_tuic_link(link)
    } else if link.starts_with("naive+") {
        parse_naive_link(link)
    } else {
        None
    }
}

fn parse_ss_link(link: &str) -> Option<SingBoxOutbound> {
    let rest = link.strip_prefix("ss://")?;
    let (encoded, tag) = rest.split_once('#').unwrap_or((rest, "SS"));
    let tag = urlencoding::decode(tag).ok()?.to_string();

    let (userinfo, host_port) = if let Some((userinfo, host_port)) = encoded.split_once('@') {
        (userinfo.to_string(), host_port.to_string())
    } else {
        let decoded = decode_base64_compat(encoded)?;
        let decoded_str = String::from_utf8(decoded).ok()?;
        let (userinfo, host_port) = decoded_str.split_once('@')?;
        (userinfo.to_string(), host_port.to_string())
    };

    let decoded_plain_userinfo = urlencoding::decode(&userinfo).ok()?.to_string();
    let decoded_userinfo = if decoded_plain_userinfo.contains(':') {
        decoded_plain_userinfo
    } else {
        let decoded = decode_base64_compat(&userinfo)?;
        String::from_utf8(decoded).ok()?
    };
    let (method, password) = decoded_userinfo.split_once(':')?;
    let (server, port) = parse_host_port(&host_port)?;

    let mut extra = std::collections::HashMap::new();
    extra.insert("method".to_string(), serde_json::Value::String(method.to_string()));
    extra.insert("password".to_string(), serde_json::Value::String(password.to_string()));

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("shadowsocks".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}

fn extract_ech_name_and_dns_server(ech: &str) -> Option<(&str, &str)> {
    let (_, resolver) = ech.split_once('+')?;
    let (name, _) = ech.split_once('+')?;
    let name = name.trim();
    let resolver = resolver.trim();
    if name.is_empty() {
        return None;
    }
    let url = url::Url::parse(resolver).ok()?;
    matches!(url.scheme(), "https" | "h3").then_some((name, resolver))
}

fn extract_ech_public_name(ech: &str) -> Option<&str> {
    extract_ech_name_and_dns_server(ech).map(|(name, _)| name)
}

fn extract_ech_dns_server(ech: &str) -> Option<&str> {
    extract_ech_name_and_dns_server(ech).map(|(_, resolver)| resolver)
}

fn parse_ech_config_lines(ech: &str) -> Option<Vec<serde_json::Value>> {
    let trimmed = ech.trim();
    if !(trimmed.contains("-----BEGIN") && trimmed.contains("-----END")) {
        return None;
    }

    let lines: Vec<serde_json::Value> = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::Value::String(line.to_string()))
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(lines)
    }
}

fn parse_vless_link(link: &str) -> Option<SingBoxOutbound> {
    let rest = link.strip_prefix("vless://")?;
    let (main_part, tag) = rest.split_once('#').unwrap_or((rest, "VLESS"));
    let tag = urlencoding::decode(tag).ok()?.to_string();

    let (user_host, query) = main_part.split_once('?').unwrap_or((main_part, ""));
    let (uuid, host_port) = user_host.split_once('@')?;

    let (server, port) = parse_host_port(host_port)?;

    let params: std::collections::HashMap<String, String> = query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_string(), urlencoding::decode(v).unwrap_or_default().to_string()))
        .collect();

    let mut extra = std::collections::HashMap::new();
    extra.insert("uuid".to_string(), serde_json::Value::String(uuid.to_string()));
    extra.insert("packet_encoding".to_string(), serde_json::Value::String("xudp".to_string()));

    let ech = params.get("ech").map(|value| value.trim()).filter(|value| !value.is_empty());
    let ech_public_name = ech.and_then(extract_ech_public_name);
    let ech_dns_server = ech.and_then(extract_ech_dns_server);

    if let Some(flow) = params.get("flow") {
        if !flow.is_empty() {
            extra.insert("flow".to_string(), serde_json::Value::String(flow.clone()));
        }
    }
    if let Some(encryption) = params
        .get("encryption")
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("none"))
    {
        extra.insert("encryption".to_string(), serde_json::Value::String(encryption.clone()));
    }

    // TLS configuration
    let security_param = params.get("security").map(|s| s.as_str()).unwrap_or("");
    let has_tls_params = params.contains_key("sni")
        || params.contains_key("servername")
        || params.contains_key("alpn")
        || params.contains_key("fp")
        || params.contains_key("insecure")
        || ech.is_some();
    let security = if security_param.is_empty() && has_tls_params {
        "tls"
    } else {
        security_param
    };
    if security == "tls" || security == "reality" {
        let mut tls = serde_json::Map::new();
        tls.insert("enabled".to_string(), serde_json::Value::Bool(true));

        if let Some(sni) = params.get("sni").or(params.get("servername")) {
            tls.insert("server_name".to_string(), serde_json::Value::String(sni.clone()));
        } else if let Some(public_name) = ech_public_name {
            tls.insert("server_name".to_string(), serde_json::Value::String(public_name.to_string()));
        } else {
            tls.insert("server_name".to_string(), serde_json::Value::String(server.clone()));
        }

        if let Some(insecure) = params.get("insecure") {
            let allow_insecure = insecure == "1" || insecure.eq_ignore_ascii_case("true");
            tls.insert("insecure".to_string(), serde_json::Value::Bool(allow_insecure));
        }

        if let Some(alpn) = params.get("alpn") {
            let alpn_arr: Vec<serde_json::Value> = alpn.split(',')
                .map(|s| serde_json::Value::String(s.to_string()))
                .collect();
            tls.insert("alpn".to_string(), serde_json::Value::Array(alpn_arr));
        }

        if let Some(fp) = params.get("fp") {
            if !fp.is_empty() {
                tls.insert("utls".to_string(), serde_json::json!({
                    "enabled": true,
                    "fingerprint": fp
                }));
            }
        }

        if let Some(ech_value) = ech {
            let mut ech_obj = serde_json::Map::new();
            ech_obj.insert("enabled".to_string(), serde_json::Value::Bool(true));

            if let Some(config_lines) = parse_ech_config_lines(ech_value) {
                ech_obj.insert("config".to_string(), serde_json::Value::Array(config_lines));
            } else if let Some(public_name) = extract_ech_public_name(ech_value) {
                ech_obj.insert(
                    "query_server_name".to_string(),
                    serde_json::Value::String(public_name.to_string()),
                );
            }

            tls.insert("ech".to_string(), serde_json::Value::Object(ech_obj));
        }

        if security == "reality" {
            let mut reality = serde_json::Map::new();
            reality.insert("enabled".to_string(), serde_json::Value::Bool(true));
            if let Some(pbk) = params.get("pbk") {
                reality.insert("public_key".to_string(), serde_json::Value::String(pbk.clone()));
            }
            if let Some(sid) = params.get("sid") {
                reality.insert("short_id".to_string(), serde_json::Value::String(sid.clone()));
            }
            tls.insert("reality".to_string(), serde_json::Value::Object(reality));
        }

        extra.insert("tls".to_string(), serde_json::Value::Object(tls));
    }

    if let Some(dns_server) = ech_dns_server {
        extra.insert(
            ECH_DNS_SERVER_META_KEY.to_string(),
            serde_json::Value::String(dns_server.to_string()),
        );
    }

    // Transport configuration
    let transport_type = params.get("type").map(|s| s.as_str()).unwrap_or("tcp");
    if transport_type != "tcp" {
        let mut transport = serde_json::Map::new();
        transport.insert("type".to_string(), serde_json::Value::String(transport_type.to_string()));

        match transport_type {
            "ws" => {
                if let Some(path) = params.get("path") {
                    transport.insert("path".to_string(), serde_json::Value::String(path.clone()));
                }
                if let Some(host) = params.get("host") {
                    transport.insert("headers".to_string(), serde_json::json!({ "Host": host }));
                }
            }
            "grpc" => {
                if let Some(sn) = params.get("serviceName") {
                    transport.insert("service_name".to_string(), serde_json::Value::String(sn.clone()));
                }
            }
            "http" | "h2" => {
                transport.insert("type".to_string(), serde_json::Value::String("http".to_string()));
                if let Some(path) = params.get("path") {
                    transport.insert("path".to_string(), serde_json::Value::String(path.clone()));
                }
                if let Some(host) = params.get("host") {
                    transport.insert("host".to_string(), serde_json::json!([host]));
                }
            }
            "xhttp" => {
                if let Some(path) = params.get("path") {
                    transport.insert("path".to_string(), serde_json::Value::String(path.clone()));
                }
                if let Some(mode) = params.get("mode") {
                    transport.insert("mode".to_string(), serde_json::Value::String(mode.clone()));
                }
                if let Some(host) = params.get("host") {
                    transport.insert("headers".to_string(), serde_json::json!({ "Host": host }));
                }
                if let Some(extra_str) = params.get("extra") {
                    if let Ok(extra_json) = serde_json::from_str::<serde_json::Value>(extra_str) {
                        if let Some(encryption) = extra_json
                            .get("encryption")
                            .and_then(|value| value.as_str())
                            .filter(|value| !value.is_empty())
                        {
                            extra.insert("encryption".to_string(), serde_json::Value::String(encryption.to_string()));
                        }

                        if let Some(extra_obj) = extra_json.as_object() {
                            let mut transport_extra = extra_obj.clone();
                            transport_extra.remove("encryption");
                            if !transport_extra.is_empty() {
                                transport.insert("extra".to_string(), serde_json::Value::Object(transport_extra));
                            }
                        } else {
                            transport.insert("extra".to_string(), extra_json);
                        }
                    } else {
                        transport.insert("extra".to_string(), serde_json::Value::String(extra_str.clone()));
                    }
                }
            }
            _ => {}
        }

        extra.insert("transport".to_string(), serde_json::Value::Object(transport));
    }

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("vless".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}

fn parse_vmess_link(link: &str) -> Option<SingBoxOutbound> {
    let rest = link.strip_prefix("vmess://")?;
    let decoded = decode_base64_compat(rest.trim())?;
    let decoded_str = String::from_utf8(decoded).ok()?;
    let json: serde_json::Value = serde_json::from_str(&decoded_str).ok()?;

    let tag = json.get("ps").and_then(|v| v.as_str()).unwrap_or("VMess").to_string();
    let server = json.get("add").and_then(|v| v.as_str())?.to_string();
    let port = json.get("port").and_then(parse_port_value)?;
    let uuid = json.get("id").and_then(|v| v.as_str())?.to_string();

    let mut extra = std::collections::HashMap::new();
    extra.insert("uuid".to_string(), serde_json::Value::String(uuid));
    extra.insert("security".to_string(), serde_json::Value::String(
        json.get("scy").or(json.get("cipher")).and_then(|v| v.as_str()).unwrap_or("auto").to_string()
    ));

    if let Some(aid) = json.get("aid").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))) {
        extra.insert("alter_id".to_string(), serde_json::Value::Number(aid.into()));
    }

    // TLS
    let tls = json.get("tls").and_then(|v| v.as_str()).unwrap_or("");
    if tls == "tls" {
        let mut tls_obj = serde_json::Map::new();
        tls_obj.insert("enabled".to_string(), serde_json::Value::Bool(true));
        if let Some(sni) = json.get("sni").and_then(|v| v.as_str()) {
            tls_obj.insert("server_name".to_string(), serde_json::Value::String(sni.to_string()));
        } else {
            tls_obj.insert("server_name".to_string(), serde_json::Value::String(server.clone()));
        }
        extra.insert("tls".to_string(), serde_json::Value::Object(tls_obj));
    }

    // Transport
    let net = json.get("net").and_then(|v| v.as_str()).unwrap_or("tcp");
    if net != "tcp" {
        let mut transport = serde_json::Map::new();
        match net {
            "ws" => {
                transport.insert("type".to_string(), serde_json::Value::String("ws".to_string()));
                if let Some(path) = json.get("path").and_then(|v| v.as_str()) {
                    transport.insert("path".to_string(), serde_json::Value::String(path.to_string()));
                }
                if let Some(host) = json.get("host").and_then(|v| v.as_str()) {
                    transport.insert("headers".to_string(), serde_json::json!({ "Host": host }));
                }
            }
            "grpc" => {
                transport.insert("type".to_string(), serde_json::Value::String("grpc".to_string()));
                if let Some(path) = json.get("path").and_then(|v| v.as_str()) {
                    transport.insert("service_name".to_string(), serde_json::Value::String(path.to_string()));
                }
            }
            "h2" => {
                transport.insert("type".to_string(), serde_json::Value::String("http".to_string()));
                if let Some(path) = json.get("path").and_then(|v| v.as_str()) {
                    transport.insert("path".to_string(), serde_json::Value::String(path.to_string()));
                }
            }
            _ => {
                transport.insert("type".to_string(), serde_json::Value::String(net.to_string()));
            }
        }
        extra.insert("transport".to_string(), serde_json::Value::Object(transport));
    }

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("vmess".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}

fn parse_trojan_link(link: &str) -> Option<SingBoxOutbound> {
    let rest = link.strip_prefix("trojan://")?;
    let (main_part, tag) = rest.split_once('#').unwrap_or((rest, "Trojan"));
    let tag = urlencoding::decode(tag).ok()?.to_string();

    let (password_host, query) = main_part.split_once('?').unwrap_or((main_part, ""));
    let (password, host_port) = password_host.split_once('@')?;
    let password = urlencoding::decode(password).ok()?.to_string();

    let (server, port) = parse_host_port(host_port)?;

    let params: std::collections::HashMap<String, String> = query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_string(), urlencoding::decode(v).unwrap_or_default().to_string()))
        .collect();

    let mut extra = std::collections::HashMap::new();
    extra.insert("password".to_string(), serde_json::Value::String(password));

    // TLS (Trojan always uses TLS)
    let mut tls = serde_json::Map::new();
    tls.insert("enabled".to_string(), serde_json::Value::Bool(true));
    if let Some(sni) = params.get("sni") {
        tls.insert("server_name".to_string(), serde_json::Value::String(sni.clone()));
    } else {
        tls.insert("server_name".to_string(), serde_json::Value::String(server.clone()));
    }
    if params.get("allowInsecure").map(|s| s == "1" || s.eq_ignore_ascii_case("true")).unwrap_or(false) {
        tls.insert("insecure".to_string(), serde_json::Value::Bool(true));
    }
    extra.insert("tls".to_string(), serde_json::Value::Object(tls));

    // Transport
    let transport_type = params.get("type").map(|s| s.as_str()).unwrap_or("tcp");
    if transport_type != "tcp" {
        let mut transport = serde_json::Map::new();
        transport.insert("type".to_string(), serde_json::Value::String(transport_type.to_string()));

        if transport_type == "ws" {
            if let Some(path) = params.get("path") {
                transport.insert("path".to_string(), serde_json::Value::String(path.clone()));
            }
            if let Some(host) = params.get("host") {
                transport.insert("headers".to_string(), serde_json::json!({ "Host": host }));
            }
        } else if transport_type == "grpc" {
            if let Some(sn) = params.get("serviceName") {
                transport.insert("service_name".to_string(), serde_json::Value::String(sn.clone()));
            }
        }

        extra.insert("transport".to_string(), serde_json::Value::Object(transport));
    }

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("trojan".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}

fn parse_hysteria2_link(link: &str) -> Option<SingBoxOutbound> {
    let rest = link.strip_prefix("hysteria2://").or_else(|| link.strip_prefix("hy2://"))?;
    let (main_part, tag) = rest.split_once('#').unwrap_or((rest, "Hysteria2"));
    let tag = urlencoding::decode(tag).ok()?.to_string();

    let (password_host, query) = main_part.split_once('?').unwrap_or((main_part, ""));
    let (password, host_port) = password_host.split_once('@')?;
    let password = urlencoding::decode(password).ok()?.to_string();

    let (server, port) = parse_host_port(host_port)?;

    let params: std::collections::HashMap<String, String> = query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_string(), urlencoding::decode(v).unwrap_or_default().to_string()))
        .collect();

    let mut extra = std::collections::HashMap::new();
    extra.insert("password".to_string(), serde_json::Value::String(password));

    // TLS (Hysteria2 always uses TLS)
    let mut tls = serde_json::Map::new();
    tls.insert("enabled".to_string(), serde_json::Value::Bool(true));
    if let Some(sni) = params.get("sni") {
        tls.insert("server_name".to_string(), serde_json::Value::String(sni.clone()));
    } else {
        tls.insert("server_name".to_string(), serde_json::Value::String(server.clone()));
    }
    if params.get("insecure").map(|s| s == "1" || s.eq_ignore_ascii_case("true")).unwrap_or(false) {
        tls.insert("insecure".to_string(), serde_json::Value::Bool(true));
    }
    if let Some(alpn) = params.get("alpn") {
        let alpn_arr: Vec<serde_json::Value> = alpn.split(',')
            .map(|s| serde_json::Value::String(s.to_string()))
            .collect();
        tls.insert("alpn".to_string(), serde_json::Value::Array(alpn_arr));
    }
    extra.insert("tls".to_string(), serde_json::Value::Object(tls));

    // Obfs
    if let Some(obfs_type) = params.get("obfs") {
        let mut obfs = serde_json::Map::new();
        obfs.insert("type".to_string(), serde_json::Value::String(obfs_type.clone()));
        if let Some(obfs_password) = params.get("obfs-password") {
            obfs.insert("password".to_string(), serde_json::Value::String(obfs_password.clone()));
        }
        extra.insert("obfs".to_string(), serde_json::Value::Object(obfs));
    }

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("hysteria2".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}

fn parse_hysteria_link(link: &str) -> Option<SingBoxOutbound> {
    let rest = link.strip_prefix("hysteria://")?;
    let (main_part, tag) = rest.split_once('#').unwrap_or((rest, "Hysteria"));
    let tag = urlencoding::decode(tag).ok()?.to_string();

    let (host_port, query) = main_part.split_once('?').unwrap_or((main_part, ""));
    let (server, port) = parse_host_port(host_port)?;

    let params: std::collections::HashMap<String, String> = query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.to_string(), urlencoding::decode(v).unwrap_or_default().to_string()))
        .collect();

    let mut extra = std::collections::HashMap::new();

    if let Some(auth) = params.get("auth") {
        extra.insert("auth_str".to_string(), serde_json::Value::String(auth.clone()));
    }

    // TLS
    let mut tls = serde_json::Map::new();
    tls.insert("enabled".to_string(), serde_json::Value::Bool(true));
    if let Some(sni) = params.get("sni").or(params.get("peer")) {
        tls.insert("server_name".to_string(), serde_json::Value::String(sni.clone()));
    } else {
        tls.insert("server_name".to_string(), serde_json::Value::String(server.clone()));
    }
    if params.get("insecure").map(|s| s == "1" || s.eq_ignore_ascii_case("true")).unwrap_or(false) {
        tls.insert("insecure".to_string(), serde_json::Value::Bool(true));
    }
    if let Some(alpn) = params.get("alpn") {
        let alpn_arr: Vec<serde_json::Value> = alpn.split(',')
            .map(|s| serde_json::Value::String(s.to_string()))
            .collect();
        tls.insert("alpn".to_string(), serde_json::Value::Array(alpn_arr));
    }
    extra.insert("tls".to_string(), serde_json::Value::Object(tls));

    // Up/Down bandwidth
    if let Some(up) = params.get("upmbps") {
        extra.insert("up_mbps".to_string(), serde_json::Value::Number(up.parse::<i64>().unwrap_or(100).into()));
    }
    if let Some(down) = params.get("downmbps") {
        extra.insert("down_mbps".to_string(), serde_json::Value::Number(down.parse::<i64>().unwrap_or(100).into()));
    }

    // Obfs
    if let Some(obfs_type) = params.get("obfs") {
        extra.insert("obfs".to_string(), serde_json::Value::String(obfs_type.clone()));
    }

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("hysteria".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}

fn parse_tuic_link(link: &str) -> Option<SingBoxOutbound> {
    let url = url::Url::parse(link).ok()?;
    let tag = url
        .fragment()
        .and_then(|value| urlencoding::decode(value).ok().map(|value| value.to_string()))
        .unwrap_or_else(|| "TUIC".to_string());
    let server = url.host_str()?.to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    let uuid = urlencoding::decode(url.username()).ok()?.to_string();
    let password = url
        .password()
        .and_then(|value| urlencoding::decode(value).ok().map(|value| value.to_string()))
        .unwrap_or_default();

    if uuid.is_empty() || password.is_empty() {
        return None;
    }

    let params: std::collections::HashMap<String, String> = url
        .query_pairs()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();

    let mut extra = std::collections::HashMap::new();
    extra.insert("uuid".to_string(), serde_json::Value::String(uuid));
    extra.insert("password".to_string(), serde_json::Value::String(password));

    if let Some(value) = params
        .get("congestion_control")
        .or_else(|| params.get("congestionControl"))
        .filter(|value| !value.is_empty())
    {
        extra.insert("congestion_control".to_string(), serde_json::Value::String(value.clone()));
    }
    if let Some(value) = params
        .get("udp_relay_mode")
        .or_else(|| params.get("udpRelayMode"))
        .filter(|value| !value.is_empty())
    {
        extra.insert("udp_relay_mode".to_string(), serde_json::Value::String(value.clone()));
    }
    if let Some(value) = params
        .get("udp_over_stream")
        .or_else(|| params.get("udpOverStream"))
    {
        extra.insert("udp_over_stream".to_string(), serde_json::Value::Bool(parse_bool_param(value)));
    }
    if let Some(value) = params
        .get("zero_rtt_handshake")
        .or_else(|| params.get("zeroRttHandshake"))
    {
        extra.insert("zero_rtt_handshake".to_string(), serde_json::Value::Bool(parse_bool_param(value)));
    }

    let mut tls = serde_json::Map::new();
    tls.insert("enabled".to_string(), serde_json::Value::Bool(true));
    tls.insert(
        "server_name".to_string(),
        serde_json::Value::String(
            params
                .get("sni")
                .or_else(|| params.get("servername"))
                .cloned()
                .unwrap_or_else(|| server.clone()),
        ),
    );
    if let Some(value) = params
        .get("allow_insecure")
        .or_else(|| params.get("allowInsecure"))
        .or_else(|| params.get("insecure"))
    {
        tls.insert("insecure".to_string(), serde_json::Value::Bool(parse_bool_param(value)));
    }
    if let Some(value) = params.get("alpn").filter(|value| !value.is_empty()) {
        tls.insert(
            "alpn".to_string(),
            serde_json::Value::Array(
                value
                    .split(',')
                    .filter(|item| !item.is_empty())
                    .map(|item| serde_json::Value::String(item.to_string()))
                    .collect(),
            ),
        );
    }
    extra.insert("tls".to_string(), serde_json::Value::Object(tls));

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("tuic".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
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
        "naive" => "naive",
        other => other,
    }.to_string()
}

fn parse_naive_link(link: &str) -> Option<SingBoxOutbound> {
    let rest = link.strip_prefix("naive+")?;
    let (main_part, tag_part) = rest.split_once('#').unwrap_or((rest, "Naive"));
    let tag = urlencoding::decode(tag_part).ok()?.to_string();

    let url = url::Url::parse(main_part).ok()?;
    let host = url.host_str()?.to_string();
    let port = url.port_or_known_default().unwrap_or(443) as u16;
    let username = urlencoding::decode(url.username()).ok()?.to_string();
    let password = url.password().and_then(|p| urlencoding::decode(p).ok().map(|s| s.to_string())).unwrap_or_default();

    let mut extra = std::collections::HashMap::new();
    extra.insert("username".to_string(), serde_json::Value::String(username));
    extra.insert("password".to_string(), serde_json::Value::String(password));

    if let Some(q) = url.query() {
        for (k, v) in url::form_urlencoded::parse(q.as_bytes()) {
            match k.as_ref() {
                "sni" => {
                    extra.insert("tls".to_string(), serde_json::json!({
                        "enabled": true,
                        "server_name": v.to_string(),
                        "insecure": false
                    }));
                }
                "insecure" => {
                    let insecure = v == "1" || v.eq_ignore_ascii_case("true");
                    let tls_value = extra.remove("tls").unwrap_or_else(|| serde_json::json!({ "enabled": true }));
                    let mut tls_obj = tls_value.as_object().cloned().unwrap_or_default();
                    tls_obj.insert("enabled".to_string(), serde_json::Value::Bool(true));
                    tls_obj.insert("insecure".to_string(), serde_json::Value::Bool(insecure));
                    if !tls_obj.contains_key("server_name") {
                        tls_obj.insert("server_name".to_string(), serde_json::Value::String(host.clone()));
                    }
                    extra.insert("tls".to_string(), serde_json::Value::Object(tls_obj));
                }
                _ => {}
            }
        }
    }

    if !extra.contains_key("tls") {
        extra.insert("tls".to_string(), serde_json::json!({
            "enabled": true,
            "server_name": host,
            "insecure": false
        }));
    }

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("naive".to_string()),
        server: Some(url.host_str()?.to_string()),
        server_port: Some(port),
        extra,
    })
}

fn extract_hostname(url: &str) -> String {
    url::Url::parse(url)
        .map(|u| u.host_str().unwrap_or("Unknown").to_string())
        .unwrap_or_else(|_| "Unknown".to_string())
}

#[tauri::command]
pub async fn node_begin_latency_tests(state: State<'_, AppState>, run_id: u64) -> Result<(), String> {
    cleanup_temp_singbox(&state).await;
    begin_latency_test_batch(run_id).await;
    Ok(())
}

#[tauri::command]
pub async fn node_test_latency(app: AppHandle, state: State<'_, AppState>, tag: String, run_id: Option<u64>) -> Result<NodeLatencyResult, String> {
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
            ).await;
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
            ).await;
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
            ).await;
            return Ok(temp_result);
        }
    }
}

#[tauri::command]
pub async fn node_test_all(app: AppHandle, state: State<'_, AppState>) -> Result<std::collections::HashMap<String, i64>, String> {
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
    let prefer_temp_backend = matches!(proxy_state, ProxyState::Connected | ProxyState::Connecting) || !main_api_ready;

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

    // Test in chunks for concurrency
    let chunk_size = 5;
    for chunk in nodes.chunks(chunk_size) {
        if cancel_token.is_cancelled() {
            break;
        }

        let futures: Vec<_> = chunk.iter()
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
                let ports_clone = ports.clone();
                async move {
                    let mut latency = -1;
                    for p in ports_clone {
                        if cancel_token.is_cancelled() {
                            break;
                        }

                        match test_latency_via_clash_api_cancellable(&lookup_tag, p, &test_url, timeout_ms, cancel_token.clone()).await {
                            Ok(v) if v > 0 => {
                                latency = v;
                                break;
                            }
                            Ok(_) | Err(LatencyProbeError::Timeout) => {}
                            Err(LatencyProbeError::Failed) => {
                                log::debug!("Latency test failed on port {} for '{}' via '{}'", p, tag_clone, lookup_tag);
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
pub async fn node_cancel_latency_tests(state: State<'_, AppState>, run_id: Option<u64>) -> Result<(), String> {
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

async fn test_latency_via_clash_api(
    proxy_name: &str,
    port: u16,
    test_url: &str,
    timeout_ms: u32,
) -> Result<i64, LatencyProbeError> {
    let effective_timeout_ms = timeout_ms.max(1) as u64;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(effective_timeout_ms.saturating_add(2_000)))
        .build()
        .map_err(|_| LatencyProbeError::Failed)?;

    let encoded_name = urlencoding::encode(proxy_name);
    let url = format!(
        "http://127.0.0.1:{}/proxies/{}/delay?url={}&timeout={}",
        port,
        encoded_name,
        urlencoding::encode(test_url),
        effective_timeout_ms
    );

    let response = client.get(&url).send().await.map_err(|e| {
        if e.is_timeout() {
            LatencyProbeError::Timeout
        } else {
            LatencyProbeError::Failed
        }
    })?;

    if !response.status().is_success() {
        if response.status() == reqwest::StatusCode::GATEWAY_TIMEOUT
            || response.status() == reqwest::StatusCode::REQUEST_TIMEOUT
        {
            return Err(LatencyProbeError::Timeout);
        }
        return Err(LatencyProbeError::Failed);
    }

    let json: serde_json::Value = response.json().await.map_err(|_| LatencyProbeError::Failed)?;
    if let Some(delay) = json.get("delay").and_then(|d| d.as_i64()) {
        if delay > 0 {
            return Ok(delay);
        }
        return Err(LatencyProbeError::Timeout);
    }

    Err(LatencyProbeError::Failed)
}

async fn test_latency_via_clash_api_cancellable(
    proxy_name: &str,
    port: u16,
    test_url: &str,
    timeout_ms: u32,
    cancel_token: CancellationToken,
) -> Result<i64, LatencyProbeError> {
    tokio::select! {
        _ = cancel_token.cancelled() => Err(LatencyProbeError::Failed),
        result = test_latency_via_clash_api(proxy_name, port, test_url, timeout_ms) => result,
    }
}

#[derive(Debug)]
enum TempStartBlockReason {
    ShutdownInProgress,
    ProxyStateTransitional(ProxyState),
    MainProcessAlive,
    UnknownProcessState,
}

#[derive(Debug)]
enum TempStartGuard {
    Allowed,
    Blocked(TempStartBlockReason),
}

async fn can_start_temp_singbox(state: &AppState, allow_main_process_alive: bool) -> TempStartGuard {
    if *state.shutdown_in_progress.lock().await {
        return TempStartGuard::Blocked(TempStartBlockReason::ShutdownInProgress);
    }
    let proxy_state = state.proxy_state.lock().await.clone();
    if matches!(proxy_state, ProxyState::Connecting | ProxyState::Disconnecting) {
        return TempStartGuard::Blocked(TempStartBlockReason::ProxyStateTransitional(proxy_state.clone()));
    }
    drop(proxy_state);
    if let Some(ref mut child) = *state.singbox_process.lock().await {
        match child.try_wait() {
            Ok(None) => {
                if !allow_main_process_alive {
                    return TempStartGuard::Blocked(TempStartBlockReason::MainProcessAlive);
                }
            }
            Ok(Some(_)) => {}
            Err(_) => {
                return TempStartGuard::Blocked(TempStartBlockReason::UnknownProcessState);
            }
        }
    }
    TempStartGuard::Allowed
}

fn get_kernel_path_with_fallback(app: &AppHandle) -> Option<std::path::PathBuf> {
    if let Ok(data_dir) = app.path().app_data_dir() {
        let data_kernel = data_dir.join("libs").join("sing-box.exe");
        if data_kernel.exists() {
            return Some(data_kernel);
        }
    }

    app.path().resource_dir().ok().map(|dir| dir.join("resources").join("libs").join("sing-box.exe"))
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

#[cfg(not(windows))]
fn support_file_available_for_executable(_executable_path: &std::path::Path, _filename: &str) -> bool {
    true
}

async fn start_temp_singbox(
    app: &AppHandle,
    state: &AppState,
    cancel_token: &CancellationToken,
    allow_main_process_alive: bool,
) -> bool {
    let _lifecycle_guard = state.lifecycle_lock.lock().await;

    if cancel_token.is_cancelled() {
        return false;
    }

    match can_start_temp_singbox(state, allow_main_process_alive).await {
        TempStartGuard::Blocked(TempStartBlockReason::ShutdownInProgress) => {
            log::debug!("start_temp_singbox refused: shutdown in progress");
            return false;
        }
        TempStartGuard::Blocked(TempStartBlockReason::ProxyStateTransitional(s)) => {
            log::debug!("start_temp_singbox refused: proxy state is {:?}", s);
            return false;
        }
        TempStartGuard::Blocked(TempStartBlockReason::MainProcessAlive) => {
            log::debug!("start_temp_singbox refused: main sing-box child is still alive");
            return false;
        }
        TempStartGuard::Blocked(TempStartBlockReason::UnknownProcessState) => {
            log::debug!("start_temp_singbox refused: could not determine main child state");
            return false;
        }
        TempStartGuard::Allowed => {}
    }

    let mut cleanup_existing = false;
    {
        let mut process = TEMP_SINGBOX_PROCESS.lock().await;
        if let Some(ref mut child) = *process {
            // Check if process is still running
            match child.try_wait() {
                Ok(None) => {
                    // Still running, check if API is responsive
                    if check_clash_api_running(TEMP_SINGBOX_PORT).await {
                        log::info!("Rebuilding temp sing-box to refresh latency test config and plugin bridges");
                        cleanup_existing = true;
                    } else {
                        cleanup_existing = true;
                    }
                }
                _ => {
                    // Process exited
                    *process = None;
                    cleanup_existing = true;
                }
            }
        }
    }

    if cleanup_existing {
        cleanup_temp_singbox(state).await;
    }

    // Get kernel path
    let kernel_path = match get_kernel_path_with_fallback(app) {
        Some(path) => path,
        None => return false,
    };

    if !kernel_path.exists() {
        log::warn!("Kernel not found for latency testing: {:?}", kernel_path);
        return false;
    }

    // Load nodes and generate temp config
    let naive_runtime_available = support_file_available_for_executable(&kernel_path, "libcronet.dll");
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
    if !naive_runtime_available
        && nodes_raw
            .iter()
            .any(|node| node.get("type").and_then(|value| value.as_str()) == Some("naive"))
    {
        append_latency_diagnostic(state, "temp sing-box will skip naive nodes because libcronet.dll is unavailable");
    }

    // Create temp config
    let temp_dir = temp_singbox_dir(state);
    if let Err(err) = remove_temp_singbox_dir(&temp_dir) {
        log::warn!("Failed to clear stale temp dir {:?}: {}", temp_dir, err);
    }
    if let Err(e) = fs::create_dir_all(&temp_dir) {
        log::error!("Failed to create temp dir: {}", e);
        return false;
    }

    let (config, temp_tag_map, plugin_bridge_specs) =
        generate_temp_config_raw(&nodes_raw, TEMP_SINGBOX_PORT, naive_runtime_available);
    if temp_tag_map.is_empty() {
        append_latency_diagnostic(state, "temp sing-box skipped because no supported proxy nodes remain after filtering");
        let _ = remove_temp_singbox_dir(&temp_dir);
        return false;
    }
    append_latency_diagnostic(
        state,
        &format!(
            "generated temp config: nodes={}, plugin_bridges={}, api_port={}",
            nodes_raw.len(),
            plugin_bridge_specs.len(),
            TEMP_SINGBOX_PORT
        ),
    );
    let config_path = temp_dir.join("config.json");

    let config_str = serde_json::to_string_pretty(&config).unwrap_or_default();
    log::info!("Temp config: {}", config_str);

    if let Err(e) = fs::write(&config_path, &config_str) {
        log::error!("Failed to write temp config: {}", e);
        let _ = remove_temp_singbox_dir(&temp_dir);
        return false;
    }

    {
        let mut map_slot = TEMP_SINGBOX_TAG_MAP.lock().await;
        *map_slot = temp_tag_map;
    }

    // Save bridge specs for temp processes if any
    let bridge_path = temp_dir.join("plugin_bridges.json");
    if !plugin_bridge_specs.is_empty() {
        if let Ok(specs_str) = serde_json::to_string(&plugin_bridge_specs) {
            let _ = fs::write(&bridge_path, specs_str);
        }
    }

    // Start xray bridges if needed
    let mut xray_processes = Vec::new();
    if !plugin_bridge_specs.is_empty() {
        if let Ok(xray_path) = crate::commands::singbox::xray_plugin_path(app) {
            append_latency_diagnostic(state, &format!("resolved temp Xray path: {:?}", xray_path));
            if xray_path.exists() {
                for spec in &plugin_bridge_specs {
                    let port = spec.get("port").and_then(|v| v.as_u64()).unwrap_or(0);
                    let plugin_config = spec.get("node").cloned().unwrap_or(serde_json::json!({}));
                    let config_for_xray = match crate::commands::singbox::build_xray_plugin_config(&plugin_config, port as u16) {
                        Ok(config) => config,
                        Err(err) => {
                            append_latency_diagnostic(state, &format!("failed to build temp Xray config port={}: {}", port, err));
                            log::warn!("Failed to build temp Xray config on port {}: {}", port, err);
                            continue;
                        }
                    };
                    let config_str = serde_json::to_string_pretty(&config_for_xray).unwrap_or_default();

                    let config_path = temp_dir.join(format!("plugin-xray-temp-{}.json", port));
                    let _ = fs::write(&config_path, &config_str);
                    let config_path_str = config_path.to_str().unwrap_or("");
                    let stdout_path = temp_dir.join(format!("plugin-xray-temp-{}.out.log", port));
                    let stderr_path = temp_dir.join(format!("plugin-xray-temp-{}.err.log", port));
                    let stdout = fs::File::create(&stdout_path).ok();
                    let stderr = fs::File::create(&stderr_path).ok();

                    let mut child_cmd = tokio::process::Command::new(&xray_path);
                    #[cfg(windows)]
                    child_cmd.creation_flags(CREATE_NO_WINDOW);

                    child_cmd
                        .args(["run", "-config", config_path_str])
                        .current_dir(&temp_dir)
                        .kill_on_drop(true);
                    if let Some(stdout) = stdout {
                        child_cmd.stdout(std::process::Stdio::from(stdout));
                    }
                    if let Some(stderr) = stderr {
                        child_cmd.stderr(std::process::Stdio::from(stderr));
                    }
                    let child = child_cmd.spawn();

                    if let Ok(c) = child {
                        append_latency_diagnostic(state, &format!("started temp Xray bridge port={}, config={:?}", port, config_path));
                        log::info!("Started temp Xray bridge on port {}, config: {:?}", port, config_path);
                        xray_processes.push(c);
                    } else if let Err(err) = child {
                        append_latency_diagnostic(state, &format!("failed to start temp Xray bridge port={}: {}", port, err));
                        log::warn!("Failed to start temp Xray bridge on port {}: {}", port, err);
                    }
                }
            }
        } else if let Err(err) = crate::commands::singbox::xray_plugin_path(app) {
            append_latency_diagnostic(state, &format!("failed to resolve temp Xray path: {}", err));
        }
    }

    {
        let mut global_xray = TEMP_XRAY_PROCESSES.lock().await;
        *global_xray = xray_processes;
    }

    // Start temp sing-box
    let config_path_str = match config_path.to_str() {
        Some(s) => s,
        None => {
            log::error!("Config path contains invalid UTF-8 characters");
            clear_temp_singbox_tag_map().await;
            let _ = remove_temp_singbox_dir(&temp_dir);
            return false;
        }
    };

    #[cfg(windows)]
    let result = {
        let stdout = fs::File::create(temp_dir.join("temp-singbox.out.log")).ok();
        let stderr = fs::File::create(temp_dir.join("temp-singbox.err.log")).ok();
        let mut command = tokio::process::Command::new(&kernel_path);
        command
            .args(["run", "-c", config_path_str])
            .current_dir(&temp_dir)
            .creation_flags(CREATE_NO_WINDOW)
            .kill_on_drop(true);
        if let Some(stdout) = stdout {
            command.stdout(std::process::Stdio::from(stdout));
        }
        if let Some(stderr) = stderr {
            command.stderr(std::process::Stdio::from(stderr));
        }
        command.spawn()
    };

    #[cfg(not(windows))]
    let result = {
        let stdout = fs::File::create(temp_dir.join("temp-singbox.out.log")).ok();
        let stderr = fs::File::create(temp_dir.join("temp-singbox.err.log")).ok();
        let mut command = tokio::process::Command::new(&kernel_path);
        command
            .args(["run", "-c", config_path_str])
            .current_dir(&temp_dir)
            .kill_on_drop(true);
        if let Some(stdout) = stdout {
            command.stdout(std::process::Stdio::from(stdout));
        }
        if let Some(stderr) = stderr {
            command.stderr(std::process::Stdio::from(stderr));
        }
        command.spawn()
    };

    match result {
        Ok(child) => {
            {
                let mut process = TEMP_SINGBOX_PROCESS.lock().await;
                *process = Some(child);
            }

            for _ in 0..20 {
                if cancel_token.is_cancelled() {
                    cleanup_temp_singbox(state).await;
                    return false;
                }
                if check_clash_api_running(TEMP_SINGBOX_PORT).await {
                    log::info!("Started temp sing-box on port {}", TEMP_SINGBOX_PORT);
                    return true;
                }
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            }

            log::warn!("Temp sing-box started but Clash API not ready in time");
            cleanup_temp_singbox(state).await;
            false
        }
        Err(e) => {
            log::error!("Failed to start temp sing-box: {}", e);
            clear_temp_singbox_tag_map().await;
            let _ = remove_temp_singbox_dir(&temp_dir);
            false
        }
    }
}

pub(crate) async fn check_clash_api_running(port: u16) -> bool {
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

fn generate_temp_config_raw(
    nodes: &[serde_json::Value],
    api_port: u16,
    naive_runtime_available: bool,
) -> (
    serde_json::Value,
    std::collections::HashMap<String, Vec<String>>,
    Vec<serde_json::Value>
) {
    let inferred_ech_dns_server = nodes
        .iter()
        .filter_map(|node| node.get(ECH_DNS_SERVER_META_KEY))
        .filter_map(|value| value.as_str())
        .next()
        .map(|value| value.to_string());
    let active_node_has_ech = nodes.iter().any(|node| {
        node.get("tls")
            .and_then(|value| value.get("ech"))
            .is_some_and(|ech| {
                ech.get("enabled").and_then(|value| value.as_bool()) == Some(true)
                    || ech.get("query_server_name").is_some()
                    || ech.get("config").is_some()
            })
    });
    let effective_remote_dns = inferred_ech_dns_server
        .clone()
        .unwrap_or_else(|| "223.5.5.5".to_string());
    let remote_dns_detour = "direct";
    let remote_dns_domain_resolver = active_node_has_ech.then_some("dns-local");

    // 处理节点，移除不合法字段并添加必要配置
    let mut tag_map = std::collections::HashMap::new();
    let mut plugin_bridge_specs = Vec::new();
    let mut outbounds: Vec<serde_json::Value> = nodes.iter()
        .filter(|node| {
            node.get("type")
                .and_then(|value| value.as_str())
                .is_some_and(|node_type| {
                    crate::commands::singbox::is_proxy_type(node_type)
                        && (node_type != "naive" || naive_runtime_available)
                })
        })
        .enumerate()
        .map(|(index, node)| {
            let bridge_index = plugin_bridge_specs.len();
            let processed_node = crate::commands::singbox::node_for_singbox_with_plugin_bridge(node, &mut plugin_bridge_specs);
            let mut node = processed_node.clone();
            if plugin_bridge_specs.len() > bridge_index {
                let bridge_port = temp_xray_bridge_port(bridge_index);
                if let Some(spec) = plugin_bridge_specs.last_mut().and_then(|spec| spec.as_object_mut()) {
                    spec.insert("port".to_string(), serde_json::json!(bridge_port));
                }
                if let Some(obj) = node.as_object_mut() {
                    obj.insert("server_port".to_string(), serde_json::json!(bridge_port));
                }
            }

            if let Some(obj) = node.as_object_mut() {
                let node_type = obj.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
                let server = obj.get("server").and_then(|s| s.as_str()).unwrap_or("").to_string();
                let port = obj.get("server_port").and_then(|p| p.as_u64()).unwrap_or(0);
                let original_tag = obj
                    .get("tag")
                    .and_then(|tag| tag.as_str())
                    .unwrap_or("")
                    .to_string();
                let temp_tag = make_temp_latency_tag(index);

                obj.insert("tag".to_string(), serde_json::Value::String(temp_tag.clone()));
                if !original_tag.is_empty() {
                    tag_map
                        .entry(original_tag)
                        .or_insert_with(Vec::new)
                        .push(temp_tag);
                }

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
                                "insecure": false
                            }));
                        }
                        "vless" | "vmess" | "trojan" => {
                            // 443 端口通常需要 TLS
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

    (
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
            "dns": {
                "servers": [
                    crate::commands::singbox::build_dns_server("local", "dns-local", "direct"),
                    crate::commands::singbox::build_dns_server_with_resolver(
                        &effective_remote_dns,
                        "dns-remote",
                        remote_dns_detour,
                        remote_dns_domain_resolver,
                    )
                ]
            },
            "inbounds": [],
            "outbounds": outbounds,
            "route": {
                "final": "direct",
                "auto_detect_interface": true,
                "default_domain_resolver": if active_node_has_ech { "dns-local" } else { "dns-remote" }
            }
        }),
        tag_map,
        plugin_bridge_specs
    )
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
    let nodes = normalize_duplicate_node_tags(parse_subscription_content(&content)?);

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

#[tauri::command]
pub async fn node_add(
    state: State<'_, AppState>,
    link: String,
    profile_id: Option<String>,
    profile_name: Option<String>,
) -> Result<SingBoxOutbound, String> {
    let node = parse_node_link(&link).ok_or("Invalid node link")?;

    let mut data = load_profiles_data(&state);
    let target_id = if let Some(profile_id) = profile_id {
        profile_id
    } else if let Some(profile_name) = profile_name {
        let trimmed_name = profile_name.trim();
        if trimmed_name.is_empty() {
            return Err("Profile name cannot be empty".to_string());
        }

        let profile = Profile {
            id: uuid::Uuid::new_v4().to_string(),
            name: trimmed_name.to_string(),
            url: String::new(),
            last_update: Some(chrono::Utc::now().timestamp_millis() as u64),
            node_count: 0,
            enabled: true,
            auto_update_interval: 0,
            dns_pre_resolve: false,
            dns_server: None,
        };

        let new_profile_id = profile.id.clone();
        data.profiles.push(profile);
        new_profile_id
    } else {
        data.active_profile_id.clone().ok_or("No target profile")?
    };

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
            let transport = node.extra.get("transport").and_then(|v| v.as_object());
            let net_type = transport.and_then(|t| t.get("type").and_then(|v| v.as_str())).unwrap_or("tcp");

            let mut query = format!("flow={}&type={}", flow, net_type);

            if let Some(t) = transport {
                if let Some(path) = t.get("path").and_then(|v| v.as_str()) {
                    query.push_str(&format!("&path={}", urlencoding::encode(path)));
                }
                if net_type == "xhttp" {
                    if let Some(mode) = t.get("mode").and_then(|v| v.as_str()) {
                        query.push_str(&format!("&mode={}", urlencoding::encode(mode)));
                    }
                    if let Some(extra) = t.get("extra") {
                        let extra_str = if let Some(s) = extra.as_str() {
                            s.to_string()
                        } else {
                            serde_json::to_string(extra).unwrap_or_default()
                        };
                        if !extra_str.is_empty() {
                            query.push_str(&format!("&extra={}", urlencoding::encode(&extra_str)));
                        }
                    }
                }
                if let Some(headers) = t.get("headers").and_then(|v| v.as_object()) {
                    if let Some(host) = headers.get("Host").and_then(|v| v.as_str()) {
                        query.push_str(&format!("&host={}", urlencoding::encode(host)));
                    }
                }
            }

            // Add SNI/server_name from TLS if present
            if let Some(tls) = node.extra.get("tls").and_then(|v| v.as_object()) {
                if let Some(sni) = tls.get("server_name").and_then(|v| v.as_str()) {
                    query.push_str(&format!("&sni={}", urlencoding::encode(sni)));
                }
            }

            Ok(format!("vless://{}@{}:{}?{}#{}", uuid, server, port, query, tag))
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


#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEMP_PROCESS_TEST_LOCK: once_cell::sync::Lazy<tokio::sync::Mutex<()>> =
        once_cell::sync::Lazy::new(|| tokio::sync::Mutex::new(()));

    fn unique_test_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kunbox-profiles-{}-{}", name, suffix))
    }

    #[test]
    fn parses_insecure_flags_from_links() {
        let trojan = parse_trojan_link("trojan://pwd@example.com:443?allowInsecure=true#demo").unwrap();
        let trojan_tls = trojan.extra.get("tls").and_then(|v| v.as_object()).unwrap();
        assert_eq!(trojan_tls.get("insecure").and_then(|v| v.as_bool()), Some(true));

        let hysteria2 = parse_hysteria2_link("hysteria2://pwd@example.com:443?insecure=true#demo").unwrap();
        let hysteria2_tls = hysteria2.extra.get("tls").and_then(|v| v.as_object()).unwrap();
        assert_eq!(hysteria2_tls.get("insecure").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn parse_clash_naive_ignores_network_field() {
        let proxies = vec![serde_json::json!({
            "name": "Naive H2",
            "type": "naive",
            "server": "naive.example.com",
            "port": 443,
            "username": "user",
            "password": "pass",
            "network": "h2",
            "sni": "naive.example.com"
        })];

        let nodes = parse_clash_proxies(&proxies).expect("expected naive node");
        assert_eq!(nodes.len(), 1);
        let node = &nodes[0];

        assert_eq!(node.outbound_type.as_deref(), Some("naive"));
        assert_eq!(node.extra.get("username").and_then(|value| value.as_str()), Some("user"));
        assert_eq!(
            node.extra
                .get("tls")
                .and_then(|value| value.get("server_name"))
                .and_then(|value| value.as_str()),
            Some("naive.example.com")
        );
        assert!(node.extra.get("network").is_none());
    }

    #[test]
    fn parse_subscription_decodes_url_safe_no_padding_base64() {
        let content = "ss://YWVzLTEyOC1nY206cGFzcw@example.com:8388#SS";
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::URL_SAFE_NO_PAD,
            content,
        );

        let nodes = parse_subscription_content(&encoded).expect("expected decoded subscription");

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].tag.as_deref(), Some("SS"));
        assert_eq!(nodes[0].server.as_deref(), Some("example.com"));
    }

    #[test]
    fn parse_ss_link_supports_plain_sip002_userinfo() {
        let node = parse_ss_link("ss://aes-128-gcm:plain-pass@example.com:8388#Plain").unwrap();

        assert_eq!(node.tag.as_deref(), Some("Plain"));
        assert_eq!(node.server.as_deref(), Some("example.com"));
        assert_eq!(node.server_port, Some(8388));
        assert_eq!(node.extra.get("method").and_then(|value| value.as_str()), Some("aes-128-gcm"));
        assert_eq!(node.extra.get("password").and_then(|value| value.as_str()), Some("plain-pass"));
    }

    #[test]
    fn parse_ss_link_supports_percent_encoded_sip002_userinfo() {
        let node = parse_ss_link("ss://aes-128-gcm%3Aplain-pass@example.com:8388#Plain").unwrap();

        assert_eq!(node.extra.get("method").and_then(|value| value.as_str()), Some("aes-128-gcm"));
        assert_eq!(node.extra.get("password").and_then(|value| value.as_str()), Some("plain-pass"));
        assert_eq!(node.server.as_deref(), Some("example.com"));
        assert_eq!(node.server_port, Some(8388));
    }

    #[test]
    fn parse_clash_proxies_rejects_out_of_range_port() {
        let proxies = vec![serde_json::json!({
            "name": "Bad Port",
            "type": "trojan",
            "server": "example.com",
            "port": 70000,
            "password": "pass"
        })];

        let nodes = parse_clash_proxies(&proxies).unwrap();

        assert!(nodes.is_empty());
    }

    #[test]
    fn parse_clash_proxies_ignores_unsupported_ssr_nodes() {
        let proxies = vec![serde_json::json!({
            "name": "SSR",
            "type": "ssr",
            "server": "example.com",
            "port": 8388,
            "cipher": "aes-256-cfb",
            "password": "pass"
        })];

        let nodes = parse_clash_proxies(&proxies).unwrap();

        assert!(nodes.is_empty());
    }

    #[test]
    fn parse_singbox_outbounds_ignores_unsupported_types() {
        let outbounds = vec![
            serde_json::json!({
                "type": "shadowsocksr",
                "tag": "SSR",
                "server": "example.com",
                "server_port": 8388
            }),
            serde_json::json!({
                "type": "trojan",
                "tag": "Trojan",
                "server": "example.com",
                "server_port": 443,
                "password": "pass"
            }),
        ];

        let nodes = parse_singbox_outbounds(&outbounds).unwrap();

        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].tag.as_deref(), Some("Trojan"));
    }

    #[test]
    fn parse_vmess_link_rejects_out_of_range_port() {
        let json = serde_json::json!({
            "v": "2",
            "ps": "Bad VMess Port",
            "add": "example.com",
            "port": 70000,
            "id": "11111111-1111-1111-1111-111111111111",
            "aid": 0
        });
        let encoded = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            serde_json::to_string(&json).unwrap(),
        );

        assert!(parse_vmess_link(&format!("vmess://{}", encoded)).is_none());
    }

    #[test]
    fn parse_tuic_link_preserves_required_fields() {
        let node = parse_node_link(
            "tuic://11111111-1111-1111-1111-111111111111:secret@example.com:443?congestion_control=bbr&udp_relay_mode=native&sni=tuic.example.com&alpn=h3&allow_insecure=1#TUIC",
        )
        .expect("expected tuic node");

        assert_eq!(node.outbound_type.as_deref(), Some("tuic"));
        assert_eq!(node.tag.as_deref(), Some("TUIC"));
        assert_eq!(node.server.as_deref(), Some("example.com"));
        assert_eq!(node.server_port, Some(443));
        assert_eq!(
            node.extra.get("uuid").and_then(|value| value.as_str()),
            Some("11111111-1111-1111-1111-111111111111")
        );
        assert_eq!(node.extra.get("password").and_then(|value| value.as_str()), Some("secret"));
        assert_eq!(
            node.extra.get("congestion_control").and_then(|value| value.as_str()),
            Some("bbr")
        );
        assert_eq!(
            node.extra.get("udp_relay_mode").and_then(|value| value.as_str()),
            Some("native")
        );
        let tls = node.extra.get("tls").and_then(|value| value.as_object()).unwrap();
        assert_eq!(tls.get("server_name").and_then(|value| value.as_str()), Some("tuic.example.com"));
        assert_eq!(tls.get("insecure").and_then(|value| value.as_bool()), Some(true));
        assert_eq!(tls.get("alpn").and_then(|value| value.as_array()).unwrap()[0].as_str(), Some("h3"));
    }

    #[test]
    fn reconcile_profile_node_selection_falls_back_when_active_tag_disappears() {
        let mut data = ProfilesData {
            profiles: vec![],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("old-node".to_string()),
            node_selections: std::collections::HashMap::from([(
                "profile-a".to_string(),
                "old-node".to_string(),
            )]),
        };
        let nodes = vec![SingBoxOutbound {
            tag: Some("new-node".to_string()),
            outbound_type: Some("trojan".to_string()),
            server: Some("example.com".to_string()),
            server_port: Some(443),
            extra: std::collections::HashMap::new(),
        }];

        reconcile_profile_node_selection(&mut data, "profile-a", &nodes);

        assert_eq!(data.active_node_tag.as_deref(), Some("new-node"));
        assert_eq!(
            data.node_selections.get("profile-a").map(String::as_str),
            Some("new-node")
        );
    }

    fn make_test_state() -> AppState {
        let dir = unique_test_dir("temp-singbox-guard");
        AppState::new(dir)
    }

    #[tokio::test]
    async fn temp_start_forbidden_when_shutdown_in_progress() {
        let state = make_test_state();
        *state.shutdown_in_progress.lock().await = true;
        let guard = can_start_temp_singbox(&state, false).await;
        match guard {
            TempStartGuard::Blocked(TempStartBlockReason::ShutdownInProgress) => {}
            other => panic!("expected ShutdownInProgress block, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn temp_start_forbidden_when_proxy_state_connecting() {
        let state = make_test_state();
        *state.proxy_state.lock().await = ProxyState::Connecting;
        let guard = can_start_temp_singbox(&state, false).await;
        match guard {
            TempStartGuard::Blocked(TempStartBlockReason::ProxyStateTransitional(s)) => {
                assert!(matches!(s, ProxyState::Connecting));
            }
            other => panic!("expected ProxyStateTransitional(Connecting) block, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn temp_start_forbidden_when_proxy_state_disconnecting() {
        let state = make_test_state();
        *state.proxy_state.lock().await = ProxyState::Disconnecting;
        let guard = can_start_temp_singbox(&state, false).await;
        match guard {
            TempStartGuard::Blocked(TempStartBlockReason::ProxyStateTransitional(s)) => {
                assert!(matches!(s, ProxyState::Disconnecting));
            }
            other => panic!("expected ProxyStateTransitional(Disconnecting) block, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn temp_start_forbidden_when_main_process_alive() {
        use tokio::process::Command;
        let state = make_test_state();
        let child = Command::new("cmd").args(["/C", "sleep", "10"]).spawn().unwrap();
        *state.singbox_process.lock().await = Some(child);
        let guard = can_start_temp_singbox(&state, false).await;
        match guard {
            TempStartGuard::Blocked(TempStartBlockReason::MainProcessAlive) => {}
            other => panic!("expected MainProcessAlive block, got {:?}", other),
        }
        let _ = state.singbox_process.lock().await.take().unwrap().kill().await;
    }

    #[tokio::test]
    async fn temp_start_allowed_when_main_process_alive_if_explicitly_permitted() {
        use tokio::process::Command;
        let state = make_test_state();
        let child = Command::new("cmd").args(["/C", "sleep", "10"]).spawn().unwrap();
        *state.singbox_process.lock().await = Some(child);
        let guard = can_start_temp_singbox(&state, true).await;
        match guard {
            TempStartGuard::Allowed => {}
            other => panic!("expected Allowed when main process reuse is permitted, got {:?}", other),
        }
        let _ = state.singbox_process.lock().await.take().unwrap().kill().await;
    }

    #[test]
    fn normalize_duplicate_node_tags_keeps_first_and_suffixes_following_duplicates() {
        let nodes = vec![
            SingBoxOutbound {
                tag: Some("SG|官方优选|94ms".to_string()),
                outbound_type: Some("vless".to_string()),
                server: Some("a.example.com".to_string()),
                server_port: Some(443),
                extra: std::collections::HashMap::new(),
            },
            SingBoxOutbound {
                tag: Some("SG|官方优选|94ms".to_string()),
                outbound_type: Some("vless".to_string()),
                server: Some("b.example.com".to_string()),
                server_port: Some(443),
                extra: std::collections::HashMap::new(),
            },
            SingBoxOutbound {
                tag: Some("SG|官方优选|94ms".to_string()),
                outbound_type: Some("vless".to_string()),
                server: Some("c.example.com".to_string()),
                server_port: Some(443),
                extra: std::collections::HashMap::new(),
            },
        ];

        let normalized = normalize_duplicate_node_tags(nodes);
        let tags: Vec<&str> = normalized
            .iter()
            .filter_map(|node| node.tag.as_deref())
            .collect();

        assert_eq!(
            tags,
            vec![
                "SG|官方优选|94ms",
                "SG|官方优选|94ms #2",
                "SG|官方优选|94ms #3"
            ]
        );
    }

    #[tokio::test]
    async fn temp_start_allowed_when_idle_and_no_main_process() {
        let state = make_test_state();
        *state.proxy_state.lock().await = ProxyState::Idle;
        assert!(state.singbox_process.lock().await.is_none());
        let guard = can_start_temp_singbox(&state, false).await;
        match guard {
            TempStartGuard::Allowed => {}
            other => panic!("expected Allowed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn temp_start_allowed_when_error_and_no_main_process() {
        let state = make_test_state();
        *state.proxy_state.lock().await = ProxyState::Error;
        assert!(state.singbox_process.lock().await.is_none());
        let guard = can_start_temp_singbox(&state, false).await;
        match guard {
            TempStartGuard::Allowed => {}
            other => panic!("expected Allowed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn temp_start_forbidden_when_main_process_exited_normally() {
        use tokio::process::Command;
        let state = make_test_state();
        let child = Command::new("cmd").args(["/C", "echo", "done"]).spawn().unwrap();
        *state.singbox_process.lock().await = Some(child);
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        let guard = can_start_temp_singbox(&state, false).await;
        match guard {
            TempStartGuard::Allowed => {}
            other => panic!("expected Allowed after main process exits, got {:?}", other),
        }
    }

    #[test]
    fn latency_uses_main_backend_when_main_api_is_ready() {
        assert_eq!(select_latency_test_backend(&ProxyState::Idle, true), LatencyTestBackend::Main);
        assert_eq!(select_latency_test_backend(&ProxyState::Error, true), LatencyTestBackend::Main);
    }

    #[test]
    fn latency_uses_main_when_connected_even_if_readiness_probe_fails() {
        assert_eq!(select_latency_test_backend(&ProxyState::Connected, false), LatencyTestBackend::Main);
        assert_eq!(select_latency_test_backend(&ProxyState::Connecting, false), LatencyTestBackend::Main);
    }

    #[test]
    fn latency_uses_temp_only_when_main_api_is_down_and_proxy_not_connected() {
        assert_eq!(select_latency_test_backend(&ProxyState::Idle, false), LatencyTestBackend::Temp);
        assert_eq!(select_latency_test_backend(&ProxyState::Error, false), LatencyTestBackend::Temp);
    }

    #[test]
    fn removes_temp_singbox_directory_recursively() {
        let temp_dir = unique_test_dir("temp-cleanup");
        let nested = temp_dir.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("config.json"), b"{}").unwrap();

        remove_temp_singbox_dir(&temp_dir).unwrap();

        assert!(!temp_dir.exists());
    }

    #[test]
    fn remove_temp_singbox_dir_succeeds_on_nonexistent_path() {
        let nonexistent = unique_test_dir("nonexistent");
        assert!(!nonexistent.exists());
        let result = remove_temp_singbox_dir(&nonexistent);
        assert!(result.is_ok());
    }

    #[test]
    fn generate_temp_config_uses_ascii_unique_tags_and_preserves_mapping() {
        let nodes = vec![
            serde_json::json!({
                "type": "trojan",
                "tag": "节点🚀",
                "server": "a.example.com",
                "server_port": 443,
                "password": "one"
            }),
            serde_json::json!({
                "type": "trojan",
                "tag": "节点🚀",
                "server": "b.example.com",
                "server_port": 443,
                "password": "two"
            }),
            serde_json::json!({
                "type": "trojan",
                "tag": "中文😀",
                "server": "c.example.com",
                "server_port": 443,
                "password": "three"
            }),
        ];

        let (config, tag_map, _) = generate_temp_config_raw(&nodes, TEMP_SINGBOX_PORT, true);
        let outbounds = config
            .get("outbounds")
            .and_then(|value| value.as_array())
            .expect("expected outbounds");

        let tags: Vec<&str> = outbounds
            .iter()
            .take(3)
            .filter_map(|outbound| outbound.get("tag").and_then(|value| value.as_str()))
            .collect();

        assert_eq!(tags, vec!["latency-0000", "latency-0001", "latency-0002"]);
        assert_eq!(tag_map.get("节点🚀"), Some(&vec!["latency-0000".to_string(), "latency-0001".to_string()]));
        assert_eq!(tag_map.get("中文😀"), Some(&vec!["latency-0002".to_string()]));
    }

    #[test]
    fn generate_temp_config_removes_naive_unsupported_fields_and_uses_local_dns() {
        let nodes = vec![serde_json::json!({
            "type": "naive",
            "tag": "Naive H2",
            "server": "naive.example.com",
            "server_port": 443,
            "username": "user",
            "password": "pass",
            "network": "h2"
        })];

        let (config, tag_map, _) = generate_temp_config_raw(&nodes, TEMP_SINGBOX_PORT, true);
        let outbound = config
            .get("outbounds")
            .and_then(|value| value.as_array())
            .and_then(|outbounds| outbounds.first())
            .expect("expected naive outbound");

        assert_eq!(outbound.get("type").and_then(|value| value.as_str()), Some("naive"));
        assert_eq!(outbound.get("tag").and_then(|value| value.as_str()), Some("latency-0000"));
        assert!(outbound.get("network").is_none());
        assert!(outbound.get("transport").is_none());
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
        assert_eq!(tag_map.get("Naive H2"), Some(&vec!["latency-0000".to_string()]));

        let dns_servers = config
            .get("dns")
            .and_then(|value| value.get("servers"))
            .and_then(|value| value.as_array())
            .expect("expected dns servers");
        let remote_dns = dns_servers
            .iter()
            .find(|server| server.get("tag").and_then(|value| value.as_str()) == Some("dns-remote"))
            .expect("expected dns-remote");

        assert_eq!(remote_dns.get("type").and_then(|value| value.as_str()), Some("udp"));
        assert_eq!(remote_dns.get("server").and_then(|value| value.as_str()), Some("223.5.5.5"));
        assert_eq!(remote_dns.get("detour"), None);
    }

    #[test]
    fn generate_temp_config_skips_naive_when_runtime_unavailable() {
        let nodes = vec![
            serde_json::json!({
                "type": "naive",
                "tag": "Naive",
                "server": "naive.example.com",
                "server_port": 443,
                "username": "user",
                "password": "pass"
            }),
            serde_json::json!({
                "type": "trojan",
                "tag": "Trojan",
                "server": "trojan.example.com",
                "server_port": 443,
                "password": "pass"
            }),
        ];

        let (config, tag_map, _) = generate_temp_config_raw(&nodes, TEMP_SINGBOX_PORT, false);
        let outbounds = config
            .get("outbounds")
            .and_then(|value| value.as_array())
            .expect("expected outbounds");

        assert!(outbounds.iter().all(|outbound| {
            outbound.get("type").and_then(|value| value.as_str()) != Some("naive")
        }));
        assert_eq!(tag_map.get("Naive"), None);
        assert_eq!(tag_map.get("Trojan"), Some(&vec!["latency-0000".to_string()]));
    }

    #[test]
    fn generate_temp_config_adds_dns_for_ech_nodes() {
        let nodes = vec![serde_json::json!({
            "type": "vless",
            "tag": "ECH",
            "server": "104.19.41.41",
            "server_port": 443,
            "uuid": "11111111-1111-1111-1111-111111111111",
            "packet_encoding": "xudp",
            "tls": {
                "enabled": true,
                "server_name": "cm.5945946.xyz",
                "ech": {
                    "enabled": true,
                    "query_server_name": "cloudflare-ech.com"
                }
            },
            "transport": {
                "type": "ws",
                "path": "/",
                "headers": { "Host": "cm.5945946.xyz" }
            },
            ECH_DNS_SERVER_META_KEY: "https://dns.alidns.com/dns-query"
        })];

        let (config, _, _) = generate_temp_config_raw(&nodes, TEMP_SINGBOX_PORT, true);

        let dns = config.get("dns").and_then(|value| value.as_object()).expect("expected dns config");
        let servers = dns.get("servers").and_then(|value| value.as_array()).expect("expected dns servers");
        let remote = servers
            .iter()
            .find(|server| server.get("tag").and_then(|value| value.as_str()) == Some("dns-remote"))
            .expect("expected dns-remote server");

        assert_eq!(remote.get("type").and_then(|value| value.as_str()), Some("https"));
        assert_eq!(remote.get("server").and_then(|value| value.as_str()), Some("dns.alidns.com"));
        assert_eq!(remote.get("server_port").and_then(|value| value.as_u64()), Some(443));
        assert_eq!(remote.get("path").and_then(|value| value.as_str()), Some("/dns-query"));
        assert_eq!(remote.get("domain_resolver").and_then(|value| value.as_str()), Some("dns-local"));
    }

    #[tokio::test]
    async fn cleanup_temp_singbox_process_kills_and_clears_process() {
        let _guard = TEMP_PROCESS_TEST_LOCK.lock().await;
        let child = tokio::process::Command::new("cmd")
            .args(["/C", "echo", "done"])
            .spawn()
            .expect("spawn test process");
        {
            let mut slot = TEMP_SINGBOX_PROCESS.lock().await;
            *slot = Some(child);
        }

        cleanup_temp_singbox_process().await;

        let slot = TEMP_SINGBOX_PROCESS.lock().await;
        assert!(slot.is_none(), "TEMP_SINGBOX_PROCESS should be None after cleanup");
    }

    #[tokio::test]
    async fn cleanup_temp_singbox_process_succeeds_when_slot_is_empty() {
        let _guard = TEMP_PROCESS_TEST_LOCK.lock().await;
        {
            let mut slot = TEMP_SINGBOX_PROCESS.lock().await;
            *slot = None;
        }

        cleanup_temp_singbox_process().await;

        let slot = TEMP_SINGBOX_PROCESS.lock().await;
        assert!(slot.is_none());
    }

    #[tokio::test]
    async fn cleanup_temp_singbox_removes_dir_and_process() {
        let _guard = TEMP_PROCESS_TEST_LOCK.lock().await;
        let state = make_test_state();
        let temp_dir = temp_singbox_dir(&state);
        let nested = temp_dir.join("nested");
        std::fs::create_dir_all(&nested).expect("create nested dir");
        std::fs::write(nested.join("config.json"), b"{}").expect("write config");

        let child = tokio::process::Command::new("cmd")
            .args(["/C", "echo", "done"])
            .spawn()
            .expect("spawn test process");
        {
            let mut slot = TEMP_SINGBOX_PROCESS.lock().await;
            *slot = Some(child);
        }

        cleanup_temp_singbox(&state).await;

        let slot = TEMP_SINGBOX_PROCESS.lock().await;
        assert!(slot.is_none(), "TEMP_SINGBOX_PROCESS should be None");
        assert!(!temp_dir.exists(), "temp dir should be removed");
    }

    #[tokio::test]
    async fn cancel_and_reset_latency_test_token_cancels_existing_waiters() {
        let _guard = TEMP_PROCESS_TEST_LOCK.lock().await;
        let old_token = current_latency_test_cancel_token().await;
        assert!(!old_token.is_cancelled());

        cancel_and_reset_latency_test_token().await;

        let new_token = current_latency_test_cancel_token().await;
        assert!(old_token.is_cancelled());
        assert!(!new_token.is_cancelled());
    }

    #[tokio::test]
    async fn cancelled_latency_batch_is_tracked_by_run_id() {
        let _guard = TEMP_PROCESS_TEST_LOCK.lock().await;
        begin_latency_test_batch(7).await;
        assert!(!is_latency_test_batch_cancelled(Some(7)).await);

        mark_latency_test_batch_cancelled(Some(7)).await;
        assert!(is_latency_test_batch_cancelled(Some(7)).await);

        begin_latency_test_batch(8).await;
        assert!(!is_latency_test_batch_cancelled(Some(8)).await);
    }

    #[tokio::test]
    async fn cancelling_old_batch_does_not_cancel_new_active_batch() {
        let _guard = TEMP_PROCESS_TEST_LOCK.lock().await;
        begin_latency_test_batch(10).await;
        let old_token = current_latency_test_cancel_token().await;

        begin_latency_test_batch(11).await;
        let new_token = current_latency_test_cancel_token().await;
        assert!(old_token.is_cancelled());
        assert!(!new_token.is_cancelled());

        mark_latency_test_batch_cancelled(Some(10)).await;
        let latest_token = current_latency_test_cancel_token().await;
        assert!(!new_token.is_cancelled());
        assert!(!latest_token.is_cancelled());
    }

    #[tokio::test]
    async fn cancelling_old_batch_keeps_new_active_batch_id() {
        let _guard = TEMP_PROCESS_TEST_LOCK.lock().await;
        begin_latency_test_batch(20).await;
        begin_latency_test_batch(21).await;

        mark_latency_test_batch_cancelled(Some(20)).await;

        assert_eq!(*ACTIVE_LATENCY_BATCH_ID.lock().await, Some(21));
    }

    #[tokio::test]
    async fn batch_tag_lookup_consumes_unique_aliases_for_duplicate_tags() {
        let _guard = TEMP_PROCESS_TEST_LOCK.lock().await;
        *TEMP_SINGBOX_OWNER_BATCH_ID.lock().await = Some(99);
        {
            let mut slot = TEMP_SINGBOX_TAG_MAP.lock().await;
            slot.clear();
            slot.insert(
                "dup".to_string(),
                vec!["latency-0000".to_string(), "latency-0001".to_string()],
            );
        }

        assert_eq!(take_temp_singbox_tag_for_batch(Some(99), "dup").await.as_deref(), Some("latency-0000"));
        assert_eq!(take_temp_singbox_tag_for_batch(Some(99), "dup").await.as_deref(), Some("latency-0001"));
        assert_eq!(take_temp_singbox_tag_for_batch(Some(99), "dup").await, None);
    }

    #[tokio::test]
    async fn stale_release_does_not_decrement_new_batch_temp_slot_owner() {
        let _guard = TEMP_PROCESS_TEST_LOCK.lock().await;
        assert!(acquire_temp_singbox_test_slot(Some(30)).await);
        release_temp_singbox_test_slot(&make_test_state(), Some(30)).await;

        assert!(acquire_temp_singbox_test_slot(Some(31)).await);
        release_temp_singbox_test_slot(&make_test_state(), Some(30)).await;

        assert_eq!(*TEMP_SINGBOX_OWNER_BATCH_ID.lock().await, Some(31));
        assert_eq!(*TEMP_SINGBOX_ACTIVE_TESTS.lock().await, 1);

        release_temp_singbox_test_slot(&make_test_state(), Some(31)).await;
    }

    #[tokio::test]
    async fn standalone_temp_latency_rejects_concurrent_none_owner() {
        let _guard = TEMP_PROCESS_TEST_LOCK.lock().await;
        assert!(acquire_temp_singbox_test_slot(None).await);
        assert!(!acquire_temp_singbox_test_slot(None).await);

        assert_eq!(*TEMP_SINGBOX_OWNER_BATCH_ID.lock().await, None);
        assert_eq!(*TEMP_SINGBOX_ACTIVE_TESTS.lock().await, 1);

        release_temp_singbox_test_slot(&make_test_state(), None).await;
    }

    #[test]
    fn parse_vless_link_enables_ech_without_invalid_config_for_name_resolver() {
        let outbound = parse_vless_link("vless://11111111-1111-1111-1111-111111111111@example.com:443?security=tls&ech=cloudflare-ech.com+https%3A%2F%2Fdns.alidns.com%2Fdns-query&sni=example.com&host=ws.example.com&fp=chrome&type=ws&path=%2Fws#ECH")
            .expect("expected vless outbound");

        let tls = outbound
            .extra
            .get("tls")
            .and_then(|value| value.as_object())
            .expect("expected tls object");

        assert_eq!(
            tls.get("server_name").and_then(|value| value.as_str()),
            Some("example.com")
        );
        assert_eq!(
            tls.get("utls")
                .and_then(|value| value.get("fingerprint"))
                .and_then(|value| value.as_str()),
            Some("chrome")
        );
        assert_eq!(
            tls.get("ech")
                .and_then(|value| value.get("enabled"))
                .and_then(|value| value.as_bool()),
            Some(true)
        );
        assert_eq!(
            tls.get("ech")
                .and_then(|value| value.get("query_server_name"))
                .and_then(|value| value.as_str()),
            Some("cloudflare-ech.com")
        );
        assert!(
            tls.get("ech")
                .and_then(|value| value.get("config"))
                .is_none(),
            "name+resolver share value must not be serialized as invalid ech.config"
        );
        assert_eq!(
            outbound.extra.get(ECH_DNS_SERVER_META_KEY).and_then(|value| value.as_str()),
            Some("https://dns.alidns.com/dns-query")
        );

        let transport = outbound
            .extra
            .get("transport")
            .and_then(|value| value.as_object())
            .expect("expected transport object");

        assert_eq!(transport.get("type").and_then(|value| value.as_str()), Some("ws"));
        assert_eq!(transport.get("path").and_then(|value| value.as_str()), Some("/ws"));
        assert_eq!(
            transport.get("headers")
                .and_then(|value| value.get("Host"))
                .and_then(|value| value.as_str()),
            Some("ws.example.com")
        );
    }

    #[test]
    fn parse_vless_link_skips_empty_ech_value() {
        let outbound = parse_vless_link("vless://11111111-1111-1111-1111-111111111111@example.com:443?security=tls&ech=&sni=example.com#ECH-EMPTY")
            .expect("expected vless outbound");

        let tls = outbound
            .extra
            .get("tls")
            .and_then(|value| value.as_object())
            .expect("expected tls object");

        assert!(tls.get("ech").is_none(), "empty ech must not create dirty config");
    }

    #[test]
    fn parse_vless_link_serializes_pem_ech_config_as_array() {
        let outbound = parse_vless_link("vless://11111111-1111-1111-1111-111111111111@example.com:443?security=tls&ech=-----BEGIN%20ECHCONFIG-----%0Aabc123%0A-----END%20ECHCONFIG-----#ECH-PEM")
            .expect("expected vless outbound");

        let tls = outbound
            .extra
            .get("tls")
            .and_then(|value| value.as_object())
            .expect("expected tls object");

        let config = tls
            .get("ech")
            .and_then(|value| value.get("config"))
            .and_then(|value| value.as_array())
            .expect("expected ech config array");

        assert_eq!(config.len(), 3);
        assert_eq!(config[0].as_str(), Some("-----BEGIN ECHCONFIG-----"));
        assert_eq!(config[1].as_str(), Some("abc123"));
        assert_eq!(config[2].as_str(), Some("-----END ECHCONFIG-----"));
    }

    #[test]
    fn parse_vless_link_does_not_treat_non_url_suffix_as_ech_resolver() {
        let outbound = parse_vless_link("vless://11111111-1111-1111-1111-111111111111@example.com:443?security=tls&ech=cloudflare-ech.com+not-a-url&sni=example.com#ECH-BAD-RESOLVER")
            .expect("expected vless outbound");

        let tls = outbound
            .extra
            .get("tls")
            .and_then(|value| value.as_object())
            .expect("expected tls object");

        assert_eq!(
            tls.get("ech")
                .and_then(|value| value.get("query_server_name")),
            None
        );
        assert_eq!(outbound.extra.get(ECH_DNS_SERVER_META_KEY), None);
    }

    #[test]
    fn parse_vless_link_xhttp() {
        let link = "vless://2edd765b-a895-46ab-a01c-c4719947546b@35.194.192.123:13324?flow=xtls-rprx-vision&type=xhttp&path=%2F2edd765b-a895-46ab-a01c-c4719947546b-xh&mode=auto&extra=%7B%22encryption%22%3A%22mlkem768x25519plus.native.0rtt.test%22%2C%22noGRPCHeader%22%3Atrue%7D&sni=apple.com#%F0%9F%87%B9%F0%9F%87%BC%20%E5%8F%B0%E6%B9%BE%20GCP-xhttp";
        let outbound = parse_vless_link(link).expect("expected vless outbound");

        assert_eq!(outbound.tag.unwrap(), "🇹🇼 台湾 GCP-xhttp");
        assert_eq!(outbound.outbound_type.unwrap(), "vless");
        assert_eq!(outbound.server.unwrap(), "35.194.192.123");
        assert_eq!(outbound.server_port.unwrap(), 13324);
        assert_eq!(outbound.extra.get("flow").and_then(|v| v.as_str()), Some("xtls-rprx-vision"));
        assert_eq!(
            outbound.extra.get("encryption").and_then(|v| v.as_str()),
            Some("mlkem768x25519plus.native.0rtt.test")
        );

        let tls = outbound.extra.get("tls")
            .and_then(|v| v.as_object())
            .expect("expected inferred tls object");
        assert_eq!(tls.get("server_name").and_then(|v| v.as_str()), Some("apple.com"));

        let transport = outbound.extra.get("transport")
            .and_then(|v| v.as_object())
            .expect("expected transport object");

        assert_eq!(transport.get("type").and_then(|v| v.as_str()), Some("xhttp"));
        assert_eq!(transport.get("path").and_then(|v| v.as_str()), Some("/2edd765b-a895-46ab-a01c-c4719947546b-xh"));
        assert_eq!(transport.get("mode").and_then(|v| v.as_str()), Some("auto"));
        assert!(transport.get("extra").and_then(|v| v.get("encryption")).is_none());
        assert_eq!(
            transport.get("extra")
                .and_then(|v| v.get("noGRPCHeader"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
    }
}
