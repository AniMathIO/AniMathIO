//! Community plugins in the running editor: the global switch and hot load/unload.
//!
//! The trust settings live in `community-plugins.json` (see `animathio_plugin_host::community`)
//! and are shared with `animathio-cli`. Changing them reconciles the plugin host in place:
//! newly enabled plugins load, disabled ones unload, the inspector's plugin list refreshes and
//! the preview re-renders, so clips of an unloaded plugin show the missing-plugin placeholder
//! and clips of a newly loaded one render, all without a restart.

use super::*;
use crate::style::{color, hex};
use animathio_plugin_host::{CommunityStore, InstalledPlugin};
mod registry;
use registry::{Downloader, Entry, Index, IndexFetcher};

pub(super) struct Browser {
    pub installed: Vec<InstalledPlugin>,
    pub index: Option<Index>,
    pub browsing: bool,
    pub fetching: bool,
    pub busy: bool,
    pub error: Option<String>,
    pub explanation: bool,
    pub fetch: IndexFetcher,
    pub download: Downloader,
    pub store: Option<CommunityStore>,
}
impl Default for Browser {
    fn default() -> Self {
        Self {
            installed: vec![],
            index: None,
            browsing: false,
            fetching: false,
            busy: false,
            error: None,
            explanation: false,
            fetch: registry::fetcher(),
            download: registry::downloader(),
            store: None,
        }
    }
}

impl Editor {
    /// Turn the global "Community plugins" switch on or off and apply it.
    pub(super) fn set_community_plugins_enabled(
        &mut self,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let store = self
            .community_store()
            .ok_or("no per-user directory is available for community plugins")?;
        store.set_enabled(enabled).map_err(|e| e.to_string())?;
        if enabled {
            let acknowledged = crate::editor_env::config_file("community-plugin-explanation");
            if !acknowledged.as_ref().is_some_and(|p| p.exists()) {
                self.community.explanation = true;
                if let Some(path) = acknowledged {
                    if let Some(parent) = path.parent() {
                        let _ = std::fs::create_dir_all(parent);
                    }
                    let _ = std::fs::write(path, b"shown");
                }
            }
        } else {
            self.community.browsing = false;
        }
        self.sync_community_plugins(cx);
        Ok(())
    }

