use crate::types::SingBoxOutbound;

use super::quic::{parse_hysteria2_link, parse_hysteria_link, parse_tuic_link};
use super::vless::parse_vless_link;
use super::{decode_base64_compat, parse_bool_param, parse_host_port, parse_port_value};

pub(crate) fn parse_node_link(link: &str) -> Option<SingBoxOutbound> {
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
    } else if link.starts_with("anytls://") {
        parse_anytls_link(link)
    } else if link.starts_with("naive+") {
        parse_naive_link(link)
    } else {
        None
    }
}

pub(super) fn parse_ss_link(link: &str) -> Option<SingBoxOutbound> {
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
    extra.insert(
        "method".to_string(),
        serde_json::Value::String(method.to_string()),
    );
    extra.insert(
        "password".to_string(),
        serde_json::Value::String(password.to_string()),
    );

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("shadowsocks".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}

pub(super) fn parse_vmess_link(link: &str) -> Option<SingBoxOutbound> {
    let rest = link.strip_prefix("vmess://")?;
    let decoded = decode_base64_compat(rest.trim())?;
    let decoded_str = String::from_utf8(decoded).ok()?;
    let json: serde_json::Value = serde_json::from_str(&decoded_str).ok()?;

    let tag = json
        .get("ps")
        .and_then(|v| v.as_str())
        .unwrap_or("VMess")
        .to_string();
    let server = json.get("add").and_then(|v| v.as_str())?.to_string();
    let port = json.get("port").and_then(parse_port_value)?;
    let uuid = json.get("id").and_then(|v| v.as_str())?.to_string();

    let mut extra = std::collections::HashMap::new();
    extra.insert("uuid".to_string(), serde_json::Value::String(uuid));
    extra.insert(
        "security".to_string(),
        serde_json::Value::String(
            json.get("scy")
                .or(json.get("cipher"))
                .and_then(|v| v.as_str())
                .unwrap_or("auto")
                .to_string(),
        ),
    );

    if let Some(aid) = json.get("aid").and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    }) {
        extra.insert(
            "alter_id".to_string(),
            serde_json::Value::Number(aid.into()),
        );
    }

    // TLS
    let tls = json.get("tls").and_then(|v| v.as_str()).unwrap_or("");
    if tls == "tls" {
        let mut tls_obj = serde_json::Map::new();
        tls_obj.insert("enabled".to_string(), serde_json::Value::Bool(true));
        if let Some(sni) = json.get("sni").and_then(|v| v.as_str()) {
            tls_obj.insert(
                "server_name".to_string(),
                serde_json::Value::String(sni.to_string()),
            );
        } else {
            tls_obj.insert(
                "server_name".to_string(),
                serde_json::Value::String(server.clone()),
            );
        }
        extra.insert("tls".to_string(), serde_json::Value::Object(tls_obj));
    }

    // Transport
    let net = json.get("net").and_then(|v| v.as_str()).unwrap_or("tcp");
    if net != "tcp" {
        let mut transport = serde_json::Map::new();
        match net {
            "ws" => {
                transport.insert(
                    "type".to_string(),
                    serde_json::Value::String("ws".to_string()),
                );
                if let Some(path) = json.get("path").and_then(|v| v.as_str()) {
                    transport.insert(
                        "path".to_string(),
                        serde_json::Value::String(path.to_string()),
                    );
                }
                if let Some(host) = json.get("host").and_then(|v| v.as_str()) {
                    transport.insert("headers".to_string(), serde_json::json!({ "Host": host }));
                }
            }
            "grpc" => {
                transport.insert(
                    "type".to_string(),
                    serde_json::Value::String("grpc".to_string()),
                );
                if let Some(path) = json.get("path").and_then(|v| v.as_str()) {
                    transport.insert(
                        "service_name".to_string(),
                        serde_json::Value::String(path.to_string()),
                    );
                }
            }
            "h2" => {
                transport.insert(
                    "type".to_string(),
                    serde_json::Value::String("http".to_string()),
                );
                if let Some(path) = json.get("path").and_then(|v| v.as_str()) {
                    transport.insert(
                        "path".to_string(),
                        serde_json::Value::String(path.to_string()),
                    );
                }
            }
            _ => {
                transport.insert(
                    "type".to_string(),
                    serde_json::Value::String(net.to_string()),
                );
            }
        }
        extra.insert(
            "transport".to_string(),
            serde_json::Value::Object(transport),
        );
    }

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("vmess".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}

