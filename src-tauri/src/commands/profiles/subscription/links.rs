use crate::types::SingBoxOutbound;

use super::quic::{parse_hysteria2_link, parse_hysteria_link, parse_tuic_link};
use super::vless::parse_vless_link;
use super::{decode_base64_compat, parse_bool_param, parse_host_port, parse_port_value};

fn apply_tls_params(
    tls: &mut serde_json::Value,
    params: &std::collections::HashMap<String, String>,
) {
    if let Some(value) = params.get("alpn") {
        tls["alpn"] = serde_json::json!(value.split(',').collect::<Vec<_>>());
    }
    if let Some(value) = params.get("fp").filter(|v| !v.is_empty()) {
        tls["utls"] = serde_json::json!({ "enabled": true, "fingerprint": value });
    }
    if let Some(value) = params
        .get("insecure")
        .or_else(|| params.get("allowInsecure"))
    {
        tls["insecure"] = serde_json::json!(parse_bool_param(value));
    }
    if params.get("security").is_some_and(|v| v == "reality") {
        tls["reality"] = serde_json::json!({ "enabled": true });
        for (key, field) in [("pbk", "public_key"), ("sid", "short_id")] {
            if let Some(value) = params.get(key) {
                tls["reality"][field] = serde_json::json!(value);
            }
        }
    }
}

pub(crate) fn parse_node_link(link: &str) -> Option<SingBoxOutbound> {
    if link.starts_with("ss://") {
        parse_ss_link(link)
    } else if link.starts_with("socks5://") || link.starts_with("socks://") {
        parse_socks_link(link)
    } else if link.starts_with("http://") || link.starts_with("https://") {
        parse_http_proxy_link(link)
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

fn decode_link_fragment(url: &url::Url, default_tag: &str) -> Option<String> {
    match url.fragment().filter(|value| !value.is_empty()) {
        Some(value) => Some(urlencoding::decode(value).ok()?.to_string()),
        None => Some(default_tag.to_string()),
    }
}

fn insert_proxy_credentials(
    url: &url::Url,
    extra: &mut std::collections::HashMap<String, serde_json::Value>,
) -> Option<()> {
    let username = urlencoding::decode(url.username()).ok()?.to_string();
    if !username.is_empty() {
        extra.insert("username".to_string(), serde_json::Value::String(username));
    }
    if let Some(password) = url.password() {
        extra.insert(
            "password".to_string(),
            serde_json::Value::String(urlencoding::decode(password).ok()?.to_string()),
        );
    }
    Some(())
}

pub(super) fn parse_socks_link(link: &str) -> Option<SingBoxOutbound> {
    let url = url::Url::parse(link).ok()?;
    if !matches!(url.scheme(), "socks5" | "socks") {
        return None;
    }

    let server = url
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = url.port().filter(|port| *port > 0)?;
    let tag = decode_link_fragment(&url, "SOCKS5")?;
    let mut extra = std::collections::HashMap::new();
    insert_proxy_credentials(&url, &mut extra)?;

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("socks".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}

pub(super) fn parse_http_proxy_link(link: &str) -> Option<SingBoxOutbound> {
    let url = url::Url::parse(link).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }

    let server = url
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    let port = url.port_or_known_default().filter(|port| *port > 0)?;
    let tag = decode_link_fragment(&url, "HTTP")?;
    let mut extra = std::collections::HashMap::new();
    insert_proxy_credentials(&url, &mut extra)?;
    if url.scheme() == "https" {
        let params: std::collections::HashMap<String, String> = url
            .query_pairs()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect();
        extra.insert(
            "tls".to_string(),
            serde_json::json!({
                "enabled": true,
                "server_name": params
                    .get("sni")
                    .or_else(|| params.get("servername"))
                    .filter(|value| !value.is_empty())
                    .cloned()
                    .unwrap_or_else(|| server.clone()),
                "insecure": params
                    .get("insecure")
                    .or_else(|| params.get("allowInsecure"))
                    .is_some_and(|value| parse_bool_param(value))
            }),
        );
        apply_tls_params(extra.get_mut("tls")?, &params);
    }

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("http".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
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

    if let Some((_, query)) = host_port.split_once('?') {
        for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
            if key == "plugin" {
                let (plugin, opts) = value.split_once(';').unwrap_or((&value, ""));
                extra.insert("plugin".into(), serde_json::json!(plugin));
                if !opts.is_empty() {
                    extra.insert("plugin_opts".into(), serde_json::json!(opts));
                }
            }
        }
    }

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
        let allow_insecure = json
            .get("allowInsecure")
            .or_else(|| json.get("insecure"))
            .is_some_and(|value| {
                value
                    .as_bool()
                    .unwrap_or_else(|| value.as_str().is_some_and(parse_bool_param))
            });
        if allow_insecure {
            tls_obj.insert("insecure".to_string(), serde_json::Value::Bool(true));
        }
        extra.insert("tls".to_string(), serde_json::Value::Object(tls_obj));
        let params = ["alpn", "fp"]
            .into_iter()
            .filter_map(|key| {
                json.get(key)
                    .and_then(|v| v.as_str())
                    .map(|v| (key.to_string(), v.to_string()))
            })
            .collect();
        apply_tls_params(extra.get_mut("tls")?, &params);
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
                    super::parse_websocket_path(&mut transport, path);
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
            "h2" | "http" => {
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
                if let Some(host) = json.get("host").and_then(|v| v.as_str()) {
                    transport.insert(
                        "host".to_string(),
                        serde_json::json!(host.split(',').collect::<Vec<_>>()),
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
    apply_tls_params(extra.get_mut("tls")?, &params);

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
                super::parse_websocket_path(&mut transport, path);
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
        } else if matches!(transport_type, "http" | "h2") {
            transport.insert("type".into(), serde_json::json!("http"));
            if let Some(path) = params.get("path") {
                transport.insert("path".into(), serde_json::json!(path));
            }
            if let Some(host) = params.get("host") {
                transport.insert(
                    "host".into(),
                    serde_json::json!(host.split(',').collect::<Vec<_>>()),
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
    let server = url
        .host_str()?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
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
    apply_tls_params(extra.get_mut("tls")?, &params);

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
    let mut node = parse_http_proxy_link(rest)?;
    // Naive uses HTTP/2 over TLS, and its URI convention is naive+https.
    if !rest.starts_with("https://") {
        return None;
    }
    node.outbound_type = Some("naive".to_string());
    if !rest.contains('#') {
        node.tag = Some("Naive".to_string());
    }
    Some(node)
}
