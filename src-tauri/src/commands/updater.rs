use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub has_update: bool,
    pub version: Option<String>,
    pub date: Option<String>,
    pub body: Option<String>,
}

fn build_updater_progress_payload(
    downloaded: &mut u64,
    chunk_length: usize,
    content_length: Option<u64>,
) -> serde_json::Value {
    *downloaded = downloaded.saturating_add(u64::try_from(chunk_length).unwrap_or(u64::MAX));
    serde_json::json!({
        "downloaded": *downloaded,
        "contentLength": content_length.unwrap_or(0)
    })
}

#[tauri::command]
pub async fn updater_check(app: AppHandle) -> Result<UpdateInfo, String> {
    let current_version = app.package_info().version.to_string();

    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;

    if let Some(update) = update {
        Ok(UpdateInfo {
            current_version,
            has_update: true,
            version: Some(update.version.clone()),
            date: update.date.map(|d| d.to_string()),
            body: update.body.clone(),
        })
    } else {
        Ok(UpdateInfo {
            current_version,
            has_update: false,
            version: None,
            date: None,
            body: None,
        })
    }
}

#[tauri::command]
pub fn updater_get_current_version(app: AppHandle) -> Result<String, String> {
    Ok(app.package_info().version.to_string())
}

#[tauri::command]
pub async fn updater_download_and_install(app: AppHandle) -> Result<UpdateInfo, String> {
    let current_version = app.package_info().version.to_string();

    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "当前已是最新版本".to_string())?;

    let target_version = update.version.clone();
    let target_date = update.date.map(|d| d.to_string());
    let target_body = update.body.clone();
    let mut downloaded = 0u64;

    update
        .download_and_install(
            |chunk_length, content_length| {
                let _ = app.emit(
                    "updater:download-progress",
                    build_updater_progress_payload(&mut downloaded, chunk_length, content_length),
                );
            },
            || {
                let _ = app.emit("updater:download-finished", ());
            },
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(UpdateInfo {
        current_version,
        has_update: true,
        version: Some(target_version),
        date: target_date,
        body: target_body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn updater_progress_payload_accumulates_chunks() {
        let mut downloaded = 0u64;

        let first = build_updater_progress_payload(&mut downloaded, 512, Some(2048));
        let second = build_updater_progress_payload(&mut downloaded, 256, Some(2048));

        assert_eq!(first["downloaded"], serde_json::json!(512u64));
        assert_eq!(second["downloaded"], serde_json::json!(768u64));
        assert_eq!(second["contentLength"], serde_json::json!(2048u64));
    }
}
