use super::singbox::{singbox_start_impl, singbox_stop_impl};
use crate::state::AppState;
use crate::types::ProxyState;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Emitter, Manager, State};

#[cfg(windows)]
#[allow(unused_imports)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const KERNEL_FILENAME: &str = "sing-box.exe";
const RAYBOX_MANIFEST_PATH: &str = "raybox-windows-manifest.json";
const RAYBOX_CANDIDATE_ENV: &str = "KUNBOX_RAYBOX_WINDOWS_CANDIDATE";
const RAYBOX_REPOSITORY: &str = "https://github.com/roseforljh/RayBox-rx";
const RAYBOX_REPOSITORY_SLUG: &str = "roseforljh/RayBox-rx";
const RAYBOX_ASSET_NAME: &str = "raybox-windows-amd64.exe";

#[derive(serde::Deserialize, Clone, Debug)]
struct RayBoxManifest {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    repository: String,
    commit: String,
    #[serde(rename = "fullHead")]
    full_head: String,
    tag: Option<String>,
    dirty: bool,
    #[serde(rename = "sourceDigest")]
    source_digest: String,
    #[serde(rename = "sourceDigestAlgorithm")]
    source_digest_algorithm: String,
    source: RayBoxSource,
    product: String,
    version: String,
    go: String,
    #[serde(rename = "goVersion")]
    go_version: String,
    goos: String,
    goarch: String,
    #[serde(rename = "buildTags")]
    build_tags: Vec<String>,
    #[serde(rename = "rxDefault")]
    rx_default: bool,
    #[serde(rename = "oobDefault")]
    oob_default: bool,
    pe: RayBoxPe,
    assets: HashMap<String, RayBoxAsset>,
}

#[derive(serde::Deserialize, Clone, Debug)]
struct RayBoxSource {
    url: String,
    #[serde(rename = "runtimeLicense")]
    runtime_license: String,
    #[serde(rename = "protocolLicense")]
    protocol_license: String,
}

#[derive(serde::Deserialize, Clone, Debug)]
struct RayBoxPe {
    #[serde(rename = "machineCode")]
    machine_code: String,
}

#[derive(serde::Deserialize, Clone, Debug)]
struct RayBoxAsset {
    file: String,
    size: u64,
    sha256: String,
}

const RAYBOX_LOCK_PATH: &str = "raybox-windows.lock.json";
const RAYBOX_INSTALL_DIR: &str = "raybox";
const RAYBOX_BACKUP_DIR: &str = "raybox.backup";

#[derive(serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct RayBoxLock {
    repository: String,
    commit: String,
    tag: String,
    manifest_sha256: String,
    source_digest: String,
    product: String,
    version: String,
}
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

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
}

fn valid_raybox_identity(commit: &str, version: &str, tag: Option<&str>, dirty: bool) -> bool {
    if !is_lower_hex(commit, 40) {
        return false;
    }
    let legacy = version == format!("1.0.0-{}", &commit[..12]);
    let semantic = version.split('.').count() == 3
        && version.split('.').all(|part| {
            !part.is_empty()
                && (part == "0" || !part.starts_with('0'))
                && part.bytes().all(|byte| byte.is_ascii_digit())
        });
    if dirty {
        return tag.is_none() && (legacy || semantic);
    }
    (legacy && tag == Some(&format!("windows-{commit}")))
        || (semantic && tag == Some(&format!("windows-v{version}")))
}

fn read_raybox_lock(path: &Path) -> Result<RayBoxLock, String> {
    let text = fs::read_to_string(path)
        .map_err(|_| "RayBox Windows 正式版本尚未配置可信 lock，暂时不能下载内核".to_string())?;
    let lock: RayBoxLock = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if !raybox_repository_matches(&lock.repository)
        || !valid_raybox_identity(&lock.commit, &lock.version, Some(&lock.tag), false)
        || !is_lower_hex(&lock.manifest_sha256, 64)
        || !is_lower_hex(&lock.source_digest, 64)
        || lock.product != "RayBox"
    {
        return Err("RayBox Windows 正式 lock 无效".to_string());
    }
    Ok(lock)
}

fn raybox_release(lock: &RayBoxLock) -> RemoteRelease {
    RemoteRelease {
        version: lock.version.clone(),
        tag_name: lock.tag.clone(),
        published_at: String::new(),
        is_prerelease: false,
        download_url: format!(
            "https://github.com/{}/releases/download/{}/{}",
            RAYBOX_REPOSITORY_SLUG, lock.tag, RAYBOX_ASSET_NAME
        ),
        asset_name: RAYBOX_ASSET_NAME.to_string(),
    }
}

