//! The OS's own screenshot shortcuts. On macOS, registering ⌘⇧3 or ⌘⇧4
//! succeeds even while the system still owns them, and the system wins: the
//! key opens macOS's screenshot tool and Lockshot never hears it. So the app
//! reads which of those shortcuts are still on, and warns when a Lockshot
//! hotkey clashes with one, until the user turns it off in System Settings.
//!
//! Windows 11 does the same with PrtSc: while "Use the Print screen key to
//! open screen capture" is on, the key opens the Snipping Tool. That setting
//! is read too, so a PrtSc hotkey gets the same warning.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SystemShortcut {
    /// macOS's id for it in `com.apple.symbolichotkeys`.
    pub id: String,
    /// What the shortcut does, as System Settings › Keyboard Shortcuts ›
    /// Screenshots names it.
    pub label: String,
    /// The combination in Lockshot's accelerator form, e.g. "Cmd+Shift+4".
    pub accelerator: String,
    pub enabled: bool,
}

#[cfg(target_os = "macos")]
mod mac {
    use super::SystemShortcut;
    use plist::Value;

    /// macOS's ids for the Screenshots shortcuts, their names, default keys,
    /// and those defaults as stored ([character, key code, modifier flags]).
    const SCREENSHOT_KEYS: [(&str, &str, &str, [u64; 3]); 5] = [
        (
            "28",
            "Save picture of screen as a file",
            "Shift+Cmd+3",
            [51, 20, 0x120000],
        ),
        (
            "29",
            "Copy picture of screen to the clipboard",
            "Ctrl+Shift+Cmd+3",
            [51, 20, 0x160000],
        ),
        (
            "30",
            "Save picture of selected area as a file",
            "Shift+Cmd+4",
            [52, 21, 0x120000],
        ),
        (
            "31",
            "Copy picture of selected area to the clipboard",
            "Ctrl+Shift+Cmd+4",
            [52, 21, 0x160000],
        ),
        (
            "184",
            "Screenshot and recording options",
            "Shift+Cmd+5",
            [53, 23, 0x120000],
        ),
    ];

    const DOMAIN: &str = "com.apple.symbolichotkeys";
    const KEY: &str = "AppleSymbolicHotKeys";

