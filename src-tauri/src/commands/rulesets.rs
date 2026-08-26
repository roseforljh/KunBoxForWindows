use crate::state::AppState;
use crate::types::RuleSet;
use std::fs;
use tauri::State;

const RULESET_CDN_PREFIX: &str = "https://cdn.jsdelivr.net/gh/";

fn get_default_rulesets() -> Vec<RuleSet> {
    vec![
        RuleSet {
            id: "1".to_string(),
            tag: "geosite-cn".to_string(),
            name: "中国网站".to_string(),
            rule_type: "remote".to_string(),
            format: "binary".to_string(),
            url: Some("https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs".to_string()),
            outbound_mode: "direct".to_string(),
            outbound_value: None,
            enabled: false,
            is_built_in: true,
        },
        RuleSet {
            id: "2".to_string(),
            tag: "geoip-cn".to_string(),
            name: "中国 IP".to_string(),
            rule_type: "remote".to_string(),
            format: "binary".to_string(),
            url: Some("https://raw.githubusercontent.com/SagerNet/sing-geoip/rule-set/geoip-cn.srs".to_string()),
            outbound_mode: "direct".to_string(),
            outbound_value: None,
            enabled: false,
            is_built_in: true,
        },
        RuleSet {
            id: "3".to_string(),
            tag: "geosite-private".to_string(),
            name: "私有地址".to_string(),
            rule_type: "remote".to_string(),
            format: "binary".to_string(),
            url: Some("https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-private.srs".to_string()),
            outbound_mode: "direct".to_string(),
            outbound_value: None,
            enabled: false,
            is_built_in: true,
        },
        RuleSet {
            id: "4".to_string(),
            tag: "geosite-category-ads-all".to_string(),
            name: "广告拦截".to_string(),
            rule_type: "remote".to_string(),
            format: "binary".to_string(),
            url: Some("https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-category-ads-all.srs".to_string()),
            outbound_mode: "block".to_string(),
            outbound_value: None,
            enabled: false,
            is_built_in: true,
        },
    ]
}

pub(crate) fn load_rulesets(state: &AppState) -> Vec<RuleSet> {
    let file = state.rulesets_file();
    if file.exists() {
        match fs::read_to_string(&file) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(rulesets) => return rulesets,
                Err(e) => log::warn!("Failed to parse rulesets file {:?}: {}", file, e),
            },
            Err(e) => log::warn!("Failed to read rulesets file {:?}: {}", file, e),
        }
    }
    get_default_rulesets()
}

fn save_rulesets(state: &AppState, rulesets: &[RuleSet]) -> Result<(), String> {
    fs::create_dir_all(&state.data_dir).map_err(|e| e.to_string())?;
    let content = serde_json::to_string_pretty(rulesets).map_err(|e| e.to_string())?;
    fs::write(state.rulesets_file(), content).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn ruleset_list(state: State<'_, AppState>) -> Result<Vec<RuleSet>, String> {
    let rulesets = load_rulesets(&state);
    *state.rulesets.lock().await = rulesets.clone();
    Ok(rulesets)
}

#[tauri::command]
pub async fn ruleset_save(
    state: State<'_, AppState>,
    rulesets: Vec<RuleSet>,
) -> Result<(), String> {
    save_rulesets(&state, &rulesets)?;
    *state.rulesets.lock().await = rulesets;
    Ok(())
}

fn is_valid_ruleset_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.len() <= 128
        && tag
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn ruleset_cache_path(state: &AppState, tag: &str) -> Result<std::path::PathBuf, String> {
    if !is_valid_ruleset_tag(tag) {
        return Err("Invalid ruleset tag".to_string());
    }

    Ok(state.rulesets_cache_dir().join(format!("{}.srs", tag)))
}

async fn build_local_proxy_client(state: &AppState, timeout_secs: u64) -> Option<reqwest::Client> {
    let settings = state.settings.lock().await.clone();
    let proxy_url = format!("http://127.0.0.1:{}", settings.local_port);

    reqwest::Proxy::all(&proxy_url).ok().and_then(|proxy| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout_secs))
            .proxy(proxy)
            .build()
            .ok()
    })
}

fn build_direct_client(timeout_secs: u64) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| e.to_string())
}

fn is_official_source_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    (lower.contains("sing-geosite/rule-set/") || lower.contains("sing-geoip/rule-set/"))
        && lower.ends_with(".json")
}

fn github_download_urls(url: &str) -> Vec<String> {
    let Some(path) = extract_github_path(url) else {
        return vec![url.to_string()];
    };
    let canonical = format!("https://raw.githubusercontent.com/{}", path);
    let mut urls = vec![canonical.clone()];
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() >= 4 {
        let owner = parts[0];
        let repo = parts[1];
        let branch = parts[2];
        let rest = parts[3..].join("/");
        urls.push(format!(
            "{}{}/{}@{}/{}",
            RULESET_CDN_PREFIX, owner, repo, branch, rest
        ));
    }
    urls
}

