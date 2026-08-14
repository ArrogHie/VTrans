//! Visibility management for application windows during region selection.
//!
//! While the transparent selector window covers the screen, the other
//! application windows (`main`, `result`, and `floater`) that were visible
//! before the selection are hidden so they neither cover nor leak into the
//! area the user is selecting. The pre-selection visibility is recorded in a
//! snapshot and restored either:
//!
//! - **immediately**, when the selection is cancelled, times out, or cannot
//!   open the selector window; or
//! - **after the follow-up action completes** (`capture_once`,
//!   `start_live_translation`, `add_translation_box`, or
//!   `update_translation_box`), on success and failure alike.
//!
//! The lifecycle decision lives in the pure [`SelectionVisibilityState`]
//! machine and the pure [`PreSelectionVisibility::restore_plan`] function, so
//! the whole hide/restore contract is unit-testable; only the thin executor
//! functions at the bottom of this module touch the window API.

use tauri::{AppHandle, Manager, Runtime};
use tracing::warn;

use crate::state::AppState;

/// Window label of the main application window.
const MAIN_WINDOW_LABEL: &str = "main";
/// Window label of the result (translation popup) window.
const RESULT_WINDOW_LABEL: &str = "result";
/// Window label of the floating ball window.
const FLOATER_WINDOW_LABEL: &str = "floater";

/// Which application windows were visible right before a region selection.
///
/// Only the three windows that could cover or leak into the selection are
/// tracked. The `selector` and `overlay` windows follow their own flows and
/// never enter this snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PreSelectionVisibility {
    /// The main window was visible.
    pub(crate) main: bool,
    /// The result window was visible.
    pub(crate) result: bool,
    /// The floating ball window was visible.
    pub(crate) floater: bool,
}

impl PreSelectionVisibility {
    /// Returns the window labels paired with their recorded visibility.
    ///
    /// Used by the executor to translate a snapshot or restore plan into
    /// concrete window API calls.
    pub(crate) const fn entries(self) -> [(&'static str, bool); 3] {
        [
            (MAIN_WINDOW_LABEL, self.main),
            (RESULT_WINDOW_LABEL, self.result),
            (FLOATER_WINDOW_LABEL, self.floater),
        ]
    }

    /// Returns `true` when no window was visible before the selection.
    pub(crate) const fn is_empty(self) -> bool {
        !self.main && !self.result && !self.floater
    }

    /// Computes the set of windows to restore from this snapshot.
    ///
    /// `main` and `result` are restored exactly when they were visible
    /// before the selection. The floating ball additionally requires
    /// `floating_ball_enabled` to be `true`: when the feature was disabled
    /// in the meantime, the floater stays hidden even if it was visible
    /// when the selection started.
    #[must_use]
    pub(crate) const fn restore_plan(self, floating_ball_enabled: bool) -> Self {
        Self {
            main: self.main,
            result: self.result,
            floater: self.floater && floating_ball_enabled,
        }
    }
}

/// Lifecycle transition reported by a command to the visibility state
/// machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisibilityTransition {
    /// A selection started while the given windows were currently visible.
    SelectionStarted(PreSelectionVisibility),
    /// The selection was cancelled, timed out, or failed to open the
    /// selector: the hidden windows must be restored immediately.
    SelectionAborted,
    /// The selection succeeded: the windows stay hidden until a follow-up
    /// action command completes.
    SelectionSucceeded,
    /// A follow-up action command completed (success or failure): restore
    /// the hidden windows now.
    FollowUpCompleted,
}

/// Window action decided by the state machine for the caller to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VisibilityAction {
    /// Hide the given currently-visible windows now.
    Hide(PreSelectionVisibility),
    /// Show the windows of the recorded pre-selection snapshot now.
    Restore(PreSelectionVisibility),
    /// Leave every window unchanged (a successful selection keeps them
    /// hidden, or there is no pending snapshot at all).
    KeepHidden,
}

