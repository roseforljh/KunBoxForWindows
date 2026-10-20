use super::links::{
    parse_http_proxy_link, parse_socks_link, parse_ss_link, parse_trojan_link, parse_vmess_link,
};
use super::quic::parse_hysteria2_link;
use super::vless::parse_vless_link;
use super::*;

#[test]
fn parses_insecure_flags_from_links() {
    let trojan = parse_trojan_link("trojan://pwd@example.com:443?allowInsecure=true#demo").unwrap();
    let trojan_tls = trojan.extra.get("tls").and_then(|v| v.as_object()).unwrap();
    assert_eq!(
        trojan_tls.get("insecure").and_then(|v| v.as_bool()),
        Some(true)
    );

    let hysteria2 =
        parse_hysteria2_link("hysteria2://pwd@example.com:443?insecure=true#demo").unwrap();
    let hysteria2_tls = hysteria2
        .extra
        .get("tls")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(
        hysteria2_tls.get("insecure").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn parse_hysteria2_link_preserves_port_hopping_and_pin_compatibility() {
    let node = parse_hysteria2_link(
        "hysteria2://pwd@example.com:443?insecure=false&mport=20000-30000&pinSHA256=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef&sni=hy2.example.com#demo",
    )
    .unwrap();

    assert_eq!(node.server_port, None);
    assert_eq!(
        node.extra.get("server_ports"),
        Some(&serde_json::json!(["20000:30000"]))
    );

    let tls = node.extra.get("tls").and_then(|v| v.as_object()).unwrap();
    assert_eq!(
        tls.get("server_name").and_then(|v| v.as_str()),
        Some("hy2.example.com")
    );
    assert_eq!(tls.get("insecure").and_then(|v| v.as_bool()), Some(true));

    let regular =
        parse_hysteria2_link("hysteria2://pwd@example.com:443?insecure=false#regular").unwrap();
    let regular_tls = regular
        .extra
        .get("tls")
        .and_then(|v| v.as_object())
        .unwrap();
    assert_eq!(regular_tls.get("insecure"), None);
}

#[test]
fn parse_clash_naive_ignores_network_field() {
    let proxies = vec![serde_json::json!({
        "name": "Naive H2",
        "type": "naive",
        "server": "naive.example.com",
        "port": 443,
        "username": "user",
        "password": "pass",
        "network": "h2",
        "sni": "naive.example.com"
    })];

    let nodes = parse_clash_proxies(&proxies).expect("expected naive node");
    assert_eq!(nodes.len(), 1);
    let node = &nodes[0];

    assert_eq!(node.outbound_type.as_deref(), Some("naive"));
    assert_eq!(
        node.extra.get("username").and_then(|value| value.as_str()),
        Some("user")
    );
    assert_eq!(
        node.extra
            .get("tls")
            .and_then(|value| value.get("server_name"))
            .and_then(|value| value.as_str()),
        Some("naive.example.com")
    );
    assert!(node.extra.get("network").is_none());
}

#[test]
fn parse_clash_anytls_adds_required_tls() {
    let proxies = vec![serde_json::json!({
        "name": "AnyTLS",
        "type": "anytls",
        "server": "204.136.11.104",
        "port": 31424,
        "password": "secret",
        "sni": "anyway.example.com",
        "skip-cert-verify": false,
        "udp": true
    })];

    let nodes = parse_clash_proxies(&proxies).expect("expected anytls node");
    assert_eq!(nodes.len(), 1);
    let node = &nodes[0];

    assert_eq!(node.outbound_type.as_deref(), Some("anytls"));
    assert_eq!(node.server.as_deref(), Some("204.136.11.104"));
    assert_eq!(node.server_port, Some(31424));
    assert_eq!(
        node.extra.get("password").and_then(|value| value.as_str()),
        Some("secret")
    );
    assert_eq!(
        node.extra
            .get("tls")
            .and_then(|value| value.get("enabled"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        node.extra
            .get("tls")
            .and_then(|value| value.get("server_name"))
            .and_then(|value| value.as_str()),
        Some("anyway.example.com")
    );
    assert_eq!(
        node.extra
            .get("tls")
            .and_then(|value| value.get("insecure"))
            .and_then(|value| value.as_bool()),
        Some(false)
    );
}

#[test]
fn parse_subscription_imports_anytls_links() {
    let content =
        "anytls://secret%40value@example.com:443?insecure=1&sni=tls.example.com#AnyTLS%20Node";
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, content);

    let nodes = parse_subscription_content(&encoded).expect("expected AnyTLS node");

    assert_eq!(nodes.len(), 1);
    let node = &nodes[0];
    assert_eq!(node.tag.as_deref(), Some("AnyTLS Node"));
    assert_eq!(node.outbound_type.as_deref(), Some("anytls"));
    assert_eq!(node.server.as_deref(), Some("example.com"));
    assert_eq!(node.server_port, Some(443));
    assert_eq!(
        node.extra.get("password").and_then(|value| value.as_str()),
        Some("secret@value")
    );
    assert_eq!(
        node.extra
            .get("tls")
            .and_then(|value| value.get("server_name"))
            .and_then(|value| value.as_str()),
        Some("tls.example.com")
    );
    assert_eq!(
        node.extra
            .get("tls")
            .and_then(|value| value.get("insecure"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
}

#[test]
fn parse_subscription_decodes_url_safe_no_padding_base64() {
    let content = "ss://YWVzLTEyOC1nY206cGFzcw@example.com:8388#SS";
    let encoded =
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, content);

    let nodes = parse_subscription_content(&encoded).expect("expected decoded subscription");

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].tag.as_deref(), Some("SS"));
    assert_eq!(nodes[0].server.as_deref(), Some("example.com"));
}

#[test]
fn parse_subscription_imports_authenticated_socks5_link() {
    let nodes = parse_subscription_content(
        "socks5://demo-user:p%40ss%3Aword@proxy.example.com:1080#US%20SOCKS",
    )
    .expect("expected SOCKS5 node");

    assert_eq!(nodes.len(), 1);
    let node = &nodes[0];
    assert_eq!(node.tag.as_deref(), Some("US SOCKS"));
    assert_eq!(node.outbound_type.as_deref(), Some("socks"));
    assert_eq!(node.server.as_deref(), Some("proxy.example.com"));
    assert_eq!(node.server_port, Some(1080));
    assert_eq!(
        node.extra.get("username").and_then(|value| value.as_str()),
        Some("demo-user")
    );
    assert_eq!(
        node.extra.get("password").and_then(|value| value.as_str()),
        Some("p@ss:word")
    );
}

#[test]
fn parse_socks5_link_supports_no_auth_and_rejects_invalid_ports() {
    let node = parse_socks_link("socks5://127.0.0.1:1080#Local").unwrap();

    assert_eq!(node.tag.as_deref(), Some("Local"));
    assert!(node.extra.get("username").is_none());
    assert!(node.extra.get("password").is_none());
    assert!(parse_socks_link("socks5://127.0.0.1:0#Invalid").is_none());
    assert!(parse_socks_link("socks5://127.0.0.1:70000#Invalid").is_none());
}

#[test]
fn parse_https_proxy_link_adds_tls_and_credentials() {
    let node = parse_http_proxy_link(
        "https://proxy-user:proxy-pass@proxy.example.com:8443?sni=tls.example.com&insecure=1#HTTPS%20Proxy",
    )
    .unwrap();

    assert_eq!(node.tag.as_deref(), Some("HTTPS Proxy"));
    assert_eq!(node.outbound_type.as_deref(), Some("http"));
    assert_eq!(node.server_port, Some(8443));
    assert_eq!(
        node.extra
            .get("tls")
            .and_then(|value| value.get("enabled"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        node.extra
            .get("tls")
            .and_then(|value| value.get("server_name"))
            .and_then(|value| value.as_str()),
        Some("tls.example.com")
    );
    assert_eq!(
        node.extra
            .get("tls")
            .and_then(|value| value.get("insecure"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        node.extra.get("username").and_then(|value| value.as_str()),
        Some("proxy-user")
    );
}

#[test]
fn parse_vmess_link_preserves_allow_insecure() {
    let json = serde_json::json!({
        "v": "2",
        "ps": "VMess TLS",
        "add": "example.com",
        "port": 443,
        "id": "11111111-1111-1111-1111-111111111111",
        "tls": "tls",
        "allowInsecure": true
    });
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        serde_json::to_string(&json).unwrap(),
    );
    let node = parse_vmess_link(&format!("vmess://{}", encoded)).unwrap();

    assert_eq!(
        node.extra
            .get("tls")
            .and_then(|value| value.get("insecure"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
}

#[test]
fn socks5_link_export_round_trips_credentials_and_tag() {
    let node =
        parse_socks_link("socks5://demo-user:p%40ss%3Aword@proxy.example.com:1080#US%20SOCKS")
            .unwrap();
    let exported = export_node_to_link(&node).unwrap();
    let round_trip = parse_socks_link(&exported).unwrap();

    assert_eq!(round_trip.tag, node.tag);
    assert_eq!(round_trip.server, node.server);
    assert_eq!(round_trip.server_port, node.server_port);
    assert_eq!(round_trip.extra.get("username"), node.extra.get("username"));
    assert_eq!(round_trip.extra.get("password"), node.extra.get("password"));
}

#[test]
fn manual_node_links_for_all_supported_protocols_are_parseable() {
    let vmess_json = serde_json::json!({
        "v": "2",
        "ps": "vmess node",
        "add": "node.example.com",
        "port": 443,
        "id": "11111111-1111-1111-1111-111111111111",
        "aid": 0,
        "scy": "auto",
        "net": "tcp",
        "type": "none",
        "host": "",
        "path": "/",
        "tls": "tls",
        "sni": "node.example.com",
        "allowInsecure": false
    });
    let vmess_link = format!(
        "vmess://{}",
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            serde_json::to_string(&vmess_json).unwrap()
        )
    );
    let links = [
        "socks5://user:password@node.example.com:1080#socks5%20node".to_string(),
        "http://user:password@node.example.com:8080#http%20node".to_string(),
        "ss://aes-128-gcm%3Apassword@node.example.com:8388#shadowsocks%20node".to_string(),
        vmess_link,
        "vless://11111111-1111-1111-1111-111111111111@node.example.com:443?type=tcp&security=tls&sni=node.example.com&fp=chrome#vless%20node".to_string(),
        "trojan://password@node.example.com:443?type=tcp&sni=node.example.com#trojan%20node".to_string(),
        "hysteria2://password@node.example.com:443?sni=node.example.com#hysteria2%20node".to_string(),
        "hysteria://node.example.com:443?auth=auth&peer=node.example.com&upmbps=100&downmbps=100#hysteria%20node".to_string(),
        "tuic://11111111-1111-1111-1111-111111111111:password@node.example.com:443?sni=node.example.com&congestion_control=bbr&udp_relay_mode=native&alpn=h3#tuic%20node".to_string(),
        "anytls://password@node.example.com:443?sni=node.example.com#anytls%20node".to_string(),
        "naive+https://user:password@node.example.com:443?sni=node.example.com#naive%20node".to_string(),
    ];
    let expected_types = [
        "socks",
        "http",
        "shadowsocks",
        "vmess",
        "vless",
        "trojan",
        "hysteria2",
        "hysteria",
        "tuic",
        "anytls",
        "naive",
    ];

    for (link, expected_type) in links.iter().zip(expected_types) {
        let node = parse_node_link(link)
            .unwrap_or_else(|| panic!("手工节点链接无法解析: {expected_type}"));
        assert_eq!(node.outbound_type.as_deref(), Some(expected_type));
        let exported =
            export_node_to_link(&node).unwrap_or_else(|err| panic!("{expected_type}: {err}"));
        let decoded = parse_node_link(&exported).unwrap();
        assert_eq!(decoded.outbound_type, node.outbound_type);
        assert_eq!(decoded.server, node.server);
        assert_eq!(decoded.server_port, node.server_port);
        assert_eq!(decoded.tag, node.tag);
    }
}

fn export_fixture(value: serde_json::Value) -> (String, SingBoxOutbound) {
    let node: SingBoxOutbound = serde_json::from_value(value).unwrap();
    let link = export_node_to_link(&node).unwrap();
    let decoded = parse_node_link(&link).unwrap();
    (link, decoded)
}

fn link_query(link: &str) -> std::collections::HashMap<String, String> {
    url::Url::parse(link)
        .unwrap()
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

#[test]
fn exports_vless_reality_public_key_and_transport_in_standard_fields() {
    let (link, decoded) = export_fixture(serde_json::json!({
        "type": "vless", "tag": "美国 # 1", "server": "2001:db8::1", "server_port": 443,
        "uuid": "11111111-1111-1111-1111-111111111111", "flow": "xtls-rprx-vision",
        "tls": {"enabled": true, "server_name": "front.example.com", "alpn": ["h2", "http/1.1"],
            "utls": {"enabled": true, "fingerprint": "chrome"},
            "reality": {"enabled": true, "public_key": "dummy-public-key", "short_id": "ab12"}},
        "transport": {"type": "grpc", "service_name": "service/a?b=c"}
    }));
    assert!(link.contains("@[2001:db8::1]:443?"));
    assert_eq!(decoded.server.as_deref(), Some("2001:db8::1"));
    let params = link_query(&link);
    for (key, expected) in [
        ("security", "reality"),
        ("pbk", "dummy-public-key"),
        ("sid", "ab12"),
        ("fp", "chrome"),
        ("alpn", "h2,http/1.1"),
        ("serviceName", "service/a?b=c"),
        ("flow", "xtls-rprx-vision"),
    ] {
        assert_eq!(params.get(key).map(String::as_str), Some(expected));
    }
    assert_eq!(decoded.tag.as_deref(), Some("美国 # 1"));
}

#[test]
fn exports_vmess_actual_cipher_alter_id_tls_and_websocket() {
    let (link, decoded) = export_fixture(serde_json::json!({
        "type": "vmess", "tag": "VMess WS", "server": "node.example.com", "server_port": 8443,
        "uuid": "11111111-1111-1111-1111-111111111111", "alter_id": 64, "security": "aes-128-gcm",
        "tls": {"enabled": true, "server_name": "sni.example.com", "insecure": true, "alpn": ["http/1.1"],
            "utls": {"enabled": true, "fingerprint": "firefox"}},
        "transport": {"type": "ws", "path": "/ws?a=b&c=d", "headers": {"Host": "cdn.example.com"}}
    }));
    let payload: serde_json::Value = serde_json::from_slice(
        &decode_base64_compat(link.strip_prefix("vmess://").unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(payload["aid"], 64);
    assert_eq!(payload["scy"], "aes-128-gcm");
    assert_eq!(payload["tls"], "tls");
    assert_eq!(payload["net"], "ws");
    assert_eq!(payload["host"], "cdn.example.com");
    assert_eq!(payload["path"], "/ws?a=b&c=d");
    assert_eq!(payload["sni"], "sni.example.com");
    assert_eq!(payload["allowInsecure"], true);
    assert_eq!(decoded.extra["tls"]["utls"]["fingerprint"], "firefox");
}

#[test]
fn exports_http_transport_host_lists_and_grpc_service_names() {
    for kind in ["vmess", "vless", "trojan"] {
        for transport in [
            serde_json::json!({"type":"http","host":["a.example.com","b.example.com"],"path":"/h2"}),
            serde_json::json!({"type":"grpc","service_name":"name/with spaces"}),
        ] {
            let mut node = serde_json::json!({"type":kind,"tag":"transport","server":"node.example.com","server_port":443,
                "tls":{"enabled":true}, "transport":transport});
            node[if kind == "trojan" { "password" } else { "uuid" }] =
                serde_json::json!("test-credential");
            let (_, decoded) = export_fixture(node);
            assert_eq!(decoded.extra["transport"], transport);
        }
    }
}

#[test]
fn exports_vless_xhttp_encryption_ech_and_extra_without_loss() {
    let (link, decoded) = export_fixture(serde_json::json!({
        "type":"vless", "tag":"XHTTP", "server":"node.example.com", "server_port":443, "uuid":"test-uuid",
        "encryption":"mlkem768x25519plus.native.1rtt.test",
        "tls":{"enabled":true,"server_name":"sni.example.com","ech":{"enabled":true,"query_server_name":"public.example.com"}},
        "x_kunbox_ech_dns_server":"https://dns.example.com/dns-query",
        "transport":{"type":"xhttp","path":"/x?q=a+b","mode":"packet-up","host":"cdn.example.com","extra":{"scMaxEachPostBytes":1000000}}
    }));
    let params = link_query(&link);
    assert_eq!(
        params["ech"],
        "public.example.com+https://dns.example.com/dns-query"
    );
    assert_eq!(params["mode"], "packet-up");
    assert_eq!(params["path"], "/x?q=a+b");
    assert_eq!(params["encryption"], "mlkem768x25519plus.native.1rtt.test");
    assert_eq!(
        decoded.extra["transport"]["extra"]["scMaxEachPostBytes"],
        1000000
    );
}

#[test]
fn exports_trojan_password_sni_and_ws_with_reserved_characters() {
    let (link, decoded) = export_fixture(serde_json::json!({
        "type":"trojan","tag":"密码节点","server":"2001:db8::2","server_port":443,
        "password":"p@ss:/?#%+中文","tls":{"enabled":true,"server_name":"sni.example.com","insecure":true,"alpn":["h2"]},
        "transport":{"type":"ws","path":"/ws?x=1&y=2","headers":{"Host":"cdn.example.com"}}
    }));
    assert!(link.contains("%40"));
    assert_eq!(decoded.extra["password"], "p@ss:/?#%+中文");
    assert_eq!(link_query(&link)["allowInsecure"], "1");
    assert_eq!(decoded.extra["tls"]["server_name"], "sni.example.com");
}

#[test]
fn exports_hysteria2_port_hopping_without_emitting_port_zero() {
    let (link, decoded) = export_fixture(serde_json::json!({
        "type":"hysteria2","tag":"Hy2","server":"node.example.com","server_ports":[443,"2000:3000"],
        "password":"p@ss:#?","tls":{"enabled":true,"server_name":"sni.example.com","insecure":true,"alpn":["h3"]},
        "obfs":{"type":"salamander","password":"obfs&+/#"}
    }));
    let params = link_query(&link);
    assert_eq!(params["mport"], "443,2000-3000");
    assert_eq!(params["obfs-password"], "obfs&+/#");
    assert!(link.contains(":443?"));
    assert_eq!(decoded.server_port, None);
    assert_eq!(
        decoded.extra["server_ports"],
        serde_json::json!([443, "2000:3000"])
    );
}

#[test]
fn exports_ss_sip002_plugin_and_aead2022_password() {
    for method in ["aes-128-gcm", "2022-blake3-aes-128-gcm"] {
        let (link, decoded) = export_fixture(serde_json::json!({
            "type":"shadowsocks","tag":"SS","server":"2001:db8::3","server_port":8388,
            "method":method,"password":"key+/=:second@key","plugin":"v2ray-plugin","plugin_opts":"tls;host=cdn.example.com;path=/ws"
        }));
        assert_eq!(
            link_query(&link)["plugin"],
            "v2ray-plugin;tls;host=cdn.example.com;path=/ws"
        );
        assert_eq!(decoded.extra["password"], "key+/=:second@key");
        assert_eq!(decoded.extra["method"], method);
        if method.starts_with("2022-") {
            assert!(link.starts_with("ss://2022-blake3-aes-128-gcm:"));
        }
    }
}

#[test]
fn exports_quic_tls_and_https_family_extended_options() {
    for link in [
        "tuic://test-uuid:p%40ss%3Aword@node.example.com:443?congestion_control=bbr&udp_relay_mode=quic&udp_over_stream=true&zero_rtt_handshake=true&sni=sni.example.com&alpn=h3&allow_insecure=1#TUIC",
        "hysteria://node.example.com:443?auth=secret%26%23&upmbps=50&downmbps=100&obfs=xplus&sni=sni.example.com&alpn=h3&insecure=1#HY",
        "anytls://p%40ss%3Aword@node.example.com:443?sni=sni.example.com&alpn=h2&fp=chrome&insecure=1#ANY",
        "https://user:p%40ss%3Aword@node.example.com:443?sni=sni.example.com&alpn=h2&insecure=1#HTTPS",
        "naive+https://user:p%40ss%3Aword@node.example.com:443?insecure=1&sni=sni.example.com&alpn=h2#Naive",
    ] {
        let node = parse_node_link(link).unwrap();
        let exported = export_node_to_link(&node).unwrap();
        let decoded = parse_node_link(&exported).unwrap();
        assert_eq!(decoded.extra, node.extra);
        assert_eq!(decoded.extra["tls"]["insecure"], true);
    }
}

#[test]
fn refuses_incomplete_links_and_unrepresentable_dependencies() {
    let base = serde_json::json!({"type":"vless","tag":"Test","server":"node.example.com","server_port":443,"uuid":"test-uuid"});
    for (field, value, error_field) in [
        (
            "detour",
            serde_json::json!("another-profile::node"),
            "detour",
        ),
        (
            "tls",
            serde_json::json!({"enabled":true,"certificate_path":"C:/private/cert.pem"}),
            "certificate_path",
        ),
        (
            "transport",
            serde_json::json!({"type":"ws","headers":{"X-Secret":"not-shareable"}}),
            "X-Secret",
        ),
    ] {
        let mut value_node = base.clone();
        value_node[field] = value;
        let node: SingBoxOutbound = serde_json::from_value(value_node).unwrap();
        let error = export_node_to_link(&node).unwrap_err();
        assert!(error.contains(error_field), "{error}");
        assert!(!error.contains("not-shareable") && !error.contains("C:/private"));
    }
    for field in ["uuid", "server", "server_port"] {
        let mut value_node = base.clone();
        value_node.as_object_mut().unwrap().remove(field);
        assert!(export_node_to_link(&serde_json::from_value(value_node).unwrap()).is_err());
    }
    let mut unsupported = base;
    unsupported["type"] = serde_json::json!("wireguard");
    assert!(export_node_to_link(&serde_json::from_value(unsupported).unwrap()).is_err());
}

#[test]
fn websocket_early_data_uses_xray_compatible_path_for_all_three_protocols() {
    for kind in ["vless", "vmess", "trojan"] {
        let mut value = serde_json::json!({"type":kind,"tag":"WS ED","server":"node.example.com","server_port":443,
            "tls":{"enabled":true},"transport":{"type":"ws","path":"/ws?token=a%2Bb&flag=yes", "max_early_data":2560,"early_data_header_name":"Sec-WebSocket-Protocol"}});
        value[if kind == "trojan" { "password" } else { "uuid" }] =
            serde_json::json!("test-credential");
        let (link, decoded) = export_fixture(value.clone());
        let path = if kind == "vmess" {
            let payload: serde_json::Value = serde_json::from_slice(
                &decode_base64_compat(link.strip_prefix("vmess://").unwrap()).unwrap(),
            )
            .unwrap();
            payload["path"].as_str().unwrap().to_owned()
        } else {
            link_query(&link)["path"].clone()
        };
        assert_eq!(path, "/ws?token=a%2Bb&flag=yes&ed=2560");
        assert_eq!(decoded.extra["transport"], value["transport"]);
    }
}

#[test]
fn export_rejects_invalid_hosts_and_does_not_enable_tls_implicitly() {
    for server in [
        "example.com/path",
        "example.com#fragment",
        "example.com?query",
    ] {
        let node = serde_json::from_value(
            serde_json::json!({"type":"vless","server":server,"server_port":443,"uuid":"test"}),
        )
        .unwrap();
        assert!(export_node_to_link(&node).is_err());
    }
    let node = serde_json::from_value(serde_json::json!({"type":"trojan","server":"example.com","server_port":443,"password":"test"})).unwrap();
    assert!(export_node_to_link(&node).unwrap_err().contains("TLS"));
}

#[test]
fn ipv6_proxy_and_quic_exports_reimport_without_brackets_in_server() {
    for link in [
        "socks5://user:password@[2001:db8::1]:1080#IPv6",
        "https://user:password@[2001:db8::1]:443?sni=sni.example.com#IPv6",
        "tuic://uuid:password@[2001:db8::1]:443?sni=sni.example.com#IPv6",
        "anytls://password@[2001:db8::1]:443?sni=sni.example.com#IPv6",
        "hysteria2://password@[2001:db8::1]:443?sni=sni.example.com#IPv6",
    ] {
        let node = parse_node_link(link).unwrap();
        assert_eq!(node.server.as_deref(), Some("2001:db8::1"));
        let exported = export_node_to_link(&node).unwrap();
        assert!(exported.contains("@[2001:db8::1]:"));
        assert_eq!(parse_node_link(&exported).unwrap().server, node.server);
    }
}

#[test]
#[ignore = "read-only check of local node fixtures, requires KUNBOX_TEST_NODE_EXPORT_DIR"]
fn exports_local_node_fixtures() {
    let root = std::env::var_os("KUNBOX_TEST_NODE_EXPORT_DIR").expect("set fixture directory");
    let mut count = 0;
    let mut failures = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        let content = std::fs::read_to_string(path).unwrap();
        let nodes: Vec<SingBoxOutbound> =
            serde_json::from_str(&content).expect("node fixture array");
        for node in nodes {
            count += 1;
            if let Err(error) = export_node_to_link(&node) {
                *failures.entry(error).or_insert(0) += 1;
            }
        }
    }
    // Do not print names, credentials, addresses, or exported URIs.
    assert!(count > 0);
    assert!(
        failures.is_empty(),
        "unsupported field counts: {failures:?}"
    );
    println!("Successfully exported and re-imported {count} local nodes; no credentials logged");
}

#[test]
fn parse_ss_link_supports_plain_sip002_userinfo() {
    let node = parse_ss_link("ss://aes-128-gcm:plain-pass@example.com:8388#Plain").unwrap();

    assert_eq!(node.tag.as_deref(), Some("Plain"));
    assert_eq!(node.server.as_deref(), Some("example.com"));
    assert_eq!(node.server_port, Some(8388));
    assert_eq!(
        node.extra.get("method").and_then(|value| value.as_str()),
        Some("aes-128-gcm")
    );
    assert_eq!(
        node.extra.get("password").and_then(|value| value.as_str()),
        Some("plain-pass")
    );
}

#[test]
fn parse_ss_link_supports_percent_encoded_sip002_userinfo() {
    let node = parse_ss_link("ss://aes-128-gcm%3Aplain-pass@example.com:8388#Plain").unwrap();

    assert_eq!(
        node.extra.get("method").and_then(|value| value.as_str()),
        Some("aes-128-gcm")
    );
    assert_eq!(
        node.extra.get("password").and_then(|value| value.as_str()),
        Some("plain-pass")
    );
    assert_eq!(node.server.as_deref(), Some("example.com"));
    assert_eq!(node.server_port, Some(8388));
}

#[test]
fn parse_clash_proxies_rejects_out_of_range_port() {
    let proxies = vec![serde_json::json!({
        "name": "Bad Port",
        "type": "trojan",
        "server": "example.com",
        "port": 70000,
        "password": "pass"
    })];

    let nodes = parse_clash_proxies(&proxies).unwrap();

    assert!(nodes.is_empty());
}

#[test]
fn parse_clash_proxies_ignores_unsupported_ssr_nodes() {
    let proxies = vec![serde_json::json!({
        "name": "SSR",
        "type": "ssr",
        "server": "example.com",
        "port": 8388,
        "cipher": "aes-256-cfb",
        "password": "pass"
    })];

    let nodes = parse_clash_proxies(&proxies).unwrap();

    assert!(nodes.is_empty());
}

#[test]
fn parse_singbox_outbounds_ignores_unsupported_types() {
    let outbounds = vec![
        serde_json::json!({
            "type": "shadowsocksr",
            "tag": "SSR",
            "server": "example.com",
            "server_port": 8388
        }),
        serde_json::json!({
            "type": "trojan",
            "tag": "Trojan",
            "server": "example.com",
            "server_port": 443,
            "password": "pass"
        }),
    ];

    let nodes = parse_singbox_outbounds(&outbounds).unwrap();

    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].tag.as_deref(), Some("Trojan"));
}

#[test]
fn parse_vmess_link_rejects_out_of_range_port() {
    let json = serde_json::json!({
        "v": "2",
        "ps": "Bad VMess Port",
        "add": "example.com",
        "port": 70000,
        "id": "11111111-1111-1111-1111-111111111111",
        "aid": 0
    });
    let encoded = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        serde_json::to_string(&json).unwrap(),
    );

    assert!(parse_vmess_link(&format!("vmess://{}", encoded)).is_none());
}

#[test]
fn parse_tuic_link_preserves_required_fields() {
    let node = parse_node_link(
        "tuic://11111111-1111-1111-1111-111111111111:secret@example.com:443?congestion_control=bbr&udp_relay_mode=native&sni=tuic.example.com&alpn=h3&allow_insecure=1#TUIC",
    )
    .expect("expected tuic node");

    assert_eq!(node.outbound_type.as_deref(), Some("tuic"));
    assert_eq!(node.tag.as_deref(), Some("TUIC"));
    assert_eq!(node.server.as_deref(), Some("example.com"));
    assert_eq!(node.server_port, Some(443));
    assert_eq!(
        node.extra.get("uuid").and_then(|value| value.as_str()),
        Some("11111111-1111-1111-1111-111111111111")
    );
    assert_eq!(
        node.extra.get("password").and_then(|value| value.as_str()),
        Some("secret")
    );
    assert_eq!(
        node.extra
            .get("congestion_control")
            .and_then(|value| value.as_str()),
        Some("bbr")
    );
    assert_eq!(
        node.extra
            .get("udp_relay_mode")
            .and_then(|value| value.as_str()),
        Some("native")
    );
    let tls = node
        .extra
        .get("tls")
        .and_then(|value| value.as_object())
        .unwrap();
    assert_eq!(
        tls.get("server_name").and_then(|value| value.as_str()),
        Some("tuic.example.com")
    );
    assert_eq!(
        tls.get("insecure").and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        tls.get("alpn").and_then(|value| value.as_array()).unwrap()[0].as_str(),
        Some("h3")
    );
}

#[test]
fn normalize_duplicate_node_tags_keeps_first_and_suffixes_following_duplicates() {
    let nodes = vec![
        SingBoxOutbound {
            tag: Some("SG|官方优选|94ms".to_string()),
            outbound_type: Some("vless".to_string()),
            server: Some("a.example.com".to_string()),
            server_port: Some(443),
            extra: std::collections::HashMap::new(),
        },
        SingBoxOutbound {
            tag: Some("SG|官方优选|94ms".to_string()),
            outbound_type: Some("vless".to_string()),
            server: Some("b.example.com".to_string()),
            server_port: Some(443),
            extra: std::collections::HashMap::new(),
        },
        SingBoxOutbound {
            tag: Some("SG|官方优选|94ms".to_string()),
            outbound_type: Some("vless".to_string()),
            server: Some("c.example.com".to_string()),
            server_port: Some(443),
            extra: std::collections::HashMap::new(),
        },
    ];

    let normalized = normalize_duplicate_node_tags(nodes);
    let tags: Vec<&str> = normalized
        .iter()
        .filter_map(|node| node.tag.as_deref())
        .collect();

    assert_eq!(
        tags,
        vec![
            "SG|官方优选|94ms",
            "SG|官方优选|94ms #2",
            "SG|官方优选|94ms #3"
        ]
    );
}

#[test]
fn parse_vless_link_enables_ech_without_invalid_config_for_name_resolver() {
    let outbound = parse_vless_link("vless://11111111-1111-1111-1111-111111111111@example.com:443?security=tls&ech=cloudflare-ech.com+https%3A%2F%2Fdns.alidns.com%2Fdns-query&sni=example.com&host=ws.example.com&fp=chrome&type=ws&path=%2Fws#ECH")
        .expect("expected vless outbound");

    let tls = outbound
        .extra
        .get("tls")
        .and_then(|value| value.as_object())
        .expect("expected tls object");

    assert_eq!(
        tls.get("server_name").and_then(|value| value.as_str()),
        Some("example.com")
    );
    assert_eq!(
        tls.get("utls")
            .and_then(|value| value.get("fingerprint"))
            .and_then(|value| value.as_str()),
        Some("chrome")
    );
    assert_eq!(
        tls.get("ech")
            .and_then(|value| value.get("enabled"))
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        tls.get("ech")
            .and_then(|value| value.get("query_server_name"))
            .and_then(|value| value.as_str()),
        Some("cloudflare-ech.com")
    );
    assert!(
        tls.get("ech")
            .and_then(|value| value.get("config"))
            .is_none(),
        "name+resolver share value must not be serialized as invalid ech.config"
    );
    assert_eq!(
        outbound
            .extra
            .get(ECH_DNS_SERVER_META_KEY)
            .and_then(|value| value.as_str()),
        Some("https://dns.alidns.com/dns-query")
    );

    let transport = outbound
        .extra
        .get("transport")
        .and_then(|value| value.as_object())
        .expect("expected transport object");

    assert_eq!(
        transport.get("type").and_then(|value| value.as_str()),
        Some("ws")
    );
    assert_eq!(
        transport.get("path").and_then(|value| value.as_str()),
        Some("/ws")
    );
    assert_eq!(
        transport
            .get("headers")
            .and_then(|value| value.get("Host"))
            .and_then(|value| value.as_str()),
        Some("ws.example.com")
    );
}

#[test]
fn parse_vless_link_skips_empty_ech_value() {
    let outbound = parse_vless_link("vless://11111111-1111-1111-1111-111111111111@example.com:443?security=tls&ech=&sni=example.com#ECH-EMPTY")
        .expect("expected vless outbound");

    let tls = outbound
        .extra
        .get("tls")
        .and_then(|value| value.as_object())
        .expect("expected tls object");

    assert!(
        tls.get("ech").is_none(),
        "empty ech must not create dirty config"
    );
}

#[test]
fn parse_vless_link_serializes_pem_ech_config_as_array() {
    let outbound = parse_vless_link("vless://11111111-1111-1111-1111-111111111111@example.com:443?security=tls&ech=-----BEGIN%20ECHCONFIG-----%0Aabc123%0A-----END%20ECHCONFIG-----#ECH-PEM")
        .expect("expected vless outbound");

    let tls = outbound
        .extra
        .get("tls")
        .and_then(|value| value.as_object())
        .expect("expected tls object");

    let config = tls
        .get("ech")
        .and_then(|value| value.get("config"))
        .and_then(|value| value.as_array())
        .expect("expected ech config array");

    assert_eq!(config.len(), 3);
    assert_eq!(config[0].as_str(), Some("-----BEGIN ECHCONFIG-----"));
    assert_eq!(config[1].as_str(), Some("abc123"));
    assert_eq!(config[2].as_str(), Some("-----END ECHCONFIG-----"));
}

#[test]
fn parse_vless_link_does_not_treat_non_url_suffix_as_ech_resolver() {
    let outbound = parse_vless_link("vless://11111111-1111-1111-1111-111111111111@example.com:443?security=tls&ech=cloudflare-ech.com+not-a-url&sni=example.com#ECH-BAD-RESOLVER")
        .expect("expected vless outbound");

    let tls = outbound
        .extra
        .get("tls")
        .and_then(|value| value.as_object())
        .expect("expected tls object");

    assert_eq!(
        tls.get("ech")
            .and_then(|value| value.get("query_server_name")),
        None
    );
    assert_eq!(outbound.extra.get(ECH_DNS_SERVER_META_KEY), None);
}

#[test]
fn parse_vless_link_xhttp() {
    let link = "vless://2edd765b-a895-46ab-a01c-c4719947546b@35.194.192.123:13324?flow=xtls-rprx-vision&type=xhttp&path=%2F2edd765b-a895-46ab-a01c-c4719947546b-xh&mode=auto&extra=%7B%22encryption%22%3A%22mlkem768x25519plus.native.0rtt.test%22%2C%22noGRPCHeader%22%3Atrue%7D&sni=apple.com#%F0%9F%87%B9%F0%9F%87%BC%20%E5%8F%B0%E6%B9%BE%20GCP-xhttp";
    let outbound = parse_vless_link(link).expect("expected vless outbound");

    assert_eq!(outbound.tag.unwrap(), "🇹🇼 台湾 GCP-xhttp");
    assert_eq!(outbound.outbound_type.unwrap(), "vless");
    assert_eq!(outbound.server.unwrap(), "35.194.192.123");
    assert_eq!(outbound.server_port.unwrap(), 13324);
    assert_eq!(
        outbound.extra.get("flow").and_then(|v| v.as_str()),
        Some("xtls-rprx-vision")
    );
    assert_eq!(
        outbound.extra.get("encryption").and_then(|v| v.as_str()),
        Some("mlkem768x25519plus.native.0rtt.test")
    );

    let tls = outbound
        .extra
        .get("tls")
        .and_then(|v| v.as_object())
        .expect("expected inferred tls object");
    assert_eq!(
        tls.get("server_name").and_then(|v| v.as_str()),
        Some("apple.com")
    );

    let transport = outbound
        .extra
        .get("transport")
        .and_then(|v| v.as_object())
        .expect("expected transport object");

    assert_eq!(
        transport.get("type").and_then(|v| v.as_str()),
        Some("xhttp")
    );
    assert_eq!(
        transport.get("path").and_then(|v| v.as_str()),
        Some("/2edd765b-a895-46ab-a01c-c4719947546b-xh")
    );
    assert_eq!(transport.get("mode").and_then(|v| v.as_str()), Some("auto"));
    assert!(transport
        .get("extra")
        .and_then(|v| v.get("encryption"))
        .is_none());
    assert_eq!(
        transport
            .get("extra")
            .and_then(|v| v.get("noGRPCHeader"))
            .and_then(|v| v.as_bool()),
        Some(true)
    );
}
