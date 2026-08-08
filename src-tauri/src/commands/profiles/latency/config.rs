use super::*;

#[cfg(test)]
pub(super) fn generate_temp_config_raw(
    nodes: &[serde_json::Value],
    api_port: u16,
    naive_runtime_available: bool,
    remote_dns: &str,
) -> (
    serde_json::Value,
    std::collections::HashMap<String, Vec<String>>,
    std::collections::HashMap<String, Vec<u16>>,
    Vec<serde_json::Value>,
) {
    generate_temp_config_with_dependencies_raw(
        nodes,
        &[],
        api_port,
        naive_runtime_available,
        remote_dns,
    )
}

pub(super) fn generate_temp_config_with_dependencies_raw(
    nodes: &[serde_json::Value],
    detour_dependencies: &[serde_json::Value],
    api_port: u16,
    naive_runtime_available: bool,
    remote_dns: &str,
) -> (
    serde_json::Value,
    std::collections::HashMap<String, Vec<String>>,
    std::collections::HashMap<String, Vec<u16>>,
    Vec<serde_json::Value>,
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
        .unwrap_or_else(|| remote_dns.to_string());
    let remote_dns_detour = "direct";
    let remote_dns_domain_resolver =
        temp_dns_domain_resolver(&effective_remote_dns, active_node_has_ech);

    // 处理节点，移除不合法字段并添加必要配置
    let mut tag_map = std::collections::HashMap::new();
    let mut proxy_port_map = std::collections::HashMap::new();
    let mut plugin_bridge_specs = Vec::new();
    let mut inbounds = Vec::new();
    let mut route_rules = vec![serde_json::json!({ "protocol": "dns", "outbound": "direct" })];
    let mut outbounds: Vec<serde_json::Value> = nodes
        .iter()
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
            let original_node_tag = node
                .get("tag")
                .and_then(|tag| tag.as_str())
                .unwrap_or("")
                .to_string();
            let bridge_index = plugin_bridge_specs.len();
            let processed_node = crate::commands::singbox::node_for_singbox_with_plugin_bridge(
                node,
                &mut plugin_bridge_specs,
            );
            let mut node = processed_node.clone();
            if plugin_bridge_specs.len() > bridge_index {
                let bridge_port = temp_xray_bridge_port(bridge_index);
                if let Some(spec) = plugin_bridge_specs
                    .last_mut()
                    .and_then(|spec| spec.as_object_mut())
                {
                    spec.insert("port".to_string(), serde_json::json!(bridge_port));
                }
                if let Some(obj) = node.as_object_mut() {
                    obj.insert("server_port".to_string(), serde_json::json!(bridge_port));
                }
            }

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
                let original_tag = original_node_tag;
                let temp_tag = make_temp_latency_tag(index);
                let inbound_tag = make_temp_latency_inbound_tag(index);
                let proxy_port = temp_proxy_inbound_port(index);

                obj.insert(
                    "tag".to_string(),
                    serde_json::Value::String(temp_tag.clone()),
                );
                if !original_tag.is_empty() {
                    tag_map
                        .entry(original_tag.clone())
                        .or_insert_with(Vec::new)
                        .push(temp_tag.clone());
                    proxy_port_map
                        .entry(original_tag)
                        .or_insert_with(Vec::new)
                        .push(proxy_port);
                }
                inbounds.push(serde_json::json!({
                    "type": "mixed",
                    "tag": inbound_tag.clone(),
                    "listen": "127.0.0.1",
                    "listen_port": proxy_port
                }));
                route_rules.push(serde_json::json!({
                    "inbound": [inbound_tag],
                    "outbound": temp_tag
                }));

                // vless/vmess/trojan 不需要 method 字段
                if node_type != "shadowsocks" && node_type != "shadowsocksr" {
                    obj.remove("method");
                }

                // 为需要 TLS 的节点添加配置
                if !obj.contains_key("tls") {
                    match node_type.as_str() {
                        "hysteria2" | "hysteria" | "tuic" => {
                            // 这些协议必须使用 TLS
                            obj.insert(
                                "tls".to_string(),
                                serde_json::json!({
                                    "enabled": true,
                                    "server_name": server,
                                    "insecure": false
                                }),
                            );
                        }
                        "vless" | "vmess" | "trojan" => {
                            // 443 端口通常需要 TLS
                            if port == 443 || port == 8443 || port == 2053 {
                                obj.insert(
                                    "tls".to_string(),
                                    serde_json::json!({
                                        "enabled": true,
                                        "server_name": server,
                                        "insecure": false
                                    }),
                                );
                            }
                        }
                        _ => {}
                    }
                }

                // vless 需要 packet_encoding
                if node_type == "vless" && !obj.contains_key("packet_encoding") {
                    obj.insert(
                        "packet_encoding".to_string(),
                        serde_json::Value::String("xudp".to_string()),
                    );
                }

                apply_temp_latency_domain_resolver(obj, "dns-bootstrap");
            }
            node
        })
        .collect();

    for dependency in detour_dependencies.iter().filter(|node| {
        node.get("type")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|node_type| {
                crate::commands::singbox::is_proxy_type(node_type)
                    && (node_type != "naive" || naive_runtime_available)
            })
    }) {
        let bridge_index = plugin_bridge_specs.len();
        let mut dependency = crate::commands::singbox::node_for_singbox_with_plugin_bridge(
            dependency,
            &mut plugin_bridge_specs,
        );
        if plugin_bridge_specs.len() > bridge_index {
            let bridge_port = temp_xray_bridge_port(bridge_index);
            if let Some(spec) = plugin_bridge_specs
                .last_mut()
                .and_then(|spec| spec.as_object_mut())
            {
                spec.insert("port".to_string(), serde_json::json!(bridge_port));
            }
            if let Some(object) = dependency.as_object_mut() {
                object.insert("server_port".to_string(), serde_json::json!(bridge_port));
            }
        }
        outbounds.push(dependency);
    }

    let mut chain_routes = std::collections::HashSet::new();
    for spec in &plugin_bridge_specs {
        let Some(chain_port) = spec
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
        if !chain_routes.insert((chain_port, outbound_tag.to_string())) {
            continue;
        }
        let inbound_tag = if chain_routes.len() == 1 {
            "kunbox-front-proxy-chain-in".to_string()
        } else {
            format!("kunbox-front-proxy-chain-in-{}", chain_routes.len())
        };
        inbounds.push(serde_json::json!({
            "type": "mixed",
            "tag": inbound_tag,
            "listen": "127.0.0.1",
            "listen_port": chain_port
        }));
        route_rules.insert(
            0,
            serde_json::json!({
                "inbound": inbound_tag,
                "outbound": outbound_tag
            }),
        );
    }

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
                    crate::commands::singbox::build_dns_bootstrap_server(),
                    crate::commands::singbox::build_dns_server("local", "dns-local", "direct"),
                    crate::commands::singbox::build_dns_server_with_resolver(
                        &effective_remote_dns,
                        "dns-remote",
                        remote_dns_detour,
                        remote_dns_domain_resolver,
                    )
                ],
                "strategy": "ipv4_only",
                "independent_cache": true
            },
            "inbounds": inbounds,
            "outbounds": outbounds,
            "route": {
                "rules": route_rules,
                "final": "direct",
                "auto_detect_interface": true,
                "default_domain_resolver": if active_node_has_ech { "dns-bootstrap" } else { "dns-remote" }
            }
        }),
        tag_map,
        proxy_port_map,
        plugin_bridge_specs,
    )
}
