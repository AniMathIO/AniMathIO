//! `animathio-cli plugins`: manage community plugins from the command line.
//!
//! Community plugins live in the per-user plugin folder and are off until the user opts in
//! (global switch plus a flag per plugin); see `docs/plugins.md`. These commands edit the same
//! folder and `community-plugins.json` settings the editor uses.

use std::path::{Path, PathBuf};

use animathio_plugin_host::{CommunityError, CommunityStore, InstallOutcome};
use clap::{Subcommand, ValueEnum};
use serde::Serialize;

use crate::Result;

/// `on` or `off`.
#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Switch {
    /// Load enabled community plugins.
    On,
    /// Load only the bundled first-party plugins.
    Off,
}

#[derive(Subcommand)]
pub(crate) enum PluginsAction {
    /// List the installed community plugins and whether each is enabled.
    List {
        /// Print JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Install a community plugin from a local `.amplugin` file or an https URL. Installed
    /// plugins start disabled; run `plugins enable <id>` afterwards.
    Install {
        /// A `.amplugin` file path or an `https://` URL.
        source: String,
        /// Expected SHA-256 of the package (64 hex characters). Required for URLs and
        /// verified before anything is written.
        #[arg(long)]
        sha256: Option<String>,
        /// Replace an already installed plugin with the same id (an update).
        #[arg(long)]
        force: bool,
    },
    /// Enable an installed community plugin.
    Enable {
        /// Plugin id from `plugins list`.
        id: String,
    },
    /// Disable a community plugin without removing it.
    Disable {
        /// Plugin id from `plugins list`.
        id: String,
    },
    /// Uninstall a community plugin: delete its file and its enabled flag.
    Remove {
        /// Plugin id from `plugins list`.
        id: String,
    },
    /// Turn the global community plugins switch on or off; without an argument, show it.
    Community {
        /// `on` or `off`.
        #[arg(value_enum)]
        state: Option<Switch>,
    },
}

#[derive(Serialize)]
struct Listing<'a> {
    community_enabled: bool,
    folder: &'a Path,
    plugins: &'a [animathio_plugin_host::InstalledPlugin],
}

pub(crate) fn run(action: PluginsAction) -> Result<()> {
    let store = CommunityStore::at_default_location(None)?;
    run_with(&store, action)
}

pub(crate) fn run_with(store: &CommunityStore, action: PluginsAction) -> Result<()> {
    match action {
        PluginsAction::List { json } => list(store, json),
        PluginsAction::Install {
            source,
            sha256,
            force,
        } => {
            let outcome = install(store, &source, sha256.as_deref(), force)?;
            report_install(store, &outcome);
        }
        PluginsAction::Enable { id } => {
            store.set_plugin_enabled(&id, true)?;
            println!("enabled {id}");
            if !store.settings().enabled {
                println!(
                    "note: community plugins are off, so it will not load; run \
                     `animathio-cli plugins community on`"
                );
            }
        }
        PluginsAction::Disable { id } => {
            store.set_plugin_enabled(&id, false)?;
            println!("disabled {id}");
        }
        PluginsAction::Remove { id } => {
            store.uninstall(&id)?;
            println!("removed {id}");
        }
        PluginsAction::Community { state } => {
            if let Some(state) = state {
                store.set_enabled(matches!(state, Switch::On))?;
            }
            let on = store.settings().enabled;
            println!("community plugins: {}", if on { "on" } else { "off" });
        }
    }
    Ok(())
}

fn install(
    store: &CommunityStore,
    source: &str,
    sha256: Option<&str>,
    force: bool,
) -> std::result::Result<InstallOutcome, CommunityError> {
    if source.len() >= 7 && source[..7].eq_ignore_ascii_case("http://")
        || source.len() >= 8 && source[..8].eq_ignore_ascii_case("https://")
    {
        let hash = sha256.ok_or(CommunityError::HashRequired)?;
        store.install_url(source, hash, force)
    } else {
        store.install_file(&PathBuf::from(source), sha256, force)
    }
}

fn report_install(store: &CommunityStore, outcome: &InstallOutcome) {
    match &outcome.replaced {
        Some(old) => println!(
            "updated {} {old} -> {} ({})",
            outcome.id,
            outcome.version,
            outcome.path.display()
        ),
        None => println!(
            "installed {} {} ({})",
            outcome.id,
            outcome.version,
            outcome.path.display()
        ),
    }
    if !store.settings().plugins.contains(&outcome.id) {
        println!(
            "it starts disabled: run `animathio-cli plugins enable {}` (and \
             `animathio-cli plugins community on` if community plugins are off)",
            outcome.id
        );
    }
}

fn list(store: &CommunityStore, json: bool) {
    let settings = store.settings();
    let (plugins, problems) = store.list();
    if json {
        let listing = Listing {
            community_enabled: settings.enabled,
            folder: store.dir(),
            plugins: &plugins,
        };
        println!(
            "{}",
            serde_json::to_string_pretty(&listing).expect("listing serializes")
        );
    } else {
        println!(
            "community plugins: {} ({})",
            if settings.enabled { "on" } else { "off" },
            store.dir().display()
        );
        if plugins.is_empty() {
            println!("no community plugins installed");
        }
        for plugin in &plugins {
            println!(
                "{}\t{}\t{}\t{}",
                plugin.id,
                plugin.version,
                if plugin.enabled {
                    "enabled"
                } else {
                    "disabled"
                },
                plugin.name
            );
        }
    }
    for problem in problems {
        eprintln!("warning: {problem}");
    }
}