/// Pure state machine for the pre-selection window-visibility snapshot.
///
/// Commands translate their control flow into [`VisibilityTransition`]s and
/// execute the returned [`VisibilityAction`]; the machine itself performs no
/// window or configuration I/O. Two guarantees are encoded here:
///
/// - **Keep-first**: a second selection never overwrites an unrestored
///   snapshot (e.g. when the user retries after a failed follow-up), so the
///   original window set is never lost.
/// - **Restore-once**: every abort or follow-up completion consumes the
///   snapshot, so one selection restores at most once.
#[derive(Debug, Default)]
pub(crate) struct SelectionVisibilityState {
    snapshot: std::sync::Mutex<Option<PreSelectionVisibility>>,
}

impl SelectionVisibilityState {
    /// Creates an empty state with no pending snapshot.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Applies a lifecycle transition and returns the action to execute.
    #[must_use]
    pub(crate) fn apply(&self, transition: VisibilityTransition) -> VisibilityAction {
        match transition {
            VisibilityTransition::SelectionStarted(visible) => {
                let mut snapshot = self.snapshot.lock().unwrap_or_else(poison_inner);
                if snapshot.is_none() {
                    *snapshot = Some(visible);
                }
                // Hide what is visible right now regardless of which
                // snapshot is kept: a window may have been re-shown
                // manually between two selections.
                VisibilityAction::Hide(visible)
            }
            VisibilityTransition::SelectionAborted | VisibilityTransition::FollowUpCompleted => {
                match self.snapshot.lock().unwrap_or_else(poison_inner).take() {
                    Some(snapshot) => VisibilityAction::Restore(snapshot),
                    None => VisibilityAction::KeepHidden,
                }
            }
            VisibilityTransition::SelectionSucceeded => VisibilityAction::KeepHidden,
        }
    }
}

/// Reads the current visibility of the three selection-affected windows.
///
/// A window whose visibility cannot be queried is treated as hidden: a
/// selection never hides a window it could not see, and a restore can only
/// re-show windows that were recorded as visible.
pub(crate) fn visible_selection_windows<R: Runtime>(app: &AppHandle<R>) -> PreSelectionVisibility {
    let mut visible = PreSelectionVisibility::default();
    for (label, slot) in [
        (MAIN_WINDOW_LABEL, &mut visible.main),
        (RESULT_WINDOW_LABEL, &mut visible.result),
        (FLOATER_WINDOW_LABEL, &mut visible.floater),
    ] {
        let is_visible = match app.get_webview_window(label) {
            Some(window) => match window.is_visible() {
                Ok(is_visible) => is_visible,
                Err(error) => {
                    tracing::warn!(
                        label,
                        error = %error,
                        "failed to query window visibility; treating as hidden"
                    );
                    false
                }
            },
            None => false,
        };
        *slot = is_visible;
    }
    visible
}

/// Executes a visibility action against the application windows.
///
/// A restore re-applies the `floating_ball.enabled` configuration constraint
/// at execution time, so a config change between the selection and the
/// restore takes effect. All window operations are best-effort: failures are
/// logged as warnings and never fail the selection flow.
pub(crate) fn execute_selection_visibility_action<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    action: VisibilityAction,
) {
    match action {
        VisibilityAction::Hide(visible) => hide_selection_windows(app, visible),
        VisibilityAction::Restore(snapshot) => {
            let plan = snapshot.restore_plan(floating_ball_enabled(state));
            if plan.is_empty() {
                tracing::debug!("no window was visible before the selection; nothing to restore");
            }
            show_selection_windows(app, plan);
        }
        VisibilityAction::KeepHidden => {}
    }
}

