use tauri::State;
use std::fs;
use crate::state::AppState;
use crate::types::{CustomRules, DomainRule};

pub(crate) fn load_custom_rules(state: &AppState) -> CustomRules {
    let file = state.custom_rules_file();
    if file.exists() {
        match fs::read_to_string(&file) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(rules) => return rules,
                Err(e) => log::warn!("Failed to parse custom rules file {:?}: {}", file, e),
            },
            Err(e) => log::warn!("Failed to read custom rules file {:?}: {}", file, e),
        }
    }
    CustomRules::default()
}

fn save_custom_rules(state: &AppState, rules: &CustomRules) -> Result<(), String> {
    fs::create_dir_all(&state.data_dir).map_err(|e| e.to_string())?;
    let content = serde_json::to_string_pretty(rules).map_err(|e| e.to_string())?;
    fs::write(state.custom_rules_file(), content).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn custom_rules_get(state: State<'_, AppState>) -> Result<CustomRules, String> {
    let rules = load_custom_rules(&state);
    *state.custom_rules.lock().await = rules.clone();
    Ok(rules)
}

#[tauri::command]
pub async fn custom_rules_save(state: State<'_, AppState>, rules: CustomRules) -> Result<(), String> {
    save_custom_rules(&state, &rules)?;
    *state.custom_rules.lock().await = rules;
    Ok(())
}

#[tauri::command]
pub async fn domain_rules_get(state: State<'_, AppState>) -> Result<Vec<DomainRule>, String> {
    let rules = load_custom_rules(&state);
    *state.custom_rules.lock().await = rules.clone();
    Ok(rules.domain_rules)
}

#[tauri::command]
pub async fn domain_rules_save(state: State<'_, AppState>, rules: Vec<DomainRule>) -> Result<(), String> {
    let mut custom_rules = state.custom_rules.lock().await;
    custom_rules.domain_rules = rules;
    save_custom_rules(&state, &custom_rules)?;
    Ok(())
}

