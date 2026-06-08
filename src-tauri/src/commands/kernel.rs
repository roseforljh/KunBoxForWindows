use tauri::{AppHandle, Emitter, Manager, State};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use crate::state::AppState;
use crate::types::ProxyState;
use super::singbox::{singbox_start_impl, singbox_stop_impl};

#[cfg(windows)]
#[allow(unused_imports)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const KERNEL_FILENAME: &str = "sing-box.exe";

// GitHub 镜像源列表
const GITHUB_MIRRORS: &[&str] = &[
    "",                                    // 原始 GitHub（无镜像）
    "https://ghfast.top/",                 // ghfast 镜像
    "https://gh.llkk.cc/",                 // llkk 镜像
    "https://ghp.ci/",                     // ghp.ci 镜像
    "https://cf.ghproxy.cc/",              // cf ghproxy
];

// GitHub API 镜像源列表
const GITHUB_API_MIRRORS: &[&str] = &[
    "https://api.github.com",              // 原始 API
    "https://ghfast.top/https://api.github.com",  // ghfast 代理
];

const VERSION_FALLBACK_API: &str = "https://data.jsdelivr.com/v1/package/gh/SagerNet/sing-box";
const MAX_KERNEL_DOWNLOAD_SIZE_BYTES: u64 = 100 * 1024 * 1024;

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KernelVersion {
    pub version: String,
    pub version_detail: String,
    pub is_alpha: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct KernelCapabilities {
    pub version: String,
    pub supports_naive: bool,
    pub supports_icmp_proxy: bool,
    pub supports_bypass_action: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RemoteRelease {
    pub version: String,
    pub tag_name: String,
    pub published_at: String,
    pub is_prerelease: bool,
    pub download_url: String,
    pub asset_name: String,
}

#[derive(serde::Deserialize, Debug)]
struct GithubRelease {
    tag_name: String,
    published_at: String,
    prerelease: bool,
    assets: Vec<GithubAsset>,
}

#[derive(serde::Deserialize, Debug)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

#[derive(serde::Deserialize, Debug)]
struct JsDelivrPackage {
    versions: Vec<String>,
}

fn get_data_kernel_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(app_data_dir.join("libs"))
}

fn get_bundle_kernel_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    Ok(resource_dir.join("resources").join("libs"))
}

fn resolve_kernel_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let data_dir = get_data_kernel_dir(app)?;
    if data_dir.join(KERNEL_FILENAME).exists() {
        return Ok(data_dir);
    }

    let bundle_dir = get_bundle_kernel_dir(app)?;
    if bundle_dir.join(KERNEL_FILENAME).exists() {
        return Ok(bundle_dir);
    }

    Err("未检测到 sing-box 内核，请先到【设置 → 内核】下载并安装。".to_string())
}

fn get_kernel_dir_for_install(app: &AppHandle) -> Result<PathBuf, String> {
    get_data_kernel_dir(app)
}

fn get_kernel_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(resolve_kernel_dir(app)?.join(KERNEL_FILENAME))
}

fn path_exists(path: &Path) -> bool {
    fs::metadata(path).is_ok()
}

#[cfg(windows)]
fn kernel_support_file_available(kernel_path: &Path, filename: &str) -> bool {
    if kernel_path
        .parent()
        .is_some_and(|dir| path_exists(&dir.join(filename)))
    {
        return true;
    }

    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|path| path_exists(&path.join(filename))))
        .unwrap_or(false)
}

#[cfg(not(windows))]
fn kernel_support_file_available(_kernel_path: &Path, _filename: &str) -> bool {
    true
}

fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }

    if path.is_dir() {
        fs::remove_dir_all(path).map_err(|e| e.to_string())?;
    } else {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn kernel_download_archive_path(kernel_dir: &Path) -> PathBuf {
    kernel_dir.join("sing-box.download.zip")
}

fn kernel_extract_temp_path(kernel_dir: &Path, filename: &str) -> PathBuf {
    kernel_dir.join(format!("{}.download", filename))
}

fn kernel_cache_targets(state: &AppState) -> Vec<PathBuf> {
    vec![
        state.config_dir.join("cache.db"),
        state.rulesets_cache_dir(),
        state.data_dir.join("cache"),
    ]
}

fn clear_kernel_cache_targets(paths: &[PathBuf]) -> Result<u64, String> {
    let mut freed_bytes = 0;

    for path in paths {
        if !path.exists() {
            continue;
        }

        freed_bytes += if path.is_dir() {
            get_dir_size(path)
        } else {
            path.metadata().map(|meta| meta.len()).unwrap_or(0)
        };

        remove_path_if_exists(path)?;
    }

    Ok(freed_bytes)
}

async fn download_kernel_archive_to_path(
    app: &AppHandle,
    response: reqwest::Response,
    archive_path: &Path,
) -> Result<(), String> {
    let total_size = response.content_length().unwrap_or(0);
    if total_size > MAX_KERNEL_DOWNLOAD_SIZE_BYTES {
        let err = "内核安装包过大，已拒绝下载";
        let _ = app.emit("kernel:download-error", err);
        return Err(err.to_string());
    }

    let mut file = fs::File::create(archive_path).map_err(|e| e.to_string())?;
    let mut downloaded = 0u64;
    let mut stream = response.bytes_stream();

    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        downloaded += chunk.len() as u64;

        if downloaded > MAX_KERNEL_DOWNLOAD_SIZE_BYTES {
            let err = "内核安装包过大，已拒绝下载";
            let _ = app.emit("kernel:download-error", err);
            return Err(err.to_string());
        }

        if total_size > 0 {
            let progress = serde_json::json!({
                "downloaded": downloaded,
                "total": total_size,
                "percent": (downloaded as f64 / total_size as f64 * 100.0) as u32
            });
            let _ = app.emit("kernel:download-progress", progress);
        }
    }

    file.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn extract_kernel_archive(
    archive_path: &Path,
    kernel_path: &Path,
    cronet_path: &Path,
) -> Result<bool, String> {
    remove_path_if_exists(kernel_path)?;
    remove_path_if_exists(cronet_path)?;

    let archive_file = fs::File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(archive_file).map_err(|e| e.to_string())?;
    let mut found_kernel = false;
    let mut found_cronet = false;

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().to_string();

        if name.ends_with("sing-box.exe") {
            let mut output = fs::File::create(kernel_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut output).map_err(|e| e.to_string())?;
            output.flush().map_err(|e| e.to_string())?;
            found_kernel = true;
            continue;
        }

        if name.ends_with("libcronet.dll") {
            let mut output = fs::File::create(cronet_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut output).map_err(|e| e.to_string())?;
            output.flush().map_err(|e| e.to_string())?;
            found_cronet = true;
        }
    }

    if !found_kernel {
        remove_path_if_exists(kernel_path)?;
    }
    if !found_cronet {
        let _ = remove_path_if_exists(cronet_path);
    }

    Ok(found_cronet)
}

fn replace_kernel_file_from_path(kernel_dir: &Path, source_path: &Path) -> Result<(), String> {
    let kernel_path = kernel_dir.join(KERNEL_FILENAME);
    let backup_path = kernel_dir.join("sing-box.exe.bak");

    let mut last_error: Option<String> = None;

    for _ in 0..6 {
        if backup_path.exists() {
            let _ = fs::remove_file(&backup_path);
        }

        if kernel_path.exists() {
            match fs::rename(&kernel_path, &backup_path) {
                Ok(_) => {}
                Err(err) => {
                    last_error = Some(err.to_string());
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    continue;
                }
            }
        }

        match fs::rename(source_path, &kernel_path) {
            Ok(_) => return Ok(()),
            Err(err) => {
                last_error = Some(err.to_string());
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
        }
    }

    Err(last_error.unwrap_or_else(|| "替换内核文件失败".to_string()))
}

fn replace_support_file(target_path: &Path, source_path: &Path) -> Result<(), String> {
    if target_path.exists() {
        fs::remove_file(target_path).map_err(|e| e.to_string())?;
    }
    fs::rename(source_path, target_path).map_err(|e| e.to_string())
}

fn find_windows_asset<'a>(assets: &'a [GithubAsset], tag_name: &str) -> Option<&'a GithubAsset> {
    let version = tag_name.trim_start_matches('v');
    let expected_name = format!("sing-box-{}-windows-amd64.zip", version);
    assets.iter().find(|a| a.name == expected_name)
}