/// Hides the application windows at the start of a region selection and
/// records the pre-selection visibility snapshot.
///
/// When an unrestored snapshot already exists (a previous selection whose
/// follow-up never completed), the first snapshot is kept and only the
/// currently-visible windows are hidden again.
pub(crate) fn hide_app_windows_for_selection<R: Runtime>(app: &AppHandle<R>, state: &AppState) {
    let visible = visible_selection_windows(app);
    let action = state
        .selection_visibility()
        .apply(VisibilityTransition::SelectionStarted(visible));
    tracing::debug!(
        main = visible.main,
        result = visible.result,
        floater = visible.floater,
        "hiding application windows for region selection"
    );
    execute_selection_visibility_action(app, state, action);
}

/// Restores the application windows hidden by a region selection.
///
/// This is the delayed-restore path: it runs after a follow-up action
/// command completes (`capture_once`, `start_live_translation`,
/// `add_translation_box`, or `update_translation_box`), on success and
/// failure alike. When no snapshot is pending the call is a no-op, so
/// ordinary start/stop flows (e.g. the resume hotkey) are unaffected.
pub(crate) fn restore_app_windows_after_follow_up<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
) {
    let action = state
        .selection_visibility()
        .apply(VisibilityTransition::FollowUpCompleted);
    execute_selection_visibility_action(app, state, action);
}

/// Restores the application windows immediately after an aborted selection.
///
/// This is the immediate-restore path for cancel, timeout, and
/// selector-unavailable outcomes; no follow-up action will run.
pub(crate) fn restore_app_windows_immediately<R: Runtime>(app: &AppHandle<R>, state: &AppState) {
    let action = state
        .selection_visibility()
        .apply(VisibilityTransition::SelectionAborted);
    execute_selection_visibility_action(app, state, action);
}

/// Reads the current `floating_ball.enabled` configuration.
///
/// A configuration failure degrades to `false` (the floater stays hidden)
/// with a warning; a restore never fails because of configuration.
fn floating_ball_enabled(state: &AppState) -> bool {
    match state.load_config() {
        Ok(config) => config.floating_ball.enabled,
        Err(error) => {
            warn!(
                error = %error,
                "config unavailable while restoring selection windows; floating ball stays hidden"
            );
            false
        }
    }
}

/// Hides exactly the windows recorded as visible.
///
/// Failures are tolerated and logged: a failed hide must never break the
/// selection flow.
fn hide_selection_windows<R: Runtime>(app: &AppHandle<R>, visible: PreSelectionVisibility) {
    for (label, was_visible) in visible.entries() {
        if !was_visible {
            continue;
        }
        let Some(window) = app.get_webview_window(label) else {
            continue;
        };
        if let Err(error) = window.hide() {
            tracing::warn!(label, error = %error, "failed to hide window for region selection");
        }
    }
}

/// Shows exactly the windows of the restore plan.
///
/// Failures are tolerated and logged: a failed restore must never fail the
/// command that triggered it.
fn show_selection_windows<R: Runtime>(app: &AppHandle<R>, plan: PreSelectionVisibility) {
    for (label, restore) in plan.entries() {
        if !restore {
            continue;
        }
        let Some(window) = app.get_webview_window(label) else {
            continue;
        };
        if let Err(error) = window.show() {
            tracing::warn!(label, error = %error, "failed to restore window after region selection");
        }
    }
}

