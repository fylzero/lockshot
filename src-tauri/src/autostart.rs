//! Launch at login. The OS entry (a Launch Agent on macOS, the Run key on
//! Windows, an XDG autostart file on Linux) is kept in step with the
//! `launchAtLogin` setting at every launch and whenever the setting changes.

use tauri::{AppHandle, Runtime};
use tauri_plugin_autostart::ManagerExt;

pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    let builder = tauri_plugin_autostart::Builder::new();
    /* A Launch Agent needs no Automation permission, unlike AppleScript. */
    #[cfg(target_os = "macos")]
    let builder = builder.macos_launcher(tauri_plugin_autostart::MacosLauncher::LaunchAgent);
    builder.build()
}

/// Add or remove the login entry so it matches `enabled`.
pub fn apply(app: &AppHandle, enabled: bool) -> Result<(), String> {
    /*
     * A dev build would register its binary under target/, which moves and
     * goes stale; leave the login entry to installed builds.
     */
    if cfg!(debug_assertions) {
        return Ok(());
    }
    let launcher = app.autolaunch();
    let current = launcher.is_enabled().unwrap_or(!enabled);
    if current == enabled {
        return Ok(());
    }
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|err| {
        let verb = if enabled { "turn on" } else { "turn off" };
        format!("Could not {verb} launch at login: {err}")
    })
}
