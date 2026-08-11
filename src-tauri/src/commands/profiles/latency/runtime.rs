use super::*;

#[derive(Debug)]
pub(super) enum TempStartBlockReason {
    ShutdownInProgress,
    ProxyStateTransitional(ProxyState),
    MainProcessAlive,
    UnknownProcessState,
}

#[derive(Debug)]
pub(super) enum TempStartGuard {
    Allowed,
    Blocked(TempStartBlockReason),
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum RunningTempProcessAction {
    Reuse,
    Rebuild,
}

pub(super) fn running_temp_process_action(
    api_ready: bool,
    active_tests: usize,
) -> RunningTempProcessAction {
    if api_ready && active_tests > 1 {
        RunningTempProcessAction::Reuse
    } else {
        RunningTempProcessAction::Rebuild
    }
}

pub(super) async fn can_start_temp_singbox(
    state: &AppState,
    allow_main_process_alive: bool,
) -> TempStartGuard {
    if *state.shutdown_in_progress.lock().await {
        return TempStartGuard::Blocked(TempStartBlockReason::ShutdownInProgress);
    }
    let proxy_state = state.proxy_state.lock().await.clone();
    if matches!(
        proxy_state,
        ProxyState::Connecting | ProxyState::Disconnecting
    ) {
        return TempStartGuard::Blocked(TempStartBlockReason::ProxyStateTransitional(
            proxy_state.clone(),
        ));
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

    app.path()
        .resource_dir()
        .ok()
        .map(|dir| dir.join("resources").join("libs").join("sing-box.exe"))
}

fn split_temp_detour_reference(owner_profile_id: &str, value: &str) -> (String, String) {
    crate::commands::singbox::parse_profile_scoped_node_ref(value)
        .map(|(profile_id, tag)| (profile_id.to_string(), tag.to_string()))
        .unwrap_or_else(|| (owner_profile_id.to_string(), value.to_string()))
}

fn resolve_temp_detour_dependency(
    key: (String, String),
    profile_nodes: &std::collections::HashMap<String, Vec<serde_json::Value>>,
    visiting: &mut std::collections::HashSet<(String, String)>,
    allocated_tags: &mut std::collections::HashMap<(String, String), String>,
    dependencies: &mut Vec<serde_json::Value>,
) -> Result<String, String> {
    if visiting.contains(&key) {
        return Err("前置代理形成循环引用".to_string());
    }
    if let Some(tag) = allocated_tags.get(&key) {
        return Ok(tag.clone());
    }

    let (profile_id, node_tag) = &key;
    let nodes = profile_nodes
        .get(profile_id)
        .ok_or_else(|| format!("前置代理所属配置已停用或失效: {}", profile_id))?;
    let mut matches = nodes
        .iter()
        .filter(|node| node.get("tag").and_then(serde_json::Value::as_str) == Some(node_tag));
    let mut node = matches
        .next()
        .cloned()
        .ok_or_else(|| format!("前置代理节点已失效: {} / {}", profile_id, node_tag))?;
    if matches.next().is_some() {
        return Err(format!("前置代理节点重名: {} / {}", profile_id, node_tag));
    }
    if node_is_metered_protected(&node) {
        return Err("高价计费保护节点不能作为前置代理".to_string());
    }
    let node_type = node
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if !crate::commands::singbox::is_proxy_type(node_type) {
        return Err(format!("前置代理协议不受支持: {}", node_type));
    }

    let runtime_tag = format!("kb-detour-{}", allocated_tags.len());
    allocated_tags.insert(key.clone(), runtime_tag.clone());
    visiting.insert(key.clone());

    let next_reference = node
        .get("detour")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    let next_runtime_tag = if let Some(next_reference) = next_reference {
        let next_key = split_temp_detour_reference(profile_id, &next_reference);
        Some(resolve_temp_detour_dependency(
            next_key,
            profile_nodes,
            visiting,
            allocated_tags,
            dependencies,
        )?)
    } else {
        None
    };

    let object = node
        .as_object_mut()
        .ok_or_else(|| "前置代理节点格式无效".to_string())?;
    object.insert(
        "tag".to_string(),
        serde_json::Value::String(runtime_tag.clone()),
    );
    if let Some(next_runtime_tag) = next_runtime_tag {
        object.insert(
            "detour".to_string(),
            serde_json::Value::String(next_runtime_tag),
        );
    } else {
        object.remove("detour");
    }

    visiting.remove(&key);
    dependencies.push(node);
    Ok(runtime_tag)
}

pub(super) fn prepare_temp_nodes_with_detours(
    active_profile_id: &str,
    nodes: &[serde_json::Value],
    profile_nodes: &std::collections::HashMap<String, Vec<serde_json::Value>>,
) -> Result<(Vec<serde_json::Value>, Vec<serde_json::Value>), String> {
    let mut prepared_nodes = Vec::with_capacity(nodes.len());
    let mut dependencies = Vec::new();
    let mut visiting = std::collections::HashSet::new();
    let mut allocated_tags = std::collections::HashMap::new();

    for source in nodes {
        let mut node = source.clone();
        let detour = node
            .get("detour")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);
        if let Some(detour) = detour {
            let key = split_temp_detour_reference(active_profile_id, &detour);
            let runtime_tag = resolve_temp_detour_dependency(
                key,
                profile_nodes,
                &mut visiting,
                &mut allocated_tags,
                &mut dependencies,
            )?;
            let object = node
                .as_object_mut()
                .ok_or_else(|| "节点格式无效".to_string())?;
            object.insert("detour".to_string(), serde_json::Value::String(runtime_tag));
        }
        prepared_nodes.push(node);
    }

    Ok((prepared_nodes, dependencies))
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

#[cfg(not(windows))]
fn support_file_available_for_executable(
    _executable_path: &std::path::Path,
    _filename: &str,
) -> bool {
    true
}

pub(super) async fn start_temp_singbox(
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
                    let api_ready = check_clash_api_running(TEMP_SINGBOX_PORT).await;
                    let active_tests = *TEMP_SINGBOX_ACTIVE_TESTS.lock().await;
                    if running_temp_process_action(api_ready, active_tests)
                        == RunningTempProcessAction::Reuse
                    {
                        log::debug!("Reusing running temp sing-box for concurrent latency batch");
                        return true;
                    }
                    if api_ready {
                        log::info!("Rebuilding temp sing-box to refresh latency test config and plugin bridges");
                    }
                    cleanup_existing = true;
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
    let naive_runtime_available =
        support_file_available_for_executable(&kernel_path, "libcronet.dll");
    let data = load_profiles_data(state);
    let profile_id = match data.active_profile_id.clone() {
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
        append_latency_diagnostic(
            state,
            "temp sing-box will skip naive nodes because libcronet.dll is unavailable",
        );
    }

    let settings = state.settings.lock().await.clone();
    let runtime_allowed_nodes: Vec<serde_json::Value> = nodes_raw
        .iter()
        .filter(|node| {
            !node_is_metered_protected(node)
                || node.get("tag").and_then(serde_json::Value::as_str)
                    == data.active_node_tag.as_deref()
        })
        .cloned()
        .collect();
    let profile_nodes: std::collections::HashMap<String, Vec<serde_json::Value>> = data
        .profiles
        .iter()
        .filter(|profile| profile.enabled)
        .map(|profile| {
            let nodes = if profile.id == profile_id {
                nodes_raw.clone()
            } else {
                load_profile_nodes_raw(state, &profile.id)
            };
            (profile.id.clone(), nodes)
        })
        .collect();
    let (nodes_raw, detour_dependencies) = match prepare_temp_nodes_with_detours(
        &profile_id,
        &runtime_allowed_nodes,
        &profile_nodes,
    ) {
        Ok(result) => result,
        Err(err) => {
            append_latency_diagnostic(state, &format!("temp detour resolution failed: {}", err));
            log::warn!("Temp detour resolution failed: {}", err);
            return false;
        }
    };
    // Create temp config
    let temp_dir = temp_singbox_dir(state);
    if let Err(err) = remove_temp_singbox_dir(&temp_dir) {
        log::warn!("Failed to clear stale temp dir {:?}: {}", temp_dir, err);
    }
    if let Err(e) = fs::create_dir_all(&temp_dir) {
        log::error!("Failed to create temp dir: {}", e);
        return false;
    }

    let temp_remote_dns = temp_latency_remote_dns(&settings);
    append_latency_diagnostic(
        state,
        &format!("temp latency dns server: {}", temp_remote_dns),
    );

    let (config, temp_tag_map, temp_proxy_port_map, plugin_bridge_specs) =
        generate_temp_config_with_dependencies_raw(
            &nodes_raw,
            &detour_dependencies,
            TEMP_SINGBOX_PORT,
            naive_runtime_available,
            &temp_remote_dns,
        );
    if temp_tag_map.is_empty() {
        append_latency_diagnostic(
            state,
            "temp sing-box skipped because no supported proxy nodes remain after filtering",
        );
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
    {
        let mut map_slot = TEMP_SINGBOX_PROXY_PORT_MAP.lock().await;
        *map_slot = temp_proxy_port_map;
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
                    let port =
                        match crate::commands::singbox::parse_plugin_bridge_port(spec, "port")
                            .and_then(|port| port.ok_or_else(|| "插件桥接缺少端口".to_string()))
                        {
                            Ok(port) => port,
                            Err(err) => {
                                append_latency_diagnostic(
                                    state,
                                    &format!("invalid temp Xray bridge port: {}", err),
                                );
                                log::warn!("Invalid temp Xray bridge port: {}", err);
                                continue;
                            }
                        };
                    let plugin_config = spec.get("node").cloned().unwrap_or(serde_json::json!({}));
                    let front_proxy_chain_port =
                        match crate::commands::singbox::parse_plugin_bridge_port(
                            spec,
                            "frontProxyChainPort",
                        ) {
                            Ok(port) => port,
                            Err(err) => {
                                append_latency_diagnostic(
                                    state,
                                    &format!("invalid temp front proxy chain port: {}", err),
                                );
                                log::warn!("Invalid temp front proxy chain port: {}", err);
                                continue;
                            }
                        };
                    let config_for_xray = match crate::commands::singbox::build_xray_plugin_config(
                        &plugin_config,
                        port,
                        front_proxy_chain_port,
                    ) {
                        Ok(config) => config,
                        Err(err) => {
                            append_latency_diagnostic(
                                state,
                                &format!("failed to build temp Xray config port={}: {}", port, err),
                            );
                            log::warn!(
                                "Failed to build temp Xray config on port {}: {}",
                                port,
                                err
                            );
                            continue;
                        }
                    };
                    let config_str =
                        serde_json::to_string_pretty(&config_for_xray).unwrap_or_default();

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
                        append_latency_diagnostic(
                            state,
                            &format!(
                                "started temp Xray bridge port={}, config={:?}",
                                port, config_path
                            ),
                        );
                        log::info!(
                            "Started temp Xray bridge on port {}, config: {:?}",
                            port,
                            config_path
                        );
                        xray_processes.push(c);
                    } else if let Err(err) = child {
                        append_latency_diagnostic(
                            state,
                            &format!("failed to start temp Xray bridge port={}: {}", port, err),
                        );
                        log::warn!("Failed to start temp Xray bridge on port {}: {}", port, err);
                    }
                }
            }
        } else if let Err(err) = crate::commands::singbox::xray_plugin_path(app) {
            append_latency_diagnostic(state, &format!("failed to resolve temp Xray path: {}", err));
        }
    }

    if xray_processes.len() != plugin_bridge_specs.len() {
        for mut child in xray_processes {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        append_latency_diagnostic(
            state,
            "temp Xray bridge startup incomplete; temp sing-box startup aborted",
        );
        clear_temp_singbox_tag_map().await;
        clear_temp_singbox_proxy_port_map().await;
        let _ = remove_temp_singbox_dir(&temp_dir);
        return false;
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
            cleanup_temp_singbox(state).await;
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
            cleanup_temp_singbox(state).await;
            false
        }
    }
}

pub(crate) async fn check_clash_api_running(port: u16) -> bool {
    let client = reqwest::Client::builder()
        .no_proxy()
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
