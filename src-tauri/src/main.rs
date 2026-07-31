// Prevents additional console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // TODO(feat/10-app): delegate to vtrans_app::setup::run() once the app layer is implemented.
}