fn get_data_kernel_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(app_data_dir.join("libs"))
}

fn get_bundle_kernel_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let resource_dir = app.path().resource_dir().map_err(|e| e.to_string())?;
    Ok(resource_dir.join("resources").join("libs"))
}

fn kernel_executable_path(kernel_dir: &Path) -> PathBuf {
    if raybox_candidate_dir().as_deref() == Some(kernel_dir)
        || kernel_dir.join(RAYBOX_ASSET_NAME).exists()
    {
        kernel_dir.join(RAYBOX_ASSET_NAME)
    } else {
        kernel_dir.join(KERNEL_FILENAME)
    }
}

fn resolve_kernel_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(candidate) = raybox_candidate_dir() {
        validate_raybox_manifest(
            &candidate.join("manifest.json"),
            &candidate.join(RAYBOX_ASSET_NAME),
            true,
        )
        .map_err(|_| "配置的 RayBox Windows 本地候选未通过 manifest 校验".to_string())?;
        return Ok(candidate);
    }
    for directory in [get_data_kernel_dir(app)?, get_bundle_kernel_dir(app)?] {
        let installed = directory.join(RAYBOX_INSTALL_DIR);
        if installed.exists() {
            validate_raybox_manifest(
                &installed.join(RAYBOX_MANIFEST_PATH),
                &installed.join(RAYBOX_ASSET_NAME),
                false,
            )?;
            return Ok(installed);
        }
        if directory.join(RAYBOX_MANIFEST_PATH).exists() {
            validate_raybox_manifest(
                &directory.join(RAYBOX_MANIFEST_PATH),
                &directory.join(KERNEL_FILENAME),
                false,
            )?;
            return Ok(directory);
        }
    }
    Err("未检测到通过来源校验的 RayBox 内核，请到【设置 → 内核】安装。".to_string())
}

fn get_kernel_dir_for_install(app: &AppHandle) -> Result<PathBuf, String> {
    get_data_kernel_dir(app)
}

fn get_kernel_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = resolve_kernel_dir(app)?;
    Ok(kernel_executable_path(&dir))
}

fn raybox_candidate_dir() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        std::env::var_os(RAYBOX_CANDIDATE_ENV).map(PathBuf::from)
    } else {
        None
    }
}

fn raybox_repository_matches(value: &str) -> bool {
    value == RAYBOX_REPOSITORY || value == RAYBOX_REPOSITORY_SLUG
}

fn raybox_manifest_path(kernel_dir: &Path) -> PathBuf {
    if raybox_candidate_dir().as_deref() == Some(kernel_dir)
        || kernel_dir.join("manifest.json").exists()
    {
        kernel_dir.join("manifest.json")
    } else {
        kernel_dir.join(RAYBOX_MANIFEST_PATH)
    }
}

pub(crate) fn runtime_kernel_path(app: &AppHandle) -> Result<PathBuf, String> {
    get_kernel_path(app)
}

pub(crate) fn raybox_manifest_available(kernel_dir: &Path) -> bool {
    raybox_manifest_path(kernel_dir).exists()
}

fn path_exists(path: &Path) -> bool {
    fs::metadata(path).is_ok()
}