fn parse_semver_triplet(version: &str) -> Option<(u64, u64, u64)> {
    let raw = version.trim().trim_start_matches('v');
    let core = raw.split('-').next().unwrap_or(raw);
    let mut it = core.split('.');
    let major = it.next()?.parse::<u64>().ok()?;
    let minor = it.next()?.parse::<u64>().ok()?;
    let patch = it.next().unwrap_or("0").parse::<u64>().ok()?;
    Some((major, minor, patch))
}

fn version_gte(version: &str, min: (u64, u64, u64)) -> bool {
    parse_semver_triplet(version)
        .map(|v| v >= min)
        .unwrap_or(false)
}

fn is_prerelease_version(version: &str) -> bool {
    version.contains('-')
}

fn build_release_from_version(version: &str, is_prerelease: bool) -> RemoteRelease {
    let tag = format!("v{}", version);
    let asset_name = format!("sing-box-{}-windows-amd64.zip", version);
    let download_url = format!(
        "https://github.com/SagerNet/sing-box/releases/download/{}/{}",
        tag, asset_name
    );

    RemoteRelease {
        version: version.to_string(),
        tag_name: tag,
        published_at: chrono::Utc::now().to_rfc3339(),
        is_prerelease,
        download_url,
        asset_name,
    }
}

async fn fetch_release_fallback_from_jsdelivr(
    client: &reqwest::Client,
    include_prerelease: bool,
) -> Result<Vec<RemoteRelease>, String> {
    let pkg = client
        .get(VERSION_FALLBACK_API)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<JsDelivrPackage>()
        .await
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();

    if let Some(stable) = pkg.versions.iter().find(|v| !is_prerelease_version(v)) {
        result.push(build_release_from_version(stable, false));
    }

    if include_prerelease {
        if let Some(pre) = pkg.versions.iter().find(|v| is_prerelease_version(v)) {
            result.push(build_release_from_version(pre, true));
        }
    }

    Ok(result)
}