fn poison_inner<T>(poisoned: std::sync::PoisonError<T>) -> T {
    tracing::debug!("recovering poisoned selection visibility lock");
    poisoned.into_inner()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vis(main: bool, result: bool, floater: bool) -> PreSelectionVisibility {
        PreSelectionVisibility {
            main,
            result,
            floater,
        }
    }

    // ── restore plan (snapshot → restore set) ──

    #[test]
    fn restore_plan_passes_through_main_and_result_but_gates_floater_on_config() {
        let snapshot = vis(true, true, true);
        let plan = snapshot.restore_plan(false);
        assert!(plan.main);
        assert!(plan.result);
        assert!(!plan.floater, "disabled floating ball must not be restored");

        let plan = snapshot.restore_plan(true);
        assert!(plan.main);
        assert!(plan.result);
        assert!(plan.floater);
    }

    #[test]
    fn restore_plan_never_restores_windows_hidden_before_the_selection() {
        // The result window was hidden before the selection, so it must
        // stay hidden after the restore even when everything else is shown.
        let snapshot = vis(true, false, true);
        let plan = snapshot.restore_plan(true);
        assert!(plan.main);
        assert!(!plan.result);
        assert!(plan.floater);
    }

    #[test]
    fn restore_plan_of_an_empty_snapshot_is_empty() {
        let plan = vis(false, false, false).restore_plan(true);
        assert!(plan.is_empty());
    }

    // ── lifecycle state machine ──

    #[test]
    fn second_selection_keeps_the_first_snapshot() {
        let state = SelectionVisibilityState::new();
        let first = vis(true, true, false);
        assert!(matches!(
            state.apply(VisibilityTransition::SelectionStarted(first)),
            VisibilityAction::Hide(_)
        ));
        // The retry records nothing new...
        let second = vis(false, false, true);
        assert!(matches!(
            state.apply(VisibilityTransition::SelectionStarted(second)),
            VisibilityAction::Hide(_)
        ));
        // ...and the follow-up restores the first snapshot, not the second.
        assert_eq!(
            state.apply(VisibilityTransition::FollowUpCompleted),
            VisibilityAction::Restore(first)
        );
    }

    #[test]
    fn cancelled_selection_restores_immediately_and_clears_the_snapshot() {
        let state = SelectionVisibilityState::new();
        let visible = vis(true, false, true);
        let _ = state.apply(VisibilityTransition::SelectionStarted(visible));

        let action = state.apply(VisibilityTransition::SelectionAborted);
        assert_eq!(action, VisibilityAction::Restore(visible));

        // The snapshot is consumed: a later follow-up completion is a no-op.
        assert_eq!(
            state.apply(VisibilityTransition::FollowUpCompleted),
            VisibilityAction::KeepHidden
        );
    }

    #[test]
    fn successful_selection_defers_restore_until_follow_up_completes() {
        let state = SelectionVisibilityState::new();
        let visible = vis(true, true, true);
        let _ = state.apply(VisibilityTransition::SelectionStarted(visible));

        // Success keeps the snapshot pending: the windows stay hidden.
        assert_eq!(
            state.apply(VisibilityTransition::SelectionSucceeded),
            VisibilityAction::KeepHidden
        );

        // A follow-up command completion restores. The transition carries no
        // outcome, so callers apply it after both success and failure.
        assert_eq!(
            state.apply(VisibilityTransition::FollowUpCompleted),
            VisibilityAction::Restore(visible)
        );
    }

    #[test]
    fn abort_or_follow_up_without_a_pending_snapshot_is_a_no_op() {
        let state = SelectionVisibilityState::new();
        assert_eq!(
            state.apply(VisibilityTransition::SelectionAborted),
            VisibilityAction::KeepHidden
        );
        assert_eq!(
            state.apply(VisibilityTransition::FollowUpCompleted),
            VisibilityAction::KeepHidden
        );
        // A normal start/stop without any selection must never touch windows.
    }

    #[test]
    fn retried_selection_still_hides_what_is_visible_now() {
        let state = SelectionVisibilityState::new();
        let _ = state.apply(VisibilityTransition::SelectionStarted(vis(
            true, true, false,
        )));

        // Between the two selections the floater was re-shown manually; the
        // retry must hide it again even though the first snapshot is kept.
        let action = state.apply(VisibilityTransition::SelectionStarted(vis(
            false, false, true,
        )));
        assert_eq!(action, VisibilityAction::Hide(vis(false, false, true)));

        // The first snapshot is preserved for restoration.
        assert_eq!(
            state.apply(VisibilityTransition::FollowUpCompleted),
            VisibilityAction::Restore(vis(true, true, false))
        );
    }
}
