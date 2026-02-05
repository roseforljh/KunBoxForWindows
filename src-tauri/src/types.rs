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
}

impl Default for ProfilesData {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            active_profile_id: None,
            active_node_tag: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: u64,
    pub level: String,
    pub tag: String,
    pub message: String,
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
    #[serde(rename = "localDns")]
    pub local_dns: String,
    #[serde(rename = "remoteDns")]
    pub remote_dns: String,
    #[serde(rename = "fakeDns")]
    pub fake_dns: bool,
    #[serde(rename = "blockAds")]
    pub block_ads: bool,
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
    #[serde(rename = "autoConnect")]
    pub auto_connect: bool,
    #[serde(rename = "minimizeToTray")]
    pub minimize_to_tray: bool,
    #[serde(rename = "startWithWindows")]
    pub start_with_windows: bool,
    #[serde(rename = "startMinimized")]
    pub start_minimized: bool,
    #[serde(rename = "exitOnClose")]
    pub exit_on_close: bool,
    pub theme: String,
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
            local_dns: "223.5.5.5".to_string(),
            remote_dns: "https://dns.google/dns-query".to_string(),
            fake_dns: false,
            block_ads: false,
            bypass_lan: true,
            routing_mode: "rule".to_string(),
            default_rule: "proxy".to_string(),
            latency_test_url: "https://www.gstatic.com/generate_204".to_string(),
            latency_test_timeout: 5000,
            auto_connect: false,
            minimize_to_tray: true,
            start_with_windows: false,
            start_minimized: false,
            exit_on_close: false,
            theme: "dark".to_string(),
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
}

impl CommandResult {
    pub fn ok() -> Self {
        Self { success: true, error: None }
    }

    pub fn err(msg: impl Into<String>) -> Self {
        Self { success: false, error: Some(msg.into()) }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelVersion {
    pub version: String,
    pub tag: String,
    #[serde(rename = "isAlpha")]
    pub is_alpha: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubRelease {
    pub tag_name: String,
    pub name: String,
    pub prerelease: bool,
    pub published_at: String,
    pub assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GithubAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
}