    /// Virtual key codes (ANSI layout) to accelerator key names.
    fn key_name(code: u64) -> Option<&'static str> {
        Some(match code {
            0 => "A",
            1 => "S",
            2 => "D",
            3 => "F",
            4 => "H",
            5 => "G",
            6 => "Z",
            7 => "X",
            8 => "C",
            9 => "V",
            11 => "B",
            12 => "Q",
            13 => "W",
            14 => "E",
            15 => "R",
            16 => "Y",
            17 => "T",
            18 => "1",
            19 => "2",
            20 => "3",
            21 => "4",
            22 => "6",
            23 => "5",
            25 => "9",
            26 => "7",
            28 => "8",
            29 => "0",
            31 => "O",
            32 => "U",
            34 => "I",
            35 => "P",
            37 => "L",
            38 => "J",
            40 => "K",
            45 => "N",
            46 => "M",
            122 => "F1",
            120 => "F2",
            99 => "F3",
            118 => "F4",
            96 => "F5",
            97 => "F6",
            98 => "F7",
            100 => "F8",
            101 => "F9",
            109 => "F10",
            103 => "F11",
            111 => "F12",
            _ => return None,
        })
    }

    /// `parameters` is [character, key code, modifier flags].
    fn accelerator(parameters: &[Value]) -> Option<String> {
        let code = parameters.get(1)?.as_unsigned_integer()?;
        let flags = parameters.get(2)?.as_unsigned_integer()?;
        let mut parts = Vec::new();
        if flags & 0x40000 != 0 {
            parts.push("Ctrl");
        }
        if flags & 0x80000 != 0 {
            parts.push("Alt");
        }
        if flags & 0x20000 != 0 {
            parts.push("Shift");
        }
        if flags & 0x100000 != 0 {
            parts.push("Cmd");
        }
        parts.push(key_name(code)?);
        Some(parts.join("+"))
    }

    /// The shortcuts as stored in `com.apple.symbolichotkeys`. One missing
    /// from the file has never been changed, so it's on with its default keys.
    pub fn parse(prefs: Option<&Value>) -> Vec<SystemShortcut> {
        let keys = prefs
            .and_then(|v| v.as_dictionary())
            .and_then(|d| d.get(KEY))
            .and_then(|v| v.as_dictionary());
        SCREENSHOT_KEYS
            .iter()
            .map(|(id, label, default, _)| {
                let entry = keys.and_then(|k| k.get(id)).and_then(|v| v.as_dictionary());
                let enabled = entry
                    .and_then(|e| e.get("enabled"))
                    .and_then(|v| v.as_boolean())
                    .unwrap_or(true);
                let accelerator = entry
                    .and_then(|e| e.get("value"))
                    .and_then(|v| v.as_dictionary())
                    .and_then(|v| v.get("parameters"))
                    .and_then(|v| v.as_array())
                    .and_then(|p| accelerator(p))
                    .unwrap_or_else(|| default.to_string());
                SystemShortcut {
                    id: id.to_string(),
                    label: label.to_string(),
                    accelerator,
                    enabled,
                }
            })
            .collect()
    }

    /// The whole preferences domain. `defaults` goes through the preferences
    /// daemon, so it sees a change made in System Settings a moment ago,
    /// unlike reading the file.
    fn export() -> Option<Value> {
        std::process::Command::new("/usr/bin/defaults")
            .args(["export", DOMAIN, "-"])
            .output()
            .ok()
            .filter(|out| out.status.success())
            .and_then(|out| Value::from_reader(std::io::Cursor::new(out.stdout)).ok())
    }

    pub fn read() -> Vec<SystemShortcut> {
        parse(export().as_ref())
    }

    /// Turn the given Screenshots shortcuts on or off in `prefs`, keeping
    /// whatever keys each is set to. Ids that aren't screenshot shortcuts are
    /// ignored, so nothing else in the domain is touched.
    pub fn with_enabled(prefs: Option<Value>, ids: &[String], enabled: bool) -> Value {
        let mut root = prefs.and_then(|v| v.into_dictionary()).unwrap_or_default();
        let mut keys = root
            .remove(KEY)
            .and_then(|v| v.into_dictionary())
            .unwrap_or_default();
        for (id, _, _, defaults) in SCREENSHOT_KEYS
            .iter()
            .filter(|k| ids.iter().any(|i| i == k.0))
        {
            let mut entry = keys
                .remove(id)
                .and_then(|v| v.into_dictionary())
                .unwrap_or_default();
            entry.insert("enabled".into(), Value::Boolean(enabled));
            /* An entry needs its keys too, or macOS forgets them. */
            if entry.get("value").and_then(|v| v.as_dictionary()).is_none() {
                let mut value = plist::Dictionary::new();
                value.insert(
                    "parameters".into(),
                    Value::Array(defaults.iter().map(|n| Value::from(*n)).collect()),
                );
                value.insert("type".into(), Value::from("standard"));
                entry.insert("value".into(), Value::Dictionary(value));
            }
            keys.insert(id.to_string(), Value::Dictionary(entry));
        }
        root.insert(KEY.into(), Value::Dictionary(keys));
        Value::Dictionary(root)
    }

    pub fn set_enabled(ids: &[String], enabled: bool) -> Result<Vec<SystemShortcut>, String> {
        use std::io::Write;
        use std::process::{Command, Stdio};

        let next = with_enabled(export(), ids, enabled);
        let mut xml = Vec::new();
        next.to_writer_xml(&mut xml)
            .map_err(|err| format!("Could not prepare the change: {err}"))?;
        let mut child = Command::new("/usr/bin/defaults")
            .args(["import", DOMAIN, "-"])
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|err| format!("Could not change the macOS shortcuts: {err}"))?;
        child
            .stdin
            .take()
            .ok_or("Could not change the macOS shortcuts.")?
            .write_all(&xml)
            .map_err(|err| format!("Could not change the macOS shortcuts: {err}"))?;
        let status = child
            .wait()
            .map_err(|err| format!("Could not change the macOS shortcuts: {err}"))?;
        if !status.success() {
            return Err(
                "macOS didn't accept the change. Turn them off in System Settings instead.".into(),
            );
        }
        /* Tell the system to pick the change up now rather than at next login. */
        let _ = Command::new(
            "/System/Library/PrivateFrameworks/SystemAdministration.framework/Resources/activateSettings",
        )
        .arg("-u")
        .status();
        Ok(read())
    }
}

