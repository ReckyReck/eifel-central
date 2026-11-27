// Eifel Mastering Central - app entry point.
// The window (ui/, built from src/) loads catalog.json itself. Everything that touches the computer is a Rust command
// here; the work is done in installer.rs:
//   get_installed  - which catalog plug-ins are on disk, which version, installed by Central or not
//   install        - download, SHA-256 check, unpack, copy (one password / UAC prompt for the system folder),
//                    installed.json
//   cancel_install - stops a running install before the copy step
// Progress goes to the window as "install-progress" events.

// Release builds on Windows: no extra console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod installer;

use installer::{Env, InstallError, InstallRequest, InstallResult, InstalledInfo, PluginQuery};
use std::path::PathBuf;
use tauri::{Emitter, Manager};

/// Mac: /Library/Audio/Plug-Ins and ~/Library/Audio/Plug-Ins.
#[cfg(not(windows))]
fn plugin_roots() -> Result<(PathBuf, PathBuf), String> {
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("no home folder")?;
    Ok((PathBuf::from("/Library/Audio/Plug-Ins"), home.join("Library/Audio/Plug-Ins")))
}

/// Windows (VST3 standard): C:\Program Files\Common Files\VST3 and %LOCALAPPDATA%\Programs\Common\VST3.
/// Central installs into the first one (all DAWs scan it); the second is only scanned for copies found there.
#[cfg(windows)]
fn plugin_roots() -> Result<(PathBuf, PathBuf), String> {
    let common = ["CommonProgramW6432", "CommonProgramFiles"]
        .iter()
        .find_map(std::env::var_os)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files\Common Files"));
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).ok_or("no LOCALAPPDATA folder")?;
    Ok((common, local.join("Programs").join("Common")))
}

fn env_for(app: &tauri::AppHandle) -> Result<Env, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| format!("no app data folder: {e}"))?;
    let (system_root, user_root) = plugin_roots()?;
    Ok(Env {
        system_root,
        user_root,
        data_dir,
        temp_dir: std::env::temp_dir(),
        use_admin_prompt: true,
        native_copy: cfg!(windows),
    })
}

#[tauri::command]
async fn get_installed(app: tauri::AppHandle, plugins: Vec<PluginQuery>) -> Result<Vec<InstalledInfo>, String> {
    let env = env_for(&app)?;
    tauri::async_runtime::spawn_blocking(move || installer::get_installed(&env, &plugins))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn install(app: tauri::AppHandle, request: InstallRequest) -> Result<InstallResult, InstallError> {
    let env = env_for(&app).map_err(InstallError::Failed)?;
    let emitter = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        installer::install(&env, &request, &|p| {
            let _ = emitter.emit("install-progress", p);
        })
    })
    .await
    .map_err(|e| InstallError::Failed(e.to_string()))?
}

#[tauri::command]
fn cancel_install() {
    installer::cancel();
}

fn main() {
    // Windows, system folder: Central starts itself once more with "Run as administrator" just to copy the
    // plug-ins ("--copy-plugins <plan.json>", see installer::run_copy_helper). That copy opens no window.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.len() == 3 && args[1] == "--copy-plugins" {
        let code = match plugin_roots() {
            Ok((system_root, _)) => installer::run_copy_helper(&PathBuf::from(&args[2]), &[system_root.join("VST3")]),
            Err(_) => 1,
        };
        std::process::exit(code);
    }
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_installed, install, cancel_install])
        .run(tauri::generate_context!())
        .expect("error while running Eifel Mastering Central");
}
