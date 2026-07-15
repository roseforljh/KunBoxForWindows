use crate::types::SingBoxOutbound;

use super::{parse_host_port, ECH_DNS_SERVER_META_KEY};

fn extract_ech_name_and_dns_server(ech: &str) -> Option<(&str, &str)> {
    let (_, resolver) = ech.split_once('+')?;
    let (name, _) = ech.split_once('+')?;
    let name = name.trim();
    let resolver = resolver.trim();
    if name.is_empty() {
        return None;
    }
    let url = url::Url::parse(resolver).ok()?;
    matches!(url.scheme(), "https" | "h3").then_some((name, resolver))
}

fn extract_ech_public_name(ech: &str) -> Option<&str> {
    extract_ech_name_and_dns_server(ech).map(|(name, _)| name)
}

fn extract_ech_dns_server(ech: &str) -> Option<&str> {
    extract_ech_name_and_dns_server(ech).map(|(_, resolver)| resolver)
}

fn parse_ech_config_lines(ech: &str) -> Option<Vec<serde_json::Value>> {
    let trimmed = ech.trim();
    if !(trimmed.contains("-----BEGIN") && trimmed.contains("-----END")) {
        return None;
    }

    let lines: Vec<serde_json::Value> = trimmed
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::Value::String(line.to_string()))
        .collect();

    if lines.is_empty() {
        None
    } else {
        Some(lines)
    }
}

