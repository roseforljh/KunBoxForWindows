use crate::state::AppState;
use crate::types::{
    Profile, ProfilesData, SingBoxOutbound, NODE_AUTO_SELECTION_ELIGIBLE_META_KEY,
    NODE_METERED_PROTECTED_META_KEY,
};
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

fn preserve_node_editor_fields(
    previous_nodes: &[SingBoxOutbound],
    refreshed_nodes: &mut [SingBoxOutbound],
) {
    for refreshed in refreshed_nodes {
        let Some(previous) = previous_nodes.iter().find(|node| node.tag == refreshed.tag) else {
            continue;
        };
        for key in [
            "detour",
            NODE_AUTO_SELECTION_ELIGIBLE_META_KEY,
            NODE_METERED_PROTECTED_META_KEY,
        ] {
            if let Some(value) = previous.extra.get(key) {
                refreshed.extra.insert(key.to_string(), value.clone());
            }
        }
    }
}

fn outbound_is_metered_protected(node: &SingBoxOutbound) -> bool {
    node.extra
        .get(NODE_METERED_PROTECTED_META_KEY)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn normalize_node_policy(node: &mut SingBoxOutbound) {
    let metered_protected = outbound_is_metered_protected(node);
    let auto_selection_eligible = !metered_protected
        && node
            .extra
            .get(NODE_AUTO_SELECTION_ELIGIBLE_META_KEY)
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
    node.extra.insert(
        NODE_AUTO_SELECTION_ELIGIBLE_META_KEY.to_string(),
        serde_json::Value::Bool(auto_selection_eligible),
    );
    node.extra.insert(
        NODE_METERED_PROTECTED_META_KEY.to_string(),
        serde_json::Value::Bool(metered_protected),
    );
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
    let previous_nodes = load_profile_nodes(&state, &id);
    let mut nodes = fetch_subscription(&url).await?;
    preserve_node_editor_fields(&previous_nodes, &mut nodes);

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

fn node_detour(node: &SingBoxOutbound) -> Option<&str> {
    node.extra
        .get("detour")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn split_node_reference(owner_profile_id: &str, value: &str) -> (String, String) {
    super::super::singbox::parse_profile_scoped_node_ref(value)
        .map(|(profile_id, tag)| (profile_id.to_string(), tag.to_string()))
        .unwrap_or_else(|| (owner_profile_id.to_string(), value.to_string()))
}

fn resolve_referenced_node(
    state: &AppState,
    data: &ProfilesData,
    owner_profile_id: &str,
    value: &str,
) -> Result<(String, String, SingBoxOutbound), String> {
    let (profile_id, tag) = split_node_reference(owner_profile_id, value);
    let profile = data
        .profiles
        .iter()
        .find(|profile| profile.id == profile_id)
        .ok_or_else(|| format!("前置代理所属配置已失效: {}", profile_id))?;
    if !profile.enabled {
        return Err(format!("前置代理所属配置已停用: {}", profile.name));
    }
    let matching_nodes: Vec<SingBoxOutbound> = load_profile_nodes(state, &profile_id)
        .into_iter()
        .filter(|node| node.tag.as_deref() == Some(tag.as_str()))
        .collect();
    if matching_nodes.len() != 1 {
        return Err(format!("前置代理节点已失效: {} / {}", profile.name, tag));
    }
    Ok((profile_id, tag, matching_nodes[0].clone()))
}

fn normalize_and_validate_detour(
    state: &AppState,
    data: &ProfilesData,
    profile_id: &str,
    original_tag: &str,
    node: &mut SingBoxOutbound,
) -> Result<(), String> {
    let Some(value) = node_detour(node).map(str::to_string) else {
        node.extra.remove("detour");
        return Ok(());
    };
    let updated_tag = node.tag.as_deref().unwrap_or("");
    let (target_profile_id, target_tag, target_node) =
        resolve_referenced_node(state, data, profile_id, &value)?;
    if target_profile_id == profile_id && (target_tag == original_tag || target_tag == updated_tag)
    {
        return Err("节点不能把自身设为前置代理".to_string());
    }
    if outbound_is_metered_protected(&target_node) {
        return Err("高价计费保护节点不能作为前置代理".to_string());
    }

    let normalized = if target_profile_id == profile_id {
        target_tag.clone()
    } else {
        format!("{}::{}", target_profile_id, target_tag)
    };
    node.extra
        .insert("detour".to_string(), serde_json::Value::String(normalized));

    let start = (profile_id.to_string(), updated_tag.to_string());
    let mut seen = HashSet::from([start.clone()]);
    let mut current_profile_id = target_profile_id;
    let mut current_tag = target_node.tag.clone().unwrap_or_default();
    let mut current_node = target_node;
    loop {
        let identity = if current_profile_id == profile_id && current_tag == original_tag {
            start.clone()
        } else {
            (current_profile_id.clone(), current_tag.clone())
        };
        if !seen.insert(identity) {
            return Err("前置代理形成循环引用".to_string());
        }
        let Some(next_ref) = node_detour(&current_node) else {
            break;
        };
        let (next_profile_id, next_tag, next_node) =
            resolve_referenced_node(state, data, &current_profile_id, next_ref)?;
        if outbound_is_metered_protected(&next_node) {
            return Err("前置代理链包含高价计费保护节点".to_string());
        }
        current_profile_id = next_profile_id;
        current_tag = next_tag;
        current_node = next_node;
    }
    Ok(())
}

fn rewrite_detour_reference(
    owner_profile_id: &str,
    node: &mut SingBoxOutbound,
    target_profile_id: &str,
    old_tag: &str,
    new_tag: &str,
) -> bool {
    let Some(value) = node_detour(node).map(str::to_string) else {
        return false;
    };
    let (profile_id, tag) = split_node_reference(owner_profile_id, &value);
    if profile_id != target_profile_id || tag != old_tag {
        return false;
    }
    let replacement = if owner_profile_id == target_profile_id {
        new_tag.to_string()
    } else {
        format!("{}::{}", target_profile_id, new_tag)
    };
    node.extra
        .insert("detour".to_string(), serde_json::Value::String(replacement));
    true
}

fn find_incoming_detour(
    state: &AppState,
    data: &ProfilesData,
    target_profile_id: &str,
    target_tag: &str,
) -> Option<String> {
    for profile in &data.profiles {
        for node in load_profile_nodes(state, &profile.id) {
            if profile.id == target_profile_id && node.tag.as_deref() == Some(target_tag) {
                continue;
            }
            let Some(value) = node_detour(&node) else {
                continue;
            };
            let (profile_id, tag) = split_node_reference(&profile.id, value);
            if profile_id == target_profile_id && tag == target_tag {
                return Some(format!(
                    "{} / {}",
                    profile.name,
                    node.tag.as_deref().unwrap_or("未命名节点")
                ));
            }
        }
    }
    None
}

#[tauri::command]
pub async fn node_update(
    state: State<'_, AppState>,
    profile_id: String,
    original_tag: String,
    mut node: SingBoxOutbound,
) -> Result<SingBoxOutbound, String> {
    let mut data = load_profiles_data(&state);
    if !data.profiles.iter().any(|profile| profile.id == profile_id) {
        return Err("Profile not found".to_string());
    }

    let original_tag = original_tag.trim();
    let updated_tag = node.tag.as_deref().unwrap_or("").trim().to_string();
    if updated_tag.is_empty() {
        return Err("节点名称不能为空".to_string());
    }
    if updated_tag.len() > 256 {
        return Err("节点名称过长".to_string());
    }
    node.tag = Some(updated_tag.clone());

    let node_type = node.outbound_type.as_deref().unwrap_or("");
    if !super::super::singbox::is_proxy_type(node_type) {
        return Err("不支持的节点协议".to_string());
    }
    if node_type != "wireguard" {
        if node
            .server
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .is_empty()
        {
            return Err("服务器地址不能为空".to_string());
        }
        if node.server_port.unwrap_or(0) == 0 {
            return Err("端口必须在 1 到 65535 之间".to_string());
        }
    }

    let mut nodes = load_profile_nodes(&state, &profile_id);
    let index = nodes
        .iter()
        .position(|item| item.tag.as_deref() == Some(original_tag))
        .ok_or_else(|| "Node not found".to_string())?;
    if nodes.iter().enumerate().any(|(item_index, item)| {
        item_index != index && item.tag.as_deref() == Some(updated_tag.as_str())
    }) {
        return Err("同一配置中已存在同名节点".to_string());
    }

    normalize_node_policy(&mut node);
    normalize_and_validate_detour(&state, &data, &profile_id, original_tag, &mut node)?;
    if outbound_is_metered_protected(&node) {
        if let Some(source) = find_incoming_detour(&state, &data, &profile_id, original_tag) {
            return Err(format!(
                "该节点正在被 {} 用作前置代理，请先移除引用",
                source
            ));
        }
    }

    nodes[index] = node.clone();
    for item in &mut nodes {
        rewrite_detour_reference(&profile_id, item, &profile_id, original_tag, &updated_tag);
    }
    save_profile_nodes(&state, &profile_id, &nodes)?;

    if original_tag != updated_tag {
        for profile in data
            .profiles
            .iter()
            .filter(|profile| profile.id != profile_id)
        {
            let mut referenced_nodes = load_profile_nodes(&state, &profile.id);
            let mut changed = false;
            for item in &mut referenced_nodes {
                changed |= rewrite_detour_reference(
                    &profile.id,
                    item,
                    &profile_id,
                    original_tag,
                    &updated_tag,
                );
            }
            if changed {
                save_profile_nodes(&state, &profile.id, &referenced_nodes)?;
            }
        }
    }

    let replacement_selection = if outbound_is_metered_protected(&node) {
        nodes
            .iter()
            .find(|item| !outbound_is_metered_protected(item))
            .and_then(|item| item.tag.clone())
    } else {
        Some(updated_tag.clone())
    };
    if data.active_profile_id.as_deref() == Some(profile_id.as_str())
        && data.active_node_tag.as_deref() == Some(original_tag)
    {
        data.active_node_tag = replacement_selection.clone();
    }
    if data.node_selections.get(&profile_id).map(String::as_str) == Some(original_tag) {
        if let Some(tag) = replacement_selection {
            data.node_selections.insert(profile_id.clone(), tag);
        } else {
            data.node_selections.remove(&profile_id);
        }
    }

    save_profiles_data(&state, &data)?;
    *state.profiles_data.lock().await = data;
    Ok(node)
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
    new_node_links: &[String],
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

    for (index, link) in new_node_links.iter().enumerate() {
        let link = link.trim();
        if link.is_empty() {
            return Err(format!("第 {} 个新节点链接为空", index + 1));
        }
        let node =
            parse_node_link(link).ok_or_else(|| format!("第 {} 个新节点参数无效", index + 1))?;
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
    new_node_links: Option<Vec<String>>,
) -> Result<Profile, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Profile name cannot be empty".to_string());
    }
    let new_node_links = new_node_links.unwrap_or_default();
    if selections.is_empty() && new_node_links.is_empty() {
        return Err("No nodes selected".to_string());
    }

    let nodes = collect_custom_profile_nodes(&state, &selections, &new_node_links)?;
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

    fn test_profile(id: &str) -> Profile {
        Profile {
            id: id.to_string(),
            name: id.to_string(),
            url: String::new(),
            last_update: None,
            node_count: 0,
            enabled: true,
            auto_update_interval: 0,
            dns_pre_resolve: false,
            dns_server: None,
        }
    }

    fn test_node(tag: &str) -> SingBoxOutbound {
        SingBoxOutbound {
            tag: Some(tag.to_string()),
            outbound_type: Some("socks".to_string()),
            server: Some("127.0.0.1".to_string()),
            server_port: Some(1080),
            extra: std::collections::HashMap::new(),
        }
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
            &[],
        )
        .unwrap();

        assert_eq!(nodes.len(), 3);
        assert_eq!(nodes[0].tag.as_deref(), Some("same"));
        assert_eq!(nodes[0].server.as_deref(), Some("b.example.com"));
        assert_eq!(nodes[1].tag.as_deref(), Some("same #2"));
        assert_eq!(nodes[1].server.as_deref(), Some("a.example.com"));
        assert_eq!(nodes[2].tag.as_deref(), Some("only-a"));
    }

    #[test]
    fn custom_profile_accepts_only_new_node_links() {
        let state = AppState::new(unique_test_dir("custom-profile-new-nodes"));
        let nodes = collect_custom_profile_nodes(
            &state,
            &[],
            &[
                "socks5://user:pass@proxy.example.com:1080#Manual%20SOCKS".to_string(),
                "anytls://secret@example.com:443?sni=tls.example.com#Manual%20AnyTLS".to_string(),
            ],
        )
        .unwrap();

        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0].tag.as_deref(), Some("Manual SOCKS"));
        assert_eq!(nodes[0].outbound_type.as_deref(), Some("socks"));
        assert_eq!(nodes[1].tag.as_deref(), Some("Manual AnyTLS"));
        assert_eq!(nodes[1].outbound_type.as_deref(), Some("anytls"));
    }

    #[test]
    fn custom_profile_rejects_invalid_new_node_without_persisting_it() {
        let state = AppState::new(unique_test_dir("custom-profile-invalid-new-node"));
        let error = collect_custom_profile_nodes(
            &state,
            &[],
            &["socks5://proxy.example.com:70000#Invalid".to_string()],
        )
        .unwrap_err();

        assert_eq!(error, "第 1 个新节点参数无效");
    }

    #[test]
    fn node_policy_keeps_metered_protection_and_auto_selection_mutually_exclusive() {
        let mut node = test_node("metered");
        node.extra.insert(
            NODE_AUTO_SELECTION_ELIGIBLE_META_KEY.to_string(),
            serde_json::Value::Bool(true),
        );
        node.extra.insert(
            NODE_METERED_PROTECTED_META_KEY.to_string(),
            serde_json::Value::Bool(true),
        );

        normalize_node_policy(&mut node);

        assert!(outbound_is_metered_protected(&node));
        assert_eq!(
            node.extra
                .get(NODE_AUTO_SELECTION_ELIGIBLE_META_KEY)
                .and_then(serde_json::Value::as_bool),
            Some(false)
        );
    }

    #[test]
    fn node_detour_rejects_cycle() {
        let state = AppState::new(unique_test_dir("node-detour-cycle"));
        let mut first = test_node("first");
        first.extra.insert(
            "detour".to_string(),
            serde_json::Value::String("second".to_string()),
        );
        save_profile_nodes(&state, "profile-a", &[first, test_node("second")]).unwrap();
        let data = ProfilesData {
            profiles: vec![test_profile("profile-a")],
            active_profile_id: Some("profile-a".to_string()),
            active_node_tag: Some("first".to_string()),
            node_selections: std::collections::HashMap::new(),
        };
        let mut updated_second = test_node("second");
        updated_second.extra.insert(
            "detour".to_string(),
            serde_json::Value::String("first".to_string()),
        );

        let error = normalize_and_validate_detour(
            &state,
            &data,
            "profile-a",
            "second",
            &mut updated_second,
        )
        .unwrap_err();

        assert_eq!(error, "前置代理形成循环引用");
    }

    #[test]
    fn node_rename_rewrites_profile_scoped_detour_reference() {
        let mut node = test_node("consumer");
        node.extra.insert(
            "detour".to_string(),
            serde_json::Value::String("profile-a::old".to_string()),
        );

        assert!(rewrite_detour_reference(
            "profile-b",
            &mut node,
            "profile-a",
            "old",
            "new"
        ));
        assert_eq!(node_detour(&node), Some("profile-a::new"));
    }
}
