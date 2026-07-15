use crate::types::SingBoxOutbound;

pub(crate) const ECH_DNS_SERVER_META_KEY: &str = "x_kunbox_ech_dns_server";

mod links;
mod quic;
#[cfg(test)]
mod tests;
mod vless;

use links::map_clash_type;
pub(super) use links::{export_node_to_link, parse_node_link};

pub(super) fn normalize_duplicate_node_tags(nodes: Vec<SingBoxOutbound>) -> Vec<SingBoxOutbound> {
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

pub(super) fn parse_subscription_content(content: &str) -> Result<Vec<SingBoxOutbound>, String> {
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

pub(super) fn decode_base64_compat(input: &str) -> Option<Vec<u8>> {
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

pub(super) fn parse_port_value(value: &serde_json::Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(parse_u16_port)
        .or_else(|| value.as_str().and_then(|s| s.parse::<u16>().ok()))
}

pub(super) fn parse_host_port(host_port: &str) -> Option<(String, u16)> {
    let host_port = host_port
        .split_once('?')
        .map(|(value, _)| value)
        .unwrap_or(host_port);
    let url = url::Url::parse(&format!("tcp://{}", host_port)).ok()?;
    Some((url.host_str()?.to_string(), url.port()?))
}

pub(super) fn parse_bool_param(value: &str) -> bool {
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
                extra.insert(
                    "username".to_string(),
                    serde_json::Value::String(username.to_string()),
                );
            }
            if let Some(pwd) = p.get("password").and_then(|v| v.as_str()) {
                extra.insert(
                    "password".to_string(),
                    serde_json::Value::String(pwd.to_string()),
                );
            }
            if let Some(uuid) = p.get("uuid").and_then(|v| v.as_str()) {
                extra.insert(
                    "uuid".to_string(),
                    serde_json::Value::String(uuid.to_string()),
                );
            }
            if let Some(flow) = p.get("flow").and_then(|v| v.as_str()) {
                extra.insert(
                    "flow".to_string(),
                    serde_json::Value::String(flow.to_string()),
                );
            }

            // Method only for shadowsocks
            if proxy_type == "shadowsocks" || proxy_type == "shadowsocksr" {
                if let Some(method) = p.get("method").or(p.get("cipher")).and_then(|v| v.as_str()) {
                    extra.insert(
                        "method".to_string(),
                        serde_json::Value::String(method.to_string()),
                    );
                }
            }

            // VMess specific
            if proxy_type == "vmess" {
                extra.insert(
                    "security".to_string(),
                    serde_json::Value::String(
                        p.get("cipher")
                            .and_then(|v| v.as_str())
                            .unwrap_or("auto")
                            .to_string(),
                    ),
                );
                if let Some(aid) = p.get("alterId").and_then(|v| v.as_u64()) {
                    extra.insert(
                        "alter_id".to_string(),
                        serde_json::Value::Number(aid.into()),
                    );
                }
            }

            // VLESS specific
            if proxy_type == "vless" {
                extra.insert(
                    "packet_encoding".to_string(),
                    serde_json::Value::String("xudp".to_string()),
                );
                if let Some(encryption) = p
                    .get("extra")
                    .and_then(|value| value.get("encryption"))
                    .and_then(|value| value.as_str())
                    .filter(|value| !value.is_empty())
                {
                    extra.insert(
                        "encryption".to_string(),
                        serde_json::Value::String(encryption.to_string()),
                    );
                }
            }

            // TLS configuration
            let network = p.get("network").and_then(|v| v.as_str()).unwrap_or("tcp");
            let tls_enabled = p.get("tls").and_then(|v| v.as_bool()).unwrap_or(false);
            let servername = p
                .get("servername")
                .or(p.get("sni"))
                .and_then(|v| v.as_str());
            let skip_cert = p
                .get("skip-cert-verify")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if proxy_type == "naive" {
                extra.insert(
                    "tls".to_string(),
                    serde_json::json!({
                        "enabled": true,
                        "server_name": servername.unwrap_or(&server),
                        "insecure": skip_cert
                    }),
                );
            } else if tls_enabled
                || network == "ws"
                || network == "grpc"
                || network == "h2"
                || proxy_type == "hysteria2"
                || proxy_type == "hysteria"
                || proxy_type == "tuic"
                || proxy_type == "anytls"
            {
                let mut tls = serde_json::Map::new();
                tls.insert("enabled".to_string(), serde_json::Value::Bool(true));
                tls.insert(
                    "server_name".to_string(),
                    serde_json::Value::String(servername.unwrap_or(&server).to_string()),
                );
                tls.insert("insecure".to_string(), serde_json::Value::Bool(skip_cert));

                // ALPN
                if let Some(alpn) = p.get("alpn").and_then(|v| v.as_array()) {
                    tls.insert("alpn".to_string(), serde_json::Value::Array(alpn.clone()));
                }

                // Client fingerprint (uTLS)
                if let Some(fp) = p.get("client-fingerprint").and_then(|v| v.as_str()) {
                    tls.insert(
                        "utls".to_string(),
                        serde_json::json!({
                            "enabled": true,
                            "fingerprint": fp
                        }),
                    );
                }

                // Reality
                if let Some(reality_opts) = p.get("reality-opts").and_then(|v| v.as_object()) {
                    let mut reality = serde_json::Map::new();
                    reality.insert("enabled".to_string(), serde_json::Value::Bool(true));
                    if let Some(pk) = reality_opts.get("public-key").and_then(|v| v.as_str()) {
                        reality.insert(
                            "public_key".to_string(),
                            serde_json::Value::String(pk.to_string()),
                        );
                    }
                    if let Some(sid) = reality_opts.get("short-id").and_then(|v| v.as_str()) {
                        reality.insert(
                            "short_id".to_string(),
                            serde_json::Value::String(sid.to_string()),
                        );
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
                    transport.insert(
                        "type".to_string(),
                        serde_json::Value::String("ws".to_string()),
                    );

                    let mut path = ws_opts
                        .and_then(|o| o.get("path"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("/")
                        .to_string();

                    if let Some(ed_pos) = path.find("?ed=") {
                        let ed_str = &path[ed_pos + 4..];
                        if let Ok(ed) = ed_str.parse::<u32>() {
                            transport.insert(
                                "max_early_data".to_string(),
                                serde_json::Value::Number(ed.into()),
                            );
                            transport.insert(
                                "early_data_header_name".to_string(),
                                serde_json::Value::String("Sec-WebSocket-Protocol".to_string()),
                            );
                        }
                        path = path[..ed_pos].to_string();
                    }

                    transport.insert("path".to_string(), serde_json::Value::String(path));

                    if let Some(headers) = ws_opts
                        .and_then(|o| o.get("headers"))
                        .and_then(|v| v.as_object())
                    {
                        transport.insert(
                            "headers".to_string(),
                            serde_json::Value::Object(headers.clone()),
                        );
                    }

                    extra.insert(
                        "transport".to_string(),
                        serde_json::Value::Object(transport),
                    );
                }
                "grpc" => {
                    let grpc_opts = p.get("grpc-opts").and_then(|v| v.as_object());
                    let mut transport = serde_json::Map::new();
                    transport.insert(
                        "type".to_string(),
                        serde_json::Value::String("grpc".to_string()),
                    );
                    if let Some(sn) = grpc_opts
                        .and_then(|o| o.get("grpc-service-name"))
                        .and_then(|v| v.as_str())
                    {
                        transport.insert(
                            "service_name".to_string(),
                            serde_json::Value::String(sn.to_string()),
                        );
                    }
                    extra.insert(
                        "transport".to_string(),
                        serde_json::Value::Object(transport),
                    );
                }
                "h2" => {
                    let h2_opts = p.get("h2-opts").and_then(|v| v.as_object());
                    let mut transport = serde_json::Map::new();
                    transport.insert(
                        "type".to_string(),
                        serde_json::Value::String("http".to_string()),
                    );
                    if let Some(path) = h2_opts
                        .and_then(|o| o.get("path"))
                        .and_then(|v| v.as_array())
                    {
                        let path_str = path
                            .iter()
                            .filter_map(|v| v.as_str())
                            .collect::<Vec<_>>()
                            .join(",");
                        transport.insert("path".to_string(), serde_json::Value::String(path_str));
                    }
                    if let Some(host) = h2_opts.and_then(|o| o.get("host")) {
                        transport.insert("host".to_string(), host.clone());
                    }
                    extra.insert(
                        "transport".to_string(),
                        serde_json::Value::Object(transport),
                    );
                }
                "xhttp" => {
                    let xhttp_opts = p.get("xhttp-opts").and_then(|v| v.as_object());
                    let mut transport = serde_json::Map::new();
                    transport.insert(
                        "type".to_string(),
                        serde_json::Value::String("xhttp".to_string()),
                    );

                    if let Some(path) = xhttp_opts
                        .and_then(|o| o.get("path"))
                        .and_then(|v| v.as_str())
                    {
                        transport.insert(
                            "path".to_string(),
                            serde_json::Value::String(path.to_string()),
                        );
                    }
                    if let Some(mode) = xhttp_opts
                        .and_then(|o| o.get("mode"))
                        .and_then(|v| v.as_str())
                    {
                        transport.insert(
                            "mode".to_string(),
                            serde_json::Value::String(mode.to_string()),
                        );
                    }
                    if let Some(extra_field) = xhttp_opts.and_then(|o| o.get("extra")) {
                        if let Some(extra_obj) = extra_field.as_object() {
                            let mut transport_extra = extra_obj.clone();
                            transport_extra.remove("encryption");
                            if !transport_extra.is_empty() {
                                transport.insert(
                                    "extra".to_string(),
                                    serde_json::Value::Object(transport_extra),
                                );
                            }
                        } else {
                            transport.insert("extra".to_string(), extra_field.clone());
                        }
                    }
                    if let Some(headers) = xhttp_opts
                        .and_then(|o| o.get("headers"))
                        .and_then(|v| v.as_object())
                    {
                        transport.insert(
                            "headers".to_string(),
                            serde_json::Value::Object(headers.clone()),
                        );
                    }

                    extra.insert(
                        "transport".to_string(),
                        serde_json::Value::Object(transport),
                    );
                }
                _ => {}
            }

            if proxy_type == "naive" {
                if let Some(udp_over_tcp) = p.get("udp-over-tcp").and_then(|v| v.as_bool()) {
                    extra.insert(
                        "udp_over_tcp".to_string(),
                        serde_json::Value::Bool(udp_over_tcp),
                    );
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

fn parse_singbox_outbounds(
    outbounds: &[serde_json::Value],
) -> Result<Vec<SingBoxOutbound>, String> {
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
