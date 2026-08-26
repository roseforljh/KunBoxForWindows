use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use tauri::Emitter;
use tauri::Manager;

mod commands;
mod state;
mod types;

use state::AppState;
use types::AppSettings;

fn append_startup_diagnostic(data_dir: &PathBuf, message: &str) {
    let _ = std::fs::create_dir_all(data_dir);
    let path = data_dir.join("startup-diagnostics.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
        let _ = writeln!(file, "[{}] {}", ts, message);
    }
}

/// Synchronously read settings from file
fn read_settings_sync(data_dir: &PathBuf) -> AppSettings {
    let settings_file = data_dir.join("settings.json");
    if settings_file.exists() {
        if let Ok(content) = std::fs::read_to_string(&settings_file) {
            if let Ok(settings) = serde_json::from_str::<AppSettings>(&content) {
                return settings;
            }
        }
    }
    AppSettings::default()
}

fn spawn_safe_exit(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        #[cfg(windows)]
        append_startup_diagnostic(&get_data_dir(), "safe exit requested");

        if let Some(state) = app_handle.try_state::<AppState>() {
            #[cfg(windows)]
            append_startup_diagnostic(
                &state.data_dir,
                "safe exit: stopping sing-box before app exit",
            );

            let stop_result =
                commands::singbox::singbox_stop_impl(app_handle.clone(), &state).await;
            match stop_result {
                Ok(_) => {
                    #[cfg(windows)]
                    append_startup_diagnostic(
                        &state.data_dir,
                        "safe exit: sing-box stopped successfully",
                    );
                }
                Err(err) => {
                    #[cfg(windows)]
                    append_startup_diagnostic(
                        &state.data_dir,
                        &format!("safe exit: sing-box stop failed: {}", err),
                    );
                    return;
                }
            }
        } else {
            #[cfg(windows)]
            append_startup_diagnostic(&get_data_dir(), "safe exit: AppState unavailable");
        }

        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        app_handle.exit(0);
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[cfg(windows)]
    let data_dir = get_data_dir();

    #[cfg(windows)]
    append_startup_diagnostic(&data_dir, "app launch started");

    // Check if we need to restart as admin
    #[cfg(windows)]
    {
        let settings = read_settings_sync(&data_dir);
        append_startup_diagnostic(
            &data_dir,
            &format!(
                "startup settings: require_admin={}, silent_start={}, start_with_windows={}, auto_connect={}, tun_enabled={}, system_proxy={}",
                settings.require_admin,
                settings.silent_start,
                settings.start_with_windows,
                settings.auto_connect,
                settings.tun_enabled,
                settings.system_proxy,
            ),
        );

        // If requireAdmin is set and we're not running as admin, restart with elevation
        if settings.require_admin && !commands::is_admin() {
            use std::env;
            append_startup_diagnostic(&data_dir, "require_admin enabled and process is not elevated, attempting ShellExecuteW(runas)");

            if let Ok(exe_path) = env::current_exe() {
                let exe_path_wide: Vec<u16> = exe_path
                    .to_string_lossy()
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();

                let runas: Vec<u16> = "runas\0".encode_utf16().collect();

                unsafe {
                    use windows::core::PCWSTR;
                    use windows::Win32::UI::Shell::ShellExecuteW;
                    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

                    let result = ShellExecuteW(
                        None,
                        PCWSTR(runas.as_ptr()),
                        PCWSTR(exe_path_wide.as_ptr()),
                        PCWSTR::null(),
                        PCWSTR::null(),
                        SW_SHOWNORMAL,
                    );

                    // If ShellExecuteW returns > 32, it succeeded - exit current instance
                    if result.0 as isize > 32 {
                        append_startup_diagnostic(
                            &data_dir,
                            "ShellExecuteW(runas) succeeded, exiting current non-elevated instance",
                        );
                        std::process::exit(0);
                    }
                    append_startup_diagnostic(
                        &data_dir,
                        &format!(
                            "ShellExecuteW(runas) failed or was cancelled, code={}",
                            result.0 as isize
                        ),
                    );
                    // If failed (user cancelled UAC), continue running without admin
                }
            } else {
                append_startup_diagnostic(
                    &data_dir,
                    "failed to resolve current_exe while attempting admin restart",
                );
            }
        }
    }

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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .setup(|app| {
            // Initialize app state
            let data_dir = get_data_dir();
            std::fs::create_dir_all(&data_dir).ok();
            append_startup_diagnostic(&data_dir, "tauri setup entered");

            // Create configs directory
            let configs_dir = data_dir.join("configs");
            std::fs::create_dir_all(&configs_dir).ok();
            append_startup_diagnostic(&data_dir, &format!("config dir ready: {:?}", configs_dir));

            log::info!("Data directory: {:?}", data_dir);

            let state = AppState::new(data_dir.clone());
            let migrated_profiles = commands::migrate_persisted_node_runtime_metadata(&state);
            if migrated_profiles > 0 {
                let message = format!(
                    "cleaned runtime metadata from {} node config(s)",
                    migrated_profiles
                );
                log::info!("{message}");
                append_startup_diagnostic(&data_dir, &message);
            }
            let settings = read_settings_sync(&data_dir);
            let rulesets = commands::rulesets::load_rulesets(&state);
            let custom_rules = commands::rules::load_custom_rules(&state);
            append_startup_diagnostic(
                &data_dir,
                &format!(
                    "state initialized: silent_start={}, auto_connect={}, tun_enabled={}, system_proxy={}",
                    settings.silent_start,
                    settings.auto_connect,
                    settings.tun_enabled,
                    settings.system_proxy,
                ),
            );

            tauri::async_runtime::block_on(async {
                *state.settings.lock().await = settings;
                *state.rulesets.lock().await = rulesets;
                *state.custom_rules.lock().await = custom_rules;
            });

            app.manage(state);

            // Show window after setup (silent start: tray only)
            if let Some(window) = app.get_webview_window("main") {
                let settings = read_settings_sync(&data_dir);
                if settings.silent_start {
                    let _ = window.hide();
                    append_startup_diagnostic(&data_dir, "main window hidden due to silent_start");
                } else {
                    let _ = window.show();
                    append_startup_diagnostic(&data_dir, "main window shown after setup");
                }
            }

            // Setup tray icon
            setup_tray(app)?;
            append_startup_diagnostic(&data_dir, "tray setup completed");

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Read settings to check minimizeToTray
                let data_dir = get_data_dir();
                let settings = read_settings_sync(&data_dir);

                if settings.minimize_to_tray {
                    // Hide window instead of closing
                    let _ = window.hide();
                    api.prevent_close();
                } else {
                    api.prevent_close();
                    spawn_safe_exit(window.app_handle().clone());
                }
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
            commands::profile_get_active,
            commands::profile_set_active,
            commands::profile_edit,
            commands::profile_set_enabled,
            commands::profile_create_custom,
            // Nodes
            commands::node_list,
            commands::node_get_active,
            commands::node_set_active,
            commands::node_delete,
            commands::node_add,
            commands::node_update,
            commands::node_export,
            commands::node_begin_latency_tests,
            commands::node_test_latency,
            commands::node_test_all,
            commands::node_cancel_latency_tests,
            commands::node_list_all,
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
            commands::singbox_test_selector_latency,
            // Window
            commands::window_minimize,
            commands::window_maximize,
            commands::window_close,
            commands::window_show,
            commands::restart_as_admin,
            commands::is_admin,
            // Kernel
            commands::kernel_get_local_version,
            commands::kernel_get_capabilities,
            commands::kernel_get_remote_releases,
            commands::kernel_download,
            commands::kernel_rollback,
            commands::kernel_can_rollback,
            commands::kernel_clear_cache,
            commands::kernel_open_releases_page,
            commands::kernel_open_directory,
            commands::kernel_get_installed_versions,
            // Plugins
            commands::plugin_get_xray_local_version,
            commands::plugin_get_xray_remote_releases,
            commands::plugin_download_xray,
            commands::plugin_open_directory,
            commands::plugin_open_xray_releases_page,
            // Updater
            commands::updater_get_current_version,
            commands::updater_check,
            commands::updater_download_and_install,
            // Custom rules
            commands::custom_rules_get,
            commands::custom_rules_save,
            commands::domain_rules_get,
            commands::domain_rules_save,
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
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

    // Main items
    let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;

    // VPN control submenu
    let vpn_start = MenuItem::with_id(app, "vpn_start", "启动 VPN", true, None::<&str>)?;
    let vpn_stop = MenuItem::with_id(app, "vpn_stop", "停止 VPN", true, None::<&str>)?;
    let vpn_restart = MenuItem::with_id(app, "vpn_restart", "重启 VPN", true, None::<&str>)?;
    let vpn_submenu = Submenu::with_items(
        app,
        "VPN 控制",
        true,
        &[&vpn_start, &vpn_stop, &vpn_restart],
    )?;

    // System proxy submenu
    let proxy_enable = MenuItem::with_id(app, "proxy_enable", "启用系统代理", true, None::<&str>)?;
    let proxy_disable =
        MenuItem::with_id(app, "proxy_disable", "关闭系统代理", true, None::<&str>)?;
    let proxy_submenu =
        Submenu::with_items(app, "系统代理", true, &[&proxy_enable, &proxy_disable])?;

    // TUN mode submenu
    let tun_enable = MenuItem::with_id(app, "tun_enable", "启用 TUN 模式", true, None::<&str>)?;
    let tun_disable = MenuItem::with_id(app, "tun_disable", "关闭 TUN 模式", true, None::<&str>)?;
    let tun_submenu = Submenu::with_items(app, "TUN 模式", true, &[&tun_enable, &tun_disable])?;

    // Separators
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;

    // Quit item
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &show_item,
            &sep1,
            &vpn_submenu,
            &proxy_submenu,
            &tun_submenu,
            &sep2,
            &quit_item,
        ],
    )?;

    let tray_icon = app
        .default_window_icon()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".to_string()))?
        .clone();

    let _tray = TrayIconBuilder::new()
        .icon(tray_icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("KunBox")
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(window) = tray.app_handle().get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "vpn_start" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit("tray-vpn-start", ());
                }
            }
            "vpn_stop" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit("tray-vpn-stop", ());
                }
            }
            "vpn_restart" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit("tray-vpn-restart", ());
                }
            }
            "proxy_enable" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit("tray-proxy-enable", ());
                }
            }
            "proxy_disable" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit("tray-proxy-disable", ());
                }
            }
            "tun_enable" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit("tray-tun-enable", ());
                }
            }
            "tun_disable" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.emit("tray-tun-disable", ());
                }
            }
            "quit" => {
                spawn_safe_exit(app.clone());
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}
