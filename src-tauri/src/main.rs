// Eifel Mastering Central - app entry point.
// Step 5 (catalog view) needs no Rust commands yet: the window loads catalog.json itself (fixed URL, HTTPS only,
// enforced by the content security policy in tauri.conf.json). Installing, the installed.json file, the DAW check
// and the password prompt come as Rust commands in the next steps.

// Release builds on Windows: no extra console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running Eifel Mastering Central");
}