pub(super) fn parse_trojan_link(link: &str) -> Option<SingBoxOutbound> {
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
        .map(|(k, v)| {
            (
                k.to_string(),
                urlencoding::decode(v).unwrap_or_default().to_string(),
            )
        })
        .collect();

    let mut extra = std::collections::HashMap::new();
    extra.insert("password".to_string(), serde_json::Value::String(password));

    // TLS (Trojan always uses TLS)
    let mut tls = serde_json::Map::new();
    tls.insert("enabled".to_string(), serde_json::Value::Bool(true));
    if let Some(sni) = params.get("sni") {
        tls.insert(
            "server_name".to_string(),
            serde_json::Value::String(sni.clone()),
        );
    } else {
        tls.insert(
            "server_name".to_string(),
            serde_json::Value::String(server.clone()),
        );
    }
    if params
        .get("allowInsecure")
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        tls.insert("insecure".to_string(), serde_json::Value::Bool(true));
    }
    extra.insert("tls".to_string(), serde_json::Value::Object(tls));

    // Transport
    let transport_type = params.get("type").map(|s| s.as_str()).unwrap_or("tcp");
    if transport_type != "tcp" {
        let mut transport = serde_json::Map::new();
        transport.insert(
            "type".to_string(),
            serde_json::Value::String(transport_type.to_string()),
        );

        if transport_type == "ws" {
            if let Some(path) = params.get("path") {
                transport.insert("path".to_string(), serde_json::Value::String(path.clone()));
            }
            if let Some(host) = params.get("host") {
                transport.insert("headers".to_string(), serde_json::json!({ "Host": host }));
            }
        } else if transport_type == "grpc" {
            if let Some(sn) = params.get("serviceName") {
                transport.insert(
                    "service_name".to_string(),
                    serde_json::Value::String(sn.clone()),
                );
            }
        }

        extra.insert(
            "transport".to_string(),
            serde_json::Value::Object(transport),
        );
    }

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("trojan".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}

fn parse_anytls_link(link: &str) -> Option<SingBoxOutbound> {
    let url = url::Url::parse(link).ok()?;
    let tag = url
        .fragment()
        .filter(|value| !value.is_empty())
        .and_then(|value| {
            urlencoding::decode(value)
                .ok()
                .map(|value| value.to_string())
        })
        .unwrap_or_else(|| "AnyTLS".to_string());
    let server = url.host_str()?.to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    let password = urlencoding::decode(url.username()).ok()?.to_string();
    if password.is_empty() {
        return None;
    }

    let params: std::collections::HashMap<String, String> = url
        .query_pairs()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();

    let mut tls = serde_json::Map::new();
    tls.insert("enabled".to_string(), serde_json::Value::Bool(true));
    tls.insert(
        "server_name".to_string(),
        serde_json::Value::String(
            params
                .get("sni")
                .or_else(|| params.get("servername"))
                .filter(|value| !value.is_empty())
                .cloned()
                .unwrap_or_else(|| server.clone()),
        ),
    );
    tls.insert(
        "insecure".to_string(),
        serde_json::Value::Bool(
            params
                .get("insecure")
                .or_else(|| params.get("allow_insecure"))
                .or_else(|| params.get("allowInsecure"))
                .or_else(|| params.get("skip-cert-verify"))
                .is_some_and(|value| parse_bool_param(value)),
        ),
    );

    let mut extra = std::collections::HashMap::new();
    extra.insert("password".to_string(), serde_json::Value::String(password));
    extra.insert("tls".to_string(), serde_json::Value::Object(tls));

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("anytls".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}

pub(super) fn map_clash_type(t: &str) -> String {
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
    }
    .to_string()
}

