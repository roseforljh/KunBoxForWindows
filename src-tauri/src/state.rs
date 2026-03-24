use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use crate::types::{AppSettings, ProfilesData, RuleSet, ProxyState, TrafficStats, CustomRules};

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
    pub start_time: Arc<Mutex<Option<u64>>>,
    pub traffic_cancel: Arc<Mutex<Option<CancellationToken>>>,
    pub shutdown_in_progress: Arc<Mutex<bool>>,
    pub lifecycle_lock: Arc<Mutex<()>>,
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
            start_time: Arc::new(Mutex::new(None)),
            traffic_cancel: Arc::new(Mutex::new(None)),
            shutdown_in_progress: Arc::new(Mutex::new(false)),
            lifecycle_lock: Arc::new(Mutex::new(())),
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