#[tauri::command]
pub async fn kernel_get_local_version(app: AppHandle) -> Result<Option<KernelVersion>, String> {
    let kernel_path = match get_kernel_path(&app) {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    
    if !kernel_path.exists() {
        return Ok(None);
    }
    
    #[cfg(windows)]
    let output = tokio::process::Command::new(&kernel_path)
        .arg("version")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    #[cfg(not(windows))]
    let output = tokio::process::Command::new(&kernel_path)
        .arg("version")
        .output()
        .await
        .map_err(|e| e.to_string())?;
    
    if output.status.success() {
        let version_str = String::from_utf8_lossy(&output.stdout);
        let version_detail = version_str.trim().to_string();
        
        // Parse version from output like "sing-box version 1.8.0"
        let version = version_str
            .lines()
            .find(|line| line.contains("version"))
            .and_then(|line| line.split_whitespace().last())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        
        return Ok(Some(KernelVersion {
            version,
            version_detail,
            is_alpha: false,
        }));
    }
    
    Ok(None)
}

#[tauri::command]
pub async fn kernel_get_capabilities(app: AppHandle) -> Result<KernelCapabilities, String> {
    let kernel_path = match get_kernel_path(&app) {
        Ok(p) => p,
        Err(_) => return Ok(KernelCapabilities::default()),
    };
    if !kernel_path.exists() {
        return Ok(KernelCapabilities::default());
    }

    #[cfg(windows)]
    let output = tokio::process::Command::new(&kernel_path)
        .arg("version")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    #[cfg(not(windows))]
    let output = tokio::process::Command::new(&kernel_path)
        .arg("version")
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let version = if output.status.success() {
        let version_str = String::from_utf8_lossy(&output.stdout);
        version_str
            .lines()
            .find(|line| line.contains("version"))
            .and_then(|line| line.split_whitespace().last())
            .unwrap_or("unknown")
            .to_string()
    } else {
        "unknown".to_string()
    };

    let supports_naive = version_gte(&version, (1, 13, 0))
        && kernel_support_file_available(&kernel_path, "libcronet.dll");

    Ok(KernelCapabilities {
        version: version.clone(),
        supports_naive,
        supports_icmp_proxy: version_gte(&version, (1, 13, 0)),
        supports_bypass_action: version_gte(&version, (1, 13, 0)),
    })
}

async fn fetch_trusted_remote_releases(include_prerelease: bool) -> Result<Vec<RemoteRelease>, String> {
    let client = reqwest::Client::builder()
        .user_agent("KunBox/1.0")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let mut releases = Vec::new();

    let mut stable_result: Option<GithubRelease> = None;
    for api_base in GITHUB_API_MIRRORS {
        let api_url = format!("{}/repos/SagerNet/sing-box/releases/latest", api_base);
        let resp = client.get(&api_url)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await;

        if let Ok(resp) = resp {
            if resp.status().is_success() {
                if let Ok(stable) = resp.json::<GithubRelease>().await {
                    stable_result = Some(stable);
                    break;
                }
            }
        }
    }

    if let Some(stable) = stable_result {
        if let Some(asset) = find_windows_asset(&stable.assets, &stable.tag_name) {
            releases.push(RemoteRelease {
                version: stable.tag_name.trim_start_matches('v').to_string(),
                tag_name: stable.tag_name.clone(),
                published_at: stable.published_at,
                is_prerelease: false,
                download_url: asset.browser_download_url.clone(),
                asset_name: asset.name.clone(),
            });
        }
    }

    if include_prerelease {
        let mut all_releases_result: Option<Vec<GithubRelease>> = None;
        for api_base in GITHUB_API_MIRRORS {
            let api_url = format!("{}/repos/SagerNet/sing-box/releases?per_page=10", api_base);
            let resp = client.get(&api_url)
                .header("Accept", "application/vnd.github.v3+json")
                .send()
                .await;

            if let Ok(resp) = resp {
                if resp.status().is_success() {
                    if let Ok(all_releases) = resp.json::<Vec<GithubRelease>>().await {
                        all_releases_result = Some(all_releases);
                        break;
                    }
                }
            }
        }

        if let Some(all_releases) = all_releases_result {
            for release in all_releases {
                if release.prerelease {
                    if let Some(asset) = find_windows_asset(&release.assets, &release.tag_name) {
                        releases.push(RemoteRelease {
                            version: release.tag_name.trim_start_matches('v').to_string(),
                            tag_name: release.tag_name.clone(),
                            published_at: release.published_at,
                            is_prerelease: true,
                            download_url: asset.browser_download_url.clone(),
                            asset_name: asset.name.clone(),
                        });
                        break;
                    }
                }
            }
        }
    }

    if releases.is_empty() {
        match fetch_release_fallback_from_jsdelivr(&client, include_prerelease).await {
            Ok(mut fallback) => {
                releases.append(&mut fallback);
            }
            Err(e) => {
                log::warn!("Kernel fallback source failed: {}", e);
            }
        }
    }

    if releases.is_empty() {
        return Err("无法获取内核版本列表，请检查网络后重试。".to_string());
    }

    Ok(releases)
}

fn is_valid_release_tag(tag_name: &str) -> bool {
    if !tag_name.starts_with('v') || tag_name.len() < 3 {
        return false;
    }

    let version = &tag_name[1..];
    let mut chars = version.chars().peekable();
    let mut has_digit = false;

    while let Some(ch) = chars.next() {
        if ch.is_ascii_digit() {
            has_digit = true;
            continue;
        }

        if matches!(ch, '.' | '-') {
            continue;
        }

        if ch.is_ascii_alphabetic() {
            continue;
        }

        return false;
    }

    has_digit
}

#[tauri::command]
pub async fn kernel_get_remote_releases(include_prerelease: Option<bool>) -> Result<Vec<RemoteRelease>, String> {
    fetch_trusted_remote_releases(include_prerelease.unwrap_or(true)).await
}

#[tauri::command]
pub async fn kernel_download(app: AppHandle, state: State<'_, AppState>, tag_name: String) -> Result<serde_json::Value, String> {
    let _ = app.emit("kernel:download-start", ());

    let normalized_tag = tag_name.trim().to_string();
    if !is_valid_release_tag(&normalized_tag) {
        let err = "无效的内核版本标识";
        let _ = app.emit("kernel:download-error", err);
        return Err(err.to_string());
    }

    let trusted_release = fetch_trusted_remote_releases(true)
        .await?
        .into_iter()
        .find(|release| release.tag_name == normalized_tag)
        .ok_or_else(|| {
            let err = format!("未找到可信的内核版本: {}", normalized_tag);
            let _ = app.emit("kernel:download-error", &err);
            err
        })?;

    if trusted_release.download_url.trim().is_empty() || trusted_release.asset_name.trim().is_empty() {
        let err = "内核版本缺少可用的下载资源";
        let _ = app.emit("kernel:download-error", err);
        return Err(err.to_string());
    }

    let client = reqwest::Client::builder()
        .user_agent("KunBox/1.0")
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;

    let mut download_response = None;
    let mut last_error = String::new();

    for mirror in GITHUB_MIRRORS {
        let download_url = if mirror.is_empty() {
            trusted_release.download_url.clone()
        } else {
            format!("{}{}", mirror, trusted_release.download_url)
        };

        log::info!("Trying to download from: {}", download_url);
        let _ = app.emit("kernel:download-mirror", &download_url);

        match client.get(&download_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                download_response = Some(resp);
                break;
            }
            Ok(resp) => {
                last_error = format!("HTTP {}", resp.status());
                log::warn!("Mirror {} failed: {}", mirror, last_error);
            }
            Err(e) => {
                last_error = e.to_string();
                log::warn!("Mirror {} failed: {}", mirror, last_error);
            }
        }
    }

    let response = download_response.ok_or_else(|| {
        let err = format!("All mirrors failed. Last error: {}", last_error);
        let _ = app.emit("kernel:download-error", &err);
        err
    })?;

    let kernel_dir = get_kernel_dir_for_install(&app)?;
    fs::create_dir_all(&kernel_dir).map_err(|e| e.to_string())?;

    let archive_path = kernel_download_archive_path(&kernel_dir);
    let extracted_kernel_path = kernel_extract_temp_path(&kernel_dir, KERNEL_FILENAME);
    let extracted_cronet_path = kernel_extract_temp_path(&kernel_dir, "libcronet.dll");

    remove_path_if_exists(&archive_path)?;
    remove_path_if_exists(&extracted_kernel_path)?;
    remove_path_if_exists(&extracted_cronet_path)?;

    download_kernel_archive_to_path(&app, response, &archive_path).await?;
    let has_cronet = extract_kernel_archive(&archive_path, &extracted_kernel_path, &extracted_cronet_path)?;
    let _ = remove_path_if_exists(&archive_path);

    if !extracted_kernel_path.exists() {
        let err = "sing-box.exe not found in archive";
        let _ = app.emit("kernel:download-error", err);
        return Err(err.to_string());
    }

    let was_running = matches!(*state.proxy_state.lock().await, ProxyState::Connected | ProxyState::Connecting);

    if was_running {
        let stop_result = singbox_stop_impl(app.clone(), &state).await?;
        if !stop_result.success {
            return Err(stop_result.error.unwrap_or_else(|| "停止内核失败".to_string()));
        }
    }

    replace_kernel_file_from_path(&kernel_dir, &extracted_kernel_path)?;
    log::info!("Kernel installed to {:?}", kernel_dir.join(KERNEL_FILENAME));

    if has_cronet {
        replace_support_file(&kernel_dir.join("libcronet.dll"), &extracted_cronet_path)?;
        log::info!("Naive runtime installed to {:?}", kernel_dir.join("libcronet.dll"));
    }

    if was_running {
        let start_result = singbox_start_impl(app.clone(), &state).await?;
        if !start_result.success {
            return Err(start_result.error.unwrap_or_else(|| "内核已更新，但重新启动失败".to_string()));
        }
    }

    let _ = app.emit("kernel:download-complete", ());

    Ok(serde_json::json!({ "success": true }))
}

