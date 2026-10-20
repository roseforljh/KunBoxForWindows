use base64::Engine;
use serde_json::{json, Value};

use crate::types::{
    SingBoxOutbound, NODE_AUTO_SELECTION_ELIGIBLE_META_KEY, NODE_METERED_PROTECTED_META_KEY,
    NODE_RUNTIME_META_KEYS,
};

use super::{parse_node_link, ECH_DNS_SERVER_META_KEY};

fn required<'a>(node: &'a SingBoxOutbound, key: &str) -> Result<&'a str, String> {
    node.extra
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("节点缺少 {key}，无法导出分享链接"))
}

fn add(query: &mut Vec<(String, String)>, key: &str, value: impl ToString) {
    query.push((key.to_string(), value.to_string()));
}

fn add_field(query: &mut Vec<(String, String)>, key: &str, value: Option<&Value>) {
    if let Some(value) = value {
        if !value.is_null() {
            add(
                query,
                key,
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string()),
            );
        }
    }
}

fn string_list(value: &Value, separator: &str) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    value
        .as_array()?
        .iter()
        .map(Value::as_str)
        .collect::<Option<Vec<_>>>()
        .map(|items| items.join(separator))
}

fn transport_host(transport: &Value) -> Option<String> {
    transport
        .get("host")
        .and_then(|v| string_list(v, ","))
        .or_else(|| {
            transport
                .pointer("/headers/Host")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

fn tls_query(node: &SingBoxOutbound, query: &mut Vec<(String, String)>) -> Result<(), String> {
    let tls = &node.extra.get("tls").cloned().unwrap_or(Value::Null);
    if tls.get("enabled").and_then(Value::as_bool) != Some(true) {
        return Ok(());
    }
    let reality = tls.pointer("/reality/enabled").and_then(Value::as_bool) == Some(true);
    add(query, "security", if reality { "reality" } else { "tls" });
    add_field(query, "sni", tls.get("server_name"));
    if let Some(alpn) = tls.get("alpn").and_then(|v| string_list(v, ",")) {
        add(query, "alpn", alpn);
    }
    if let Some(insecure) = tls.get("insecure").and_then(Value::as_bool) {
        add(query, "insecure", if insecure { "1" } else { "0" });
        // V2Ray-compatible Trojan clients use this spelling.
        if node.outbound_type.as_deref() == Some("trojan") {
            add(query, "allowInsecure", if insecure { "1" } else { "0" });
        }
    }
    if tls.pointer("/utls/enabled").and_then(Value::as_bool) == Some(true) {
        add_field(query, "fp", tls.pointer("/utls/fingerprint"));
    }
    if reality {
        let key = tls
            .pointer("/reality/public_key")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
            .ok_or("Reality 节点缺少 public_key，无法导出")?;
        add(query, "pbk", key);
        add_field(query, "sid", tls.pointer("/reality/short_id"));
    }
    if tls.pointer("/ech/enabled").and_then(Value::as_bool) == Some(true) {
        if let Some(config) = tls
            .pointer("/ech/config")
            .and_then(|v| string_list(v, "\n"))
        {
            add(query, "ech", config);
        } else if let (Some(name), Some(resolver)) = (
            tls.pointer("/ech/query_server_name")
                .and_then(Value::as_str),
            node.extra
                .get(ECH_DNS_SERVER_META_KEY)
                .and_then(Value::as_str),
        ) {
            add(query, "ech", format!("{name}+{resolver}"));
        } else {
            return Err("ECH 配置缺少可分享的配置内容或 DNS 地址，请使用完整配置文件".into());
        }
    }
    Ok(())
}

fn transport_path(transport: &Value) -> Result<Option<String>, String> {
    let path = transport.get("path").and_then(Value::as_str);
    if transport.get("type").and_then(Value::as_str) == Some("ws") {
        if let Some(size) = transport
            .get("max_early_data")
            .and_then(Value::as_u64)
            .filter(|v| *v > 0)
        {
            if transport
                .get("early_data_header_name")
                .and_then(Value::as_str)
                != Some("Sec-WebSocket-Protocol")
            {
                return Err(
                    "分享链接无法保留自定义 WebSocket Early Data 请求头，请使用完整配置文件".into(),
                );
            }
            let path = path.unwrap_or("/");
            if path.split_once('?').is_some_and(|(_, query)| {
                url::form_urlencoded::parse(query.as_bytes()).any(|(k, _)| k == "ed")
            }) {
                return Err("WebSocket 路径已有 ed 参数，与 Early Data 配置冲突".into());
            }
            return Ok(Some(format!(
                "{path}{}ed={size}",
                if path.contains('?') { "&" } else { "?" }
            )));
        }
    }
    Ok(path.map(str::to_owned))
}

fn transport_query(
    node: &SingBoxOutbound,
    query: &mut Vec<(String, String)>,
) -> Result<(), String> {
    let Some(transport) = node.extra.get("transport") else {
        add(query, "type", "tcp");
        return Ok(());
    };
    add_field(query, "type", transport.get("type"));
    if let Some(path) = transport_path(transport)? {
        add(query, "path", path);
    }
    add_field(query, "serviceName", transport.get("service_name"));
    add_field(query, "mode", transport.get("mode"));
    add_field(query, "extra", transport.get("extra"));
    if let Some(host) = transport_host(transport) {
        add(query, "host", host);
    }
    Ok(())
}

/// A URI cannot represent every sing-box option (certificates, detours, custom headers, etc.).
/// Compare with the decoded URI before returning it, so future fields cannot silently disappear.
fn check_preserved(path: &str, original: &Value, decoded: Option<&Value>) -> Result<(), String> {
    if decoded == Some(original) {
        return Ok(());
    }
    // Numeric single ports and the editor shorthand represent the same range.
    if path == "server_ports" {
        if let Some(decoded) = decoded {
            let mut normalized = original.clone();
            let mut decoded = decoded.clone();
            crate::types::normalize_hysteria2_server_ports(&mut normalized);
            crate::types::normalize_hysteria2_server_ports(&mut decoded);
            if decoded == normalized {
                return Ok(());
            }
        }
    }
    if decoded.is_none() {
        let is_default = match path {
            "packet_encoding" => original == "xudp",
            "version" => original == "5",
            "encryption" => original == "none",
            "username" | "password" | "detour" => original == "",
            "transport.max_early_data" => original == 0,
            "tls.enabled"
            | "tls.insecure"
            | "tls.utls.enabled"
            | "tls.reality.enabled"
            | "tls.ech.enabled" => original == false,
            "transport.type" => original == "tcp",
            _ => false,
        };
        if is_default || original.is_null() {
            return Ok(());
        }
    }
    if let Some(object) = original.as_object() {
        for (key, value) in object {
            check_preserved(
                &format!("{path}.{key}"),
                value,
                decoded.and_then(|v| v.get(key)),
            )?;
        }
        return Ok(());
    }
    Err(format!(
        "分享链接无法完整保留字段 {path}，请使用完整配置文件；未复制不完整的链接"
    ))
}

pub(crate) fn export_node_to_link(node: &SingBoxOutbound) -> Result<String, String> {
    let kind = node.outbound_type.as_deref().ok_or("节点缺少协议类型")?;
    if !matches!(
        kind,
        "socks"
            | "http"
            | "shadowsocks"
            | "vmess"
            | "vless"
            | "trojan"
            | "hysteria2"
            | "hysteria"
            | "tuic"
            | "anytls"
            | "naive"
    ) {
        return Err(format!(
            "{kind} 暂无受支持的分享链接格式，请使用完整配置文件"
        ));
    }
    let server = node
        .server
        .as_deref()
        .filter(|v| !v.trim().is_empty())
        .ok_or("节点缺少服务器地址")?;
    let server = server.trim_start_matches('[').trim_end_matches(']');
    let host = if server.contains(':') {
        format!("[{server}]")
    } else {
        server.to_string()
    };
    let mut query = Vec::new();
    let mut port = node.server_port;
    if kind == "hysteria2" {
        if let Some(ports) = node
            .extra
            .get("server_ports")
            .and_then(Value::as_array)
            .filter(|v| !v.is_empty())
        {
            let ports = ports
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string())
                        .replace(':', "-")
                })
                .collect::<Vec<_>>();
            port = port.or_else(|| ports.first()?.split('-').next()?.parse().ok());
            add(&mut query, "mport", ports.join(","));
        }
    }
    let port = port
        .filter(|v| *v > 0)
        .ok_or("节点缺少有效端口，无法导出分享链接")?;
    let tag = urlencoding::encode(node.tag.as_deref().unwrap_or("Node"));
    tls_query(node, &mut query)?;
    let credentials = || {
        format!(
            "{}:{}@",
            urlencoding::encode(
                node.extra
                    .get("username")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            ),
            urlencoding::encode(
                node.extra
                    .get("password")
                    .and_then(Value::as_str)
                    .unwrap_or("")
            )
        )
    };
    let tls_enabled = node
        .extra
        .get("tls")
        .and_then(|v| v.get("enabled"))
        .and_then(Value::as_bool)
        == Some(true);
    if matches!(
        kind,
        "trojan" | "hysteria2" | "hysteria" | "tuic" | "anytls" | "naive"
    ) && !tls_enabled
    {
        return Err("此协议的标准分享链接要求启用 TLS，当前节点无法无损导出".into());
    }
    let (scheme, userinfo) = match kind {
        "socks" | "http" | "naive" => {
            let scheme = match kind {
                "socks" => "socks5",
                "naive" => "naive+https",
                _ if tls_enabled => "https",
                _ => "http",
            };
            let auth = if node.extra.contains_key("username") || node.extra.contains_key("password")
            {
                credentials()
            } else {
                String::new()
            };
            (scheme, auth)
        }
        "shadowsocks" => {
            let method = required(node, "method")?;
            let password = required(node, "password")?;
            if let Some(plugin) = node.extra.get("plugin").and_then(Value::as_str) {
                let opts = node
                    .extra
                    .get("plugin_opts")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                add(
                    &mut query,
                    "plugin",
                    if opts.is_empty() {
                        plugin.to_owned()
                    } else {
                        format!("{plugin};{opts}")
                    },
                );
            }
            // SIP002: AEAD-2022 userinfo uses percent encoding, older methods use base64url.
            let auth = if method.starts_with("2022-") {
                format!(
                    "{}:{}",
                    urlencoding::encode(method),
                    urlencoding::encode(password)
                )
            } else {
                base64::engine::general_purpose::URL_SAFE_NO_PAD
                    .encode(format!("{method}:{password}"))
            };
            ("ss", format!("{auth}@"))
        }
        "vless" | "vmess" => {
            let uuid = required(node, "uuid")?;
            if kind == "vmess" {
                let transport = node.extra.get("transport").cloned().unwrap_or(Value::Null);
                let net = transport
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("tcp");
                let mut config = json!({
                    "v": "2", "ps": node.tag.as_deref().unwrap_or("Node"), "add": server, "port": port,
                    "id": uuid, "aid": node.extra.get("alter_id").cloned().unwrap_or(json!(0)),
                    "scy": node.extra.get("security").cloned().unwrap_or(json!("auto")),
                    "net": if net == "http" { "h2" } else { net }, "type": "none", "tls": if tls_enabled { "tls" } else { "" },
                });
                if let Some(host) = transport_host(&transport) {
                    config["host"] = json!(host);
                }
                if let Some(path) = transport.get(if net == "grpc" {
                    "service_name"
                } else {
                    "path"
                }) {
                    config["path"] = path.clone();
                }
                if net == "ws" {
                    if let Some(path) = transport_path(&transport)? {
                        config["path"] = json!(path);
                    }
                }
                for (key, value) in &query {
                    match key.as_str() {
                        "sni" | "alpn" | "fp" => config[key] = json!(value),
                        "insecure" => config["allowInsecure"] = json!(value == "1"),
                        _ => {}
                    }
                }
                let link = format!(
                    "vmess://{}",
                    base64::engine::general_purpose::STANDARD.encode(config.to_string())
                );
                return finish_export(node, link);
            }
            if !tls_enabled {
                add(&mut query, "security", "none");
            }
            add_field(&mut query, "flow", node.extra.get("flow"));
            add(
                &mut query,
                "encryption",
                node.extra
                    .get("encryption")
                    .and_then(Value::as_str)
                    .unwrap_or("none"),
            );
            transport_query(node, &mut query)?;
            ("vless", format!("{}@", urlencoding::encode(uuid)))
        }
        "trojan" | "hysteria2" | "anytls" => {
            if kind == "trojan" {
                transport_query(node, &mut query)?;
            }
            if kind == "hysteria2" {
                if let Some(obfs) = node.extra.get("obfs") {
                    add_field(&mut query, "obfs", obfs.get("type"));
                    add_field(&mut query, "obfs-password", obfs.get("password"));
                }
            }
            (
                kind,
                format!("{}@", urlencoding::encode(required(node, "password")?)),
            )
        }
        "hysteria" => {
            for (key, field) in [
                ("auth", "auth_str"),
                ("upmbps", "up_mbps"),
                ("downmbps", "down_mbps"),
                ("obfs", "obfs"),
            ] {
                add_field(&mut query, key, node.extra.get(field));
            }
            (kind, String::new())
        }
        "tuic" => {
            for key in [
                "congestion_control",
                "udp_relay_mode",
                "udp_over_stream",
                "zero_rtt_handshake",
            ] {
                add_field(&mut query, key, node.extra.get(key));
            }
            (
                kind,
                format!(
                    "{}:{}@",
                    urlencoding::encode(required(node, "uuid")?),
                    urlencoding::encode(required(node, "password")?)
                ),
            )
        }
        _ => unreachable!(),
    };
    let query = if query.is_empty() {
        String::new()
    } else {
        format!(
            "?{}",
            query
                .iter()
                .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
                .collect::<Vec<_>>()
                .join("&")
        )
    };
    finish_export(
        node,
        format!("{scheme}://{userinfo}{host}:{port}{query}#{tag}"),
    )
}

