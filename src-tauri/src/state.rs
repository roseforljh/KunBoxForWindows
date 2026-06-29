use crate::types::{AppSettings, CustomRules, ProfilesData, ProxyState, RuleSet, TrafficStats};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct AppState {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub profiles_data: Arc<Mutex<ProfilesData>>,
    pub settings: Arc<Mutex<AppSettings>>,
    pub rulesets: Arc<Mutex<Vec<RuleSet>>>,
    pub custom_rules: Arc<Mutex<CustomRules>>,
    pub proxy_state: Arc<Mutex<ProxyState>>,
    pub traffic_stats: Arc<Mutex<TrafficStats>>,
    pub singbox_process: Arc<Mutex<Option<tokio::process::Child>>>,
    pub plugin_processes: Arc<Mutex<Vec<tokio::process::Child>>>,
    pub start_time: Arc<Mutex<Option<u64>>>,
    pub traffic_cancel: Arc<Mutex<Option<CancellationToken>>>,
    pub health_cancel: Arc<Mutex<Option<CancellationToken>>>,
    pub shutdown_in_progress: Arc<Mutex<bool>>,
    pub lifecycle_lock: Arc<Mutex<()>>,
    pub clash_api_port: Arc<Mutex<u16>>,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Self {
        let config_dir = data_dir.clone();
        Self {
            data_dir,
            config_dir,
            profiles_data: Arc::new(Mutex::new(ProfilesData::default())),
            settings: Arc::new(Mutex::new(AppSettings::default())),
            rulesets: Arc::new(Mutex::new(Vec::new())),
            custom_rules: Arc::new(Mutex::new(CustomRules::default())),
            proxy_state: Arc::new(Mutex::new(ProxyState::Idle)),
            traffic_stats: Arc::new(Mutex::new(TrafficStats::default())),
            singbox_process: Arc::new(Mutex::new(None)),
            plugin_processes: Arc::new(Mutex::new(Vec::new())),
            start_time: Arc::new(Mutex::new(None)),
            traffic_cancel: Arc::new(Mutex::new(None)),
            health_cancel: Arc::new(Mutex::new(None)),
            shutdown_in_progress: Arc::new(Mutex::new(false)),
            lifecycle_lock: Arc::new(Mutex::new(())),
            clash_api_port: Arc::new(Mutex::new(9090)),
        }
    }

    pub fn profiles_file(&self) -> PathBuf {
        self.data_dir.join("profiles.json")
    }

    pub fn settings_file(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }

    pub fn rulesets_file(&self) -> PathBuf {
        self.data_dir.join("rulesets.json")
    }

    pub fn configs_dir(&self) -> PathBuf {
        self.data_dir.join("configs")
    }

    pub fn rulesets_cache_dir(&self) -> PathBuf {
        self.data_dir.join("rulesets")
    }

    pub fn custom_rules_file(&self) -> PathBuf {
        self.data_dir.join("custom_rules.json")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_path(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kunbox-state-{}-{}", name, suffix))
    }

    #[tokio::test]
    async fn app_state_initializes_empty_health_cancel() {
        let state = AppState::new(unique_test_path("health-cancel"));

        assert!(state.health_cancel.lock().await.is_none());
    }
}