/// Reading the Snipping Tool's PrtSc setting. `reg` keeps this free of a
/// registry crate; the parsing is plain text so it's tested on every OS.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod win {
    use super::SystemShortcut;

    pub const ID: &str = "snipping-print-screen";
    const LABEL: &str = "Use the Print screen key to open screen capture";
    /// Windows 11's first build. It turned the setting on by default; Windows
    /// 10 left it off. Either way the value is absent until someone flips it.
    const WINDOWS_11: u32 = 22000;

    /// The value of `name` in `reg query` output, e.g.
    /// "    PrintScreenKeyForSnippingEnabled    REG_DWORD    0x1".
    pub fn reg_value<'a>(output: &'a str, name: &str) -> Option<&'a str> {
        output.lines().find_map(|line| {
            let mut parts = line.split_whitespace();
            (parts.next()? == name).then_some(())?;
            parts.next()?;
            parts.next()
        })
    }

    pub fn snipping_enabled(dword: Option<&str>, build: Option<&str>) -> bool {
        match dword.and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()) {
            Some(value) => value != 0,
            None => build
                .and_then(|b| b.parse::<u32>().ok())
                .is_some_and(|b| b >= WINDOWS_11),
        }
    }

    pub fn shortcut(enabled: bool) -> SystemShortcut {
        SystemShortcut {
            id: ID.into(),
            label: LABEL.into(),
            accelerator: "PrintScreen".into(),
            enabled,
        }
    }

    #[cfg(target_os = "windows")]
    fn query(key: &str, name: &str) -> Option<String> {
        use std::os::windows::process::CommandExt;
        /* CREATE_NO_WINDOW, so no console flashes up. */
        let out = std::process::Command::new("reg")
            .args(["query", key, "/v", name])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        reg_value(&text, name).map(str::to_string)
    }

    #[cfg(target_os = "windows")]
    pub fn read() -> Vec<SystemShortcut> {
        let dword = query(
            r"HKCU\Control Panel\Keyboard",
            "PrintScreenKeyForSnippingEnabled",
        );
        let build = query(
            r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            "CurrentBuildNumber",
        );
        vec![shortcut(snipping_enabled(
            dword.as_deref(),
            build.as_deref(),
        ))]
    }
}

/// Turn the given system screenshot shortcuts (by id) off or back on, and
/// return the list as it now stands. macOS only.
pub fn set_screenshot_shortcuts(
    ids: &[String],
    enabled: bool,
) -> Result<Vec<SystemShortcut>, String> {
    #[cfg(target_os = "macos")]
    {
        mac::set_enabled(ids, enabled)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (ids, enabled);
        Err("Lockshot can only change the system shortcuts on macOS.".into())
    }
}

/// The OS's screenshot shortcuts and whether each is still on. Empty where
/// Lockshot can't tell (Linux).
pub fn screenshot_shortcuts() -> Vec<SystemShortcut> {
    #[cfg(target_os = "macos")]
    {
        mac::read()
    }
    #[cfg(target_os = "windows")]
    {
        win::read()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Vec::new()
    }
}

#[cfg(test)]
mod win_tests {
    use super::win::{reg_value, shortcut, snipping_enabled};

    const ON: &str = "\r\nHKEY_CURRENT_USER\\Control Panel\\Keyboard\r\n    PrintScreenKeyForSnippingEnabled    REG_DWORD    0x1\r\n\r\n";
    const BUILD: &str = "\r\nHKEY_LOCAL_MACHINE\\SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion\r\n    CurrentBuildNumber    REG_SZ    26100\r\n";

