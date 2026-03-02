use tauri::{AppHandle, Emitter, Manager, State};
use std::fs;
use std::path::{Path, PathBuf};
use crate::state::AppState;

#[cfg(windows)]
#[allow(unused_imports)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const KERNEL_FILENAME: &str = "sing-box.exe";

// GitHub 镜像源列表
const GITHUB_MIRRORS: &[&str] = &[
    "",                                    // 原始 GitHub（无镜像）
    "https://mirror.ghproxy.com/",         // ghproxy 镜像
    "https://ghproxy.net/",                // ghproxy.net
    "https://gh-proxy.com/",               // gh-proxy
];

// GitHub API 镜像源列表
const GITHUB_API_MIRRORS: &[&str] = &[
    "https://api.github.com",              // 原始 API
    "https://gh-api.p3terx.com",           // P3TERX 镜像
    "https://api.github.moeyy.xyz",        // GitHub API 镜像
    "https://github.api.99988866.xyz",     // GitHub API 镜像
];

const VERSION_FALLBACK_API: &str = "https://data.jsdelivr.com/v1/package/gh/SagerNet/sing-box";

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

    Ok(data_dir)
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
    let kernel_path = get_kernel_path(&app)?;
    
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
    let kernel_path = get_kernel_path(&app)?;
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

    Ok(KernelCapabilities {
        version: version.clone(),
        supports_naive: version_gte(&version, (1, 13, 0)),
        supports_icmp_proxy: version_gte(&version, (1, 13, 0)),
        supports_bypass_action: version_gte(&version, (1, 13, 0)),
    })
}

#[tauri::command]
pub async fn kernel_get_remote_releases(include_prerelease: Option<bool>) -> Result<Vec<RemoteRelease>, String> {
    let client = reqwest::Client::builder()
        .user_agent("KunBox/1.0")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let mut releases = Vec::new();

    // 尝试不同的 API 镜像源
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

    // 处理 stable release
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

    // Get prerelease if requested
    let include_pre = include_prerelease.unwrap_or(true);
    if include_pre {
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
                        break; // Only get latest prerelease
                    }
                }
            }
        }
    }

    if releases.is_empty() {
        match fetch_release_fallback_from_jsdelivr(&client, include_pre).await {
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

#[tauri::command]
pub async fn kernel_download(app: AppHandle, release: RemoteRelease) -> Result<serde_json::Value, String> {
    let _ = app.emit("kernel:download-start", ());

    let client = reqwest::Client::builder()
        .user_agent("KunBox/1.0")
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;

    // 尝试不同的镜像源下载
    let mut download_response = None;
    let mut last_error = String::new();

    for mirror in GITHUB_MIRRORS {
        let download_url = if mirror.is_empty() {
            release.download_url.clone()
        } else {
            format!("{}{}", mirror, release.download_url)
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
    
    let total_size = response.content_length().unwrap_or(0);
    let mut downloaded: u64 = 0;
    
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    
    use futures_util::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        bytes.extend_from_slice(&chunk);
        downloaded += chunk.len() as u64;
        
        if total_size > 0 {
            let progress = serde_json::json!({
                "downloaded": downloaded,
                "total": total_size,
                "percent": (downloaded as f64 / total_size as f64 * 100.0) as u32
            });
            let _ = app.emit("kernel:download-progress", progress);
        }
    }
    
    // Extract zip
    let kernel_dir = get_kernel_dir_for_install(&app)?;
    fs::create_dir_all(&kernel_dir).map_err(|e| e.to_string())?;
    
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;
    
    let mut found = false;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().to_string();

        if name.ends_with("sing-box.exe") {
            let kernel_path = kernel_dir.join(KERNEL_FILENAME);

            if kernel_path.exists() {
                let backup_path = kernel_dir.join("sing-box.exe.bak");
                if backup_path.exists() {
                    let _ = fs::remove_file(&backup_path);
                }
                let _ = fs::rename(&kernel_path, &backup_path);
            }

            let mut outfile = fs::File::create(&kernel_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;

            log::info!("Kernel installed to {:?}", kernel_path);
            found = true;
            continue;
        }

        if name.ends_with("libcronet.dll") {
            let dll_path = kernel_dir.join("libcronet.dll");
            let mut outfile = fs::File::create(&dll_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut outfile).map_err(|e| e.to_string())?;
            log::info!("Naive runtime installed to {:?}", dll_path);
        }
    }
    
    if !found {
        let err = "sing-box.exe not found in archive";
        let _ = app.emit("kernel:download-error", err);
        return Err(err.to_string());
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
    let cache_dir = state.data_dir.join("cache");
    let mut freed_bytes: u64 = 0;
    
    if cache_dir.exists() {
        freed_bytes = get_dir_size(&cache_dir);
        fs::remove_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    }
    
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
