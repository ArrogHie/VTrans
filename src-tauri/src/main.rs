// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    vtrans_app::builder()
        .run(tauri::generate_context!())
        .expect("error while running VTrans");
}