#[tauri::command]
pub async fn kernel_rollback(app: AppHandle) -> Result<serde_json::Value, String> {
    let kernel_dir = get_kernel_dir_for_install(&app)?;
    let kernel_path = kernel_dir.join(KERNEL_FILENAME);
    let backup_path = kernel_dir.join("sing-box.exe.bak");
    
    if !backup_path.exists() {
        return Ok(serde_json::json!({ "success": false, "error": "No backup available" }));
    }
    
    // Swap current and backup
    let temp_path = kernel_dir.join("sing-box.exe.tmp");
    
    if kernel_path.exists() {
        fs::rename(&kernel_path, &temp_path).map_err(|e| e.to_string())?;
    }
    
    fs::rename(&backup_path, &kernel_path).map_err(|e| e.to_string())?;
    
    if temp_path.exists() {
        fs::rename(&temp_path, &backup_path).map_err(|e| e.to_string())?;
    }
    
    Ok(serde_json::json!({ "success": true }))
}

#[tauri::command]
pub async fn kernel_can_rollback(app: AppHandle) -> Result<bool, String> {
    let kernel_dir = get_kernel_dir_for_install(&app)?;
    let backup_path = kernel_dir.join("sing-box.exe.bak");
    Ok(path_exists(&backup_path))
}

