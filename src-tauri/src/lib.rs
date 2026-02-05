use tauri::Manager;
use std::path::PathBuf;

mod types;
mod state;
mod commands;

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .setup(|app| {
            // Initialize app state
            let data_dir = get_data_dir();
            std::fs::create_dir_all(&data_dir).ok();
            
            // Create configs directory
            let configs_dir = data_dir.join("configs");
            std::fs::create_dir_all(&configs_dir).ok();
            
            log::info!("Data directory: {:?}", data_dir);
            
            let state = AppState::new(data_dir);
            app.manage(state);

            // Show window after setup
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
            }

            // Setup tray icon
            setup_tray(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Hide window instead of closing
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            // Settings
            commands::get_settings,
            commands::set_settings,
            // Profiles
            commands::profile_list,
            commands::profile_add,
            commands::profile_update,
            commands::profile_delete,
            commands::profile_set_active,
            commands::profile_edit,
            commands::profile_set_enabled,
            // Nodes
            commands::node_list,
            commands::node_set_active,
            commands::node_delete,
            commands::node_add,
            commands::node_export,
            commands::node_test_latency,
            commands::node_test_all,
            // Profiles extra
            commands::profile_import_content,
            // Rulesets
            commands::ruleset_list,
            commands::ruleset_save,
            commands::ruleset_download,
            commands::ruleset_is_cached,
            commands::ruleset_fetch_hub,
            // Singbox
            commands::singbox_start,
            commands::singbox_stop,
            commands::singbox_restart,
            commands::singbox_get_status,
            commands::singbox_switch_node,
            commands::singbox_enable_system_proxy,
            commands::singbox_disable_system_proxy,
            // Window
            commands::window_minimize,
            commands::window_maximize,
            commands::window_close,
            commands::window_show,
            commands::quit_app,
            // Kernel
            commands::kernel_get_local_version,
            commands::kernel_get_remote_releases,
            commands::kernel_download,
            commands::kernel_rollback,
            commands::kernel_can_rollback,
            commands::kernel_clear_cache,
            commands::kernel_open_releases_page,
            commands::kernel_open_directory,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn get_data_dir() -> PathBuf {
    if let Some(appdata) = std::env::var_os("APPDATA") {
        PathBuf::from(appdata).join("KunBox")
    } else {
        PathBuf::from(".")
    }
}

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::menu::{Menu, MenuItem};

    let show_item = MenuItem::with_id(app, "show", "显示", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .on_menu_event(|app, event| {
            match event.id.as_ref() {
                "show" => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}
