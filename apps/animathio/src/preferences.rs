//! User preferences, separate from the project and dock layout.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Preferences {
    pub(crate) autosave_seconds: u64,
    pub(crate) export_folder: PathBuf,
    pub(crate) compact: bool,
    pub(crate) reduce_motion: bool,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            autosave_seconds: 60,
            export_folder: PathBuf::new(),
            compact: true,
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
