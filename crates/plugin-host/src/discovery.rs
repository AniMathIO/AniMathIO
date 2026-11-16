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
    /// The per-user community plugin folder. It is never loaded wholesale: only the plugins
    /// the user enabled, and only while the global community switch is on (see
    /// [`crate::CommunityStore`] and [`crate::PluginHost::sync_community_plugins`]).
    pub community: bool,
}

/// Discover plugins without consulting the current working directory.
/// Explicit overrides take precedence over bundled and user plugins.
#[must_use]
pub fn plugin_search_dirs() -> Vec<PluginDirectory> {
    search_dirs(
        std::env::var_os("ANIMATHIO_PLUGIN_DIR").map(PathBuf::from),
        std::env::current_exe().ok().as_deref(),
        crate::community::user_plugin_dir(),
    )
}

/// The `ANIMATHIO_PLUGIN_DIR` override when it is set but is not a directory.
pub(crate) fn invalid_override_dir() -> Option<PathBuf> {
    std::env::var_os("ANIMATHIO_PLUGIN_DIR")
        .map(PathBuf::from)
        .filter(|path| !path.is_dir())
}

fn search_dirs(
    override_dir: Option<PathBuf>,
    exe: Option<&Path>,
    user: Option<PathBuf>,
) -> Vec<PluginDirectory> {
    let mut dirs = Vec::new();
    if let Some(path) = override_dir {
        dirs.push(PluginDirectory {
            path,
            allow_reserved_ids: true,
            community: false,
        });
    }
    if let Some(bin) = exe.and_then(Path::parent) {
        dirs.push(PluginDirectory {
            path: bin.join("plugins"),
            allow_reserved_ids: true,
            community: false,
        });
        // Installed packages keep their bundled resources next to, not inside,
        // the binary directory: `usr/lib/<bin>/plugins` in deb and AppImage
        // layouts, `Contents/Resources/plugins` inside a macOS `.app`.
        if let Some(name) = exe.and_then(Path::file_stem) {
            dirs.push(PluginDirectory {
                path: bin.join("..").join("lib").join(name).join("plugins"),
                allow_reserved_ids: true,
                community: false,
            });
        }
        dirs.push(PluginDirectory {
            path: bin.join("..").join("lib/animathio/plugins"),
            allow_reserved_ids: true,
            community: false,
        });
        dirs.push(PluginDirectory {
            path: bin.join("..").join("Resources").join("plugins"),
            allow_reserved_ids: true,
            community: false,
        });
        // Development bundles are built into target/plugins. Never search
        // upward from the CWD, even in development builds.
        #[cfg(debug_assertions)]
        if let Some(target) = bin.parent() {
            dirs.push(PluginDirectory {
                path: target.join("plugins"),
                allow_reserved_ids: true,
                community: false,
            });
        }
    }
    if let Some(path) = user {
        dirs.push(PluginDirectory {
            path,
            allow_reserved_ids: false,
            community: true,
        });
    }
    // An explicit development override often points at the same directory as
    // the executable-relative fallback. Keep the first source (and its trust
    // policy), rather than loading every bundle again and reporting false ID
    // conflicts. Canonical paths also catch relative paths and symlink aliases.
    let mut seen = std::collections::BTreeSet::new();
    dirs.retain(|dir| seen.insert(dir.path.canonicalize().unwrap_or_else(|_| dir.path.clone())));
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
            Some(PathBuf::from("/user/data/animathio/plugins")),
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
                allow_reserved_ids: false,
                community: true,
            }
        );
        assert_eq!(dirs.len(), if cfg!(debug_assertions) { 6 } else { 5 });
        assert!(search_dirs(None, None, None).is_empty());
    }

    #[test]
    fn installed_layouts_are_searched_relative_to_the_executable() {
        let dirs = search_dirs(None, Some(Path::new("/opt/app/bin/animathio")), None);
        let paths: Vec<PathBuf> = dirs.into_iter().map(|dir| dir.path).collect();
        // deb/AppImage resources live in `usr/lib/<binary>/`, macOS bundle
        // resources in `Contents/Resources/`.
        assert!(paths.contains(&PathBuf::from("/opt/app/bin/../lib/animathio/plugins")));
        assert!(paths.contains(&PathBuf::from("/opt/app/bin/../Resources/plugins")));
    }

    #[test]
    fn installed_cli_finds_shared_bundles() {
        let root =
            std::env::temp_dir().join(format!("animathio-cli-layout-{}", std::process::id()));
        let plugins = root.join("usr/lib/animathio/plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        let dirs = search_dirs(None, Some(&root.join("usr/bin/animathio-cli")), None);
        assert!(dirs.iter().any(|dir| dir.allow_reserved_ids
            && dir.path.canonicalize().ok() == plugins.canonicalize().ok()));
        assert!(
            dirs.iter()
                .any(|dir| dir.path.ends_with("lib/animathio-cli/plugins"))
        );
        std::fs::remove_dir_all(root).unwrap();
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

    #[test]
    fn override_and_bundled_alias_load_the_directory_once() {
        let root = std::env::temp_dir().join(format!(
            "animathio-discovery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let plugins = root.join("plugins");
        std::fs::create_dir_all(&plugins).unwrap();
        let canonical_plugins = plugins.canonicalize().unwrap();
        let dirs = search_dirs(
            Some(root.join("plugins/../plugins")),
            Some(&root.join("animathio")),
            Some(root.join("animathio/plugins")),
        );
        assert_eq!(
            dirs.iter()
                .filter(|dir| dir.path.canonicalize().ok().as_ref() == Some(&canonical_plugins))
                .count(),
            1
        );
        assert_eq!(dirs[0].path, root.join("plugins/../plugins"));
        assert!(dirs[0].allow_reserved_ids);
        std::fs::remove_dir_all(root).unwrap();
    }
}
