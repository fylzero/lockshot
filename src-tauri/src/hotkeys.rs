//! Global hotkeys. Each action has at most one accelerator; changing one
//! registers the new combination before letting go of the old, so a combo
//! another app already owns is reported and the previous one keeps working.

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutEvent, ShortcutState};

use crate::settings::{Hotkeys, ACTIONS};

#[derive(Default)]
pub struct HotkeyState {
    /// Registered shortcut id → action.
    bound: Mutex<HashMap<u32, String>>,
    /// While the settings page is recording a new combo, presses must not
    /// fire captures.
    paused: Mutex<bool>,
}

pub fn parse(accelerator: &str) -> Result<Shortcut, String> {
    native(accelerator)
        .parse::<Shortcut>()
        .map_err(|err| format!("\"{accelerator}\" is not a shortcut Lockshot understands: {err}"))
}

/**
 * The accelerator as this OS's keyboard sends it. macOS has no Print Screen
 * key: a PC keyboard's PrtSc arrives as F13, while global-hotkey registers
 * "PrintScreen" as a key code nothing ever sends. So on macOS a PrtSc
 * binding (say, one copied from Windows) is registered as F13, and the two
 * count as the same shortcut.
 */
fn native(accelerator: &str) -> std::borrow::Cow<'_, str> {
    if !cfg!(target_os = "macos") {
        return accelerator.into();
    }
    match accelerator.rsplit_once('+') {
        Some((mods, key)) if is_print_screen(key) => format!("{mods}+F13").into(),
        None if is_print_screen(accelerator) => "F13".into(),
        _ => accelerator.into(),
    }
}

fn is_print_screen(key: &str) -> bool {
    ["PrintScreen", "PrtSc", "Print"]
        .iter()
        .any(|name| key.eq_ignore_ascii_case(name))
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::parse;

    #[test]
    fn print_screen_is_f13_on_macos() {
        assert_eq!(
            parse("PrintScreen").unwrap().id(),
            parse("F13").unwrap().id()
        );
        assert_eq!(
            parse("Shift+PrintScreen").unwrap().id(),
            parse("Shift+F13").unwrap().id()
        );
        assert_eq!(
            parse("Alt+Shift+4").unwrap().id(),
            parse("Shift+Alt+4").unwrap().id()
        );
    }
}

pub fn handle(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state() != ShortcutState::Pressed {
        return;
    }
    let state = app.state::<HotkeyState>();
    if *state.paused.lock().unwrap() {
        return;
    }
    let action = state.bound.lock().unwrap().get(&shortcut.id()).cloned();
    if let Some(action) = action {
        crate::run_action(app, &action);
    }
}

fn register(app: &AppHandle, action: &str, accelerator: &str) -> Result<(), String> {
    let shortcut = parse(accelerator)?;
    app.global_shortcut().register(shortcut).map_err(|err| {
        format!(
            "Could not use {accelerator}: another app or the system may already own it ({err})."
        )
    })?;
    app.state::<HotkeyState>()
        .bound
        .lock()
        .unwrap()
        .insert(shortcut.id(), action.to_string());
    Ok(())
}

/// Claim a key for a short while, outside the user's hotkeys: Esc to stop a
/// Scrolling Capture. It must not be one of theirs.
pub fn register_extra(app: &AppHandle, action: &str, accelerator: &str) -> Result<(), String> {
    let id = parse(accelerator)?.id();
    if app
        .state::<HotkeyState>()
        .bound
        .lock()
        .unwrap()
        .contains_key(&id)
    {
        return Err(format!("{accelerator} is already a Lockshot hotkey."));
    }
    register(app, action, accelerator)
}

pub fn unregister_extra(app: &AppHandle, accelerator: &str) {
    unregister(app, accelerator);
}

fn unregister(app: &AppHandle, accelerator: &str) {
    if let Ok(shortcut) = parse(accelerator) {
        let _ = app.global_shortcut().unregister(shortcut);
        app.state::<HotkeyState>()
            .bound
            .lock()
            .unwrap()
            .remove(&shortcut.id());
    }
}

/// Register every configured hotkey, returning the ones that failed.
pub fn register_all(app: &AppHandle, hotkeys: &Hotkeys) -> Vec<String> {
    let mut failures = Vec::new();
    for action in ACTIONS {
        let accelerator = hotkeys.get(action).unwrap_or_default();
        if accelerator.is_empty() {
            continue;
        }
        if let Err(err) = register(app, action, accelerator) {
            failures.push(err);
        }
    }
    failures
}

/**
 * Point `action` at `next` (empty to turn it off). Returns an error and keeps
 * the old binding when the new combo is invalid, taken by another action, or
 * cannot be registered with the OS.
 */
pub fn rebind(app: &AppHandle, hotkeys: &Hotkeys, action: &str, next: &str) -> Result<(), String> {
    let current = hotkeys
        .get(action)
        .ok_or("Unknown hotkey action.")?
        .to_string();
    if current == next {
        return Ok(());
    }
    if !next.is_empty() {
        let wanted = parse(next)?;
        for other in ACTIONS.iter().filter(|a| **a != action) {
            let theirs = hotkeys.get(other).unwrap_or_default();
            if parse(theirs).is_ok_and(|s| s.id() == wanted.id()) {
                return Err(format!(
                    "{next} is already used by another Lockshot action."
                ));
            }
        }
        if parse(&current).is_ok_and(|s| s.id() == wanted.id()) {
            return Ok(());
        }
        register(app, action, next)?;
    }
    unregister(app, &current);
    Ok(())
}

/**
 * Switch every hotkey to `next` at once, so combos can swap between actions.
 * Checks them all first; if the OS refuses one, the new registrations are
 * dropped and the old ones restored, and the error is returned.
 */
pub fn replace_all(app: &AppHandle, current: &Hotkeys, next: &Hotkeys) -> Result<(), String> {
    let mut seen = HashMap::new();
    for action in ACTIONS {
        let accelerator = next.get(action).unwrap_or_default();
        if accelerator.is_empty() {
            continue;
        }
        let shortcut = parse(accelerator)?;
        if seen.insert(shortcut.id(), action).is_some() {
            return Err(format!("{accelerator} is set for more than one action."));
        }
    }

    let changed: Vec<&str> = ACTIONS
        .into_iter()
        .filter(|a| current.get(a) != next.get(a))
        .collect();
    for action in &changed {
        unregister(app, current.get(action).unwrap_or_default());
    }
    let mut registered = Vec::new();
    for action in &changed {
        let accelerator = next.get(action).unwrap_or_default();
        if accelerator.is_empty() {
            continue;
        }
        if let Err(err) = register(app, action, accelerator) {
            for accelerator in registered {
                unregister(app, accelerator);
            }
            for action in &changed {
                let old = current.get(action).unwrap_or_default();
                if !old.is_empty() {
                    let _ = register(app, action, old);
                }
            }
            return Err(err);
        }
        registered.push(accelerator);
    }
    Ok(())
}

/// Drop and register every hotkey again. Used once macOS lets go of a
/// screenshot shortcut, so the key reaches Lockshot without a relaunch.
pub fn reregister_all(app: &AppHandle, hotkeys: &Hotkeys) -> Vec<String> {
    for action in ACTIONS {
        unregister(app, hotkeys.get(action).unwrap_or_default());
    }
    register_all(app, hotkeys)
}

pub fn set_paused(app: &AppHandle, paused: bool) {
    *app.state::<HotkeyState>().paused.lock().unwrap() = paused;
}
