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
        other => panic!(
            "expected ProxyStateTransitional(Connecting) block, got {:?}",
            other
        ),
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
        other => panic!(
            "expected ProxyStateTransitional(Disconnecting) block, got {:?}",
            other
        ),
    }
}

#[tokio::test]
async fn temp_start_forbidden_when_main_process_alive() {
    use tokio::process::Command;
    let state = make_test_state();
    let child = Command::new("cmd")
        .args(["/C", "sleep", "10"])
        .spawn()
        .unwrap();
    *state.singbox_process.lock().await = Some(child);
    let guard = can_start_temp_singbox(&state, false).await;
    match guard {
        TempStartGuard::Blocked(TempStartBlockReason::MainProcessAlive) => {}
        other => panic!("expected MainProcessAlive block, got {:?}", other),
    }
    let _ = state
        .singbox_process
        .lock()
        .await
        .take()
        .unwrap()
        .kill()
        .await;
}

#[tokio::test]
async fn temp_start_allowed_when_main_process_alive_if_explicitly_permitted() {
    use tokio::process::Command;
    let state = make_test_state();
    let child = Command::new("cmd")
        .args(["/C", "sleep", "10"])
        .spawn()
        .unwrap();
    *state.singbox_process.lock().await = Some(child);
    let guard = can_start_temp_singbox(&state, true).await;
    match guard {
        TempStartGuard::Allowed => {}
        other => panic!(
            "expected Allowed when main process reuse is permitted, got {:?}",
            other
        ),
    }
    let _ = state
        .singbox_process
        .lock()
        .await
        .take()
        .unwrap()
        .kill()
        .await;
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
    let child = Command::new("cmd")
        .args(["/C", "echo", "done"])
        .spawn()
        .unwrap();
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
    assert_eq!(
        select_latency_test_backend(&ProxyState::Idle, true),
        LatencyTestBackend::Main
    );
    assert_eq!(
        select_latency_test_backend(&ProxyState::Error, true),
        LatencyTestBackend::Main
    );
}

#[test]
fn latency_uses_main_when_connected_even_if_readiness_probe_fails() {
    assert_eq!(
        select_latency_test_backend(&ProxyState::Connected, false),
        LatencyTestBackend::Main
    );
    assert_eq!(
        select_latency_test_backend(&ProxyState::Connecting, false),
        LatencyTestBackend::Main
    );
}

#[test]
fn latency_uses_temp_only_when_main_api_is_down_and_proxy_not_connected() {
    assert_eq!(
        select_latency_test_backend(&ProxyState::Idle, false),
        LatencyTestBackend::Temp
    );
    assert_eq!(
        select_latency_test_backend(&ProxyState::Error, false),
        LatencyTestBackend::Temp
    );
}

#[test]
fn proxy_failure_maps_to_node_failure_status() {
    let result = map_latency_probe_result(
        Err(LatencyProbeError::ProxyFailed),
        NodeLatencyStatus::LocalTestFailed,
    );

    assert_eq!(result.status, NodeLatencyStatus::ProxyFailed);
    assert_eq!(result.latency_ms, None);
}

#[test]
fn proxy_latency_fallback_only_runs_when_clash_probe_did_not_succeed() {
    assert!(!should_try_proxy_latency_fallback(&Ok(123), Some(19280)));
    assert!(!should_try_proxy_latency_fallback(
        &Err(LatencyProbeError::ProxyFailed),
        None
    ));
    assert!(should_try_proxy_latency_fallback(
        &Err(LatencyProbeError::ProxyFailed),
        Some(19280)
    ));
    assert!(should_try_proxy_latency_fallback(
        &Err(LatencyProbeError::Timeout),
        Some(19280)
    ));
}