fn write_cache_atomically(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    let temp_path = path.with_extension("srs.tmp");
    fs::write(&temp_path, bytes).map_err(|e| e.to_string())?;
    if let Err(err) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(err.to_string());
    }
    Ok(())
}

#[tauri::command]
pub async fn ruleset_download(
    state: State<'_, AppState>,
    ruleset: RuleSet,
) -> Result<serde_json::Value, String> {
    if ruleset.rule_type != "remote" {
        return Ok(serde_json::json!({ "success": true, "cached": true }));
    }

    let cache_dir = state.rulesets_cache_dir();
    fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;

    let cache_file = ruleset_cache_path(&state, &ruleset.tag)?;

    if cache_file.exists() {
        return Ok(serde_json::json!({ "success": true, "cached": true }));
    }

    let original_url = ruleset.url.ok_or("No URL for ruleset")?;

    if ruleset.format == "source" && is_official_source_url(&original_url) {
        return Err("官方规则集仓库当前只提供 Binary 格式，请选择 Binary 下载".to_string());
    }

    // 提取 GitHub 路径（如果是 GitHub URL）
    let github_path = extract_github_path(&original_url);

    // 创建代理客户端（使用本地 VPN 代理）
    let proxy_client = build_local_proxy_client(&state, 30).await;

    // 创建直连客户端
    let direct_client = build_direct_client(30)?;

    // 尝试下载的 URL 列表
    let urls_to_try = if github_path.is_some() {
        github_download_urls(&original_url)
    } else {
        vec![original_url.clone()]
    };

    #[allow(unused_assignments)]
    let mut last_error = String::new();

    for url in &urls_to_try {
        // 1. 先尝试代理下载
        if let Some(client) = &proxy_client {
            match download_and_verify(client, url, &ruleset.format).await {
                Ok(bytes) => {
                    write_cache_atomically(&cache_file, &bytes)?;
                    log::info!("Ruleset downloaded via proxy: {}", ruleset.tag);
                    return Ok(serde_json::json!({ "success": true, "cached": false, "url": url }));
                }
                Err(e) => {
                    log::warn!("Proxy download failed for {}: {}", url, e);
                    // 继续尝试直连，不需要保存错误
                }
            }
        }

        // 2. 回退到直连
        match download_and_verify(&direct_client, url, &ruleset.format).await {
            Ok(bytes) => {
                write_cache_atomically(&cache_file, &bytes)?;
                log::info!("Ruleset downloaded via direct: {}", ruleset.tag);
                return Ok(serde_json::json!({ "success": true, "cached": false, "url": url }));
            }
            Err(e) => {
                log::warn!("Direct download failed for {}: {}", url, e);
                last_error = e;
            }
        }
    }

    Err(format!("All download attempts failed: {}", last_error))
}

/// 从 GitHub URL 提取路径部分
fn extract_github_path(url: &str) -> Option<String> {
    let raw_prefix = "https://raw.githubusercontent.com/";

    if url.starts_with(raw_prefix) {
        return Some(url[raw_prefix.len()..].to_string());
    }

    // 处理已经使用镜像的 URL
    if url.contains("raw.githubusercontent.com/") {
        let idx = url.find("raw.githubusercontent.com/")?;
        return Some(url[idx + "raw.githubusercontent.com/".len()..].to_string());
    }

    // jsDelivr 格式: https://cdn.jsdelivr.net/gh/user/repo@branch/path
    if url.contains("cdn.jsdelivr.net/gh/") {
        let idx = url.find("cdn.jsdelivr.net/gh/")?;
        let path = &url[idx + "cdn.jsdelivr.net/gh/".len()..];
        // 转换 user/repo@branch/file 到 user/repo/branch/file
        if let Some((user_repo, rest)) = path.split_once('@') {
            return Some(format!("{}/{}", user_repo, rest));
        }
    }

    None
}

const MAX_RULESET_SIZE_BYTES: usize = 10 * 1024 * 1024;

/// 下载并验证文件
async fn download_and_verify(
    client: &reqwest::Client,
    url: &str,
    format: &str,
) -> Result<Vec<u8>, String> {
    let response = client.get(url).send().await.map_err(|e| {
        if e.is_timeout() {
            format!("连接超时: {}", e)
        } else if e.is_connect() {
            format!("连接失败: {}", e)
        } else {
            format!("请求失败: {}", e)
        }
    })?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }

    if let Some(content_length) = response.content_length() {
        if content_length as usize > MAX_RULESET_SIZE_BYTES {
            return Err("Ruleset too large".to_string());
        }
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("读取响应失败: {}", e))?
        .to_vec();

    if bytes.len() > MAX_RULESET_SIZE_BYTES {
        return Err("Ruleset too large".to_string());
    }

    // 验证文件内容
    if bytes.len() < 10 {
        return Err("File too small".to_string());
    }

    // 检查是否是 HTML 错误页面
    let header = String::from_utf8_lossy(&bytes[..std::cmp::min(64, bytes.len())]);
    let header_lower = header.to_lowercase();

    if header_lower.contains("<!doctype html") || header_lower.contains("<html") {
        return Err("Received HTML instead of binary".to_string());
    }

    if format == "source" {
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .map_err(|e| format!("Source JSON 格式无效: {}", e))?;
    } else if header.trim().starts_with('{') {
        return Err("收到 JSON 错误响应，而不是 Binary 规则集".to_string());
    }

    Ok(bytes)
}

