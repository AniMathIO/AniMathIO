//! Community plugins in the running editor: the global switch and hot load/unload.
//!
//! The trust settings live in `community-plugins.json` (see `animathio_plugin_host::community`)
//! and are shared with `animathio-cli`. Changing them reconciles the plugin host in place:
//! newly enabled plugins load, disabled ones unload, the inspector's plugin list refreshes and
//! the preview re-renders, so clips of an unloaded plugin show the missing-plugin placeholder
//! and clips of a newly loaded one render, all without a restart.

use super::*;

impl Editor {
    /// Turn the global "Community plugins" switch on or off and apply it.
    pub(super) fn set_community_plugins_enabled(
        &mut self,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let store = crate::editor_env::community_store()
            .ok_or("no per-user directory is available for community plugins")?;
        store.set_enabled(enabled).map_err(|e| e.to_string())?;
        self.sync_community_plugins(cx);
        Ok(())
    }

    /// Whether the global community plugins switch is on.
    pub(super) fn community_plugins_enabled(&self) -> bool {
        crate::editor_env::community_store().is_some_and(|store| store.settings().enabled)
    }

    /// Reconcile the loaded community plugins with the saved settings (the CLI may have
    /// changed them), refresh the plugin list and re-render the preview when anything changed.
    pub(super) fn sync_community_plugins(&mut self, cx: &mut Context<Self>) {
        let (Some(store), Some(host)) = (
            crate::editor_env::community_store(),
            self.plugin_host.as_mut(),
        ) else {
            return;
        };
        let report = host.sync_community_plugins(&store);
        for problem in &report.problems {
            self.notices
                .push(crate::style::Toast::warning(problem.clone()));
        }
        if report.changed() {
            if let Some(host) = &self.plugin_host {
                self.plugins = super::support::plugin_descriptors(host);
            }
            let time = self.current_render_time();
            self.request_render(time);
        }
        cx.notify();
    }
}
