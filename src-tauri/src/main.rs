// Eifel Mastering Central - app entry point.
// The window (ui/, built from src/) loads catalog.json itself. Everything that touches the computer is a Rust command
// here; the work is done in installer.rs:
//   get_installed  - which catalog plug-ins are on disk, which version, installed by Central or not
//   install        - download, SHA-256 check, unpack, copy (one password prompt for the system folder), installed.json
//   cancel_install - stops a running install before the copy step
// Progress goes to the window as "install-progress" events.

// Release builds on Windows: no extra console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod installer;

use installer::{Env, InstallError, InstallRequest, InstallResult, InstalledInfo, PluginQuery};
use std::path::PathBuf;
use tauri::{Emitter, Manager};

fn env_for(app: &tauri::AppHandle) -> Result<Env, String> {
    let data_dir = app.path().app_data_dir().map_err(|e| format!("no app data folder: {e}"))?;
    let home = std::env::var_os("HOME").map(PathBuf::from).ok_or("no home folder")?;
    Ok(Env {
        system_root: PathBuf::from("/Library/Audio/Plug-Ins"),
        user_root: home.join("Library/Audio/Plug-Ins"),
        data_dir,
        temp_dir: std::env::temp_dir(),
        use_admin_prompt: true,
    })
}

#[tauri::command]
async fn get_installed(app: tauri::AppHandle, plugins: Vec<PluginQuery>) -> Result<Vec<InstalledInfo>, String> {
    if !cfg!(target_os = "macos") {
        return Ok(Vec::new()); // Windows: step 8
    }
    let env = env_for(&app)?;
    tauri::async_runtime::spawn_blocking(move || installer::get_installed(&env, &plugins))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn install(app: tauri::AppHandle, request: InstallRequest) -> Result<InstallResult, InstallError> {
    if !cfg!(target_os = "macos") {
        return Err(InstallError::Failed("Installing on Windows comes in a later build of Central.".into()));
    }
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
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![get_installed, install, cancel_install])
        .run(tauri::generate_context!())
        .expect("error while running Eifel Mastering Central");
}
