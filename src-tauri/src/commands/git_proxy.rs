use crate::state::AppState;
use crate::types::AppSettings;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Mutex;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

static GIT_PROXY_OPERATION_LOCK: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq)]
enum GitProxyPlan {
    Keep,
    Replace(Option<String>),
}

trait GitProxyBackend {
    fn read(&self) -> Result<Option<String>, String>;
    fn replace(&self, value: Option<&str>) -> Result<(), String>;
}

struct GitCliBackend {
    global_config_path: Option<PathBuf>,
}

impl GitCliBackend {
    fn new() -> Self {
        Self {
            global_config_path: None,
        }
    }

    #[cfg(test)]
    fn with_global_config(path: PathBuf) -> Self {
        Self {
            global_config_path: Some(path),
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new("git");
        if let Some(path) = &self.global_config_path {
            command.env("GIT_CONFIG_GLOBAL", path);
        }
        #[cfg(windows)]
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }

    fn command_error(output: &std::process::Output) -> String {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            format!("Git 配置命令返回 {}", output.status)
        } else {
            stderr
        }
    }
}

impl GitProxyBackend for GitCliBackend {
    fn read(&self) -> Result<Option<String>, String> {
        let output = self
            .command()
            .args(["config", "--global", "--get", "http.proxy"])
            .output()
            .map_err(|err| format!("无法执行 Git: {}", err))?;
        if output.status.success() {
            let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
            return Ok((!value.is_empty()).then_some(value));
        }
        if output.status.code() == Some(1) {
            return Ok(None);
        }
        Err(Self::command_error(&output))
    }

