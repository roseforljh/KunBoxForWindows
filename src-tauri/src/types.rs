use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ProxyState {
    #[default]
    Idle,
    Connecting,
    Connected,
    Disconnecting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TrafficStats {
    #[serde(rename = "uploadSpeed")]
    pub upload_speed: u64,
    #[serde(rename = "downloadSpeed")]
    pub download_speed: u64,
    #[serde(rename = "uploadTotal")]
    pub upload_total: u64,
    #[serde(rename = "downloadTotal")]
    pub download_total: u64,
    pub duration: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Unknown,
    Healthy,
    Suspect,
    Failed,
    Recovering,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthEventKind {
    SelectorFailedOver,
    SelectorNoBackup,
    FixedNodeFailed,
    MainNodeNeedsManualSwitch,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthEvent {
    pub kind: HealthEventKind,
    pub selector: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub node: Option<String>,
    pub rule: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub url: String,
    #[serde(rename = "lastUpdate")]
    pub last_update: Option<u64>,
    #[serde(rename = "nodeCount")]
    pub node_count: u32,
    pub enabled: bool,
    #[serde(rename = "autoUpdateInterval")]
    pub auto_update_interval: u32,
    #[serde(rename = "dnsPreResolve")]
    pub dns_pre_resolve: bool,
    #[serde(rename = "dnsServer")]
    pub dns_server: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilesData {
    pub profiles: Vec<Profile>,
    #[serde(rename = "activeProfileId")]
    pub active_profile_id: Option<String>,
    #[serde(rename = "activeNodeTag")]
    pub active_node_tag: Option<String>,
    #[serde(rename = "nodeSelections", default)]
    pub node_selections: HashMap<String, String>,
}

impl Default for ProfilesData {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            active_profile_id: None,
            active_node_tag: None,
            node_selections: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(rename = "localPort")]
    pub local_port: u16,
    #[serde(rename = "socksPort")]
    pub socks_port: u16,
    #[serde(rename = "allowLan")]
    pub allow_lan: bool,
    #[serde(rename = "systemProxy")]
    pub system_proxy: bool,
    #[serde(rename = "tunEnabled")]
    pub tun_enabled: bool,
    #[serde(rename = "tunStack")]
    pub tun_stack: String,
    #[serde(rename = "tunStrictRoute", default)]
    pub tun_strict_route: bool,
    #[serde(rename = "localDns")]
    pub local_dns: String,
    #[serde(rename = "remoteDns")]
    pub remote_dns: String,
    #[serde(rename = "fakeDns")]
    pub fake_dns: bool,
    #[serde(rename = "bypassLan")]
    pub bypass_lan: bool,
    #[serde(rename = "routingMode")]
    pub routing_mode: String,
    #[serde(rename = "defaultRule")]
    pub default_rule: String,
    #[serde(rename = "latencyTestUrl")]
    pub latency_test_url: String,
    #[serde(rename = "latencyTestTimeout")]
    pub latency_test_timeout: u32,
    #[serde(
        rename = "healthMonitorEnabled",
        default = "default_health_monitor_enabled"
    )]
    pub health_monitor_enabled: bool,
    #[serde(rename = "mainNodeAutoFailover", default)]
    pub main_node_auto_failover: bool,
    #[serde(
        rename = "healthProbeIntervalSec",
        default = "default_health_probe_interval_sec"
    )]
    pub health_probe_interval_sec: u64,
    #[serde(rename = "autoConnect")]
    pub auto_connect: bool,
    #[serde(rename = "minimizeToTray")]
    pub minimize_to_tray: bool,
    #[serde(rename = "startWithWindows")]
    pub start_with_windows: bool,
    #[serde(rename = "startMinimized")]
    pub start_minimized: bool,
    #[serde(rename = "silentStart", default)]
    pub silent_start: bool,
    #[serde(rename = "exitOnClose")]
    pub exit_on_close: bool,
    pub theme: String,
    /// If true, the app will auto-restart as admin on startup when not elevated
    #[serde(rename = "requireAdmin", default)]
    pub require_admin: bool,
    #[serde(rename = "enableRuntimeLogs", default = "default_enable_runtime_logs")]
    pub enable_runtime_logs: bool,
}

fn default_enable_runtime_logs() -> bool {
    true
}

fn default_health_monitor_enabled() -> bool {
    true
}

