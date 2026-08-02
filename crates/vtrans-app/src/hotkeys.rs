//! Global shortcut registration and dispatch.

use std::str::FromStr;

use tauri::{AppHandle, Emitter, Manager, Runtime};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use tracing::{info, warn};

use crate::commands::LiveTranslationConfig;
use crate::events::{emit_pipeline_event, CAPTURE_STATUS_CHANGED};
use crate::state::AppState;
use crate::AppError;

/// Registers configured global shortcuts.
///
/// # Errors
///
/// Returns `AppError::HotkeyFailed` when a shortcut is invalid or cannot be registered.
///
/// The configured strings are parsed before any registration is attempted, so
/// an invalid setting fails atomically from the caller's perspective. A
/// handler is installed for the three default actions: single selection,
/// live translation, and live stop.
pub fn register_hotkeys<R: Runtime>(app: &AppHandle<R>) -> Result<(), AppError> {
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
    let actions = std::sync::Arc::new(actions);
    let action_count = actions.len();
    let callback_actions = std::sync::Arc::clone(&actions);
    app.global_shortcut()
        .on_shortcuts(shortcuts, move |handle, shortcut, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            let action = callback_actions
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

fn dispatch_hotkey<R: Runtime>(app: AppHandle<R>, action: HotkeyAction) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        match action {
            HotkeyAction::Select => {
                if let Some(window) = app.get_webview_window("selector") {
                    if let Err(error) = window.show() {
                        warn!(error = %error, "failed to show selector window from hotkey");
                    }
                }
                let _ = app.emit(
                    CAPTURE_STATUS_CHANGED,
                    serde_json::json!({"status":"selecting"}),
                );
            }
            HotkeyAction::StartLive => {
                if state.live_task_is_running().await {
                    return;
                }
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
                if let Err(error) = start_live_from_hotkey(app.clone(), state.inner(), request) {
                    warn!(error = %error, "live hotkey action failed");
                }
            }
            HotkeyAction::StopLive => {
                if let Some(pipeline) = state.pipeline() {
                    if let Err(error) = pipeline.stop().await {
                        warn!(error = %error, "stop hotkey action failed");
                    }
                }
            }
        }
    });
}

fn start_live_from_hotkey<R: Runtime>(
    app: AppHandle<R>,
    state: &AppState,
    config: LiveTranslationConfig,
) -> Result<(), AppError> {
    let pipeline = state.build_pipeline(
        vtrans_core::PipelineMode::LiveRegion,
        config.region,
        config.capture_interval_ms,
        config.difference_threshold,
    )?;
    let pipeline = state.set_pipeline(pipeline);
    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(32);
    tauri::async_runtime::spawn(async move {
        let run = pipeline.run(event_tx);
        tokio::pin!(run);
        loop {
            tokio::select! {
                result = &mut run => {
                    if let Err(error) = result {
                        emit_pipeline_event(&app, vtrans_pipeline::PipelineEvent::Error(error));
                    }
                    break;
                }
                event = event_rx.recv() => {
                    match event {
                        Some(event) => emit_pipeline_event(&app, event),
                        None => break,
                    }
                }
            }
        }
    });
    Ok(())
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
