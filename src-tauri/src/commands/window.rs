use tauri::{AppHandle, State, WebviewWindow};
use crate::state::AppState;
use crate::types::ProxyState;

#[cfg(windows)]
use std::process::Command as StdCommand;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

/// Check if the current process is running with administrator privileges
#[tauri::command]
pub fn is_admin() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Security::{
            GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
        };
        use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
        use windows::Win32::Foundation::HANDLE;
        
        unsafe {
            let mut token_handle: HANDLE = HANDLE::default();
            
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token_handle).is_err() {
                return false;
            }
            
            let mut elevation = TOKEN_ELEVATION::default();
            let mut size: u32 = 0;
            
            let result = GetTokenInformation(
                token_handle,
                TokenElevation,
                Some(&mut elevation as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            );
            
            let _ = windows::Win32::Foundation::CloseHandle(token_handle);
            
            if result.is_err() {
                return false;
            }
            
            elevation.TokenIsElevated != 0
        }
    }
    
    #[cfg(not(windows))]
    {
        false
    }
}

/// Set the app to always run as administrator via registry
/// Uses "~ RUNASADMIN" format for better compatibility
#[tauri::command]
pub async fn set_run_as_admin(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::env;
        
        let exe_path = env::current_exe().map_err(|e| e.to_string())?;
        let exe_path_str = exe_path.to_string_lossy().to_string();
        
        let args = if enabled {
            vec![
                "add".to_string(),
                r"HKEY_CURRENT_USER\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers".to_string(),
                "/v".to_string(),
                exe_path_str,
                "/t".to_string(),
                "REG_SZ".to_string(),
                "/d".to_string(),
                "~ RUNASADMIN".to_string(),  // Use "~ RUNASADMIN" for better compatibility
                "/f".to_string(),
            ]
        } else {
            vec![
                "delete".to_string(),
                r"HKEY_CURRENT_USER\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers".to_string(),
                "/v".to_string(),
                exe_path_str,
                "/f".to_string(),
            ]
        };
        
        let output = StdCommand::new("reg")
            .args(&args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        
        if !output.status.success() && enabled {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to set registry: {}", stderr));
        }
        
        Ok(())
    }
    
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Err("This feature is only supported on Windows".to_string())
    }
}

/// Check if the app is set to always run as administrator
#[tauri::command]
pub async fn get_run_as_admin() -> Result<bool, String> {
    #[cfg(windows)]
    {
        use std::env;
        
        let exe_path = env::current_exe().map_err(|e| e.to_string())?;
        let exe_path_str = exe_path.to_string_lossy().to_string();
        
        let output = StdCommand::new("reg")
            .args([
                "query",
                r"HKEY_CURRENT_USER\Software\Microsoft\Windows NT\CurrentVersion\AppCompatFlags\Layers",
                "/v",
                &exe_path_str,
            ])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| e.to_string())?;
        
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            Ok(stdout.contains("RUNASADMIN"))
        } else {
            Ok(false)
        }
    }
    
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

#[tauri::command]
pub async fn window_minimize(window: WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn window_maximize(window: WebviewWindow) -> Result<(), String> {
    if window.is_maximized().unwrap_or(false) {
        window.unmaximize().map_err(|e| e.to_string())
    } else {
        window.maximize().map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn window_close(window: WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn window_show(window: WebviewWindow) -> Result<(), String> {
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn restart_as_admin(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    #[cfg(windows)]
    {
        use std::env;
        use windows::core::PCWSTR;
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        
        // Stop sing-box process if running
        if let Some(cancel) = state.traffic_cancel.lock().await.take() {
            cancel.cancel();
        }
        
        if let Some(mut child) = state.singbox_process.lock().await.take() {
            let _ = child.kill().await;
        }
        
        // Disable system proxy
        let _ = StdCommand::new("reg")
            .args(["add", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings", "/v", "ProxyEnable", "/t", "REG_DWORD", "/d", "0", "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        
        *state.proxy_state.lock().await = ProxyState::Idle;
        
        // Get current executable path
        let exe_path = env::current_exe().map_err(|e| e.to_string())?;
        let exe_path_wide: Vec<u16> = exe_path.to_string_lossy()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        
        let runas: Vec<u16> = "runas\0".encode_utf16().collect();
        
        let result = unsafe {
            ShellExecuteW(
                None,
                PCWSTR(runas.as_ptr()),
                PCWSTR(exe_path_wide.as_ptr()),
                PCWSTR::null(),
                PCWSTR::null(),
                SW_SHOWNORMAL,
            )
        };

        // ShellExecuteW returns value > 32 on success
        if result.0 as isize <= 32 {
            return Err("管理员重启失败或已取消 UAC 授权".to_string());
        }

        // Exit current instance
        app.exit(0);
        Ok(())
    }
    
    #[cfg(not(windows))]
    {
        Err("Admin restart is only supported on Windows".to_string())
    }
}

#[tauri::command]
pub async fn quit_app(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    // Stop sing-box process if running
    if let Some(cancel) = state.traffic_cancel.lock().await.take() {
        cancel.cancel();
    }
    
    if let Some(mut child) = state.singbox_process.lock().await.take() {
        let _ = child.kill().await;
    }
    
    // Disable system proxy
    #[cfg(windows)]
    {
        let _ = StdCommand::new("reg")
            .args(["add", "HKCU\\Software\\Microsoft\\Windows\\CurrentVersion\\Internet Settings", "/v", "ProxyEnable", "/t", "REG_DWORD", "/d", "0", "/f"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    
    *state.proxy_state.lock().await = ProxyState::Idle;
    
    log::info!("Safe exit: sing-box stopped, system proxy disabled");
    
    app.exit(0);
    Ok(())
}