#[test]
fn take_temp_proxy_port_preserves_duplicate_node_order() {
    let mut port_map = std::collections::HashMap::new();
    port_map.insert("dup".to_string(), vec![19280, 19281]);

    assert_eq!(take_temp_proxy_port(&mut port_map, "dup"), Some(19280));
    assert_eq!(take_temp_proxy_port(&mut port_map, "dup"), Some(19281));
    assert_eq!(take_temp_proxy_port(&mut port_map, "dup"), None);
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
fn temp_latency_remote_dns_falls_back_for_local_or_fakeip() {
    let mut settings = AppSettings::default();

    settings.remote_dns = "fakeip".to_string();
    assert_eq!(
        temp_latency_remote_dns(&settings),
        TEMP_LATENCY_FALLBACK_REMOTE_DNS
    );
    assert_eq!(temp_latency_remote_dns(&settings), "223.5.5.5");

    settings.remote_dns = "local".to_string();
    assert_eq!(
        temp_latency_remote_dns(&settings),
        TEMP_LATENCY_FALLBACK_REMOTE_DNS
    );

    settings.remote_dns = "  https://dns.alidns.com/dns-query  ".to_string();
    assert_eq!(
        temp_latency_remote_dns(&settings),
        "https://dns.alidns.com/dns-query"
    );
}

#[test]
fn temp_node_detour_resolves_cross_profile_chain() {
    let target = serde_json::json!({
        "type": "trojan",
        "tag": "target",
        "server": "target.example.com",
        "server_port": 443,
        "password": "target",
        "detour": "profile-b::front"
    });
    let profile_nodes = std::collections::HashMap::from([
        ("profile-a".to_string(), vec![target.clone()]),
        (
            "profile-b".to_string(),
            vec![
                serde_json::json!({
                    "type": "socks",
                    "tag": "front",
                    "server": "front.example.com",
                    "server_port": 1080,
                    "detour": "hop"
                }),
                serde_json::json!({
                    "type": "http",
                    "tag": "hop",
                    "server": "hop.example.com",
                    "server_port": 8080
                }),
            ],
        ),
    ]);

    let (prepared, dependencies) =
        prepare_temp_nodes_with_detours("profile-a", &[target], &profile_nodes).unwrap();

    assert_eq!(prepared[0]["detour"].as_str(), Some("kb-detour-0"));
    assert_eq!(dependencies.len(), 2);
    let front = dependencies
        .iter()
        .find(|node| node["server"].as_str() == Some("front.example.com"))
        .unwrap();
    let hop = dependencies
        .iter()
        .find(|node| node["server"].as_str() == Some("hop.example.com"))
        .unwrap();
    assert_eq!(front["tag"].as_str(), Some("kb-detour-0"));
    assert_eq!(front["detour"].as_str(), hop["tag"].as_str());
}

#[test]
fn temp_node_detour_rejects_metered_dependency() {
    let target = serde_json::json!({
        "type": "trojan",
        "tag": "target",
        "server": "target.example.com",
        "server_port": 443,
        "password": "target",
        "detour": "metered"
    });
    let metered = serde_json::json!({
        "type": "socks",
        "tag": "metered",
        "server": "metered.example.com",
        "server_port": 1080,
        "x_kunbox_metered_protected": true
    });
    let profile_nodes =
        std::collections::HashMap::from([("profile-a".to_string(), vec![target.clone(), metered])]);

    let error =
        prepare_temp_nodes_with_detours("profile-a", &[target], &profile_nodes).unwrap_err();

    assert_eq!(error, "高价计费保护节点不能作为前置代理");
}

#[test]
fn temp_xhttp_node_detour_builds_non_null_chain_route() {
    let target = serde_json::json!({
        "type": "vless",
        "tag": "xhttp-target",
        "server": "target.example.com",
        "server_port": 443,
        "uuid": "00000000-0000-0000-0000-000000000000",
        "detour": "front",
        "transport": { "type": "xhttp", "path": "/proxy" }
    });
    let front = serde_json::json!({
        "type": "socks",
        "tag": "front",
        "server": "front.example.com",
        "server_port": 1080
    });
    let profile_nodes =
        std::collections::HashMap::from([("profile-a".to_string(), vec![target.clone(), front])]);
    let (prepared, dependencies) =
        prepare_temp_nodes_with_detours("profile-a", &[target], &profile_nodes).unwrap();

    let (config, _, _, specs) = generate_temp_config_with_dependencies_raw(
        &prepared,
        &dependencies,
        TEMP_SINGBOX_PORT,
        true,
        TEMP_LATENCY_FALLBACK_REMOTE_DNS,
    );

    assert_eq!(specs[0]["frontProxyTag"].as_str(), Some("kb-detour-0"));
    assert!(config["route"]["rules"]
        .as_array()
        .unwrap()
        .iter()
        .any(|rule| {
            rule["inbound"].as_str() == Some("kunbox-front-proxy-chain-in")
                && rule["outbound"].as_str() == Some("kb-detour-0")
        }));
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

    let (config, tag_map, proxy_port_map, _) = generate_temp_config_raw(
        &nodes,
        TEMP_SINGBOX_PORT,
        true,
        TEMP_LATENCY_FALLBACK_REMOTE_DNS,
    );
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
    assert_eq!(
        tag_map.get("节点🚀"),
        Some(&vec![
            "latency-0000".to_string(),
            "latency-0001".to_string()
        ])
    );
    assert_eq!(
        tag_map.get("中文😀"),
        Some(&vec!["latency-0002".to_string()])
    );
    assert_eq!(
        proxy_port_map.get("节点🚀"),
        Some(&vec![
            TEMP_PROXY_INBOUND_PORT_BASE,
            TEMP_PROXY_INBOUND_PORT_BASE + 1
        ])
    );
    assert_eq!(
        proxy_port_map.get("中文😀"),
        Some(&vec![TEMP_PROXY_INBOUND_PORT_BASE + 2])
    );

    let inbounds = config
        .get("inbounds")
        .and_then(|value| value.as_array())
        .expect("expected inbounds");
    assert_eq!(inbounds.len(), 3);
    assert_eq!(
        inbounds[0].get("type").and_then(|value| value.as_str()),
        Some("mixed")
    );
    assert_eq!(
        inbounds[0].get("tag").and_then(|value| value.as_str()),
        Some("latency-in-0000")
    );
    assert_eq!(
        inbounds[0].get("listen").and_then(|value| value.as_str()),
        Some("127.0.0.1")
    );
    assert_eq!(
        inbounds[0]
            .get("listen_port")
            .and_then(|value| value.as_u64()),
        Some(TEMP_PROXY_INBOUND_PORT_BASE as u64)
    );

    let rules = config
        .get("route")
        .and_then(|value| value.get("rules"))
        .and_then(|value| value.as_array())
        .expect("expected route rules");
    assert!(rules.iter().any(|rule| {
        rule.get("inbound")
            .and_then(|value| value.as_array())
            .is_some_and(|inbounds| {
                inbounds
                    .iter()
                    .any(|inbound| inbound.as_str() == Some("latency-in-0000"))
            })
            && rule.get("outbound").and_then(|value| value.as_str()) == Some("latency-0000")
    }));
}

#[test]
fn generate_temp_config_removes_naive_unsupported_fields_and_uses_remote_dns() {
    let nodes = vec![serde_json::json!({
        "type": "naive",
        "tag": "Naive H2",
        "server": "naive.example.com",
        "server_port": 443,
        "username": "user",
        "password": "pass",
        "network": "h2"
    })];

    let (config, tag_map, _, _) = generate_temp_config_raw(
        &nodes,
        TEMP_SINGBOX_PORT,
        true,
        "https://dns.google/dns-query",
    );
    let outbound = config
        .get("outbounds")
        .and_then(|value| value.as_array())
        .and_then(|outbounds| outbounds.first())
        .expect("expected naive outbound");

    assert_eq!(
        outbound.get("type").and_then(|value| value.as_str()),
        Some("naive")
    );
    assert_eq!(
        outbound.get("tag").and_then(|value| value.as_str()),
        Some("latency-0000")
    );
    assert!(outbound.get("network").is_none());
    assert!(outbound.get("transport").is_none());
    assert_eq!(
        outbound.get("quic").and_then(|value| value.as_bool()),
        Some(false)
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
        tag_map.get("Naive H2"),
        Some(&vec!["latency-0000".to_string()])
    );

    let dns_servers = config
        .get("dns")
        .and_then(|value| value.get("servers"))
        .and_then(|value| value.as_array())
        .expect("expected dns servers");
    let bootstrap_dns = dns_servers
        .iter()
        .find(|server| server.get("tag").and_then(|value| value.as_str()) == Some("dns-bootstrap"))
        .expect("expected dns-bootstrap");
    assert_eq!(
        bootstrap_dns.get("type").and_then(|value| value.as_str()),
        Some("https")
    );
    assert_eq!(
        bootstrap_dns.get("server").and_then(|value| value.as_str()),
        Some("223.5.5.5")
    );
    let remote_dns = dns_servers
        .iter()
        .find(|server| server.get("tag").and_then(|value| value.as_str()) == Some("dns-remote"))
        .expect("expected dns-remote");

    assert_eq!(
        remote_dns.get("type").and_then(|value| value.as_str()),
        Some("https")
    );
    assert_eq!(
        remote_dns.get("server").and_then(|value| value.as_str()),
        Some("dns.google")
    );
    assert_eq!(
        remote_dns.get("path").and_then(|value| value.as_str()),
        Some("/dns-query")
    );
    assert_eq!(
        remote_dns
            .get("domain_resolver")
            .and_then(|value| value.as_str()),
        Some("dns-bootstrap")
    );
    assert_eq!(remote_dns.get("detour"), None);
    assert_eq!(
        config
            .get("dns")
            .and_then(|value| value.get("strategy"))
            .and_then(|value| value.as_str()),
        Some("ipv4_only")
    );
    assert_eq!(
        config
            .get("dns")
            .and_then(|value| value.get("independent_cache"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
}

#[test]
fn generate_temp_config_does_not_add_domain_resolver_for_ip_dns_server() {
    let nodes = vec![serde_json::json!({
        "type": "trojan",
        "tag": "Trojan",
        "server": "trojan.example.com",
        "server_port": 443,
        "password": "pass"
    })];

    let (config, _, _, _) = generate_temp_config_raw(&nodes, TEMP_SINGBOX_PORT, true, "223.5.5.5");
    let dns_servers = config
        .get("dns")
        .and_then(|value| value.get("servers"))
        .and_then(|value| value.as_array())
        .expect("expected dns servers");
    let remote_dns = dns_servers
        .iter()
        .find(|server| server.get("tag").and_then(|value| value.as_str()) == Some("dns-remote"))
        .expect("expected dns-remote");

    assert_eq!(
        remote_dns.get("server").and_then(|value| value.as_str()),
        Some("223.5.5.5")
    );
    assert!(remote_dns.get("domain_resolver").is_none());
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

    let (config, tag_map, _, _) = generate_temp_config_raw(
        &nodes,
        TEMP_SINGBOX_PORT,
        false,
        TEMP_LATENCY_FALLBACK_REMOTE_DNS,
    );
    let outbounds = config
        .get("outbounds")
        .and_then(|value| value.as_array())
        .expect("expected outbounds");

    assert!(outbounds.iter().all(|outbound| {
        outbound.get("type").and_then(|value| value.as_str()) != Some("naive")
    }));
    assert_eq!(tag_map.get("Naive"), None);
    assert_eq!(
        tag_map.get("Trojan"),
        Some(&vec!["latency-0000".to_string()])
    );
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

    let (config, _, _, _) = generate_temp_config_raw(
        &nodes,
        TEMP_SINGBOX_PORT,
        true,
        TEMP_LATENCY_FALLBACK_REMOTE_DNS,
    );

    let dns = config
        .get("dns")
        .and_then(|value| value.as_object())
        .expect("expected dns config");
    let servers = dns
        .get("servers")
        .and_then(|value| value.as_array())
        .expect("expected dns servers");
    let remote = servers
        .iter()
        .find(|server| server.get("tag").and_then(|value| value.as_str()) == Some("dns-remote"))
        .expect("expected dns-remote server");

    assert_eq!(
        remote.get("type").and_then(|value| value.as_str()),
        Some("https")
    );
    assert_eq!(
        remote.get("server").and_then(|value| value.as_str()),
        Some("dns.alidns.com")
    );
    assert_eq!(
        remote.get("server_port").and_then(|value| value.as_u64()),
        Some(443)
    );
    assert_eq!(
        remote.get("path").and_then(|value| value.as_str()),
        Some("/dns-query")
    );
    assert_eq!(
        remote
            .get("domain_resolver")
            .and_then(|value| value.as_str()),
        Some("dns-bootstrap")
    );
    assert_eq!(
        config
            .get("route")
            .and_then(|value| value.get("default_domain_resolver"))
            .and_then(|value| value.as_str()),
        Some("dns-bootstrap")
    );
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
    assert!(
        slot.is_none(),
        "TEMP_SINGBOX_PROCESS should be None after cleanup"
    );
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

    assert_eq!(
        take_temp_singbox_tag_for_batch(Some(99), "dup")
            .await
            .as_deref(),
        Some("latency-0000")
    );
    assert_eq!(
        take_temp_singbox_tag_for_batch(Some(99), "dup")
            .await
            .as_deref(),
        Some("latency-0001")
    );
    assert_eq!(take_temp_singbox_tag_for_batch(Some(99), "dup").await, None);
}

#[test]
fn temp_start_reuses_responsive_process_during_concurrent_batch() {
    assert_eq!(
        running_temp_process_action(true, 2),
        RunningTempProcessAction::Reuse
    );
    assert_eq!(
        running_temp_process_action(true, 1),
        RunningTempProcessAction::Rebuild
    );
    assert_eq!(
        running_temp_process_action(false, 2),
        RunningTempProcessAction::Rebuild
    );
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
