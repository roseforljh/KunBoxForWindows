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
        append_latency_diagnostic(
            state,
            "temp sing-box will skip naive nodes because libcronet.dll is unavailable",
        );
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

    let temp_remote_dns = {
        let settings = state.settings.lock().await.clone();
        temp_latency_remote_dns(&settings)
    };
    append_latency_diagnostic(
        state,
        &format!("temp latency dns server: {}", temp_remote_dns),
    );

    let (config, temp_tag_map, temp_proxy_port_map, plugin_bridge_specs) = generate_temp_config_raw(
        &nodes_raw,
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
                    let port = spec.get("port").and_then(|v| v.as_u64()).unwrap_or(0);
                    let plugin_config = spec.get("node").cloned().unwrap_or(serde_json::json!({}));
                    let config_for_xray = match crate::commands::singbox::build_xray_plugin_config(
                        &plugin_config,
                        port as u16,
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
            clear_temp_singbox_proxy_port_map().await;
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
            clear_temp_singbox_proxy_port_map().await;
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