fn default_health_probe_interval_sec() -> u64 {
    15
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            local_port: 7890,
            socks_port: 7891,
            allow_lan: false,
            system_proxy: true,
            tun_enabled: false,
            tun_stack: "mixed".to_string(),
            tun_strict_route: true,
            local_dns: "223.5.5.5".to_string(),
            remote_dns: "https://dns.google/dns-query".to_string(),
            fake_dns: false,
            bypass_lan: true,
            routing_mode: "rule".to_string(),
            default_rule: "proxy".to_string(),
            latency_test_url: "https://www.gstatic.com/generate_204".to_string(),
            latency_test_timeout: 5000,
            health_monitor_enabled: true,
            main_node_auto_failover: false,
            health_probe_interval_sec: 15,
            auto_connect: false,
            minimize_to_tray: true,
            start_with_windows: false,
            start_minimized: false,
            silent_start: false,
            exit_on_close: false,
            theme: "dark".to_string(),
            require_admin: false,
            enable_runtime_logs: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SingBoxOutbound {
    pub tag: Option<String>,
    #[serde(rename = "type")]
    pub outbound_type: Option<String>,
    pub server: Option<String>,
    pub server_port: Option<u16>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuleSet {
    pub id: String,
    pub tag: String,
    pub name: String,
    #[serde(rename = "type")]
    pub rule_type: String,
    pub format: String,
    pub url: Option<String>,
    #[serde(rename = "outboundMode")]
    pub outbound_mode: String,
    #[serde(rename = "outboundValue")]
    pub outbound_value: Option<String>,
    pub enabled: bool,
    #[serde(rename = "isBuiltIn")]
    pub is_built_in: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

impl CommandResult {
    pub fn ok() -> Self {
        Self {
            success: true,
            error: None,
            warning: None,
        }
    }

    pub fn ok_with_warning(msg: impl Into<String>) -> Self {
        Self {
            success: true,
            error: None,
            warning: Some(msg.into()),
        }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            error: Some(msg.into()),
            warning: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NodeLatencyStatus {
    Success,
    Timeout,
    ControllerUnavailable,
    ProxyFailed,
    LocalTestFailed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NodeLatencyResult {
    pub status: NodeLatencyStatus,
    #[serde(rename = "latencyMs", skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<i64>,
}

impl NodeLatencyResult {
    pub fn success(latency_ms: i64) -> Self {
        Self {
            status: NodeLatencyStatus::Success,
            latency_ms: Some(latency_ms),
        }
    }

    pub fn timeout() -> Self {
        Self {
            status: NodeLatencyStatus::Timeout,
            latency_ms: None,
        }
    }

    pub fn controller_unavailable() -> Self {
        Self {
            status: NodeLatencyStatus::ControllerUnavailable,
            latency_ms: None,
        }
    }

    pub fn proxy_failed() -> Self {
        Self {
            status: NodeLatencyStatus::ProxyFailed,
            latency_ms: None,
        }
    }

    pub fn local_test_failed() -> Self {
        Self {
            status: NodeLatencyStatus::LocalTestFailed,
            latency_ms: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainRule {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub rule_type: String,
    pub value: String,
    #[serde(rename = "outboundMode")]
    pub outbound_mode: String,
    #[serde(rename = "outboundValue")]
    pub outbound_value: Option<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomRules {
    #[serde(rename = "domainRules")]
    pub domain_rules: Vec<DomainRule>,
}

impl Default for CustomRules {
    fn default() -> Self {
        Self {
            domain_rules: vec![
                DomainRule {
                    id: "default-localhost".to_string(),
                    name: "localhost".to_string(),
                    rule_type: "domain".to_string(),
                    value: "localhost".to_string(),
                    outbound_mode: "direct".to_string(),
                    outbound_value: None,
                    enabled: true,
                },
                DomainRule {
                    id: "default-localhost-v4".to_string(),
                    name: "127.0.0.1".to_string(),
                    rule_type: "domain".to_string(),
                    value: "127.0.0.1".to_string(),
                    outbound_mode: "direct".to_string(),
                    outbound_value: None,
                    enabled: true,
                },
                DomainRule {
                    id: "default-localhost-v6".to_string(),
                    name: "::1".to_string(),
                    rule_type: "domain".to_string(),
                    value: "::1".to_string(),
                    outbound_mode: "direct".to_string(),
                    outbound_value: None,
                    enabled: true,
                },
                DomainRule {
                    id: "default-local-suffix".to_string(),
                    name: ".local".to_string(),
                    rule_type: "domain_suffix".to_string(),
                    value: "local".to_string(),
                    outbound_mode: "direct".to_string(),
                    outbound_value: None,
                    enabled: true,
                },
            ],
        }
    }
}
