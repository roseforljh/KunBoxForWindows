use crate::types::SingBoxOutbound;

use super::{parse_bool_param, parse_host_port};

pub(super) fn parse_hysteria2_link(link: &str) -> Option<SingBoxOutbound> {
    let rest = link
        .strip_prefix("hysteria2://")
        .or_else(|| link.strip_prefix("hy2://"))?;
    let (main_part, tag) = rest.split_once('#').unwrap_or((rest, "Hysteria2"));
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

    // TLS (Hysteria2 always uses TLS)
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
        .get("insecure")
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        tls.insert("insecure".to_string(), serde_json::Value::Bool(true));
    }
    if let Some(alpn) = params.get("alpn") {
        let alpn_arr: Vec<serde_json::Value> = alpn
            .split(',')
            .map(|s| serde_json::Value::String(s.to_string()))
            .collect();
        tls.insert("alpn".to_string(), serde_json::Value::Array(alpn_arr));
    }
    extra.insert("tls".to_string(), serde_json::Value::Object(tls));

    // Obfs
    if let Some(obfs_type) = params.get("obfs") {
        let mut obfs = serde_json::Map::new();
        obfs.insert(
            "type".to_string(),
            serde_json::Value::String(obfs_type.clone()),
        );
        if let Some(obfs_password) = params.get("obfs-password") {
            obfs.insert(
                "password".to_string(),
                serde_json::Value::String(obfs_password.clone()),
            );
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

pub(super) fn parse_hysteria_link(link: &str) -> Option<SingBoxOutbound> {
    let rest = link.strip_prefix("hysteria://")?;
    let (main_part, tag) = rest.split_once('#').unwrap_or((rest, "Hysteria"));
    let tag = urlencoding::decode(tag).ok()?.to_string();

    let (host_port, query) = main_part.split_once('?').unwrap_or((main_part, ""));
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

    if let Some(auth) = params.get("auth") {
        extra.insert(
            "auth_str".to_string(),
            serde_json::Value::String(auth.clone()),
        );
    }

    // TLS
    let mut tls = serde_json::Map::new();
    tls.insert("enabled".to_string(), serde_json::Value::Bool(true));
    if let Some(sni) = params.get("sni").or(params.get("peer")) {
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
        .get("insecure")
        .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        tls.insert("insecure".to_string(), serde_json::Value::Bool(true));
    }
    if let Some(alpn) = params.get("alpn") {
        let alpn_arr: Vec<serde_json::Value> = alpn
            .split(',')
            .map(|s| serde_json::Value::String(s.to_string()))
            .collect();
        tls.insert("alpn".to_string(), serde_json::Value::Array(alpn_arr));
    }
    extra.insert("tls".to_string(), serde_json::Value::Object(tls));

    // Up/Down bandwidth
    if let Some(up) = params.get("upmbps") {
        extra.insert(
            "up_mbps".to_string(),
            serde_json::Value::Number(up.parse::<i64>().unwrap_or(100).into()),
        );
    }
    if let Some(down) = params.get("downmbps") {
        extra.insert(
            "down_mbps".to_string(),
            serde_json::Value::Number(down.parse::<i64>().unwrap_or(100).into()),
        );
    }

    // Obfs
    if let Some(obfs_type) = params.get("obfs") {
        extra.insert(
            "obfs".to_string(),
            serde_json::Value::String(obfs_type.clone()),
        );
    }

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("hysteria".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}

pub(super) fn parse_tuic_link(link: &str) -> Option<SingBoxOutbound> {
    let url = url::Url::parse(link).ok()?;
    let tag = url
        .fragment()
        .and_then(|value| {
            urlencoding::decode(value)
                .ok()
                .map(|value| value.to_string())
        })
        .unwrap_or_else(|| "TUIC".to_string());
    let server = url.host_str()?.to_string();
    let port = url.port_or_known_default().unwrap_or(443);
    let uuid = urlencoding::decode(url.username()).ok()?.to_string();
    let password = url
        .password()
        .and_then(|value| {
            urlencoding::decode(value)
                .ok()
                .map(|value| value.to_string())
        })
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
        extra.insert(
            "congestion_control".to_string(),
            serde_json::Value::String(value.clone()),
        );
    }
    if let Some(value) = params
        .get("udp_relay_mode")
        .or_else(|| params.get("udpRelayMode"))
        .filter(|value| !value.is_empty())
    {
        extra.insert(
            "udp_relay_mode".to_string(),
            serde_json::Value::String(value.clone()),
        );
    }
    if let Some(value) = params
        .get("udp_over_stream")
        .or_else(|| params.get("udpOverStream"))
    {
        extra.insert(
            "udp_over_stream".to_string(),
            serde_json::Value::Bool(parse_bool_param(value)),
        );
    }
    if let Some(value) = params
        .get("zero_rtt_handshake")
        .or_else(|| params.get("zeroRttHandshake"))
    {
        extra.insert(
            "zero_rtt_handshake".to_string(),
            serde_json::Value::Bool(parse_bool_param(value)),
        );
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
        tls.insert(
            "insecure".to_string(),
            serde_json::Value::Bool(parse_bool_param(value)),
        );
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
