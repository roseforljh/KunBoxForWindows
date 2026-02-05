use tauri::State;
use std::fs;
use crate::state::AppState;
use crate::types::RuleSet;

// GitHub 镜像列表
const GITHUB_MIRRORS: &[&str] = &[
    "https://raw.githubusercontent.com/",  // 原始地址
    "https://mirror.ghproxy.com/https://raw.githubusercontent.com/",  // ghproxy
    "https://raw.gitmirror.com/",  // gitmirror
    "https://raw.fastgit.org/",  // fastgit (可能失效)
];

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

fn load_rulesets(state: &AppState) -> Vec<RuleSet> {
    let file = state.rulesets_file();
    if file.exists() {
        if let Ok(content) = fs::read_to_string(&file) {
            if let Ok(rulesets) = serde_json::from_str(&content) {
                return rulesets;
            }
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
pub async fn ruleset_save(state: State<'_, AppState>, rulesets: Vec<RuleSet>) -> Result<(), String> {
    save_rulesets(&state, &rulesets)?;
    *state.rulesets.lock().await = rulesets;
    Ok(())
}

#[tauri::command]
pub async fn ruleset_download(state: State<'_, AppState>, ruleset: RuleSet) -> Result<serde_json::Value, String> {
    if ruleset.rule_type != "remote" {
        return Ok(serde_json::json!({ "success": true, "cached": true }));
    }

    let cache_dir = state.rulesets_cache_dir();
    fs::create_dir_all(&cache_dir).map_err(|e| e.to_string())?;
    
    let cache_file = cache_dir.join(format!("{}.srs", ruleset.tag));
    
    if cache_file.exists() {
        return Ok(serde_json::json!({ "success": true, "cached": true }));
    }

    let original_url = ruleset.url.ok_or("No URL for ruleset")?;
    
    // 提取 GitHub 路径（如果是 GitHub URL）
    let github_path = extract_github_path(&original_url);
    
    // 创建代理客户端（使用本地 VPN 代理）
    let proxy_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .proxy(reqwest::Proxy::all("http://127.0.0.1:7890").ok().unwrap())
        .build()
        .ok();
    
    // 创建直连客户端
    let direct_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    
    // 尝试下载的 URL 列表
    let urls_to_try: Vec<String> = if let Some(path) = &github_path {
        // 如果是 GitHub 地址，尝试多个镜像
        GITHUB_MIRRORS.iter().map(|mirror| format!("{}{}", mirror, path)).collect()
    } else {
        // 非 GitHub 地址，直接使用原始 URL
        vec![original_url.clone()]
    };
    
    let mut last_error = String::new();
    
    for url in &urls_to_try {
        // 1. 先尝试代理下载
        if let Some(client) = &proxy_client {
            match download_and_verify(client, url).await {
                Ok(bytes) => {
                    fs::write(&cache_file, bytes).map_err(|e| e.to_string())?;
                    log::info!("Ruleset downloaded via proxy: {}", ruleset.tag);
                    return Ok(serde_json::json!({ "success": true, "cached": false, "url": url }));
                }
                Err(e) => {
                    log::warn!("Proxy download failed for {}: {}", url, e);
                    last_error = e;
                }
            }
        }
        
        // 2. 回退到直连
        match download_and_verify(&direct_client, url).await {
            Ok(bytes) => {
                fs::write(&cache_file, bytes).map_err(|e| e.to_string())?;
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

/// 下载并验证文件
async fn download_and_verify(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let response = client.get(url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }
    
    let bytes = response.bytes()
        .await
        .map_err(|e| format!("Read body failed: {}", e))?
        .to_vec();
    
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
    
    // 检查是否是 JSON 错误
    if header.trim().starts_with('{') {
        return Err("Received JSON error response".to_string());
    }
    
    Ok(bytes)
}

#[tauri::command]
pub async fn ruleset_is_cached(state: State<'_, AppState>, tag: String) -> Result<bool, String> {
    let cache_file = state.rulesets_cache_dir().join(format!("{}.srs", tag));
    Ok(cache_file.exists())
}

/// 从 GitHub API 获取规则集仓库列表（代理优先 + 直连回退）
#[tauri::command]
pub async fn ruleset_fetch_hub() -> Result<serde_json::Value, String> {
    let url = "https://api.github.com/repos/SagerNet/sing-geosite/git/trees/rule-set?recursive=1";
    
    // 创建代理客户端
    let proxy_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .proxy(reqwest::Proxy::all("http://127.0.0.1:7890").ok().unwrap())
        .build()
        .ok();
    
    // 创建直连客户端
    let direct_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    
    // 1. 先尝试代理
    if let Some(client) = &proxy_client {
        match client.get(url)
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
    
    // 2. 回退到直连
    let resp = direct_client.get(url)
        .header("User-Agent", "KunBox-Windows-App")
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    
    let data = resp.json::<serde_json::Value>()
        .await
        .map_err(|e| format!("Parse error: {}", e))?;
    
    log::info!("Fetched hub via direct");
    Ok(data)
}
