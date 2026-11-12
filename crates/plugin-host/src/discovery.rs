//! Shared discovery policy for the editor and headless control handler.

use std::path::{Path, PathBuf};

/// A plugin directory and the trust attached to its source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginDirectory {
    /// Directory containing `.amplugin` bundles.
    pub path: PathBuf,
    /// Only explicit overrides and executable-relative bundles may claim
    /// first-party `org.animathio.*` ids.
    pub allow_reserved_ids: bool,
}

/// Discover plugins without consulting the current working directory.
/// Explicit overrides take precedence over bundled and user plugins.
#[must_use]
pub fn plugin_search_dirs() -> Vec<PluginDirectory> {
    search_dirs(
        std::env::var_os("ANIMATHIO_PLUGIN_DIR").map(PathBuf::from),
        std::env::current_exe().ok().as_deref(),
        directories::BaseDirs::new()
            .as_ref()
            .map(directories::BaseDirs::data_dir),
    )
}

fn search_dirs(
    override_dir: Option<PathBuf>,
    exe: Option<&Path>,
    data: Option<&Path>,
) -> Vec<PluginDirectory> {
    let mut dirs = Vec::new();
    if let Some(path) = override_dir {
        dirs.push(PluginDirectory {
            path,
            allow_reserved_ids: true,
        });
    }
    if let Some(bin) = exe.and_then(Path::parent) {
        dirs.push(PluginDirectory {
            path: bin.join("plugins"),
            allow_reserved_ids: true,
        });
        // Development bundles are built into target/plugins. Never search
        // upward from the CWD, even in development builds.
        #[cfg(debug_assertions)]
        if let Some(target) = bin.parent() {
            dirs.push(PluginDirectory {
                path: target.join("plugins"),
                allow_reserved_ids: true,
            });
        }
    }
    if let Some(data) = data {
        dirs.push(PluginDirectory {
            path: data.join("animathio/plugins"),
            allow_reserved_ids: false,
        });
    }
    dirs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_uses_only_explicit_executable_and_user_locations() {
        let dirs = search_dirs(
            Some(PathBuf::from("/override")),
            Some(Path::new("/install/bin/animathio")),
            Some(Path::new("/user/data")),
        );
        assert_eq!(dirs[0].path, Path::new("/override"));
        assert_eq!(dirs[1].path, Path::new("/install/bin/plugins"));
        assert!(
            dirs[..dirs.len() - 1]
                .iter()
                .all(|dir| dir.allow_reserved_ids)
        );
        assert_eq!(
            dirs.last().unwrap(),
            &PluginDirectory {
                path: PathBuf::from("/user/data/animathio/plugins"),
                allow_reserved_ids: false
            }
        );
        assert_eq!(dirs.len(), if cfg!(debug_assertions) { 4 } else { 3 });
        assert!(search_dirs(None, None, None).is_empty());
    }

    #[test]
    fn user_plugins_cannot_claim_reserved_ids() {
        assert!(matches!(
            super::super::check_plugin_id("org.animathio.latex", false),
            Err(crate::HostError::ReservedId(_))
        ));
        assert!(super::super::check_plugin_id("org.animathio.latex", true).is_ok());
        assert!(super::super::check_plugin_id("com.example.math", false).is_ok());
    }
}
