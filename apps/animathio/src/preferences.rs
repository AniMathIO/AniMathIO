//! User preferences, separate from the project and dock layout.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Preferences {
    pub(crate) autosave_seconds: u64,
    pub(crate) export_folder: PathBuf,
    pub(crate) compact: bool,
    pub(crate) ui_scale: Option<u16>,
    pub(crate) reduce_motion: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            autosave_seconds: 60,
            export_folder: PathBuf::new(),
            compact: true,
            ui_scale: None,
            reduce_motion: false,
        }
    }
}
impl Preferences {
    /// Where exports go by default: the configured folder, else the saved
    /// project's folder, else the user's Videos (or home) folder. Never the
    /// process working directory, which is `/` or `$HOME` from a launcher.
    pub(crate) fn export_dir(&self, project: Option<&Path>) -> PathBuf {
        if !self.export_folder.as_os_str().is_empty() {
            return self.export_folder.clone();
        }
        if let Some(dir) = project
            .and_then(Path::parent)
            .filter(|dir| !dir.as_os_str().is_empty())
        {
            return dir.to_path_buf();
        }
        directories::UserDirs::new().map_or_else(PathBuf::new, |dirs| {
            dirs.video_dir()
                .unwrap_or_else(|| dirs.home_dir())
                .to_path_buf()
        })
    }

    pub(crate) fn effective_scale(&self) -> f32 {
        let percent = std::env::var("ANIMATHIO_UI_SCALE")
            .ok()
            .and_then(|s| s.trim_end_matches('%').parse::<f32>().ok())
            .filter(|n| n.is_finite())
            .or(self.ui_scale.map(f32::from))
            .unwrap_or_else(desktop_text_percent);
        percent.clamp(80., 200.) / 100.
    }

    fn path() -> Option<PathBuf> {
        directories::ProjectDirs::from("org", "AniMathIO", "AniMathIO")
            .map(|dirs| dirs.config_dir().join("preferences.json"))
    }
    pub(crate) fn load() -> Self {
        Self::path()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }
    pub(crate) fn save(&self) -> Result<(), String> {
        let path = Self::path().ok_or("configuration directory unavailable")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes)
            .and_then(|()| std::fs::rename(temporary, path))
            .map_err(|e| e.to_string())
    }
}
/// Read local desktop configuration without spawning a settings service.
fn desktop_text_percent() -> f32 {
    static SCALE: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *SCALE.get_or_init(detect_desktop_text_percent)
}
fn detect_desktop_text_percent() -> f32 {
    windows_text_percent()
        .or_else(unix_text_percent)
        .unwrap_or(100.)
}

/// The Windows accessibility text-size slider (100–225%), stored as
/// `TextScaleFactor` under `HKCU\Software\Microsoft\Accessibility`.
#[cfg(windows)]
fn windows_text_percent() -> Option<f32> {
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Accessibility",
            "/v",
            "TextScaleFactor",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let raw = text.lines().find_map(|line| {
        let mut fields = line.split_whitespace();
        (fields.next()? == "TextScaleFactor")
            .then(|| fields.last())
            .flatten()
    })?;
    let percent = if let Some(hex) = raw.strip_prefix("0x") {
        i32::from_str_radix(hex, 16).ok()?
    } else {
        raw.parse::<i32>().ok()?
    };
    (90..=250).contains(&percent).then_some(percent as f32)
}

#[cfg(not(windows))]
fn windows_text_percent() -> Option<f32> {
    None
}

/// KDE `forceFontDPI`, `~/.Xresources` `Xft.dpi` or GNOME's `text-scaling-factor`,
/// read straight from config files (no settings daemon).
#[cfg(unix)]
fn unix_text_percent() -> Option<f32> {
    let Some(home) = std::env::var_os("HOME") else {
        return None;
    };
    let home = PathBuf::from(home);
    let config =
        std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
    for (path, key) in [
        (config.join("kcmfonts"), "forceFontDPI="),
        (home.join(".Xresources"), "Xft.dpi:"),
    ] {
        if let Ok(text) = std::fs::read_to_string(path)
            && let Some(dpi) = text
                .lines()
                .find_map(|line| line.trim().strip_prefix(key)?.trim().parse::<f32>().ok())
                .filter(|dpi| dpi.is_finite() && *dpi > 0.)
        {
            return Some(dpi / 96. * 100.);
        }
    }
    if std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_lowercase()
        .contains("gnome")
        && let Ok(output) = std::process::Command::new("timeout")
            .args([
                "1",
                "gsettings",
                "get",
                "org.gnome.desktop.interface",
                "text-scaling-factor",
            ])
            .output()
        && let Ok(scale) = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<f32>()
        && scale.is_finite()
        && scale > 0.
    {
        return Some(scale * 100.);
    }
    None
}

#[cfg(not(unix))]
fn unix_text_percent() -> Option<f32> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exports_default_next_to_the_project_never_the_working_directory() {
        let mut prefs = Preferences::default();
        let project = Path::new("/projects/talk/talk.amio");
        assert_eq!(prefs.export_dir(Some(project)), Path::new("/projects/talk"));
        let unsaved = prefs.export_dir(None);
        if directories::UserDirs::new().is_some() {
            assert!(unsaved.is_absolute(), "{unsaved:?}");
        }
        prefs.export_folder = PathBuf::from("/renders");
        assert_eq!(prefs.export_dir(Some(project)), Path::new("/renders"));
    }

    #[test]
    fn scale_defaults_and_saved_choice_are_backwards_compatible() {
        let old: Preferences = serde_json::from_str("{}").unwrap();
        assert_eq!(old.ui_scale, None);
        let saved: Preferences = serde_json::from_str(r#"{"ui_scale":150}"#).unwrap();
        assert_eq!(saved.ui_scale, Some(150));
        assert_eq!(
            serde_json::from_str::<Preferences>(&serde_json::to_string(&saved).unwrap())
                .unwrap()
                .ui_scale,
            Some(150)
        );
    }

    #[test]
    fn preferences_round_trip_and_default_missing_fields() {
        let prefs: Preferences = serde_json::from_str("{}").unwrap();
        assert_eq!(prefs.autosave_seconds, 60);
        assert!(!prefs.reduce_motion);
        let text = serde_json::to_string(&prefs).unwrap();
        assert_eq!(
            serde_json::from_str::<Preferences>(&text).unwrap().compact,
            prefs.compact
        );
    }
}
