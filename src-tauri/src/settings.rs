use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "settings.json";

/// The actions a global hotkey can trigger.
pub const ACTIONS: [&str; 3] = ["area", "screen", "editor"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotkeys {
    /// Drag to select part of a screen.
    pub area: String,
    /// The whole screen under the pointer.
    pub screen: String,
    /// Bring the Lockshot window forward.
    pub editor: String,
}

impl Default for Hotkeys {
    /*
     * Alt+Shift with the same digits macOS uses for its own screenshot keys,
     * so the muscle memory carries over without fighting the system
     * shortcuts, which still own Cmd+Shift+3/4 (macOS) and PrtSc (Windows)
     * until the user turns them off.
     */
    fn default() -> Self {
        Self {
            area: "Alt+Shift+4".into(),
            screen: "Alt+Shift+3".into(),
            editor: "Alt+Shift+L".into(),
        }
    }
}

impl Hotkeys {
    pub fn get(&self, action: &str) -> Option<&str> {
        match action {
            "area" => Some(&self.area),
            "screen" => Some(&self.screen),
            "editor" => Some(&self.editor),
            _ => None,
        }
    }

    pub fn set(&mut self, action: &str, value: String) -> bool {
        match action {
            "area" => self.area = value,
            "screen" => self.screen = value,
            "editor" => self.editor = value,
            _ => return false,
        }
        true
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub hotkeys: Hotkeys,
    /// Where Save writes to. Empty means the Pictures folder (Desktop fallback).
    pub save_dir: String,
    /// png, jpg, heic, tiff, gif, bmp, or pdf.
    pub format: String,
    /// File name with {date}, {time} and friends filled in at capture time.
    pub filename_template: String,
    pub jpeg_quality: u8,
    /// Put every capture on the clipboard as soon as it is taken.
    pub copy_on_capture: bool,
    pub theme: String,
    /// The welcome guide has been dismissed.
    pub onboarded: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkeys: Hotkeys::default(),
            save_dir: String::new(),
            format: "png".into(),
            filename_template: "Screenshot-{date}-{time}".into(),
            jpeg_quality: 90,
            copy_on_capture: false,
            theme: "system".into(),
            onboarded: false,
        }
    }
}

fn app_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|err| format!("Could not resolve the app data directory: {err}"))?;
    fs::create_dir_all(&dir)
        .map_err(|err| format!("Could not create the app data directory: {err}"))?;
    Ok(dir)
}

pub fn path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_dir(app)?.join(SETTINGS_FILE))
}

pub fn load(app: &AppHandle) -> Settings {
    let Ok(path) = path(app) else {
        return Settings::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = path(app)?;
    let raw = serde_json::to_string_pretty(settings)
        .map_err(|err| format!("Could not serialize {SETTINGS_FILE}: {err}"))?;
    fs::write(&path, raw).map_err(|err| format!("Could not write {SETTINGS_FILE}: {err}"))
}

/// The folder Save uses when the user has not picked one.
pub fn default_save_dir(app: &AppHandle) -> PathBuf {
    let paths = app.path();
    paths
        .picture_dir()
        .or_else(|_| paths.desktop_dir())
        .or_else(|_| paths.home_dir())
        .unwrap_or_else(|_| std::env::temp_dir())
}

pub fn resolved_save_dir(app: &AppHandle, settings: &Settings) -> PathBuf {
    if settings.save_dir.trim().is_empty() {
        default_save_dir(app)
    } else {
        PathBuf::from(settings.save_dir.trim())
    }
}