#[tauri::command]
pub async fn ruleset_is_cached(state: State<'_, AppState>, tag: String) -> Result<bool, String> {
    let cache_file = ruleset_cache_path(&state, &tag)?;
    Ok(cache_file.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_ruleset_tags() {
        assert!(ruleset_cache_path(
            &AppState::new(std::path::PathBuf::from("/tmp/test")),
            "geosite-cn"
        )
        .is_ok());
        assert!(ruleset_cache_path(
            &AppState::new(std::path::PathBuf::from("/tmp/test")),
            "../evil"
        )
        .is_err());
    }

    #[test]
    fn extracts_github_paths() {
        assert_eq!(
            extract_github_path(
                "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs"
            ),
            Some("SagerNet/sing-geosite/rule-set/geosite-cn.srs".to_string())
        );
        assert_eq!(
            extract_github_path(
                "https://cdn.jsdelivr.net/gh/SagerNet/sing-geosite@rule-set/geosite-cn.srs"
            ),
            Some("SagerNet/sing-geosite/rule-set/geosite-cn.srs".to_string())
        );
        assert_eq!(extract_github_path("https://example.com/file.srs"), None);
    }

    #[test]
    fn github_download_urls_use_official_then_cdn() {
        assert_eq!(
            github_download_urls(
                "https://ghp.ci/https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs"
            ),
            vec![
                "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.srs",
                "https://cdn.jsdelivr.net/gh/SagerNet/sing-geosite@rule-set/geosite-cn.srs",
            ]
        );
    }

    #[test]
    fn official_source_urls_are_rejected_but_custom_sources_are_allowed() {
        assert!(is_official_source_url(
            "https://raw.githubusercontent.com/SagerNet/sing-geosite/rule-set/geosite-cn.json"
        ));
        assert!(!is_official_source_url(
            "https://example.com/custom-rules.json"
        ));
    }

    #[test]
    fn atomic_cache_write_leaves_no_temp_file() {
        let dir = std::env::temp_dir().join(format!(
            "kunbox-ruleset-atomic-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("demo.srs");
        write_cache_atomically(&path, b"valid ruleset bytes").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"valid ruleset bytes");
        assert!(!dir.join("demo.srs.tmp").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn source_content_accepts_json_and_binary_rejects_json_error() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for body in [b"{\"rules\":[]}".as_slice(), b"{\"error\":true}".as_slice()] {
                let (socket, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 512];
                let _ = socket.readable().await;
                let _ = socket.try_read(&mut request);
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    String::from_utf8_lossy(body)
                );
                socket.writable().await.unwrap();
                socket.try_write(response.as_bytes()).unwrap();
            }
        });

        let client = build_direct_client(5).unwrap();
        let source = download_and_verify(&client, &format!("http://{}/source", address), "source")
            .await
            .unwrap();
        assert_eq!(source, b"{\"rules\":[]}");
        let binary = download_and_verify(&client, &format!("http://{}/binary", address), "binary")
            .await
            .unwrap_err();
        assert!(binary.contains("JSON"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn local_proxy_client_uses_settings_port() {
        let state = AppState::new(std::path::PathBuf::from("/tmp/test"));
        state.settings.lock().await.local_port = 17890;

        let client = build_local_proxy_client(&state, 5).await;

        assert!(client.is_some());
    }
}

/// 从 GitHub API 获取规则集仓库列表（代理优先 + 直连回退）
#[tauri::command]
pub async fn ruleset_fetch_hub(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let url = "https://api.github.com/repos/SagerNet/sing-geosite/git/trees/rule-set?recursive=1";
    let proxy_client = build_local_proxy_client(&state, 15).await;

    let direct_client = build_direct_client(15)?;

    if let Some(client) = &proxy_client {
        match client
            .get(url)
            .header("User-Agent", "KunBox-Windows-App")
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(data) = resp.json::<serde_json::Value>().await {
                    log::info!("Fetched hub via proxy");
                    return Ok(data);
                }
            }
            Ok(resp) => {
                log::warn!("Proxy request failed with status: {}", resp.status());
            }
            Err(e) => {
                log::warn!("Proxy request error: {}", e);
            }
        }
    }

    let resp = direct_client
        .get(url)
        .header("User-Agent", "KunBox-Windows-App")
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                format!("连接超时: {}", e)
            } else {
                format!("请求失败: {}", e)
            }
        })?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }

    let data = resp
        .json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;

    log::info!("Fetched hub via direct");
    Ok(data)
}