    /// Reconcile the loaded community plugins with the saved settings (the CLI may have
    /// changed them), refresh the plugin list and re-render the preview when anything changed.
    pub(super) fn sync_community_plugins(&mut self, cx: &mut Context<Self>) {
        let (Some(store), Some(host)) = (self.community_store(), self.plugin_host.as_mut()) else {
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

impl Editor {
    pub(super) fn community_store(&self) -> Option<CommunityStore> {
        self.community
            .store
            .clone()
            .or_else(crate::editor_env::community_store)
    }
    pub(super) fn community_enabled(&self) -> bool {
        self.community_store().is_some_and(|s| s.settings().enabled)
    }
    pub(super) fn refresh_community_inventory(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.community_store() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let (installed, problems) = cx
                .background_executor()
                .spawn(async move { store.list() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.community.installed = installed;
                for problem in problems {
                    this.notices.push(crate::style::Toast::warning(problem));
                }
                cx.notify();
            });
        })
        .detach();
    }
    pub(super) fn browse_community(&mut self, cx: &mut Context<Self>) {
        if !self.community_enabled() || self.community.fetching {
            return;
        }
        self.community.browsing = true;
        self.community.fetching = true;
        self.community.error = None;
        let fetch = self.community.fetch.clone();
        cx.spawn(async move |this, cx| {
            let result = cx.background_executor().spawn(async move { fetch() }).await;
            let _ = this.update(cx, |this, cx| {
                this.community.fetching = false;
                match result {
                    Ok(index) => this.community.index = Some(index),
                    Err(error) => {
                        this.community.error =
                            Some(format!("Could not fetch community plugins: {error}"))
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    fn community_job(
        &mut self,
        job: impl FnOnce(CommunityStore) -> Result<String, String> + Send + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.community.busy {
            return;
        }
        let Some(store) = self.community_store() else {
            return;
        };
        self.community.busy = true;
        self.community.error = None;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let result = job(store.clone());
                    (result, store.list().0)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.community.busy = false;
                this.community.installed = result.1;
                match result.0 {
                    Ok(message) => this.notices.push(crate::style::Toast::info(message)),
                    Err(error) => this.community.error = Some(error),
                }
                this.sync_community_plugins(cx);
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }
    pub(super) fn install_community_entry(&mut self, entry: Entry, cx: &mut Context<Self>) {
        if !self.community_enabled() || entry.compatible().is_err() {
            return;
        }
        let download = self.community.download.clone();
        let update = self.community.installed.iter().any(|p| p.id == entry.id);
        self.community_job(
            move |store| {
                let installed = entry.install(&store, &download, update)?;
                Ok(format!(
                    "Installed {} {}. Enable it in Installed.",
                    installed.name, installed.version
                ))
            },
            cx,
        );
    }
    pub(super) fn toggle_community_plugin(
        &mut self,
        id: String,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.community_enabled() {
            return;
        }
        self.community_job(
            move |store| {
                store
                    .set_plugin_enabled(&id, enabled)
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "{} {id}",
                    if enabled { "Enabled" } else { "Disabled" }
                ))
            },
            cx,
        );
    }
    pub(super) fn remove_community_plugin(&mut self, id: String, cx: &mut Context<Self>) {
        self.community_job(
            move |store| {
                store.uninstall(&id).map_err(|e| e.to_string())?;
                Ok(format!("Removed {id}"))
            },
            cx,
        );
    }
    fn install_community_file(&mut self, cx: &mut Context<Self>) {
        self.community_job(
            move |store| {
                let Some(path) = rfd::FileDialog::new()
                    .add_filter("AniMathIO plugin", &["amplugin"])
                    .pick_file()
                else {
                    return Ok("Plugin installation cancelled".into());
                };
                let installed = store
                    .install_file(&path, None, false)
                    .map_err(|e| e.to_string())?;
                Ok(format!(
                    "Installed {}. Enable it in Installed.",
                    installed.name
                ))
            },
            cx,
        );
    }
    pub(super) fn missing_plugin_ids(&self) -> BTreeSet<String> {
        let mut ids = BTreeSet::new();
        for track in self.project.tracks() {
            for effect in track.audio_effects() {
                ids.insert(effect.plugin.clone());
            }
            for clip in track.clips().values() {
                if let ClipKind::Plugin { plugin_id, .. } = &clip.kind {
                    ids.insert(plugin_id.clone());
                }
                for effect in clip.effects.iter().chain(&clip.audio_effects) {
                    ids.insert(effect.plugin.clone());
                }
            }
        }
        ids.retain(|id| {
            self.plugin_host
                .as_ref()
                .is_none_or(|h| h.get_plugin(id).is_none())
        });
        ids
    }
}

impl Editor {
    fn bundled_inventory(&self, cx: &mut Context<Self>) -> Div {
        let mut body = div().flex().flex_col().gap_2();
        let Some(host) = &self.plugin_host else {
            return body;
        };
        for id in host.loaded_plugin_ids() {
            if self.community.installed.iter().any(|p| p.id == id) {
                continue;
            }
            let Some(plugin) = host.get_plugin(&id) else {
                continue;
            };
            let kinds = plugin
                .manifest
                .kinds
                .iter()
                .map(|kind| match kind {
                    PluginKind::ClipSource => "Clip source",
                    PluginKind::Effect => "Video effects",
                    PluginKind::AudioEffect => "Audio effects",
                    PluginKind::AnimationPreset => "Animation presets",
                    PluginKind::Importer => "Importer",
                    _ => "Animation",
                })
                .collect::<Vec<_>>()
                .join(", ");
            let source_full = plugin
                .source_path
                .as_ref()
                .map_or_else(|| "In-memory package".into(), |p| p.display().to_string());
            // The bundle file name is what identifies the package; the full path is the tooltip.
            let source = plugin
                .source_path
                .as_ref()
                .and_then(|p| p.file_name())
                .map_or_else(|| source_full.clone(), |n| n.to_string_lossy().into_owned());
            let origin = if id.starts_with("org.animathio.") {
                "First-party"
            } else {
                "Third-party"
            };
            if !self.library_matches(
                LeftTab::Plugins,
                &format!(
                    "{} {} {} {} {} {}",
                    id, plugin.manifest.name, plugin.manifest.version, kinds, source_full, origin
                ),
            ) {
                continue;
            }
            let selector = format!("installed-{id}");
            let source_tooltip = source_full.clone();
            let id_tooltip = id.clone();
            body = body.child(
                div()
                    .id(SharedString::from(selector.clone()))
                    .debug_selector(move || selector.clone())
                    .p_2()
                    .rounded(px(6.))
                    .border_1()
                    .border_color(hex(color::BORDER))
                    .bg(hex(color::RAISED))
                    .text_size(crate::style::type_size::XS)
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .items_center()
                            .child(icons::icon(IconName::Puzzle).small())
                            .child(div().flex_1().min_w_0().child(plugin.manifest.name.clone()))
                            .child(
                                div()
                                    .text_color(hex(color::TEXT_SECONDARY))
                                    .child(format!("v{} · Enabled", plugin.manifest.version)),
                            ),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("plugin-id-{id}")))
                            .truncate()
                            .child(id)
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(id_tooltip.clone())
                                    .build(window, cx)
                            }),
                    )
                    .child(
                        div()
                            .text_color(hex(color::TEXT_SECONDARY))
                            .child(format!("{kinds} · {origin}")),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!(
                                "plugin-source-{}",
                                plugin.manifest.id
                            )))
                            .truncate()
                            .text_color(hex(color::TEXT_MUTED))
                            .child(source)
                            .tooltip(move |window, cx| {
                                gpui_component::tooltip::Tooltip::new(source_tooltip.clone())
                                    .build(window, cx)
                            }),
                    ),
            );
        }
        for failure in host.load_failures() {
            body = body.child(
                div()
                    .text_size(crate::style::type_size::XS)
                    .text_color(hex(color::DANGER))
                    .child(failure.clone()),
            );
        }
        body
    }
}