fn validate_raybox_manifest(
    manifest_path: &Path,
    executable_path: &Path,
    allow_dirty: bool,
) -> Result<RayBoxManifest, String> {
    let text = fs::read_to_string(manifest_path).map_err(|e| e.to_string())?;
    let manifest: RayBoxManifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let commit = &manifest.commit;
    let valid_commit = commit.len() == 40
        && commit
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase());
    if manifest.schema_version != 1
        || !raybox_repository_matches(&manifest.repository)
        || !valid_commit
        || manifest.full_head != manifest.commit
        || (!allow_dirty && manifest.dirty)
        || !valid_raybox_identity(
            commit,
            &manifest.version,
            manifest.tag.as_deref(),
            manifest.dirty,
        )
    {
        return Err("RayBox Windows manifest 身份或 dirty 状态无效".to_string());
    }
    if manifest.source_digest_algorithm != "sha256-git-path-kind-content-v1"
        || manifest.source_digest.len() != 64
        || !manifest
            .source_digest
            .chars()
            .all(|character| character.is_ascii_hexdigit() && !character.is_ascii_uppercase())
        || manifest.source.url
            != format!(
                "https://github.com/{}/tree/{}",
                RAYBOX_REPOSITORY_SLUG, commit
            )
        || manifest.source.runtime_license != "docs/reference/runtime/LICENSE"
        || manifest.source.protocol_license != "docs/reference/protocol/LICENSE"
    {
        return Err("RayBox Windows manifest 来源证明无效".to_string());
    }
    if manifest.goos != "windows"
        || manifest.goarch != "amd64"
        || manifest.pe.machine_code != "0x8664"
        || manifest.rx_default
        || manifest.oob_default
        || manifest.go_version != manifest.go
        || !manifest.go.starts_with("go version go1.27.3 ")
        || manifest
            .build_tags
            .iter()
            .any(|tag| tag == "with_tailscale")
    {
        return Err("RayBox Windows manifest 平台、工具链或默认安全策略无效".to_string());
    }
    if manifest.product != "RayBox" {
        return Err("RayBox Windows manifest 版本与 commit 不匹配".to_string());
    }
    let asset = manifest
        .assets
        .get(RAYBOX_ASSET_NAME)
        .ok_or("RayBox Windows manifest 缺少 amd64 资产")?;
    let root = manifest_path
        .parent()
        .ok_or("RayBox Windows manifest 路径无效")?;
    for name in [
        RAYBOX_ASSET_NAME,
        "LICENSE",
        "RayBox-Protocol-LICENSE",
        "SOURCE.txt",
    ] {
        if !manifest.assets.contains_key(name) {
            return Err(format!("RayBox Windows manifest 缺少资产: {}", name));
        }
    }
    for (name, entry) in &manifest.assets {
        let relative = Path::new(&entry.file);
        if name != &entry.file
            || !matches!(
                relative.components().next(),
                Some(std::path::Component::Normal(_))
            )
            || relative.components().count() != 1
            || !is_lower_hex(&entry.sha256, 64)
        {
            return Err("RayBox Windows manifest 资产路径或 SHA-256 无效".to_string());
        }
        let path = if name == RAYBOX_ASSET_NAME {
            executable_path.to_path_buf()
        } else {
            root.join(relative)
        };
        let metadata = fs::metadata(&path).map_err(|e| e.to_string())?;
        if metadata.len() != entry.size {
            return Err(format!("RayBox Windows 资产大小不匹配: {}", entry.file));
        }
        let bytes = fs::read(&path).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        if format!("{:x}", hasher.finalize()) != entry.sha256 {
            return Err(format!(
                "RayBox Windows 资产 SHA-256 不匹配: {}",
                entry.file
            ));
        }
    }
    if asset.file != RAYBOX_ASSET_NAME || !executable_path.exists() {
        return Err("RayBox Windows 资产文件不存在或名称不匹配".to_string());
    }
    let metadata = fs::metadata(executable_path).map_err(|e| e.to_string())?;
    if metadata.len() != asset.size || asset.sha256.len() != 64 {
        return Err("RayBox Windows 资产大小或 SHA-256 字段无效".to_string());
    }
    let bytes = fs::read(executable_path).map_err(|e| e.to_string())?;
    if bytes.len() < 0x40 || bytes.get(..2) != Some(b"MZ") {
        return Err("RayBox Windows 资产不是 PE 文件".to_string());
    }
    let pe_offset_bytes = bytes
        .get(0x3c..0x40)
        .ok_or("RayBox Windows PE 头偏移字段截断")?;
    let pe_offset = u32::from_le_bytes(
        pe_offset_bytes
            .try_into()
            .map_err(|_| "RayBox Windows PE 头偏移字段无效")?,
    ) as usize;
    let pe_header = bytes
        .get(
            pe_offset
                ..pe_offset
                    .checked_add(6)
                    .ok_or("RayBox Windows PE 头偏移溢出")?,
        )
        .ok_or("RayBox Windows PE 头截断")?;
    if pe_header.get(..4) != Some(b"PE\0\0") {
        return Err("RayBox Windows PE 签名无效".to_string());
    }
    let machine = u16::from_le_bytes(
        pe_header[4..6]
            .try_into()
            .map_err(|_| "RayBox Windows PE machine 字段截断")?,
    );
    if machine != 0x8664 {
        return Err("RayBox Windows PE machine 不是 amd64".to_string());
    }
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = format!("{:x}", hasher.finalize());
    if digest != asset.sha256 {
        return Err("RayBox Windows 资产 SHA-256 不匹配".to_string());
    }
    Ok(manifest)
}