pub(super) fn parse_vless_link(link: &str) -> Option<SingBoxOutbound> {
    let rest = link.strip_prefix("vless://")?;
    let (main_part, tag) = rest.split_once('#').unwrap_or((rest, "VLESS"));
    let tag = urlencoding::decode(tag).ok()?.to_string();

    let (user_host, query) = main_part.split_once('?').unwrap_or((main_part, ""));
    let (uuid, host_port) = user_host.split_once('@')?;

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
    extra.insert(
        "uuid".to_string(),
        serde_json::Value::String(uuid.to_string()),
    );
    extra.insert(
        "packet_encoding".to_string(),
        serde_json::Value::String("xudp".to_string()),
    );

    let ech = params
        .get("ech")
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let ech_public_name = ech.and_then(extract_ech_public_name);
    let ech_dns_server = ech.and_then(extract_ech_dns_server);

    if let Some(flow) = params.get("flow") {
        if !flow.is_empty() {
            extra.insert("flow".to_string(), serde_json::Value::String(flow.clone()));
        }
    }
    if let Some(encryption) = params
        .get("encryption")
        .filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("none"))
    {
        extra.insert(
            "encryption".to_string(),
            serde_json::Value::String(encryption.clone()),
        );
    }

    // TLS configuration
    let security_param = params.get("security").map(|s| s.as_str()).unwrap_or("");
    let has_tls_params = params.contains_key("sni")
        || params.contains_key("servername")
        || params.contains_key("alpn")
        || params.contains_key("fp")
        || params.contains_key("insecure")
        || ech.is_some();
    let security = if security_param.is_empty() && has_tls_params {
        "tls"
    } else {
        security_param
    };
    if security == "tls" || security == "reality" {
        let mut tls = serde_json::Map::new();
        tls.insert("enabled".to_string(), serde_json::Value::Bool(true));

        if let Some(sni) = params.get("sni").or(params.get("servername")) {
            tls.insert(
                "server_name".to_string(),
                serde_json::Value::String(sni.clone()),
            );
        } else if let Some(public_name) = ech_public_name {
            tls.insert(
                "server_name".to_string(),
                serde_json::Value::String(public_name.to_string()),
            );
        } else {
            tls.insert(
                "server_name".to_string(),
                serde_json::Value::String(server.clone()),
            );
        }

        if let Some(insecure) = params.get("insecure") {
            let allow_insecure = insecure == "1" || insecure.eq_ignore_ascii_case("true");
            tls.insert(
                "insecure".to_string(),
                serde_json::Value::Bool(allow_insecure),
            );
        }

        if let Some(alpn) = params.get("alpn") {
            let alpn_arr: Vec<serde_json::Value> = alpn
                .split(',')
                .map(|s| serde_json::Value::String(s.to_string()))
                .collect();
            tls.insert("alpn".to_string(), serde_json::Value::Array(alpn_arr));
        }

        if let Some(fp) = params.get("fp") {
            if !fp.is_empty() {
                tls.insert(
                    "utls".to_string(),
                    serde_json::json!({
                        "enabled": true,
                        "fingerprint": fp
                    }),
                );
            }
        }

        if let Some(ech_value) = ech {
            let mut ech_obj = serde_json::Map::new();
            ech_obj.insert("enabled".to_string(), serde_json::Value::Bool(true));

            if let Some(config_lines) = parse_ech_config_lines(ech_value) {
                ech_obj.insert("config".to_string(), serde_json::Value::Array(config_lines));
            } else if let Some(public_name) = extract_ech_public_name(ech_value) {
                ech_obj.insert(
                    "query_server_name".to_string(),
                    serde_json::Value::String(public_name.to_string()),
                );
            }

            tls.insert("ech".to_string(), serde_json::Value::Object(ech_obj));
        }

        if security == "reality" {
            let mut reality = serde_json::Map::new();
            reality.insert("enabled".to_string(), serde_json::Value::Bool(true));
            if let Some(pbk) = params.get("pbk") {
                reality.insert(
                    "public_key".to_string(),
                    serde_json::Value::String(pbk.clone()),
                );
            }
            if let Some(sid) = params.get("sid") {
                reality.insert(
                    "short_id".to_string(),
                    serde_json::Value::String(sid.clone()),
                );
            }
            tls.insert("reality".to_string(), serde_json::Value::Object(reality));
        }

        extra.insert("tls".to_string(), serde_json::Value::Object(tls));
    }

    if let Some(dns_server) = ech_dns_server {
        extra.insert(
            ECH_DNS_SERVER_META_KEY.to_string(),
            serde_json::Value::String(dns_server.to_string()),
        );
    }

    // Transport configuration
    let transport_type = params.get("type").map(|s| s.as_str()).unwrap_or("tcp");
    if transport_type != "tcp" {
        let mut transport = serde_json::Map::new();
        transport.insert(
            "type".to_string(),
            serde_json::Value::String(transport_type.to_string()),
        );

        match transport_type {
            "ws" => {
                if let Some(path) = params.get("path") {
                    transport.insert("path".to_string(), serde_json::Value::String(path.clone()));
                }
                if let Some(host) = params.get("host") {
                    transport.insert("headers".to_string(), serde_json::json!({ "Host": host }));
                }
            }
            "grpc" => {
                if let Some(sn) = params.get("serviceName") {
                    transport.insert(
                        "service_name".to_string(),
                        serde_json::Value::String(sn.clone()),
                    );
                }
            }
            "http" | "h2" => {
                transport.insert(
                    "type".to_string(),
                    serde_json::Value::String("http".to_string()),
                );
                if let Some(path) = params.get("path") {
                    transport.insert("path".to_string(), serde_json::Value::String(path.clone()));
                }
                if let Some(host) = params.get("host") {
                    transport.insert("host".to_string(), serde_json::json!([host]));
                }
            }
            "xhttp" => {
                if let Some(path) = params.get("path") {
                    transport.insert("path".to_string(), serde_json::Value::String(path.clone()));
                }
                if let Some(mode) = params.get("mode") {
                    transport.insert("mode".to_string(), serde_json::Value::String(mode.clone()));
                }
                if let Some(host) = params.get("host") {
                    transport.insert("headers".to_string(), serde_json::json!({ "Host": host }));
                }
                if let Some(extra_str) = params.get("extra") {
                    if let Ok(extra_json) = serde_json::from_str::<serde_json::Value>(extra_str) {
                        if let Some(encryption) = extra_json
                            .get("encryption")
                            .and_then(|value| value.as_str())
                            .filter(|value| !value.is_empty())
                        {
                            extra.insert(
                                "encryption".to_string(),
                                serde_json::Value::String(encryption.to_string()),
                            );
                        }

                        if let Some(extra_obj) = extra_json.as_object() {
                            let mut transport_extra = extra_obj.clone();
                            transport_extra.remove("encryption");
                            if !transport_extra.is_empty() {
                                transport.insert(
                                    "extra".to_string(),
                                    serde_json::Value::Object(transport_extra),
                                );
                            }
                        } else {
                            transport.insert("extra".to_string(), extra_json);
                        }
                    } else {
                        transport.insert(
                            "extra".to_string(),
                            serde_json::Value::String(extra_str.clone()),
                        );
                    }
                }
            }
            _ => {}
        }

        extra.insert(
            "transport".to_string(),
            serde_json::Value::Object(transport),
        );
    }

    Some(SingBoxOutbound {
        tag: Some(tag),
        outbound_type: Some("vless".to_string()),
        server: Some(server),
        server_port: Some(port),
        extra,
    })
}
