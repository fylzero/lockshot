use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const SETTINGS_FILE: &str = "settings.json";

/// The actions a global hotkey can trigger.
pub const ACTIONS: [&str; 5] = ["area", "window", "scroll", "screen", "editor"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotkeys {
    /// Drag to select part of a screen.
    pub area: String,
    /// Click a window to capture just that window.
    pub window: String,
    /// Scrolling Capture: click a window and it is scrolled and captured a
    /// page at a time.
    pub scroll: String,
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
            window: "Alt+Shift+5".into(),
            scroll: "Alt+Shift+6".into(),
            screen: "Alt+Shift+3".into(),
            editor: "Alt+Shift+L".into(),
        }
    }
}

impl Hotkeys {
    pub fn get(&self, action: &str) -> Option<&str> {
        match action {
            "area" => Some(&self.area),
            "window" => Some(&self.window),
            "scroll" => Some(&self.scroll),
            "screen" => Some(&self.screen),
            "editor" => Some(&self.editor),
            _ => None,
        }
    }

    pub fn set(&mut self, action: &str, value: String) -> bool {
        match action {
            "area" => self.area = value,
            "window" => self.window = value,
            "scroll" => self.scroll = value,
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
    /// Save each capture straight to a file instead of opening the editor.
    pub quick_save: bool,
    /// Where those files go. Empty means the save folder.
    pub quick_save_dir: String,
    /// Scrolling Capture: "down" towards the end, or "up" towards the top
    /// (older messages in most chat apps).
    pub scroll_direction: String,
    /// How long to wait after each scroll for the app to draw, in ms.
    pub scroll_delay_ms: u32,
    /// Stop after this many pages, so an endless feed cannot fill the disk.
    pub scroll_max_pages: u32,
    /// What a run saves: "both" the pages and one long image of them,
    /// just the long "image", or just the "pages".
    pub scroll_output: String,
    /// Also read the text off the pages into a text file (macOS).
    pub scroll_text: bool,
    /// Start Lockshot in the tray when the user logs in.
    pub launch_at_login: bool,
    /// macOS: Cmd+Q closes the window and leaves Lockshot in the menu bar,
    /// so the hotkeys keep working. Quit from the menu bar icon still quits.
    pub keep_running_in_background: bool,
    /// Look for a new release at launch and every few hours, and say when one
    /// is ready to install.
    pub auto_check_updates: bool,
    /// The screen the editor opens on, by the name Settings lists it under.
    /// Empty means the screen the capture was taken on (under the pointer).
    /// A screen that is not connected falls back to that too.
    pub editor_screen: String,
    /// Size the window to each capture that opens, up to the screen's size.
    pub resize_window_to_capture: bool,
    /// Open every capture at 1:1, even one too big for the window. Off, a
    /// capture opens at 1:1 if it fits and is shrunk to fit if it does not.
    pub always_actual_size: bool,
    pub theme: String,
    /// How solid the background behind the image is, 0.05 to 1.
    /// Lower shows the desktop through it.
    pub opacity: f32,
    /// Blur radius behind that see-through background, 0 (off) to 40.
    pub blur: u32,
    /// The background behind the image as "#rrggbb", white by default.
    /// Empty means the theme's own: near-black, or white in the light theme.
    pub background_color: String,
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
            quick_save: false,
            quick_save_dir: String::new(),
            scroll_direction: "down".into(),
            scroll_delay_ms: 500,
            scroll_max_pages: 200,
            scroll_output: "both".into(),
            scroll_text: false,
            launch_at_login: true,
            keep_running_in_background: true,
            auto_check_updates: true,
            editor_screen: String::new(),
            resize_window_to_capture: true,
            always_actual_size: false,
            theme: "system".into(),
            opacity: 0.05,
            blur: 30,
            background_color: "#ffffff".into(),
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
    let mut settings: Settings = fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    if migrate_appearance(&mut settings) {
        let _ = save(app, &settings);
    }
    settings
}

/// Settings saved by 1.5.0 hold its appearance defaults (30% opacity, 4px
/// blur, the theme's background). Move anyone still on all three to the
/// current defaults; a changed value means it was chosen, so it stays.
fn migrate_appearance(settings: &mut Settings) -> bool {
    if settings.opacity != 0.3 || settings.blur != 4 || !settings.background_color.is_empty() {
        return false;
    }
    let defaults = Settings::default();
    settings.opacity = defaults.opacity;
    settings.blur = defaults.blur;
    settings.background_color = defaults.background_color;
    true
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
pub const SCROLL_DIRECTIONS: [&str; 2] = ["down", "up"];
pub const SCROLL_OUTPUTS: [&str; 3] = ["both", "image", "pages"];
pub const SCROLL_DELAY_MS: std::ops::RangeInclusive<u32> = 100..=5000;
pub const SCROLL_MAX_PAGES: std::ops::RangeInclusive<u32> = 2..=1000;
pub const OPACITY: std::ops::RangeInclusive<f32> = 0.05..=1.0;
pub const BLUR: std::ops::RangeInclusive<u32> = 0..=40;
/// Empty, or a color as "#rrggbb".
pub fn is_background_color(value: &str) -> bool {
    value.is_empty()
        || (value.len() == 7
            && value.starts_with('#')
            && value[1..].chars().all(|c| c.is_ascii_hexdigit()))
}

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
    if !SCROLL_DIRECTIONS.contains(&settings.scroll_direction.as_str()) {
        return Err("\"scrollDirection\" must be \"down\" or \"up\".".into());
    }
    if !SCROLL_OUTPUTS.contains(&settings.scroll_output.as_str()) {
        return Err("\"scrollOutput\" must be \"both\", \"image\" or \"pages\".".into());
    }
    if !SCROLL_DELAY_MS.contains(&settings.scroll_delay_ms) {
        return Err("\"scrollDelayMs\" must be between 100 and 5000.".into());
    }
    if !SCROLL_MAX_PAGES.contains(&settings.scroll_max_pages) {
        return Err("\"scrollMaxPages\" must be between 2 and 1000.".into());
    }
    if !OPACITY.contains(&settings.opacity) {
        return Err("\"opacity\" must be between 0.05 and 1.".into());
    }
    if !BLUR.contains(&settings.blur) {
        return Err("\"blur\" must be between 0 and 40.".into());
    }
    if !is_background_color(&settings.background_color) {
        return Err(
            "\"backgroundColor\" must be a color like \"#1e1e1e\", or empty for the theme's own."
                .into(),
        );
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

/// The folder Save Without Editing writes to.
pub fn resolved_quick_save_dir(app: &AppHandle, settings: &Settings) -> PathBuf {
    if settings.quick_save_dir.trim().is_empty() {
        resolved_save_dir(app, settings)
    } else {
        PathBuf::from(settings.quick_save_dir.trim())
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
        assert_eq!(parsed.hotkeys.window, Hotkeys::default().window);
        assert_eq!(parsed.jpeg_quality, 90);
        assert!(parsed.launch_at_login);
        assert!(parsed.keep_running_in_background);
    }

    #[test]
    fn migrates_the_old_appearance_defaults() {
        let mut old = from_json(r#"{ "opacity": 0.3, "blur": 4, "backgroundColor": "" }"#).unwrap();
        assert!(migrate_appearance(&mut old));
        assert_eq!(old.opacity, 0.05);
        assert_eq!(old.blur, 30);
        assert_eq!(old.background_color, "#ffffff");
        assert!(!migrate_appearance(&mut old));

        let mut chosen =
            from_json(r#"{ "opacity": 0.3, "blur": 10, "backgroundColor": "" }"#).unwrap();
        assert!(!migrate_appearance(&mut chosen));
        assert_eq!(chosen.blur, 10);
        assert_eq!(chosen.opacity, 0.3);
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
            from_json(r#"{ "hotkeys": { "zoom": "Alt+W" } }"#).unwrap_err(),
            "\"hotkeys.zoom\" is not a Lockshot setting."
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
        assert!(from_json(r#"{ "keepRunningInBackground": "no" }"#).is_err());
        assert!(from_json(r#"{ "quickSave": "on" }"#).is_err());
        assert!(from_json(r#"{ "resizeWindowToCapture": "yes" }"#).is_err());
        assert!(from_json(r#"{ "alwaysActualSize": 1 }"#).is_err());
        assert!(from_json(r#"{ "scrollDirection": "left" }"#).is_err());
        assert!(from_json(r#"{ "scrollDelayMs": 10 }"#).is_err());
        assert!(from_json(r#"{ "scrollMaxPages": 0 }"#).is_err());
        assert!(from_json(r#"{ "opacity": 0 }"#).is_err());
        assert!(from_json(r#"{ "opacity": 0.05 }"#).is_ok());
        assert!(from_json(r#"{ "opacity": 1.5 }"#).is_err());
        assert!(from_json(r#"{ "blur": 100 }"#).is_err());
        assert!(from_json(r##"{ "backgroundColor": "#12" }"##).is_err());
        assert!(from_json(r#"{ "backgroundColor": "red" }"#).is_err());
        assert!(from_json(r##"{ "backgroundColor": "#1E1e1e" }"##).is_ok());
        assert!(from_json("[]").is_err());
    }
}
