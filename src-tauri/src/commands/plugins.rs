use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const XRAY_FILENAME: &str = "xray.exe";
const XRAY_ARCHIVE_NAME: &str = "Xray-windows-64.zip";
const MAX_PLUGIN_DOWNLOAD_SIZE_BYTES: u64 = 100 * 1024 * 1024;
const GITHUB_API_BASE: &str = "https://api.github.com";
const GITHUB_MIRRORS: &[&str] = &["", "https://mirror.ghproxy.com/", "https://ghproxy.net/", "https://gh-proxy.com/"];

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PluginVersion {
    pub version: String,
    pub version_detail: String,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PluginRelease {
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

fn get_plugin_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(app_data_dir.join("libs"))
}

fn get_xray_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(get_plugin_dir(app)?.join(XRAY_FILENAME))
}

fn plugin_download_archive_path(plugin_dir: &Path) -> PathBuf {
    plugin_dir.join("xray.download.zip")
}

fn plugin_extract_temp_path(plugin_dir: &Path) -> PathBuf {
    plugin_dir.join("xray.exe.download")
}

fn remove_path_if_exists(path: &Path) -> Result<(), String> {
    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
fn build_xray_release_from_version(version: &str, is_prerelease: bool) -> PluginRelease {
    let tag = format!("v{}", version.trim_start_matches('v'));
    PluginRelease {
        version: version.trim_start_matches('v').to_string(),
        tag_name: tag.clone(),
        published_at: chrono::Utc::now().to_rfc3339(),
        is_prerelease,
        download_url: format!("https://github.com/XTLS/Xray-core/releases/download/{}/{}", tag, XRAY_ARCHIVE_NAME),
        asset_name: XRAY_ARCHIVE_NAME.to_string(),
    }
}

fn find_xray_windows_asset(assets: &[GithubAsset]) -> Option<&GithubAsset> {
    assets.iter().find(|asset| asset.name == XRAY_ARCHIVE_NAME)
}

fn is_valid_release_tag(tag_name: &str) -> bool {
    tag_name.starts_with('v')
        && tag_name.len() >= 3
        && tag_name.chars().skip(1).all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-')
}

async fn fetch_xray_releases(include_prerelease: bool) -> Result<Vec<PluginRelease>, String> {
    let client = reqwest::Client::builder()
        .user_agent("KunBox/1.0")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let api_url = format!("{}/repos/XTLS/Xray-core/releases?per_page=10", GITHUB_API_BASE);
    let releases = client
        .get(api_url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .json::<Vec<GithubRelease>>()
        .await
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for release in releases {
        if release.prerelease && !include_prerelease {
            continue;
        }
        if let Some(asset) = find_xray_windows_asset(&release.assets) {
            result.push(PluginRelease {
                version: release.tag_name.trim_start_matches('v').to_string(),
                tag_name: release.tag_name,
                published_at: release.published_at,
                is_prerelease: release.prerelease,
                download_url: asset.browser_download_url.clone(),
                asset_name: asset.name.clone(),
            });
        }
        if result.len() >= 2 {
            break;
        }
    }

    if result.is_empty() {
        return Err("无法获取 Xray 插件版本列表，请检查网络后重试。".to_string());
    }

    Ok(result)
}

fn github_release_to_plugin_release(release: GithubRelease) -> Option<PluginRelease> {
    let asset = find_xray_windows_asset(&release.assets)?;
    Some(PluginRelease {
        version: release.tag_name.trim_start_matches('v').to_string(),
        tag_name: release.tag_name,
        published_at: release.published_at,
        is_prerelease: release.prerelease,
        download_url: asset.browser_download_url.clone(),
        asset_name: asset.name.clone(),
    })
}

async fn fetch_xray_release_by_tag(tag_name: &str) -> Result<PluginRelease, String> {
    let client = reqwest::Client::builder()
        .user_agent("KunBox/1.0")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let api_url = format!("{}/repos/XTLS/Xray-core/releases/tags/{}", GITHUB_API_BASE, tag_name);
    let response = client
        .get(api_url)
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !response.status().is_success() {
        return Err(format!("未找到可信的 Xray 版本: {}", tag_name));
    }

    let release = response.json::<GithubRelease>().await.map_err(|e| e.to_string())?;
    github_release_to_plugin_release(release)
        .ok_or_else(|| format!("Xray 版本缺少 Windows x64 下载资源: {}", tag_name))
}

async fn download_archive_to_path(
    app: &AppHandle,
    response: reqwest::Response,
    archive_path: &Path,
) -> Result<(), String> {
    let total_size = response.content_length().unwrap_or(0);
    if total_size > MAX_PLUGIN_DOWNLOAD_SIZE_BYTES {
        let err = "插件安装包过大，已拒绝下载";
        let _ = app.emit("plugin:download-error", err);
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

        if downloaded > MAX_PLUGIN_DOWNLOAD_SIZE_BYTES {
            let err = "插件安装包过大，已拒绝下载";
            let _ = app.emit("plugin:download-error", err);
            return Err(err.to_string());
        }

        if total_size > 0 {
            let _ = app.emit("plugin:download-progress", serde_json::json!({
                "downloaded": downloaded,
                "total": total_size,
                "percent": (downloaded as f64 / total_size as f64 * 100.0) as u32
            }));
        }
    }

    file.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn extract_xray_archive(archive_path: &Path, extracted_path: &Path) -> Result<(), String> {
    remove_path_if_exists(extracted_path)?;

    let archive_file = fs::File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(archive_file).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        if file.name().ends_with(XRAY_FILENAME) {
            let mut output = fs::File::create(extracted_path).map_err(|e| e.to_string())?;
            std::io::copy(&mut file, &mut output).map_err(|e| e.to_string())?;
            output.flush().map_err(|e| e.to_string())?;
            return Ok(());
        }
    }

    Err("xray.exe not found in archive".to_string())
}

fn replace_plugin_file(target_path: &Path, source_path: &Path) -> Result<(), String> {
    remove_path_if_exists(target_path)?;
    fs::rename(source_path, target_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugin_get_xray_local_version(app: AppHandle) -> Result<Option<PluginVersion>, String> {
    let xray_path = get_xray_path(&app)?;
    if !xray_path.exists() {
        return Ok(None);
    }

    #[cfg(windows)]
    let output = tokio::process::Command::new(&xray_path)
        .arg("version")
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    #[cfg(not(windows))]
    let output = tokio::process::Command::new(&xray_path)
        .arg("version")
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Ok(None);
    }

    let version_detail = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let version = version_detail
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().find(|part| part.chars().any(|ch| ch.is_ascii_digit())))
        .unwrap_or("unknown")
        .trim_start_matches('v')
        .to_string();

    Ok(Some(PluginVersion { version, version_detail }))
}

#[tauri::command]
pub async fn plugin_get_xray_remote_releases(include_prerelease: Option<bool>) -> Result<Vec<PluginRelease>, String> {
    fetch_xray_releases(include_prerelease.unwrap_or(false)).await
}

#[tauri::command]
pub async fn plugin_download_xray(app: AppHandle, tag_name: String) -> Result<serde_json::Value, String> {
    let _ = app.emit("plugin:download-start", ());
    let normalized_tag = tag_name.trim().to_string();
    if !is_valid_release_tag(&normalized_tag) {
        let err = "无效的 Xray 版本标识";
        let _ = app.emit("plugin:download-error", err);
        return Err(err.to_string());
    }

    let trusted_release = fetch_xray_release_by_tag(&normalized_tag).await.map_err(|err| {
        let _ = app.emit("plugin:download-error", &err);
        err
    })?;

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

        let _ = app.emit("plugin:download-mirror", &download_url);
        match client.get(&download_url).send().await {
            Ok(resp) if resp.status().is_success() => {
                download_response = Some(resp);
                break;
            }
            Ok(resp) => last_error = format!("HTTP {}", resp.status()),
            Err(e) => last_error = e.to_string(),
        }
    }

    let response = download_response.ok_or_else(|| {
        let err = format!("Xray 下载失败: {}", last_error);
        let _ = app.emit("plugin:download-error", &err);
        err
    })?;

    let plugin_dir = get_plugin_dir(&app)?;
    fs::create_dir_all(&plugin_dir).map_err(|e| e.to_string())?;
    let archive_path = plugin_download_archive_path(&plugin_dir);
    let extracted_path = plugin_extract_temp_path(&plugin_dir);
    let xray_path = plugin_dir.join(XRAY_FILENAME);

    remove_path_if_exists(&archive_path)?;
    remove_path_if_exists(&extracted_path)?;
    download_archive_to_path(&app, response, &archive_path).await?;
    extract_xray_archive(&archive_path, &extracted_path)?;
    let _ = remove_path_if_exists(&archive_path);
    replace_plugin_file(&xray_path, &extracted_path)?;

    let _ = app.emit("plugin:download-complete", ());
    Ok(serde_json::json!({ "success": true }))
}

#[tauri::command]
pub async fn plugin_open_directory(app: AppHandle) -> Result<(), String> {
    let plugin_dir = get_plugin_dir(&app)?;
    fs::create_dir_all(&plugin_dir).map_err(|e| e.to_string())?;
    open::that(plugin_dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn plugin_open_xray_releases_page() -> Result<(), String> {
    open::that("https://github.com/XTLS/Xray-core/releases").map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_xray_release_from_version() {
        let release = build_xray_release_from_version("25.5.16", false);
        assert_eq!(release.tag_name, "v25.5.16");
        assert_eq!(release.asset_name, "Xray-windows-64.zip");
        assert!(release.download_url.contains("/v25.5.16/Xray-windows-64.zip"));
    }

    #[test]
    fn finds_xray_windows_64_asset() {
        let assets = vec![
            GithubAsset {
                name: "Xray-linux-64.zip".to_string(),
                browser_download_url: "linux".to_string(),
            },
            GithubAsset {
                name: "Xray-windows-64.zip".to_string(),
                browser_download_url: "windows".to_string(),
            },
        ];

        let asset = find_xray_windows_asset(&assets).unwrap();
        assert_eq!(asset.browser_download_url, "windows");
    }

    #[test]
    fn converts_exact_github_release_to_plugin_release() {
        let release = GithubRelease {
            tag_name: "v26.3.27".to_string(),
            published_at: "2026-03-27T17:51:11Z".to_string(),
            prerelease: false,
            assets: vec![
                GithubAsset {
                    name: "Xray-linux-64.zip".to_string(),
                    browser_download_url: "linux".to_string(),
                },
                GithubAsset {
                    name: "Xray-windows-64.zip".to_string(),
                    browser_download_url: "https://github.com/XTLS/Xray-core/releases/download/v26.3.27/Xray-windows-64.zip".to_string(),
                },
            ],
        };

        let plugin_release = github_release_to_plugin_release(release).unwrap();

        assert_eq!(plugin_release.tag_name, "v26.3.27");
        assert_eq!(plugin_release.version, "26.3.27");
        assert_eq!(plugin_release.asset_name, "Xray-windows-64.zip");
        assert!(plugin_release.download_url.ends_with("/Xray-windows-64.zip"));
    }
}
