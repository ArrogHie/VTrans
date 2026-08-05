//! Global shortcut registration and dispatch.

use std::str::FromStr;

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tracing::{info, warn};

use crate::commands::{select_region, start_live_task, stop_live_task, LiveTranslationConfig};
use crate::events::REGION_SELECTED;
use crate::overlay::hide_region_overlay;
use crate::state::AppState;
use crate::AppError;

/// Registers configured global shortcuts.
///
/// # Errors
///
/// Returns `AppError::HotkeyFailed` when a shortcut is invalid or cannot be registered.
#[tracing::instrument(skip(app))]
pub fn register_hotkeys(app: &AppHandle) -> Result<(), AppError> {
    let state = app.state::<AppState>();
    let config = state.load_config()?;
    let entries = [
        (config.hotkeys.select_and_translate, HotkeyAction::Select),
        (config.hotkeys.live_translate, HotkeyAction::StartLive),
        (config.hotkeys.stop_live, HotkeyAction::StopLive),
    ];
    let mut shortcuts = Vec::with_capacity(entries.len());
    let mut actions = Vec::with_capacity(entries.len());
    for (value, action) in entries {
        let shortcut = Shortcut::from_str(&value)
            .map_err(|error| AppError::HotkeyFailed(error.to_string()))?;
        shortcuts.push(shortcut);
        actions.push((shortcut.id(), action));
    }
    let action_count = actions.len();
    let actions = std::sync::Arc::new(actions);
    app.global_shortcut()
        .on_shortcuts(shortcuts, move |handle, shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let action = actions
                .iter()
                .find_map(|(id, action)| (*id == shortcut.id()).then_some(*action));
            let Some(action) = action else {
                warn!(
                    shortcut_id = shortcut.id(),
                    "received unknown global shortcut"
                );
                return;
            };
            dispatch_hotkey(handle.clone(), action);
        })
        .map_err(|error| AppError::HotkeyFailed(error.to_string()))?;
    info!(count = action_count, "global shortcuts registered");
    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum HotkeyAction {
    Select,
    StartLive,
    StopLive,
}

fn dispatch_hotkey(app: AppHandle, action: HotkeyAction) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        match action {
            HotkeyAction::Select => match select_region(app.clone(), state.inner()).await {
                Ok(region) => {
                    let _ = app.emit(REGION_SELECTED, region);
                }
                Err(error) => warn!(error = %error, "region selection hotkey failed"),
            },
            HotkeyAction::StartLive => {
                let Some(region) = state.selected_region() else {
                    warn!("live hotkey ignored because no region is selected");
                    return;
                };
                let Ok(config) = state.load_config() else {
                    warn!("live hotkey ignored because config could not be loaded");
                    return;
                };
                let request = LiveTranslationConfig {
                    region,
                    capture_interval_ms: config.capture.interval_ms,
                    difference_threshold: config.capture.difference_threshold,
                };
                if let Err(error) = start_live_task(app.clone(), state.inner(), request).await {
                    warn!(error = %error, "live hotkey action failed");
                }
            }
            HotkeyAction::StopLive => {
                if let Err(error) = stop_live_task(state.inner()).await {
                    warn!(error = %error, "stop hotkey action failed");
                }
                // A hotkey stop is always a real stop (never a pause), so the
                // region marker is cleared even when every webview is hidden
                // or throttled by the OS.
                hide_region_overlay(&app);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_are_copyable_for_registration_table() {
        let action = HotkeyAction::StartLive;
        assert!(matches!(action, HotkeyAction::StartLive));
    }
}
