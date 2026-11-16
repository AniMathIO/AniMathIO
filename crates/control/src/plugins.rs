//! The plugin inventory: `plugins.list`.
//!
//! Read-only. It reports the bundled first-party plugins that are loaded and every installed
//! community plugin with its enabled flag, together with the global community switch.
//! Installing, enabling and removing plugins is deliberately not part of the control protocol:
//! those are user actions in the editor and the CLI (`animathio-cli plugins ...`).

use animathio_plugin_host::{CommunityStore, PluginHost};
use serde::{Deserialize, Serialize};

/// Where a plugin comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PluginSource {
    /// Shipped with AniMathIO or supplied through `ANIMATHIO_PLUGIN_DIR`; always enabled.
    Bundled,
    /// Installed in the per-user folder; runs only when the user enabled it.
    Community,
}

/// One plugin in the inventory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginInfo {
    /// Plugin id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Description from the manifest.
    pub description: String,
    /// Plugin kinds (`clip-source`, `effect`, ...).
    pub kinds: Vec<String>,
    /// Bundled or community.
    pub source: PluginSource,
    /// Whether the plugin is enabled (bundled plugins always are).
    pub enabled: bool,
    /// Whether the plugin is loaded into the running host right now.
    pub loaded: bool,
}

/// The answer of `plugins.list`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginsListing {
    /// The global community plugins switch.
    pub community_enabled: bool,
    /// Plugins sorted by id.
    pub plugins: Vec<PluginInfo>,
}

/// Build the inventory from the loaded `host` and the community `store`.
#[must_use]
pub fn plugins_listing(
    host: Option<&PluginHost>,
    store: Option<&CommunityStore>,
) -> PluginsListing {
    let mut plugins = Vec::new();
    if let Some(host) = host {
        for id in host.loaded_plugin_ids() {
            let Some(plugin) = host.get_plugin(&id) else {
                continue;
            };
            let community = host.is_community_plugin(&id);
            let manifest = &plugin.manifest;
            plugins.push(PluginInfo {
                id: id.clone(),
                name: manifest.name.clone(),
                version: manifest.version.clone(),
                description: manifest.description.clone(),
                kinds: manifest
                    .kinds
                    .iter()
                    .map(|k| k.as_str().to_owned())
                    .collect(),
                source: if community {
                    PluginSource::Community
                } else {
                    PluginSource::Bundled
                },
                enabled: true,
                loaded: true,
            });
        }
    }
    let mut community_enabled = false;
    if let Some(store) = store {
        community_enabled = store.settings().enabled;
        for installed in store.list().0 {
            if let Some(known) = plugins.iter_mut().find(|p| p.id == installed.id) {
                // Loaded community plugin: keep the loaded entry, record the flag.
                if known.source == PluginSource::Community {
                    known.enabled = installed.enabled;
                }
                continue;
            }
            plugins.push(PluginInfo {
                id: installed.id,
                name: installed.name,
                version: installed.version,
                description: installed.description,
                kinds: installed.kinds,
                source: PluginSource::Community,
                enabled: installed.enabled,
                loaded: false,
            });
        }
    }
    plugins.sort_by(|a, b| a.id.cmp(&b.id));
    PluginsListing {
        community_enabled,
        plugins,
    }
}