#[tauri::command]
pub async fn kernel_clear_cache(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let freed_bytes = clear_kernel_cache_targets(&kernel_cache_targets(&state))?;
    
    Ok(serde_json::json!({ "success": true, "freedBytes": freed_bytes }))
}

fn get_dir_size(path: &std::path::Path) -> u64 {
    let mut size = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                size += get_dir_size(&path);
            } else if let Ok(meta) = path.metadata() {
                size += meta.len();
            }
        }
    }
    size
}

#[tauri::command]
pub async fn kernel_open_releases_page() -> Result<(), String> {
    open::that("https://github.com/SagerNet/sing-box/releases").map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kernel_open_directory(app: AppHandle) -> Result<(), String> {
    let data_dir = get_data_kernel_dir(&app)?;
    let bundle_dir = get_bundle_kernel_dir(&app)?;
    let data_has_kernel = path_exists(&data_dir.join(KERNEL_FILENAME));
    let bundle_has_kernel = path_exists(&bundle_dir.join(KERNEL_FILENAME));

    let target_dir = if data_has_kernel {
        data_dir
    } else if bundle_has_kernel {
        bundle_dir
    } else {
        fs::create_dir_all(&data_dir).ok();
        data_dir
    };

    open::that(&target_dir).map_err(|e| e.to_string())
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstalledKernel {
    pub version: String,
    pub version_detail: String,
    pub is_backup: bool,
    pub path: String,
}

#[tauri::command]
pub async fn kernel_get_installed_versions(app: AppHandle) -> Result<Vec<InstalledKernel>, String> {
    let mut installed = Vec::new();

    let data_dir = get_data_kernel_dir(&app)?;
    let bundle_dir = get_bundle_kernel_dir(&app)?;

    let primary_kernel_path = if path_exists(&data_dir.join(KERNEL_FILENAME)) {
        data_dir.join(KERNEL_FILENAME)
    } else {
        bundle_dir.join(KERNEL_FILENAME)
    };

    if path_exists(&primary_kernel_path) {
        if let Some(version_info) = get_kernel_version_info(&primary_kernel_path).await {
            installed.push(InstalledKernel {
                version: version_info.0,
                version_detail: version_info.1,
                is_backup: false,
                path: primary_kernel_path.to_string_lossy().to_string(),
            });
        }
    }

    let backup_path = data_dir.join("sing-box.exe.bak");
    if path_exists(&backup_path) {
        if let Some(version_info) = get_kernel_version_info(&backup_path).await {
            installed.push(InstalledKernel {
                version: version_info.0,
                version_detail: version_info.1,
                is_backup: true,
                path: backup_path.to_string_lossy().to_string(),
            });
        }
    }

    Ok(installed)
}

async fn get_kernel_version_info(kernel_path: &std::path::Path) -> Option<(String, String)> {
    #[cfg(windows)]
    let output = tokio::process::Command::new(kernel_path)
        .arg("version")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .ok()?;

    #[cfg(not(windows))]
    let output = tokio::process::Command::new(kernel_path)
        .arg("version")
        .output()
        .await
        .ok()?;

    if output.status.success() {
        let version_str = String::from_utf8_lossy(&output.stdout);
        let version_detail = version_str.trim().to_string();

        let version = version_str
            .lines()
            .find(|line| line.contains("version"))
            .and_then(|line| line.split_whitespace().last())
            .map(|v| v.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        return Some((version, version_detail));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_test_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("kunbox-kernel-{}-{}", name, suffix))
    }

    #[test]
    fn validates_release_tags() {
        assert!(is_valid_release_tag("v1.13.0"));
        assert!(is_valid_release_tag("v1.13.0-beta.1"));
        assert!(!is_valid_release_tag("1.13.0"));
        assert!(!is_valid_release_tag("v../evil"));
        assert!(!is_valid_release_tag(""));
    }

    #[test]
    fn builds_release_from_version() {
        let release = build_release_from_version("1.13.0", false);
        assert_eq!(release.tag_name, "v1.13.0");
        assert_eq!(release.asset_name, "sing-box-1.13.0-windows-amd64.zip");
        assert!(release.download_url.contains("/v1.13.0/sing-box-1.13.0-windows-amd64.zip"));
    }

    #[test]
    fn clears_actual_cache_db_and_ruleset_cache() {
        let data_dir = unique_test_dir("clear-cache");
        let state = AppState::new(data_dir.clone());

        fs::create_dir_all(state.rulesets_cache_dir()).unwrap();
        fs::create_dir_all(state.data_dir.join("cache")).unwrap();
        fs::write(state.config_dir.join("cache.db"), vec![1u8; 16]).unwrap();
        fs::write(state.rulesets_cache_dir().join("demo.srs"), vec![2u8; 24]).unwrap();
        fs::write(state.data_dir.join("cache").join("legacy.bin"), vec![3u8; 8]).unwrap();

        let freed = clear_kernel_cache_targets(&kernel_cache_targets(&state)).unwrap();

        assert!(freed >= 48);
        assert!(!state.config_dir.join("cache.db").exists());
        assert!(!state.rulesets_cache_dir().exists());
        assert!(!state.data_dir.join("cache").exists());

        let _ = fs::remove_dir_all(data_dir);
    }
}