fn finish_export(node: &SingBoxOutbound, link: String) -> Result<String, String> {
    let decoded = parse_node_link(&link).ok_or("节点配置无法生成有效的分享链接")?;
    let normalize_host = |host: &Option<String>| {
        host.as_deref()
            .unwrap_or("")
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase()
    };
    if normalize_host(&node.server) != normalize_host(&decoded.server) {
        return Err("服务器地址无法正确编码为分享链接".into());
    }
    if !node.extra.contains_key("server_ports") && node.server_port != decoded.server_port {
        return Err("分享链接无法完整保留服务器端口".into());
    }
    if node.tag.as_ref().is_some_and(|tag| !tag.is_empty()) && node.tag != decoded.tag {
        return Err("分享链接无法完整保留节点名称".into());
    }
    for (key, value) in &node.extra {
        if key == NODE_AUTO_SELECTION_ELIGIBLE_META_KEY
            || key == NODE_METERED_PROTECTED_META_KEY
            || NODE_RUNTIME_META_KEYS.contains(&key.as_str())
        {
            continue;
        }
        // xhttp stores Host in either shape; both have the same URI representation.
        if key == "transport"
            && value.get("type").and_then(Value::as_str) == Some("xhttp")
            && value.get("host").is_some()
        {
            let mut normalized = value.clone();
            let host = normalized.as_object_mut().unwrap().remove("host").unwrap();
            if normalized
                .pointer("/headers/Host")
                .is_some_and(|value| value != &host)
            {
                return Err("XHTTP 的 host 与 headers.Host 冲突，无法完整导出".into());
            }
            normalized["headers"]["Host"] = host;
            check_preserved(key, &normalized, decoded.extra.get(key))?;
        } else {
            check_preserved(key, value, decoded.extra.get(key))?;
        }
    }
    Ok(link)
}