fn parse_naive_link(link: &str) -> Option<SingBoxOutbound> {
    let rest = link.strip_prefix("naive+")?;
    let (main_part, tag_part) = rest.split_once('#').unwrap_or((rest, "Naive"));
    let tag = urlencoding::decode(tag_part).ok()?.to_string();

    let url = url::Url::parse(main_part).ok()?;
    let host = url.host_str()?.to_string();
    let port = url.port_or_known_default().unwrap_or(443) as u16;
    let username = urlencoding::decode(url.username()).ok()?.to_string();
    let password = url
        .password()
        .and_then(|p| urlencoding::decode(p).ok().map(|s| s.to_string()))
        .unwrap_or_default();

    let mut extra = std::collections::HashMap::new();
    extra.insert("username".to_string(), serde_json::Value::String(username));
    extra.insert("password".to_string(), serde_json::Value::String(password));

    if let Some(q) = url.query() {
        for (k, v) in url::form_urlencoded::parse(q.as_bytes()) {
            match k.as_ref() {
                "sni" => {
                    extra.insert(
                        "tls".to_string(),
                        serde_json::json!({
                            "enabled": true,
                            "server_name": v.to_string(),
                            "insecure": false
                        }),
                    );
                }
                "insecure" => {
                    let insecure = v == "1" || v.eq_ignore_ascii_case("true");
                    let tls_value = extra
                        .remove("tls")
                        .unwrap_or_else(|| serde_json::json!({ "enabled": true }));
                    let mut tls_obj = tls_value.as_object().cloned().unwrap_or_default();
                    tls_obj.insert("enabled".to_string(), serde_json::Value::Bool(true));
                    tls_obj.insert("insecure".to_string(), serde_json::Value::Bool(insecure));
                    if !tls_obj.contains_key("server_name") {
                        tls_obj.insert(
                            "server_name".to_string(),
                            serde_json::Value::String(host.clone()),
                        );
                    }
                    extra.insert("tls".to_string(), serde_json::Value::Object(tls_obj));
                }
                _ => {}
            }
        }
    }

    if !extra.contains_key("tls") {
        extra.insert(
            "tls".to_string(),
            serde_json::json!({
                "enabled": true,
                "server_name": host,
                "insecure": false
            }),
        );
    }

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("naive".to_string()),
        server: Some(url.host_str()?.to_string()),
        server_port: Some(port),
        extra,
    })
}

pub(crate) fn export_node_to_link(node: &SingBoxOutbound) -> Result<String, String> {
    let default_tag = "Node".to_string();
    let default_server = String::new();

    let tag = urlencoding::encode(node.tag.as_ref().unwrap_or(&default_tag));
    let node_type = node
        .outbound_type
        .as_ref()
        .map(|s| s.as_str())
        .unwrap_or("");
    let server = node.server.as_ref().unwrap_or(&default_server);
    let port = node.server_port.unwrap_or(0);

    match node_type.to_lowercase().as_str() {
        "shadowsocks" => {
            let method = node
                .extra
                .get("method")
                .and_then(|v| v.as_str())
                .unwrap_or("aes-256-gcm");
            let password = node
                .extra
                .get("password")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let user_info = base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                format!("{}:{}", method, password),
            );
            Ok(format!("ss://{}@{}:{}#{}", user_info, server, port, tag))
        }
        "vmess" => {
            let uuid = node
                .extra
                .get("uuid")
                .and_then(|v| v.as_str())
                .unwrap_or("");

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
                config.to_string(),
            );
            Ok(format!("vmess://{}", encoded))
        }
        "vless" => {
            let uuid = node
                .extra
                .get("uuid")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let flow = node
                .extra
                .get("flow")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let transport = node.extra.get("transport").and_then(|v| v.as_object());
            let net_type = transport
                .and_then(|t| t.get("type").and_then(|v| v.as_str()))
                .unwrap_or("tcp");

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

            Ok(format!(
                "vless://{}@{}:{}?{}#{}",
                uuid, server, port, query, tag
            ))
        }
        "trojan" => {
            let password = node
                .extra
                .get("password")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            Ok(format!("trojan://{}@{}:{}#{}", password, server, port, tag))
        }
        "hysteria2" => {
            let password = node
                .extra
                .get("password")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            Ok(format!(
                "hysteria2://{}@{}:{}#{}",
                password, server, port, tag
            ))
        }
        _ => Ok(serde_json::to_string_pretty(node).map_err(|e| e.to_string())?),
    }
}
