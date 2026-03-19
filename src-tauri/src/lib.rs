use tauri::Manager;
use tauri::Emitter;
use std::path::PathBuf;

mod types;
mod state;
mod commands;

use state::AppState;
use types::AppSettings;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Check if we need to restart as admin
    #[cfg(windows)]
    {
        let data_dir = get_data_dir();
        let settings = read_settings_sync(&data_dir);
        
        // If requireAdmin is set and we're not running as admin, restart with elevation
        if settings.require_admin && !commands::is_admin() {
            use std::env;
            
            if let Ok(exe_path) = env::current_exe() {
                let exe_path_wide: Vec<u16> = exe_path.to_string_lossy()
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
                        std::process::exit(0);
                    }
                    // If failed (user cancelled UAC), continue running without admin
                }
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
            
            // Create configs directory
            let configs_dir = data_dir.join("configs");
            std::fs::create_dir_all(&configs_dir).ok();
            
            log::info!("Data directory: {:?}", data_dir);

            let state = AppState::new(data_dir.clone());
            let settings = read_settings_sync(&data_dir);
            let rulesets = commands::rulesets::load_rulesets(&state);
            let custom_rules = commands::rules::load_custom_rules(&state);

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
                } else {
                    let _ = window.show();
                }
            }

            // Setup tray icon
            setup_tray(app)?;

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
                    // Allow window to close (app will exit)
                    // First emit quit event to cleanup
                    let _ = window.emit("tray-quit", ());
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
            // Nodes
            commands::node_list,
            commands::node_set_active,
            commands::node_delete,
            commands::node_add,
            commands::node_export,
            commands::node_test_latency,
            commands::node_test_all,
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
    use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};

    // Main items
    let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    
    // VPN control submenu
    let vpn_start = MenuItem::with_id(app, "vpn_start", "启动 VPN", true, None::<&str>)?;
    let vpn_stop = MenuItem::with_id(app, "vpn_stop", "停止 VPN", true, None::<&str>)?;
    let vpn_restart = MenuItem::with_id(app, "vpn_restart", "重启 VPN", true, None::<&str>)?;
    let vpn_submenu = Submenu::with_items(app, "VPN 控制", true, &[&vpn_start, &vpn_stop, &vpn_restart])?;
    
    // System proxy submenu
    let proxy_enable = MenuItem::with_id(app, "proxy_enable", "启用系统代理", true, None::<&str>)?;
    let proxy_disable = MenuItem::with_id(app, "proxy_disable", "关闭系统代理", true, None::<&str>)?;
    let proxy_submenu = Submenu::with_items(app, "系统代理", true, &[&proxy_enable, &proxy_disable])?;
    
    // TUN mode submenu
    let tun_enable = MenuItem::with_id(app, "tun_enable", "启用 TUN 模式", true, None::<&str>)?;
    let tun_disable = MenuItem::with_id(app, "tun_disable", "关闭 TUN 模式", true, None::<&str>)?;
    let tun_submenu = Submenu::with_items(app, "TUN 模式", true, &[&tun_enable, &tun_disable])?;
    
    // Separators
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    
    // Quit item
    let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    
    let menu = Menu::with_items(app, &[
        &show_item,
        &sep1,
        &vpn_submenu,
        &proxy_submenu,
        &tun_submenu,
        &sep2,
        &quit_item,
    ])?;

    let tray_icon = app.default_window_icon()
        .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".to_string()))?
        .clone();

    let _tray = TrayIconBuilder::new()
        .icon(tray_icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("KunBox")
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
                    // Safe exit: stop singbox and disable system proxy
                    let app_handle = app.clone();
                    std::thread::spawn(move || {
                        // Disable system proxy synchronously
                        #[cfg(windows)]
                        {
                            use std::process::Command;
                            const CREATE_NO_WINDOW: u32 = 0x08000000;
                            use std::os::windows::process::CommandExt;
                            let _ = Command::new("reg")
                                .args(["add", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings", "/v", "ProxyEnable", "/t", "REG_DWORD", "/d", "0", "/f"])
                                .creation_flags(CREATE_NO_WINDOW)
                                .output();
                        }
                        
                        // Kill sing-box process
                        #[cfg(windows)]
                        {
                            use std::process::Command;
                            const CREATE_NO_WINDOW: u32 = 0x08000000;
                            use std::os::windows::process::CommandExt;
                            let _ = Command::new("taskkill")
                                .args(["/F", "/IM", "sing-box.exe"])
                                .creation_flags(CREATE_NO_WINDOW)
                                .output();
                        }
                        
                        log::info!("Safe exit: sing-box stopped, system proxy disabled");
                        std::thread::sleep(std::time::Duration::from_millis(200));
                        app_handle.exit(0);
                    });
                }
                _ => {}
            }
        })
        .build(app)?;

    Ok(())
}
