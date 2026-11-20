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
    /// Start Lockshot in the tray when the user logs in.
    pub launch_at_login: bool,
    pub theme: String,
    /// Setup on Home is finished; until then the window opens at launch.
    pub onboarded: bool,
    /// Which setup step Home shows: "permission", "hotkeys" or "tray".
    /// Empty in settings saved before the steps existed.
    pub setup_step: String,
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
            launch_at_login: true,
            theme: "system".into(),
            onboarded: false,
            setup_step: String::new(),
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
    fs::write(&path, to_json(settings)?)
        .map_err(|err| format!("Could not write {SETTINGS_FILE}: {err}"))
}

/// Settings as the pretty JSON the settings file holds.
pub fn to_json(settings: &Settings) -> Result<String, String> {
    serde_json::to_string_pretty(settings)
        .map(|raw| raw + "\n")
        .map_err(|err| format!("Could not serialize {SETTINGS_FILE}: {err}"))
}

pub const THEMES: [&str; 3] = ["system", "light", "dark"];
pub const SETUP_STEPS: [&str; 4] = ["", "permission", "hotkeys", "tray"];

/**
 * Read settings JSON typed or imported by the user. Unlike `load`, which
 * quietly falls back to defaults, this says exactly what is wrong: bad JSON
 * with its line and column, a misspelled key, or a value out of range.
 * Missing keys keep their defaults. Hotkey combos are checked by the caller,
 * which owns the OS registrations.
 */
pub fn from_json(raw: &str) -> Result<Settings, String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|err| {
        format!(
            "Line {}, column {}: {}",
            err.line(),
            err.column(),
            json_error_text(&err)
        )
    })?;
    if !value.is_object() {
        return Err("Settings must be a JSON object, like { \"format\": \"png\" }.".into());
    }
    let settings: Settings = serde_json::from_value(value.clone())
        .map_err(|err| format!("{}.", capitalize(&err.to_string())))?;
    let known = serde_json::to_value(&settings).map_err(|err| err.to_string())?;
    if let Some(key) = unknown_key(&value, &known, "") {
        return Err(format!("\"{key}\" is not a Lockshot setting."));
    }
    if !crate::export::FORMATS.contains(&settings.format.as_str()) {
        return Err(format!(
            "\"format\" must be one of {}.",
            crate::export::FORMATS.join(", ")
        ));
    }
    if !(1..=100).contains(&settings.jpeg_quality) {
        return Err("\"jpegQuality\" must be between 1 and 100.".into());
    }
    if !THEMES.contains(&settings.theme.as_str()) {
        return Err(format!("\"theme\" must be one of {}.", THEMES.join(", ")));
    }
    if !SETUP_STEPS.contains(&settings.setup_step.as_str()) {
        return Err("\"setupStep\" must be \"permission\", \"hotkeys\", \"tray\" or empty.".into());
    }
    Ok(settings)
}

/// The first key in `given` that `known` does not have, as a dotted path.
fn unknown_key(
    given: &serde_json::Value,
    known: &serde_json::Value,
    prefix: &str,
) -> Option<String> {
    let (Some(given), Some(known)) = (given.as_object(), known.as_object()) else {
        return None;
    };
    for (key, value) in given {
        let path = format!("{prefix}{key}");
        match known.get(key) {
            None => return Some(path),
            Some(inner) => {
                if let Some(found) = unknown_key(value, inner, &format!("{path}.")) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// serde_json's message without the " at line X column Y" it appends.
fn json_error_text(err: &serde_json::Error) -> String {
    let text = err.to_string();
    let text = text
        .rsplit_once(" at line ")
        .map_or(text.as_str(), |(head, _)| head);
    capitalize(text)
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_the_defaults() {
        let raw = to_json(&Settings::default()).unwrap();
        let parsed = from_json(&raw).unwrap();
        assert_eq!(to_json(&parsed).unwrap(), raw);
    }

    #[test]
    fn missing_keys_keep_their_defaults() {
        let parsed = from_json(r#"{ "format": "jpg", "hotkeys": { "area": "" } }"#).unwrap();
        assert_eq!(parsed.format, "jpg");
        assert_eq!(parsed.hotkeys.area, "");
        assert_eq!(parsed.hotkeys.screen, Hotkeys::default().screen);
        assert_eq!(parsed.jpeg_quality, 90);
        assert!(parsed.launch_at_login);
    }

    #[test]
    fn reports_where_the_json_breaks() {
        let err = from_json("{\n  \"format\": \"png\",\n}").unwrap_err();
        assert!(err.starts_with("Line 3, column 1: "), "{err}");
        assert!(!err.contains(" at line "), "{err}");
    }

    #[test]
    fn rejects_unknown_keys() {
        assert_eq!(
            from_json(r#"{ "fromat": "png" }"#).unwrap_err(),
            "\"fromat\" is not a Lockshot setting."
        );
        assert_eq!(
            from_json(r#"{ "hotkeys": { "window": "Alt+W" } }"#).unwrap_err(),
            "\"hotkeys.window\" is not a Lockshot setting."
        );
    }

    #[test]
    fn rejects_bad_values() {
        assert!(from_json(r#"{ "format": "webp" }"#).is_err());
        assert!(from_json(r#"{ "jpegQuality": 0 }"#).is_err());
        assert!(from_json(r#"{ "jpegQuality": 300 }"#).is_err());
        assert!(from_json(r#"{ "theme": "blue" }"#).is_err());
        assert!(from_json(r#"{ "setupStep": "later" }"#).is_err());
        assert!(from_json(r#"{ "copyOnCapture": "yes" }"#).is_err());
        assert!(from_json(r#"{ "launchAtLogin": 1 }"#).is_err());
        assert!(from_json("[]").is_err());
    }
}
