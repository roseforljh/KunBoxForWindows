use tauri::{AppHandle, Emitter, Manager, State};
use std::fs;
use std::path::PathBuf;
use crate::state::AppState;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const GITHUB_API_STABLE: &str = "https://api.github.com/repos/SagerNet/sing-box/releases/latest";
const GITHUB_API_RELEASES: &str = "https://api.github.com/repos/SagerNet/sing-box/releases?per_page=10";
const KERNEL_FILENAME: &str = "sing-box.exe";

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct KernelVersion {
    pub version: String,
    pub version_detail: String,
    pub is_alpha: bool,
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

fn get_kernel_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    Ok(resource_dir.join("resources").join("libs"))
}

fn get_kernel_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(get_kernel_dir(app)?.join(KERNEL_FILENAME))
}

fn find_windows_asset<'a>(assets: &'a [GithubAsset], tag_name: &str) -> Option<&'a GithubAsset> {
    let version = tag_name.trim_start_matches('v');
    let expected_name = format!("sing-box-{}-windows-amd64.zip", version);
    assets.iter().find(|a| a.name == expected_name)
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
pub async fn kernel_get_remote_releases(include_prerelease: Option<bool>) -> Result<Vec<RemoteRelease>, String> {
    let client = reqwest::Client::builder()
        .user_agent("KunBox/1.0")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    
    let mut releases = Vec::new();
    
    // Get stable release
    let stable_response = client.get(GITHUB_API_STABLE)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await;
    
    if let Ok(resp) = stable_response {
        if resp.status().is_success() {
            if let Ok(stable) = resp.json::<GithubRelease>().await {
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
        }
    }
    
    // Get prerelease if requested
    let include_pre = include_prerelease.unwrap_or(true);
    if include_pre {
        let releases_response = client.get(GITHUB_API_RELEASES)
            .header("Accept", "application/vnd.github.v3+json")
            .send()
            .await;
        
        if let Ok(resp) = releases_response {
            if resp.status().is_success() {
                if let Ok(all_releases) = resp.json::<Vec<GithubRelease>>().await {
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
        }
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
    
    // Download the zip file
    let response = client.get(&release.download_url)
        .send()
        .await
        .map_err(|e| {
            let _ = app.emit("kernel:download-error", e.to_string());
            e.to_string()
        })?;
    
    if !response.status().is_success() {
        let err = format!("Download failed: {}", response.status());
        let _ = app.emit("kernel:download-error", &err);
        return Err(err);
    }
    
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
    let kernel_dir = get_kernel_dir(&app)?;
    fs::create_dir_all(&kernel_dir).map_err(|e| e.to_string())?;
    
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;
    
    let mut found = false;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().to_string();
        
        if name.ends_with("sing-box.exe") {
            let kernel_path = kernel_dir.join(KERNEL_FILENAME);
            
            // Backup existing
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
            break;
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
    let kernel_dir = get_kernel_dir(&app)?;
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
    let kernel_dir = get_kernel_dir(&app)?;
    let backup_path = kernel_dir.join("sing-box.exe.bak");
    Ok(backup_path.exists())
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
    let kernel_dir = get_kernel_dir(&app)?;
    fs::create_dir_all(&kernel_dir).ok();
    open::that(&kernel_dir).map_err(|e| e.to_string())
}
