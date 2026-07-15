use crate::state::AppState;
use crate::types::{Profile, ProfilesData, SingBoxOutbound};
use std::collections::HashSet;
use std::fs;
use tauri::State;
use uuid::Uuid;

use super::subscription::{
    export_node_to_link, fetch_subscription, normalize_duplicate_node_tags, parse_node_link,
    parse_subscription_content,
};

pub(super) fn load_profiles_data(state: &AppState) -> ProfilesData {
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
        && profile_id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn profile_nodes_path(state: &AppState, profile_id: &str) -> Result<std::path::PathBuf, String> {
    if !is_valid_profile_id(profile_id) {
        return Err("Invalid profile id".to_string());
    }

    Ok(state.configs_dir().join(format!("{}.json", profile_id)))
}

pub(super) fn load_profile_nodes(state: &AppState, profile_id: &str) -> Vec<SingBoxOutbound> {
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

pub(super) fn load_profile_nodes_raw(state: &AppState, profile_id: &str) -> Vec<serde_json::Value> {
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

fn save_profile_nodes(
    state: &AppState,
    profile_id: &str,
    nodes: &[SingBoxOutbound],
) -> Result<(), String> {
    fs::create_dir_all(state.configs_dir()).map_err(|e| e.to_string())?;
    let file = profile_nodes_path(state, profile_id)?;
    let content = serde_json::to_string_pretty(nodes).map_err(|e| e.to_string())?;
    fs::write(file, content).map_err(|e| e.to_string())?;
    Ok(())
}

fn reconcile_profile_node_selection(
    data: &mut ProfilesData,
    profile_id: &str,
    nodes: &[SingBoxOutbound],
) {
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
        data.node_selections
            .insert(profile_id.to_string(), tag.clone());
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
    let profile_idx = data
        .profiles
        .iter()
        .position(|p| p.id == id)
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
    data.active_node_tag = data
        .node_selections
        .get(&id)
        .and_then(|saved_tag| {
            nodes
                .iter()
                .any(|n| n.tag.as_deref() == Some(saved_tag.as_str()))
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
    let profile_idx = data
        .profiles
        .iter()
        .position(|p| p.id == id)
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
        data.profiles[profile_idx].dns_server = if server.is_empty() {
            None
        } else {
            Some(server)
        };
    }

    save_profiles_data(&state, &data)?;
    let profile = data.profiles[profile_idx].clone();
    *state.profiles_data.lock().await = data;
    Ok(profile)
}

#[tauri::command]
pub async fn profile_set_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    let mut data = load_profiles_data(&state);
    let profile = data
        .profiles
        .iter_mut()
        .find(|p| p.id == id)
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

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomProfileNodeSelection {
    pub source_profile_id: String,
    pub tag: String,
}

#[tauri::command]
pub async fn node_list_all(
    state: State<'_, AppState>,
    include_disabled: Option<bool>,
) -> Result<Vec<NodeWithProfile>, String> {
    let data = load_profiles_data(&state);
    let mut all_nodes = Vec::new();
    let include_disabled = include_disabled.unwrap_or(false);

    for profile in &data.profiles {
        if !include_disabled && !profile.enabled {
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

fn collect_custom_profile_nodes(
    state: &AppState,
    selections: &[CustomProfileNodeSelection],
) -> Result<Vec<SingBoxOutbound>, String> {
    let mut selected_nodes = Vec::new();
    let mut seen = HashSet::new();

    for selection in selections {
        let source_profile_id = selection.source_profile_id.trim();
        let tag = selection.tag.trim();
        if source_profile_id.is_empty() || tag.is_empty() {
            return Err("Invalid node selection".to_string());
        }
        if !seen.insert((source_profile_id.to_string(), tag.to_string())) {
            continue;
        }

        let nodes = load_profile_nodes(state, source_profile_id);
        let node = nodes
            .into_iter()
            .find(|node| node.tag.as_deref() == Some(tag))
            .ok_or_else(|| format!("Node not found: {}", tag))?;
        selected_nodes.push(node);
    }

    if selected_nodes.is_empty() {
        return Err("No nodes selected".to_string());
    }

    Ok(normalize_duplicate_node_tags(selected_nodes))
}

#[tauri::command]
pub async fn profile_create_custom(
    state: State<'_, AppState>,
    name: String,
    selections: Vec<CustomProfileNodeSelection>,
) -> Result<Profile, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Profile name cannot be empty".to_string());
    }
    if selections.is_empty() {
        return Err("No nodes selected".to_string());
    }

    let nodes = collect_custom_profile_nodes(&state, &selections)?;
    let mut data = load_profiles_data(&state);
    let profile = Profile {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        url: String::new(),
        last_update: Some(chrono::Utc::now().timestamp_millis() as u64),
        node_count: nodes.len() as u32,
        enabled: true,
        auto_update_interval: 0,
        dns_pre_resolve: false,
        dns_server: None,
    };

    save_profile_nodes(&state, &profile.id, &nodes)?;

    if data.active_profile_id.is_none() {
        data.active_profile_id = Some(profile.id.clone());
        data.active_node_tag = nodes.first().and_then(|node| node.tag.clone());
    }
    data.profiles.push(profile.clone());
    save_profiles_data(&state, &data)?;
    *state.profiles_data.lock().await = data;

    Ok(profile)
}

fn extract_hostname(url: &str) -> String {
    url::Url::parse(url)
        .map(|u| u.host_str().unwrap_or("Unknown").to_string())
        .unwrap_or_else(|_| "Unknown".to_string())
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
    let node = nodes
        .iter()
        .find(|n| n.tag.as_ref() == Some(&tag))
        .ok_or("Node not found")?;

    export_node_to_link(node)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kunbox-profiles-{}-{}", name, suffix))
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

    #[test]
    fn custom_profile_nodes_follow_selection_order_and_normalize_duplicate_tags() {
        let state = AppState::new(unique_test_dir("custom-profile-nodes"));
        save_profile_nodes(
            &state,
            "profile-a",
            &[
                SingBoxOutbound {
                    tag: Some("same".to_string()),
                    outbound_type: Some("trojan".to_string()),
                    server: Some("a.example.com".to_string()),
                    server_port: Some(443),
                    extra: std::collections::HashMap::new(),
                },
                SingBoxOutbound {
                    tag: Some("only-a".to_string()),
                    outbound_type: Some("vless".to_string()),
                    server: Some("only.example.com".to_string()),
                    server_port: Some(8443),
                    extra: std::collections::HashMap::new(),
                },
            ],
        )
        .unwrap();
        save_profile_nodes(
            &state,
            "profile-b",
            &[SingBoxOutbound {
                tag: Some("same".to_string()),
                outbound_type: Some("shadowsocks".to_string()),
                server: Some("b.example.com".to_string()),
                server_port: Some(8388),
                extra: std::collections::HashMap::new(),
            }],
        )
        .unwrap();

        let nodes = collect_custom_profile_nodes(
            &state,
            &[
                CustomProfileNodeSelection {
                    source_profile_id: "profile-b".to_string(),
                    tag: "same".to_string(),
                },
                CustomProfileNodeSelection {
                    source_profile_id: "profile-a".to_string(),
                    tag: "same".to_string(),
                },
                CustomProfileNodeSelection {
                    source_profile_id: "profile-a".to_string(),
                    tag: "only-a".to_string(),
                },
            ],
        )
        .unwrap();

        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].tag.as_deref(), Some("same"));
        assert_eq!(nodes[0].server.as_deref(), Some("b.example.com"));
        assert_eq!(nodes[1].tag.as_deref(), Some("same #2"));
        assert_eq!(nodes[1].server.as_deref(), Some("a.example.com"));
        assert_eq!(nodes[2].tag.as_deref(), Some("only-a"));
    }
}
