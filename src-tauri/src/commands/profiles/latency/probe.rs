use super::*;

pub(super) async fn test_latency_via_clash_api(
    proxy_name: &str,
    port: u16,
    test_url: &str,
    timeout_ms: u32,
) -> Result<i64, LatencyProbeError> {
    let effective_timeout_ms = timeout_ms.max(1) as u64;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_millis(
            effective_timeout_ms.saturating_add(2_000),
        ))
        .build()
        .map_err(|_| LatencyProbeError::Failed)?;

    let encoded_name = urlencoding::encode(proxy_name);
    let url = format!(
        "http://127.0.0.1:{}/proxies/{}/delay?url={}&timeout={}",
        port,
        encoded_name,
        urlencoding::encode(test_url),
        effective_timeout_ms
    );

    let response = client.get(&url).send().await.map_err(|e| {
        if e.is_timeout() {
            LatencyProbeError::Timeout
        } else {
            LatencyProbeError::Failed
        }
    })?;

    if !response.status().is_success() {
        if response.status() == reqwest::StatusCode::GATEWAY_TIMEOUT
            || response.status() == reqwest::StatusCode::REQUEST_TIMEOUT
        {
            return Err(LatencyProbeError::Timeout);
        }
        if response.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE
            || response.status() == reqwest::StatusCode::BAD_GATEWAY
        {
            return Err(LatencyProbeError::ProxyFailed);
        }
        return Err(LatencyProbeError::Failed);
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|_| LatencyProbeError::Failed)?;
    if let Some(delay) = json.get("delay").and_then(|d| d.as_i64()) {
        if delay > 0 {
            return Ok(delay);
        }
        return Err(LatencyProbeError::Timeout);
    }

    Err(LatencyProbeError::Failed)
}

pub(super) async fn test_latency_via_clash_api_cancellable(
    proxy_name: &str,
    port: u16,
    test_url: &str,
    timeout_ms: u32,
    cancel_token: CancellationToken,
) -> Result<i64, LatencyProbeError> {
    tokio::select! {
        _ = cancel_token.cancelled() => Err(LatencyProbeError::Failed),
        result = test_latency_via_clash_api(proxy_name, port, test_url, timeout_ms) => result,
    }
}

pub(super) async fn test_latency_via_http_proxy(
    proxy_port: u16,
    test_url: &str,
    timeout_ms: u32,
) -> Result<i64, LatencyProbeError> {
    let effective_timeout_ms = timeout_ms.max(1) as u64;
    let proxy_url = format!("http://127.0.0.1:{}", proxy_port);
    let proxy = reqwest::Proxy::all(&proxy_url).map_err(|_| LatencyProbeError::Failed)?;
    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(std::time::Duration::from_millis(
            effective_timeout_ms.saturating_add(2_000),
        ))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .map_err(|_| LatencyProbeError::Failed)?;
    let started = std::time::Instant::now();
    let response = client.get(test_url).send().await.map_err(|e| {
        if e.is_timeout() {
            LatencyProbeError::Timeout
        } else {
            LatencyProbeError::ProxyFailed
        }
    })?;

    if response.status().is_server_error() {
        return Err(LatencyProbeError::ProxyFailed);
    }

    let latency = started.elapsed().as_millis().max(1) as i64;
    Ok(latency)
}

pub(super) async fn test_latency_via_http_proxy_cancellable(
    proxy_port: u16,
    test_url: &str,
    timeout_ms: u32,
    cancel_token: CancellationToken,
) -> Result<i64, LatencyProbeError> {
    tokio::select! {
        _ = cancel_token.cancelled() => Err(LatencyProbeError::Failed),
        result = test_latency_via_http_proxy(proxy_port, test_url, timeout_ms) => result,
    }
}
