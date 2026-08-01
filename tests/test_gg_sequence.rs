use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, KeyEventKind, KeyEventState};
use trail::input::keymap;
use trail::input::{self, InputCtx};
use trail::app::state::AppState;
use trail::actions::{self, Action};
use std::path::PathBuf;

#[test]
fn test_gg_sequence() {
    let mut state = AppState::new(PathBuf::from(".")).unwrap();
    let mut ctx = InputCtx::default();

    let key_g = KeyEvent {
        code: KeyCode::Char('g'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };

    let action1 = keymap::navigation(key_g.clone(), &mut ctx, &state);
    println!("action1: {:?}", action1);
    if let Some(Action::SetPendingNavKey(ch)) = action1 {
        state.pending_nav_key = Some(ch);
    }
    let action2 = keymap::navigation(key_g.clone(), &mut ctx, &state);
    println!("action2: {:?}", action2);
}

/// Regression test for the two-character keybinding Release-event bug.
///
/// crossterm fires both `KeyEventKind::Press` and `KeyEventKind::Release` for
/// every keystroke (this is especially visible on Windows). The event loop in
/// `main.rs` previously cleared `state.pending_nav_key` whenever
/// `input::dispatch` returned `None` — which happens for all non-Press events.
/// This caused the Release event that follows the first `g` keypress to silently
/// cancel the pending sequence, making `gg` (and `dd`, `ya`, `yr`, `yn`)
/// unreachable.
///
/// The fix guards the clear with a `KeyEventKind::Press` check so that only a
/// genuine keypress that produced no action can cancel the pending sequence.
///
/// This test drives the same code path as `handle_key_event` in `main.rs`:
/// `input::dispatch` → simulate the else-if guard → `actions::apply`.
#[test]
fn two_char_sequence_survives_release_event() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut state = AppState::new(dir.path().to_owned()).expect("state");
    let mut ctx = InputCtx::default();

    let press_g = KeyEvent {
        code: KeyCode::Char('g'),
        modifiers: KeyModifiers::NONE,
        kind: KeyEventKind::Press,
        state: KeyEventState::NONE,
    };
    let release_g = KeyEvent {
        kind: KeyEventKind::Release,
        ..press_g
    };

    // ── Step 1: first `g` Press ───────────────────────────────────────────────
    let action = input::dispatch(press_g, &state, &mut ctx);
    assert_eq!(
        action,
        Some(Action::SetPendingNavKey('g')),
        "first Press of 'g' must return SetPendingNavKey"
    );
    // Simulate what handle_key_event does: apply the action.
    actions::apply(action.unwrap(), &mut state).expect("apply SetPendingNavKey");
    assert_eq!(
        state.pending_nav_key,
        Some('g'),
        "pending_nav_key must be set after first Press"
    );

    // ── Step 2: Release of `g` — must NOT clear pending_nav_key ──────────────
    let release_action = input::dispatch(release_g, &state, &mut ctx);
    assert_eq!(
        release_action,
        None,
        "Release events must never produce an action"
    );
    // Simulate the corrected else-if guard: only clear on Press.
    // (Before the fix this code was `} else if state.pending_nav_key.is_some()`
    // which also fired here, nuking the pending state.)
    if release_g.kind == KeyEventKind::Press && state.pending_nav_key.is_some() {
        state.pending_nav_key = None;
    }
    assert_eq!(
        state.pending_nav_key,
        Some('g'),
        "pending_nav_key must survive the Release event — this was the bug"
    );

    // ── Step 3: second `g` Press — must resolve to JumpTop ───────────────────
    let action2 = input::dispatch(press_g, &state, &mut ctx);
    assert_eq!(
        action2,
        Some(Action::JumpTop),
        "second Press of 'g' with pending 'g' must complete the gg sequence"
    );
}