pub(crate) fn raybox_manifest_is_valid(kernel_dir: &Path) -> bool {
    let candidate = raybox_candidate_dir();
    let installed = kernel_dir.join(RAYBOX_INSTALL_DIR);
    let default = if installed.exists() {
        installed.as_path()
    } else {
        kernel_dir
    };
    let dir = candidate.as_deref().unwrap_or(default);
    validate_raybox_manifest(
        &raybox_manifest_path(dir),
        &kernel_executable_path(dir),
        candidate.is_some(),
    )
    .is_ok()
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

fn replace_raybox_directory(kernel_dir: &Path, staged: &Path) -> Result<(), String> {
    let current = kernel_dir.join(RAYBOX_INSTALL_DIR);
    let backup = kernel_dir.join(RAYBOX_BACKUP_DIR);
    let retired = kernel_dir.join(format!("raybox.retired-{}", uuid::Uuid::new_v4()));
    if backup.exists() {
        fs::rename(&backup, &retired).map_err(|e| e.to_string())?;
    }
    let had_current = current.exists();
    if had_current {
        if let Err(error) = fs::rename(&current, &backup) {
            if retired.exists() {
                fs::rename(&retired, &backup).map_err(|e| e.to_string())?;
            }
            return Err(error.to_string());
        }
    }
    if let Err(error) = fs::rename(staged, &current) {
        if had_current {
            fs::rename(&backup, &current)
                .map_err(|e| format!("安装失败: {}; 恢复失败: {}", error, e))?;
        }
        if retired.exists() {
            fs::rename(&retired, &backup).map_err(|e| e.to_string())?;
        }
        return Err(error.to_string());
    }
    if retired.exists() {
        fs::remove_dir_all(retired).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn rollback_raybox_directory(kernel_dir: &Path) -> Result<(), String> {
    let current = kernel_dir.join(RAYBOX_INSTALL_DIR);
    let backup = kernel_dir.join(RAYBOX_BACKUP_DIR);
    validate_raybox_manifest(
        &backup.join(RAYBOX_MANIFEST_PATH),
        &backup.join(RAYBOX_ASSET_NAME),
        false,
    )?;
    let temporary = kernel_dir.join(format!("raybox.rollback-{}", uuid::Uuid::new_v4()));
    let had_current = current.exists();
    if had_current {
        fs::rename(&current, &temporary).map_err(|e| e.to_string())?;
    }
    if let Err(error) = fs::rename(&backup, &current) {
        if had_current {
            fs::rename(&temporary, &current).map_err(|e| e.to_string())?;
        }
        return Err(error.to_string());
    }
    if had_current {
        if let Err(error) = fs::rename(&temporary, &backup) {
            fs::rename(&current, &backup).map_err(|e| e.to_string())?;
            fs::rename(&temporary, &current).map_err(|e| e.to_string())?;
            return Err(error.to_string());
        }
    }
    Ok(())
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

fn parse_semver_triplet(version: &str) -> Option<(u64, u64, u64)> {
    let raw = version.trim().trim_start_matches('v');
    let core = raw.split('-').next().unwrap_or(raw);
    let mut it = core.split('.');
    let major = it.next()?.parse::<u64>().ok()?;
    let minor = it.next()?.parse::<u64>().ok()?;
    let patch = it.next().unwrap_or("0").parse::<u64>().ok()?;
    Some((major, minor, patch))
}

pub(crate) fn version_gte(version: &str, min: (u64, u64, u64)) -> bool {
    parse_semver_triplet(version)
        .map(|v| v >= min)
        .unwrap_or(false)
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

    let directory = kernel_path.parent().ok_or("RayBox Windows 内核路径无效")?;
    let manifest = validate_raybox_manifest(
        &raybox_manifest_path(directory),
        &kernel_path,
        raybox_candidate_dir().is_some(),
    )?;
    let supports_naive = manifest
        .build_tags
        .iter()
        .any(|tag| tag == "with_naive_outbound")
        && kernel_support_file_available(&kernel_path, "libcronet.dll");
    Ok(KernelCapabilities {
        version,
        supports_naive,
        supports_icmp_proxy: true,
        supports_bypass_action: true,
    })
}

async fn stage_raybox_release(
    app: &AppHandle,
    lock: &RayBoxLock,
    directory: &Path,
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .user_agent("KunBox/1.0")
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| e.to_string())?;
    let remote: serde_json::Value = client
        .get(format!(
            "https://api.github.com/repos/{}/commits/{}",
            RAYBOX_REPOSITORY_SLUG, lock.tag
        ))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    if remote.get("sha").and_then(serde_json::Value::as_str) != Some(lock.commit.as_str()) {
        return Err("RayBox Windows release tag 与 lock commit 不一致".to_string());
    }
    let base = format!(
        "https://github.com/{}/releases/download/{}",
        RAYBOX_REPOSITORY_SLUG, lock.tag
    );
    let manifest_path = directory.join(RAYBOX_MANIFEST_PATH);
    let response = client
        .get(format!("{}/manifest.json", base))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    download_kernel_archive_to_path(app, response, &manifest_path).await?;
    let bytes = fs::read(&manifest_path).map_err(|e| e.to_string())?;
    if format!("{:x}", Sha256::digest(&bytes)) != lock.manifest_sha256 {
        return Err("RayBox Windows manifest SHA-256 与 lock 不一致".to_string());
    }
    let manifest: RayBoxManifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if manifest.commit != lock.commit
        || manifest.dirty
        || manifest.tag.as_deref() != Some(lock.tag.as_str())
        || manifest.source_digest != lock.source_digest
        || manifest.product != lock.product
        || manifest.version != lock.version
    {
        return Err("RayBox Windows manifest 与正式 lock 身份不一致".to_string());
    }
    for (name, asset) in &manifest.assets {
        let relative = Path::new(name);
        if name != &asset.file
            || relative.components().count() != 1
            || !matches!(
                relative.components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            return Err("RayBox Windows manifest 下载路径无效".to_string());
        }
        let response = client
            .get(format!("{}/{}", base, name))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        download_kernel_archive_to_path(app, response, &directory.join(name)).await?;
    }
    validate_raybox_manifest(&manifest_path, &directory.join(RAYBOX_ASSET_NAME), false)?;
    let mut command = tokio::process::Command::new(directory.join(RAYBOX_ASSET_NAME));
    command.arg("version");
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let output = command.output().await.map_err(|e| e.to_string())?;
    let version = String::from_utf8_lossy(&output.stdout);
    if !output.status.success()
        || !version
            .lines()
            .any(|line| line == format!("RayBox version {}", lock.version))
    {
        return Err("RayBox Windows 实际版本与 lock 不一致".to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn kernel_get_remote_releases(
    app: AppHandle,
    include_prerelease: Option<bool>,
) -> Result<Vec<RemoteRelease>, String> {
    let _ = include_prerelease;
    let lock = read_raybox_lock(&get_bundle_kernel_dir(&app)?.join(RAYBOX_LOCK_PATH))?;
    Ok(vec![raybox_release(&lock)])
}

#[tauri::command]
pub async fn kernel_download(
    app: AppHandle,
    state: State<'_, AppState>,
    tag_name: String,
) -> Result<serde_json::Value, String> {
    let lock = read_raybox_lock(&get_bundle_kernel_dir(&app)?.join(RAYBOX_LOCK_PATH))?;
    if tag_name != lock.tag {
        return Err("请求的版本与可信 RayBox lock 不一致".to_string());
    }
    let _ = app.emit("kernel:download-start", ());
    let kernel_dir = get_kernel_dir_for_install(&app)?;
    fs::create_dir_all(&kernel_dir).map_err(|e| e.to_string())?;
    let staged = kernel_dir.join(format!("raybox.download-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staged).map_err(|e| e.to_string())?;
    let result = async {
        stage_raybox_release(&app, &lock, &staged).await?;
        let was_running = matches!(
            *state.proxy_state.lock().await,
            ProxyState::Connected | ProxyState::Connecting
        );
        if was_running {
            let stopped = singbox_stop_impl(app.clone(), &state).await?;
            if !stopped.success {
                return Err(stopped.error.unwrap_or_else(|| "停止内核失败".to_string()));
            }
        }
        if let Err(error) = replace_raybox_directory(&kernel_dir, &staged) {
            if was_running {
                let _ = singbox_start_impl(app.clone(), &state).await;
            }
            return Err(error);
        }
        if was_running {
            let started = singbox_start_impl(app.clone(), &state).await;
            let start_error = match started {
                Ok(result) if result.success => None,
                Ok(result) => Some(result.error.unwrap_or_else(|| "新内核启动失败".to_string())),
                Err(error) => Some(error),
            };
            if let Some(error) = start_error {
                let _ = singbox_stop_impl(app.clone(), &state).await;
                rollback_raybox_directory(&kernel_dir)?;
                return Err(format!("{}；已恢复上一个 RayBox 工件，保持停止", error));
            }
        }
        Ok(())
    }
    .await;
    if staged.exists() {
        let _ = fs::remove_dir_all(&staged);
    }
    if let Err(error) = result {
        let _ = app.emit("kernel:download-error", &error);
        return Err(error);
    }
    let _ = app.emit("kernel:download-complete", ());
    Ok(serde_json::json!({ "success": true }))
}

#[tauri::command]
pub async fn kernel_rollback(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    if matches!(
        *state.proxy_state.lock().await,
        ProxyState::Connected | ProxyState::Connecting | ProxyState::Disconnecting
    ) {
        return Err("请先停止连接再回滚 RayBox 内核".to_string());
    }
    let kernel_dir = get_kernel_dir_for_install(&app)?;
    rollback_raybox_directory(&kernel_dir)?;
    Ok(serde_json::json!({ "success": true }))
}

#[tauri::command]
pub async fn kernel_can_rollback(app: AppHandle) -> Result<bool, String> {
    let kernel_dir = get_kernel_dir_for_install(&app)?;
    let backup = kernel_dir.join(RAYBOX_BACKUP_DIR);
    Ok(validate_raybox_manifest(
        &backup.join(RAYBOX_MANIFEST_PATH),
        &backup.join(RAYBOX_ASSET_NAME),
        false,
    )
    .is_ok())
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
    open::that(format!("{}/releases", RAYBOX_REPOSITORY)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kernel_open_directory(app: AppHandle) -> Result<(), String> {
    let target_dir = match resolve_kernel_dir(&app) {
        Ok(directory) => directory,
        Err(_) => {
            let directory = get_data_kernel_dir(&app)?;
            fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
            directory
        }
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
    let primary_kernel_path = get_kernel_path(&app)?;

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

    let backup = data_dir.join(RAYBOX_BACKUP_DIR);
    let backup_path = backup.join(RAYBOX_ASSET_NAME);
    if validate_raybox_manifest(&backup.join(RAYBOX_MANIFEST_PATH), &backup_path, false).is_ok() {
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
    fn raybox_version_identity_accepts_only_bound_legacy_or_semantic_tags() {
        let commit = "0123456789abcdef0123456789abcdef01234567";
        assert!(valid_raybox_identity(
            commit,
            "1.0.0-0123456789ab",
            Some(&format!("windows-{commit}")),
            false
        ));
        assert!(valid_raybox_identity(
            commit,
            "0.0.1",
            Some("windows-v0.0.1"),
            false
        ));
        assert!(valid_raybox_identity(commit, "0.0.1", None, true));
        assert!(!valid_raybox_identity(
            commit,
            "0.0.1",
            Some("windows-v0.0.2"),
            false
        ));
        assert!(!valid_raybox_identity(
            commit,
            "00.0.1",
            Some("windows-v00.0.1"),
            false
        ));
        assert!(!valid_raybox_identity(
            commit,
            "0.0.1",
            Some("windows-v0.0.1"),
            true
        ));
        assert!(!valid_raybox_identity(
            "bad",
            "0.0.1",
            Some("windows-v0.0.1"),
            false
        ));
    }

    #[test]
    fn missing_raybox_lock_rejects_download() {
        assert!(read_raybox_lock(&unique_test_dir("missing-lock"))
            .unwrap_err()
            .contains("lock"));
    }

    #[test]
    fn pinned_release_artifacts_match_lock_and_manifest() {
        let directory = std::env::var_os("KUNBOX_TEST_RAYBOX_RELEASE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/resources/libs/raybox"
                ))
            });
        let dir = directory;
        let lock = read_raybox_lock(Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/resources/libs/raybox-windows.lock.json"
        )))
        .unwrap();
        let bytes = fs::read(raybox_manifest_path(&dir)).unwrap();
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes)),
            lock.manifest_sha256
        );
        let manifest = validate_raybox_manifest(
            &raybox_manifest_path(&dir),
            &dir.join(RAYBOX_ASSET_NAME),
            false,
        )
        .unwrap();
        assert_eq!(manifest.commit, lock.commit);
        assert_eq!(manifest.source_digest, lock.source_digest);
        assert_eq!(manifest.product, lock.product);
        assert_eq!(manifest.version, lock.version);
        let output = std::process::Command::new(dir.join(RAYBOX_ASSET_NAME))
            .arg("version")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout)
            .lines()
            .any(|line| line == format!("RayBox version {}", lock.version)));
    }

    #[test]
    fn failed_replacement_restores_previous_kernel_and_backup() {
        let dir = unique_test_dir("rollback-on-failure");
        fs::create_dir_all(dir.join(RAYBOX_INSTALL_DIR)).unwrap();
        fs::create_dir_all(dir.join(RAYBOX_BACKUP_DIR)).unwrap();
        fs::write(dir.join(RAYBOX_INSTALL_DIR).join("identity"), b"previous").unwrap();
        fs::write(dir.join(RAYBOX_BACKUP_DIR).join("identity"), b"backup").unwrap();
        assert!(replace_raybox_directory(&dir, &dir.join("missing")).is_err());
        assert_eq!(
            fs::read(dir.join(RAYBOX_INSTALL_DIR).join("identity")).unwrap(),
            b"previous"
        );
        assert_eq!(
            fs::read(dir.join(RAYBOX_BACKUP_DIR).join("identity")).unwrap(),
            b"backup"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn raybox_lock_binds_tag_manifest_digest_and_release_url() {
        let dir = unique_test_dir("lock");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(RAYBOX_LOCK_PATH);
        let commit = "a".repeat(40);
        let mut value = serde_json::json!({
            "repository": RAYBOX_REPOSITORY_SLUG, "commit": commit,
            "tag": format!("windows-{}", commit), "manifestSha256": "b".repeat(64),
            "sourceDigest": "c".repeat(64), "product": "RayBox", "version": format!("1.0.0-{}", &commit[..12])
        });
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let lock = read_raybox_lock(&path).unwrap();
        assert_eq!(
            raybox_release(&lock).download_url,
            format!(
                "https://github.com/{}/releases/download/windows-{}/{}",
                RAYBOX_REPOSITORY_SLUG, commit, RAYBOX_ASSET_NAME
            )
        );
        for field in [
            "commit",
            "tag",
            "manifestSha256",
            "sourceDigest",
            "version",
            "product",
            "repository",
        ] {
            let original = value[field].clone();
            value[field] = serde_json::json!("invalid");
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(read_raybox_lock(&path).is_err(), "{}", field);
            value[field] = original;
        }
        fs::remove_dir_all(dir).unwrap();
    }

    fn synthetic_manifest(directory: &Path, dirty: bool) -> serde_json::Value {
        let mut pe = vec![0u8; 80];
        pe[..2].copy_from_slice(b"MZ");
        pe[0x3c..0x40].copy_from_slice(&64u32.to_le_bytes());
        pe[64..68].copy_from_slice(b"PE\0\0");
        pe[68..70].copy_from_slice(&0x8664u16.to_le_bytes());
        let mut assets = serde_json::Map::new();
        for name in [
            RAYBOX_ASSET_NAME,
            "LICENSE",
            "RayBox-Protocol-LICENSE",
            "SOURCE.txt",
        ] {
            let data = if name == RAYBOX_ASSET_NAME {
                pe.clone()
            } else {
                name.as_bytes().to_vec()
            };
            fs::write(directory.join(name), &data).unwrap();
            assets.insert(name.to_string(), serde_json::json!({
                "file": name, "size": data.len(), "sha256": format!("{:x}", Sha256::digest(&data))
            }));
        }
        let commit = "a".repeat(40);
        serde_json::json!({
            "schemaVersion": 1, "repository": RAYBOX_REPOSITORY_SLUG,
            "commit": commit, "fullHead": commit,
            "tag": if dirty { None } else { Some(format!("windows-{}", commit)) }, "dirty": dirty,
            "sourceDigest": "b".repeat(64), "sourceDigestAlgorithm": "sha256-git-path-kind-content-v1",
            "source": {"url": format!("{}/tree/{}", RAYBOX_REPOSITORY, commit),
                "runtimeLicense": "docs/reference/runtime/LICENSE",
                "protocolLicense": "docs/reference/protocol/LICENSE"},
            "product": "RayBox", "version": "1.0.0-aaaaaaaaaaaa",
            "go": "go version go1.27.3 windows/amd64", "goVersion": "go version go1.27.3 windows/amd64",
            "goos": "windows", "goarch": "amd64", "buildTags": ["with_clash_api"],
            "rxDefault": false, "oobDefault": false, "pe": {"machineCode": "0x8664"}, "assets": assets
        })
    }

    #[test]
    fn raybox_manifest_rejects_tampering_and_dirty_release() {
        let dir = unique_test_dir("manifest");
        fs::create_dir_all(&dir).unwrap();
        let value = synthetic_manifest(&dir, true);
        let path = dir.join(RAYBOX_MANIFEST_PATH);
        let exe = dir.join(RAYBOX_ASSET_NAME);
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(validate_raybox_manifest(&path, &exe, true).is_ok());
        assert!(validate_raybox_manifest(&path, &exe, false).is_err());
        for (pointer, replacement) in [
            ("/fullHead", serde_json::json!("b".repeat(40))),
            ("/sourceDigest", serde_json::json!("invalid")),
            ("/sourceDigestAlgorithm", serde_json::json!("invalid")),
            ("/tag", serde_json::json!("windows-fake")),
            ("/rxDefault", serde_json::json!(true)),
            ("/oobDefault", serde_json::json!(true)),
            ("/buildTags", serde_json::json!(["with_tailscale"])),
            ("/goVersion", serde_json::json!("invalid")),
            ("/source/url", serde_json::json!("https://example.invalid")),
            ("/source/runtimeLicense", serde_json::json!("other")),
            ("/source/protocolLicense", serde_json::json!("other")),
            ("/product", serde_json::json!("other")),
            ("/version", serde_json::json!("1.0.0-wrong")),
            ("/assets/LICENSE/file", serde_json::json!("../LICENSE")),
        ] {
            let mut mutated = value.clone();
            *mutated.pointer_mut(pointer).unwrap() = replacement;
            fs::write(&path, serde_json::to_vec(&mutated).unwrap()).unwrap();
            assert!(
                validate_raybox_manifest(&path, &exe, true).is_err(),
                "{}",
                pointer
            );
        }
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        fs::write(dir.join("LICENSE"), b"tampered").unwrap();
        assert!(validate_raybox_manifest(&path, &exe, true).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn raybox_pe_truncation_is_rejected_without_panic() {
        let dir = unique_test_dir("pe");
        fs::create_dir_all(&dir).unwrap();
        let mut value = synthetic_manifest(&dir, true);
        let path = dir.join(RAYBOX_MANIFEST_PATH);
        let exe = dir.join(RAYBOX_ASSET_NAME);
        let mut bytes = fs::read(&exe).unwrap();
        bytes[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
        fs::write(&exe, &bytes).unwrap();
        value["assets"][RAYBOX_ASSET_NAME]["sha256"] =
            serde_json::json!(format!("{:x}", Sha256::digest(&bytes)));
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(validate_raybox_manifest(&path, &exe, true).is_err());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn raybox_directory_install_and_rollback_keep_manifest_with_executable() {
        let dir = unique_test_dir("directory-install");
        let first = dir.join("first");
        let second = dir.join("second");
        fs::create_dir_all(&first).unwrap();
        fs::create_dir_all(&second).unwrap();
        let first_manifest = synthetic_manifest(&first, false);
        fs::write(
            first.join(RAYBOX_MANIFEST_PATH),
            serde_json::to_vec(&first_manifest).unwrap(),
        )
        .unwrap();
        let second_manifest = synthetic_manifest(&second, false);
        fs::write(
            second.join(RAYBOX_MANIFEST_PATH),
            serde_json::to_vec(&second_manifest).unwrap(),
        )
        .unwrap();
        fs::write(first.join("marker"), b"first").unwrap();
        fs::write(second.join("marker"), b"second").unwrap();
        replace_raybox_directory(&dir, &first).unwrap();
        replace_raybox_directory(&dir, &second).unwrap();
        assert_eq!(
            fs::read(dir.join(RAYBOX_INSTALL_DIR).join("marker")).unwrap(),
            b"second"
        );
        rollback_raybox_directory(&dir).unwrap();
        assert_eq!(
            fs::read(dir.join(RAYBOX_INSTALL_DIR).join("marker")).unwrap(),
            b"first"
        );
        assert_eq!(
            fs::read(dir.join(RAYBOX_BACKUP_DIR).join("marker")).unwrap(),
            b"second"
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn clears_actual_cache_db_and_ruleset_cache() {
        let data_dir = unique_test_dir("clear-cache");
        let state = AppState::new(data_dir.clone());

        fs::create_dir_all(state.rulesets_cache_dir()).unwrap();
        fs::create_dir_all(state.data_dir.join("cache")).unwrap();
        fs::write(state.config_dir.join("cache.db"), vec![1u8; 16]).unwrap();
        fs::write(state.rulesets_cache_dir().join("demo.srs"), vec![2u8; 24]).unwrap();
        fs::write(
            state.data_dir.join("cache").join("legacy.bin"),
            vec![3u8; 8],
        )
        .unwrap();

        let freed = clear_kernel_cache_targets(&kernel_cache_targets(&state)).unwrap();

        assert!(freed >= 48);
        assert!(!state.config_dir.join("cache.db").exists());
        assert!(!state.rulesets_cache_dir().exists());
        assert!(!state.data_dir.join("cache").exists());

        let _ = fs::remove_dir_all(data_dir);
    }
}