fn card(id: String) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.clone()))
        .debug_selector(move || id.clone())
        .p_2()
        .rounded(px(6.))
        .border_1()
        .border_color(hex(color::BORDER))
        .bg(hex(color::RAISED))
        .text_size(crate::style::type_size::XS)
        .flex()
        .flex_col()
        .gap_2()
}
impl Editor {
    pub(super) fn plugin_inventory(&self, cx: &mut Context<Self>) -> Div {
        let enabled = self.community_enabled();
        let busy = self.community.busy;
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .debug_selector(|| "plugin-inventory".into())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().child("Community plugins"))
                    .child(
                        gpui_component::switch::Switch::new("community-switch")
                            .checked(enabled)
                            .accessibility_label("Community plugins")
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                if let Err(error) = this.set_community_plugins_enabled(*checked, cx)
                                {
                                    this.community.error = Some(error);
                                }
                                cx.notify();
                            })),
                    ),
            );
        if self.community.explanation && enabled {
            body = body.child(card("community-explanation".into())
                .child("Community plugins run sandboxed with memory and time limits. They are third-party code and are not audited by AniMathIO.")
                .child(Button::new("dismiss-plugin-explanation").small().ghost().label("Got it")
                    .on_click(cx.listener(|this, _, _, cx| { this.community.explanation = false; cx.notify(); }))));
        }
        let mut navigation = div().flex().flex_wrap().gap_2().child(
            Button::new("community-installed")
                .small()
                .ghost()
                .label("Installed")
                .selected(!self.community.browsing)
                .on_click(cx.listener(|this, _, _, cx| {
                    this.community.browsing = false;
                    this.refresh_community_inventory(cx);
                    cx.notify();
                })),
        );
        if enabled {
            navigation = navigation.child(
                Button::new("community-browse")
                    .small()
                    .ghost()
                    .label("Browse")
                    .debug_selector(|| "community-browse".into())
                    .selected(self.community.browsing)
                    .disabled(self.community.fetching)
                    .on_click(cx.listener(|this, _, _, cx| this.browse_community(cx))),
            );
        }
        body = body.child(navigation).child(
            Button::new("community-install-file")
                .small()
                .ghost()
                .icon(icons::icon(IconName::FolderOpen))
                .label("Install from file...")
                .disabled(busy)
                .on_click(cx.listener(|this, _, _, cx| this.install_community_file(cx))),
        );
        if busy {
            body = body.child("Installing or updating plugin settings...");
        }
        if let Some(error) = &self.community.error {
            body = body.child(
                card("community-error".into())
                    .text_color(hex(color::DANGER))
                    .child(error.clone())
                    .when(enabled && self.community.browsing, |row| {
                        row.child(
                            Button::new("community-retry")
                                .small()
                                .label("Retry")
                                .disabled(self.community.fetching)
                                .on_click(cx.listener(|this, _, _, cx| this.browse_community(cx))),
                        )
                    }),
            );
        }
        for id in self.missing_plugin_ids() {
            let entry = self
                .community
                .index
                .as_ref()
                .and_then(|i| i.plugins.iter().find(|p| p.id == id))
                .cloned();
            let mut row = card(format!("missing-plugin-{id}")).child(format!(
                "Missing plugin: {id}. Its clips use a placeholder until it is enabled."
            ));
            if enabled {
                if let Some(entry) = entry {
                    row = row.child(
                        Button::new(SharedString::from(format!("missing-install-{id}")))
                            .small()
                            .label("Install")
                            .disabled(
                                busy || entry.compatible().is_err()
                                    || self.community.installed.iter().any(|p| p.id == id),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.install_community_entry(entry.clone(), cx)
                            })),
                    );
                } else {
                    row = row.child(
                        Button::new(SharedString::from(format!("missing-browse-{id}")))
                            .small()
                            .ghost()
                            .label("Find in Browse")
                            .on_click(cx.listener(|this, _, _, cx| this.browse_community(cx))),
                    );
                }
            }
            body = body.child(row);
        }
        if self.community.browsing && enabled {
            return body.child(self.community_catalog(cx));
        }
        body = body.child(div().font_weight(FontWeight::SEMIBOLD).child("Installed"));
        for plugin in &self.community.installed {
            if !self.library_matches(
                LeftTab::Plugins,
                &format!(
                    "{} {} {} Community",
                    plugin.id, plugin.name, plugin.description
                ),
            ) {
                continue;
            }
            let id = plugin.id.clone();
            let remove = id.clone();
            let on = plugin.enabled;
            let mut row = card(format!("installed-{id}"))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("{} · v{}", plugin.name, plugin.version)),
                )
                .child(plugin.id.clone())
                .child(plugin.description.clone())
                .child(format!(
                    "Community · ABI {} · {}",
                    plugin.api_version,
                    if on && enabled { "Enabled" } else { "Disabled" }
                ))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            Button::new(SharedString::from(format!("enable-{id}")))
                                .small()
                                .label(if on { "Disable" } else { "Enable" })
                                .disabled(busy || !enabled)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_community_plugin(id.clone(), !on, cx)
                                })),
                        )
                        .child(
                            Button::new(SharedString::from(format!("remove-{remove}")))
                                .small()
                                .ghost()
                                .label("Remove")
                                .disabled(busy)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_community_plugin(remove.clone(), cx)
                                })),
                        ),
                );
            if let Some(index) = &self.community.index {
                if let Some(entry) = index
                    .plugins
                    .iter()
                    .find(|e| e.id == plugin.id && e.newer_than(&plugin.version))
                {
                    let entry = entry.clone();
                    row = row.child(
                        Button::new(SharedString::from(format!("update-{}", entry.id)))
                            .small()
                            .label(format!("Update available · {}", entry.version))
                            .disabled(busy || !enabled || entry.compatible().is_err())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.install_community_entry(entry.clone(), cx)
                            })),
                    );
                }
                if let Some(delisted) = index.delisted.iter().find(|d| d.id == plugin.id) {
                    row = row.child(
                        div()
                            .text_color(hex(color::WARNING))
                            .child(format!("Delisted: {}", delisted.reason)),
                    );
                }
            }
            body = body.child(row);
        }
        body.child(self.bundled_inventory(cx))
    }
    fn community_catalog(&self, cx: &mut Context<Self>) -> Div {
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .debug_selector(|| "community-catalog".into());
        if self.community.fetching {
            body = body.child("Fetching community index...");
        }
        let Some(index) = &self.community.index else {
            return body;
        };
        let mut count = 0;
        for entry in &index.plugins {
            if !self.library_matches(
                LeftTab::Plugins,
                &format!(
                    "{} {} {} {}",
                    entry.id, entry.name, entry.author, entry.description
                ),
            ) {
                continue;
            }
            count += 1;
            let compatibility = entry.compatible();
            let installed = self.community.installed.iter().find(|p| p.id == entry.id);
            let update = installed.is_some_and(|p| entry.newer_than(&p.version));
            let action = entry.clone();
            let mut row = card(format!("catalog-{}", entry.id))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(entry.name.clone()),
                )
                .child(format!("by {} · v{}", entry.author, entry.version))
                .child(entry.description.clone())
                .child(format!("{} · ABI {}", entry.license, entry.min_abi));
            if let Err(reason) = &compatibility {
                row = row.child(
                    div()
                        .text_color(hex(color::WARNING))
                        .child(format!("Incompatible: {reason}")),
                );
            }
            if entry.source_url.starts_with("https://github.com/") {
                let source = entry.source_url.clone();
                row = row.child(
                    Button::new(SharedString::from(format!("source-{}", entry.id)))
                        .small()
                        .ghost()
                        .icon(icons::icon(IconName::ExternalLink))
                        .label("Source")
                        .on_click(move |_, _, cx| cx.open_url(&source)),
                );
            }
            row = row.child(
                Button::new(SharedString::from(format!("install-{}", entry.id)))
                    .small()
                    .debug_selector({
                        let id = entry.id.clone();
                        move || format!("install-{id}")
                    })
                    .label(if update {
                        "Update available"
                    } else if installed.is_some() {
                        "Installed"
                    } else {
                        "Install"
                    })
                    .disabled(
                        self.community.busy
                            || compatibility.is_err()
                            || (installed.is_some() && !update),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.install_community_entry(action.clone(), cx)
                    })),
            );
            body = body.child(row);
        }
        if count == 0 {
            body = body.child("No community plugins match your search.");
        }
        body
    }
}