    #[test]
    fn reads_reg_query_output() {
        assert_eq!(
            reg_value(ON, "PrintScreenKeyForSnippingEnabled"),
            Some("0x1")
        );
        assert_eq!(reg_value(BUILD, "CurrentBuildNumber"), Some("26100"));
        assert_eq!(
            reg_value(
                "ERROR: The system was unable to find the specified registry key or value.",
                "CurrentBuildNumber"
            ),
            None
        );
    }

    #[test]
    fn snipping_setting_falls_back_to_the_windows_default() {
        assert!(snipping_enabled(Some("0x1"), Some("19045")));
        assert!(!snipping_enabled(Some("0x0"), Some("26100")));
        assert!(
            snipping_enabled(None, Some("26100")),
            "Windows 11 turns it on"
        );
        assert!(
            !snipping_enabled(None, Some("19045")),
            "Windows 10 leaves it off"
        );
        assert!(!snipping_enabled(None, None));
        assert_eq!(shortcut(true).accelerator, "PrintScreen");
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::mac::{parse, with_enabled};
    use plist::{Dictionary, Value};

    fn entry(enabled: bool, code: u64, flags: u64) -> Value {
        let mut value = Dictionary::new();
        value.insert(
            "parameters".into(),
            Value::Array(vec![
                Value::from(65u64),
                Value::from(code),
                Value::from(flags),
            ]),
        );
        value.insert("type".into(), Value::from("standard"));
        let mut e = Dictionary::new();
        e.insert("enabled".into(), Value::Boolean(enabled));
        e.insert("value".into(), Value::Dictionary(value));
        Value::Dictionary(e)
    }

    fn prefs(entries: Vec<(&str, Value)>) -> Value {
        let mut keys = Dictionary::new();
        for (id, v) in entries {
            keys.insert(id.into(), v);
        }
        let mut root = Dictionary::new();
        root.insert("AppleSymbolicHotKeys".into(), Value::Dictionary(keys));
        Value::Dictionary(root)
    }

    #[test]
    fn missing_entries_are_on_with_default_keys() {
        let list = parse(None);
        assert_eq!(list.len(), 5);
        assert!(list.iter().all(|s| s.enabled));
        assert_eq!(list[0].accelerator, "Shift+Cmd+3");
        assert_eq!(list[3].accelerator, "Ctrl+Shift+Cmd+4");
    }

    #[test]
    fn reads_disabled_and_rebound_shortcuts() {
        /* ⌘⇧4 turned off; ⌘⇧5 moved to ⌥⌘S. */
        let p = prefs(vec![
            ("30", entry(false, 21, 0x120000)),
            ("184", entry(true, 1, 0x180000)),
        ]);
        let list = parse(Some(&p));
        assert!(!list[2].enabled);
        assert_eq!(list[2].accelerator, "Shift+Cmd+4");
        assert!(list[4].enabled);
        assert_eq!(list[4].accelerator, "Alt+Cmd+S");
    }

    #[test]
    fn turning_off_keeps_keys_and_leaves_other_entries_alone() {
        /* 30 rebound to ⌥⌘S; 60 is an unrelated shortcut that must survive. */
        let p = prefs(vec![
            ("30", entry(true, 1, 0x180000)),
            ("60", entry(true, 49, 0x40000)),
        ]);
        let next = with_enabled(Some(p), &["28".into(), "30".into(), "999".into()], false);
        let list = parse(Some(&next));
        assert!(!list[0].enabled, "28 had no entry and is now off");
        assert_eq!(
            list[0].accelerator, "Shift+Cmd+3",
            "with its default keys written out"
        );
        assert!(!list[2].enabled);
        assert_eq!(list[2].accelerator, "Alt+Cmd+S", "keeps its custom keys");
        assert!(list[1].enabled && list[3].enabled && list[4].enabled);
        let keys = next
            .as_dictionary()
            .unwrap()
            .get("AppleSymbolicHotKeys")
            .unwrap()
            .as_dictionary()
            .unwrap();
        assert!(keys.get("60").is_some(), "unrelated shortcut kept");
        assert!(keys.get("999").is_none(), "unknown ids ignored");
        let back = parse(Some(&with_enabled(
            Some(next.clone()),
            &["28".into(), "30".into()],
            true,
        )));
        assert!(back[0].enabled && back[2].enabled);
    }
}