    fn replace(&self, value: Option<&str>) -> Result<(), String> {
        if value.is_none() && self.read()?.is_none() {
            return Ok(());
        }
        let mut command = self.command();
        command.args(["config", "--global"]);
        match value {
            Some(value) => {
                command.args(["--replace-all", "http.proxy", value]);
            }
            None => {
                command.args(["--unset-all", "http.proxy"]);
            }
        }
        let output = command
            .output()
            .map_err(|err| format!("无法执行 Git: {}", err))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Self::command_error(&output))
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct GitProxySnapshot {
    original: Option<String>,
    managed: Option<String>,
}

fn git_proxy_snapshot_path(state: &AppState) -> PathBuf {
    state.data_dir.join("git_proxy_snapshot.json")
}

fn save_git_proxy_snapshot(state: &AppState, snapshot: &GitProxySnapshot) -> Result<(), String> {
    fs::create_dir_all(&state.data_dir).map_err(|err| err.to_string())?;
    let content = serde_json::to_vec(snapshot).map_err(|err| err.to_string())?;
    fs::write(git_proxy_snapshot_path(state), content).map_err(|err| err.to_string())
}

fn load_git_proxy_snapshot(state: &AppState) -> Result<Option<GitProxySnapshot>, String> {
    let path = git_proxy_snapshot_path(state);
    if !path.exists() {
        return Ok(None);
    }
    let content = fs::read(path).map_err(|err| err.to_string())?;
    serde_json::from_slice(&content)
        .map(Some)
        .map_err(|err| err.to_string())
}

fn clear_git_proxy_snapshot(state: &AppState) -> Result<(), String> {
    let path = git_proxy_snapshot_path(state);
    if path.exists() {
        fs::remove_file(path).map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn is_loopback_proxy(value: &str) -> bool {
    let value = value.trim();
    let normalized = if value.contains("://") {
        value.to_string()
    } else {
        format!("http://{}", value)
    };
    url::Url::parse(&normalized)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .is_some_and(|host| matches!(host.as_str(), "127.0.0.1" | "localhost" | "::1"))
}

fn git_proxy_plan(settings: &AppSettings, current: Option<&str>) -> GitProxyPlan {
    if settings.tun_enabled && current.is_some_and(is_loopback_proxy) {
        return GitProxyPlan::Replace(None);
    }

    if !settings.tun_enabled
        && settings.system_proxy
        && current.map(is_loopback_proxy).unwrap_or(true)
    {
        let desired = format!("http://127.0.0.1:{}", settings.local_port);
        if current != Some(desired.as_str()) {
            return GitProxyPlan::Replace(Some(desired));
        }
    }

    GitProxyPlan::Keep
}

fn sync_git_proxy_with_backend(
    state: &AppState,
    settings: &AppSettings,
    backend: &impl GitProxyBackend,
) -> Result<(), String> {
    restore_git_proxy_with_backend(state, backend)?;
    let current = backend.read()?;
    let GitProxyPlan::Replace(managed) = git_proxy_plan(settings, current.as_deref()) else {
        return Ok(());
    };

    save_git_proxy_snapshot(
        state,
        &GitProxySnapshot {
            original: current,
            managed: managed.clone(),
        },
    )?;
    if let Err(err) = backend.replace(managed.as_deref()) {
        let _ = clear_git_proxy_snapshot(state);
        return Err(err);
    }
    Ok(())
}

fn restore_git_proxy_with_backend(
    state: &AppState,
    backend: &impl GitProxyBackend,
) -> Result<(), String> {
    let Some(snapshot) = load_git_proxy_snapshot(state)? else {
        return Ok(());
    };
    let current = backend.read()?;
    if current == snapshot.managed {
        backend.replace(snapshot.original.as_deref())?;
    }
    clear_git_proxy_snapshot(state)
}

pub(crate) fn sync_for_connection(state: &AppState, settings: &AppSettings) -> Result<(), String> {
    let _guard = GIT_PROXY_OPERATION_LOCK
        .lock()
        .map_err(|_| "Git 代理操作锁失败".to_string())?;
    sync_git_proxy_with_backend(state, settings, &GitCliBackend::new())
}

pub(crate) fn restore_after_disconnect(state: &AppState) -> Result<(), String> {
    let _guard = GIT_PROXY_OPERATION_LOCK
        .lock()
        .map_err(|_| "Git 代理操作锁失败".to_string())?;
    restore_git_proxy_with_backend(state, &GitCliBackend::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use crate::types::AppSettings;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct MemoryGitProxy {
        value: RefCell<Option<String>>,
    }

    impl MemoryGitProxy {
        fn new(value: Option<&str>) -> Self {
            Self {
                value: RefCell::new(value.map(str::to_string)),
            }
        }

        fn current(&self) -> Option<String> {
            self.value.borrow().clone()
        }
    }

    impl GitProxyBackend for MemoryGitProxy {
        fn read(&self) -> Result<Option<String>, String> {
            Ok(self.current())
        }

        fn replace(&self, value: Option<&str>) -> Result<(), String> {
            *self.value.borrow_mut() = value.map(str::to_string);
            Ok(())
        }
    }

    fn unique_test_path(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kunbox-git-proxy-{}-{}", name, suffix))
    }

    #[test]
    fn tun_connection_clears_stale_loopback_git_proxy() {
        let mut settings = AppSettings::default();
        settings.tun_enabled = true;
        settings.system_proxy = false;

        assert_eq!(
            git_proxy_plan(&settings, Some("http://127.0.0.1:2581")),
            GitProxyPlan::Replace(None)
        );
    }

    #[test]
    fn system_proxy_connection_tracks_effective_http_port() {
        let mut settings = AppSettings::default();
        settings.tun_enabled = false;
        settings.system_proxy = true;
        settings.local_port = 13_635;

        assert_eq!(
            git_proxy_plan(&settings, Some("http://127.0.0.1:2581")),
            GitProxyPlan::Replace(Some("http://127.0.0.1:13635".to_string()))
        );
    }

    #[test]
    fn connection_preserves_remote_git_proxy() {
        let data_dir = unique_test_path("remote-proxy");
        let state = AppState::new(data_dir.clone());
        let backend = MemoryGitProxy::new(Some("http://proxy.example.com:8080"));
        let mut settings = AppSettings::default();
        settings.tun_enabled = false;
        settings.system_proxy = true;
        settings.local_port = 13_635;

        sync_git_proxy_with_backend(&state, &settings, &backend).unwrap();

        assert_eq!(
            backend.current().as_deref(),
            Some("http://proxy.example.com:8080")
        );
        assert!(!git_proxy_snapshot_path(&state).exists());

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn connection_sync_is_restored_after_disconnect() {
        let data_dir = unique_test_path("restore");
        let state = AppState::new(data_dir.clone());
        let backend = MemoryGitProxy::new(Some("http://127.0.0.1:2581"));
        let mut settings = AppSettings::default();
        settings.tun_enabled = false;
        settings.system_proxy = true;
        settings.local_port = 13_635;

        sync_git_proxy_with_backend(&state, &settings, &backend).unwrap();
        assert_eq!(backend.current().as_deref(), Some("http://127.0.0.1:13635"));

        restore_git_proxy_with_backend(&state, &backend).unwrap();
        assert_eq!(backend.current().as_deref(), Some("http://127.0.0.1:2581"));
        assert!(!git_proxy_snapshot_path(&state).exists());

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn disconnect_preserves_git_proxy_changed_by_user() {
        let data_dir = unique_test_path("user-change");
        let state = AppState::new(data_dir.clone());
        let backend = MemoryGitProxy::new(Some("http://127.0.0.1:2581"));
        let mut settings = AppSettings::default();
        settings.tun_enabled = false;
        settings.system_proxy = true;
        settings.local_port = 13_635;

        sync_git_proxy_with_backend(&state, &settings, &backend).unwrap();
        backend
            .replace(Some("http://proxy.example.com:8080"))
            .unwrap();
        restore_git_proxy_with_backend(&state, &backend).unwrap();

        assert_eq!(
            backend.current().as_deref(),
            Some("http://proxy.example.com:8080")
        );
        assert!(!git_proxy_snapshot_path(&state).exists());

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn next_connection_recovers_snapshot_before_tracking_new_port() {
        let data_dir = unique_test_path("crash-recovery");
        let state = AppState::new(data_dir.clone());
        let backend = MemoryGitProxy::new(Some("http://127.0.0.1:2581"));
        let mut first_settings = AppSettings::default();
        first_settings.tun_enabled = false;
        first_settings.system_proxy = true;
        first_settings.local_port = 13_635;

        sync_git_proxy_with_backend(&state, &first_settings, &backend).unwrap();

        let mut next_settings = first_settings.clone();
        next_settings.local_port = 14_000;
        sync_git_proxy_with_backend(&state, &next_settings, &backend).unwrap();
        assert_eq!(backend.current().as_deref(), Some("http://127.0.0.1:14000"));

        restore_git_proxy_with_backend(&state, &backend).unwrap();
        assert_eq!(backend.current().as_deref(), Some("http://127.0.0.1:2581"));

        let _ = std::fs::remove_dir_all(data_dir);
    }

    #[test]
    fn git_cli_backend_reads_writes_and_clears_isolated_global_proxy() {
        let data_dir = unique_test_path("git-cli");
        std::fs::create_dir_all(&data_dir).unwrap();
        let backend = GitCliBackend::with_global_config(data_dir.join("gitconfig"));

        assert_eq!(backend.read().unwrap(), None);
        backend.replace(Some("http://127.0.0.1:13635")).unwrap();
        assert_eq!(
            backend.read().unwrap().as_deref(),
            Some("http://127.0.0.1:13635")
        );
        backend.replace(None).unwrap();
        assert_eq!(backend.read().unwrap(), None);

        let _ = std::fs::remove_dir_all(data_dir);
    }
}
