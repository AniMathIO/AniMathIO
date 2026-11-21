//! Community plugins in the running editor: the global switch and hot load/unload.
//!
//! The trust settings live in `community-plugins.json` (see `animathio_plugin_host::community`)
//! and are shared with `animathio-cli`. Changing them reconciles the plugin host in place:
//! newly enabled plugins load, disabled ones unload, the inspector's plugin list refreshes and
//! the preview re-renders, so clips of an unloaded plugin show the missing-plugin placeholder
//! and clips of a newly loaded one render, all without a restart.

use super::*;
use crate::style::{self, color, hex};
use animathio_plugin_host::{CommunityStore, InstalledPlugin};
use gpui_component::Disableable;
pub(in crate::editor) mod registry;
use registry::{Downloader, Entry, Index, IndexFetcher};

pub(super) struct Browser {
    pub installed: Vec<InstalledPlugin>,
    pub index: Option<Index>,
    pub browsing: bool,
    pub fetching: bool,
    pub busy: bool,
    inventory_revision: u64,
    pub error: Option<String>,
    /// Plugin whose Remove is waiting for the user's confirmation.
    pub confirm_remove: Option<String>,
    /// Plugins installed from the registry in this session: their SHA-256 was verified.
    pub verified: BTreeSet<String>,
    /// Registry entries of plugins installed in this session (author, source link).
    pub origins: BTreeMap<String, Entry>,
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
            inventory_revision: 0,
            error: None,
            confirm_remove: None,
            verified: BTreeSet::new(),
            origins: BTreeMap::new(),
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
        if self.community.busy {
            return Err("Wait for the current plugin operation to finish".into());
        }
        let store = self
            .community_store()
            .ok_or("no per-user directory is available for community plugins")?;
        store.set_enabled(enabled).map_err(|e| e.to_string())?;
        if !enabled {
            self.community.browsing = false;
            self.community.confirm_remove = None;
        }
        self.sync_community_plugins(cx);
        self.refresh_community_inventory(cx);
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
        self.community.inventory_revision = self.community.inventory_revision.wrapping_add(1);
        let revision = self.community.inventory_revision;
        cx.spawn(async move |this, cx| {
            let (installed, problems) = cx
                .background_executor()
                .spawn(async move { store.list() })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.community.busy || this.community.inventory_revision != revision {
                    return;
                }
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
                            Some(format!("Could not fetch community plugins: {error}"));
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
        self.community_job_then(job, |_| {}, cx);
    }
    /// Run `job` on a worker; `on_ok` updates the panel state when it succeeded.
    fn community_job_then(
        &mut self,
        job: impl FnOnce(CommunityStore) -> Result<String, String> + Send + 'static,
        on_ok: impl FnOnce(&mut Browser) + 'static,
        cx: &mut Context<Self>,
    ) {
        if self.community.busy {
            return;
        }
        let Some(store) = self.community_store() else {
            return;
        };
        self.community.busy = true;
        self.community.inventory_revision = self.community.inventory_revision.wrapping_add(1);
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
                    Ok(message) => {
                        on_ok(&mut this.community);
                        this.notices.push(crate::style::Toast::info(message));
                    }
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
        let origin = entry.clone();
        self.community_job_then(
            move |store| {
                let installed = entry.install(&store, &download, update)?;
                Ok(if installed.replaced.is_some() {
                    format!(
                        "Updated {} {} (SHA-256 verified).",
                        installed.name, installed.version
                    )
                } else {
                    format!(
                        "Installed {} {} (SHA-256 verified). Choose Enable when ready.",
                        installed.name, installed.version
                    )
                })
            },
            move |browser| {
                browser.verified.insert(origin.id.clone());
                browser.origins.insert(origin.id.clone(), origin);
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
        self.community.confirm_remove = None;
        let forget = id.clone();
        self.community_job_then(
            move |store| {
                store.uninstall(&id).map_err(|e| e.to_string())?;
                Ok(format!(
                    "Removed {id}. Clips that use it show a placeholder until it is installed again."
                ))
            },
            move |browser| {
                browser.verified.remove(&forget);
                browser.origins.remove(&forget);
            },
            cx,
        );
    }
    /// Pick a local `.amplugin` and install it (reached from the Library's + button).
    pub(super) fn install_community_file(&mut self, cx: &mut Context<Self>) {
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
    fn bundled_inventory(&self, _cx: &mut Context<Self>) -> Div {
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
/// The one-line trust notice for third-party plugins (wraps instead of clipping).
fn trust_line() -> Div {
    div()
        .flex()
        .items_start()
        .gap_1()
        .text_color(hex(color::TEXT_SECONDARY))
        .child(
            div().pt_px().child(
                icons::icon(IconName::ShieldAlert)
                    .small()
                    .text_color(hex(color::WARNING)),
            ),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .child("Third-party · sandboxed · not audited"),
        )
}

/// The permanent caption under the Community plugins switch.
fn trust_caption(id: &'static str) -> Stateful<Div> {
    trust_line()
        .id(id)
        .debug_selector(move || id.into())
        .text_size(crate::style::type_size::XS)
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
                    .child(
                        div()
                            .flex_1()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Community plugins"),
                    )
                    .child(
                        gpui_component::switch::Switch::new("community-switch")
                            .checked(enabled)
                            .disabled(busy)
                            .accessibility_label("Community plugins")
                            .on_click(cx.listener(move |this, checked: &bool, _, cx| {
                                if let Err(error) = this.set_community_plugins_enabled(*checked, cx)
                                {
                                    this.community.error = Some(error);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(trust_caption("community-trust-caption"));
        if enabled {
            // Installed | Browse only make sense while community plugins are on.
            body = body.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("community-installed")
                            .debug_selector(|| "community-installed".into())
                            .small()
                            .ghost()
                            .label("Installed")
                            .selected(!self.community.browsing)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.community.browsing = false;
                                this.refresh_community_inventory(cx);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("community-browse")
                            .small()
                            .ghost()
                            .label("Browse")
                            .debug_selector(|| "community-browse".into())
                            .selected(self.community.browsing)
                            .disabled(self.community.fetching)
                            .on_click(cx.listener(|this, _, _, cx| this.browse_community(cx))),
                    ),
            );
            if !self.community.browsing {
                body = body.child(card("community-explanation".into()).child(
                    "Runs sandboxed with memory and time limits, and is not audited by AniMathIO. \
                     Check the Source link before you install.",
                ));
            }
        }
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
                        style::primary_button(
                            SharedString::from(format!("missing-install-{id}")),
                            cx,
                        )
                        .small()
                        .label("Install")
                        .disabled(
                            busy || entry.compatible().is_err()
                                || self.community.installed.iter().any(|p| p.id == id),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.install_community_entry(entry.clone(), cx);
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
        if enabled {
            body = body.child(div().font_weight(FontWeight::SEMIBOLD).child("Installed"));
            if self.community.installed.is_empty() {
                body = body.child(
                    div()
                        .id("community-empty")
                        .debug_selector(|| "community-empty".into())
                        .text_size(crate::style::type_size::XS)
                        .text_color(hex(color::TEXT_SECONDARY))
                        .child(
                            "No community plugins yet. Choose Browse to find some, or use + at \
                             the top of the Library to install a .amplugin file.",
                        ),
                );
            }
        }
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
            body = body.child(self.community_installed_card(plugin, enabled, cx));
        }
        body.child(self.bundled_inventory(cx))
    }

    /// An installed community plugin, laid out like the first-party cards: icon, name, version,
    /// id, kinds, author and Source link, then its own controls.
    fn community_installed_card(
        &self,
        plugin: &InstalledPlugin,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let busy = self.community.busy;
        let id = plugin.id.clone();
        let remove = id.clone();
        let on = plugin.enabled;
        let origin = self.community.origins.get(&plugin.id).or_else(|| {
            self.community
                .index
                .as_ref()
                .and_then(|i| i.plugins.iter().find(|e| e.id == plugin.id))
        });
        let verified = self.community.verified.contains(&plugin.id);
        let kinds = plugin.kinds.join(", ");
        let mut details = format!("{kinds} · Community · ABI {}", plugin.api_version);
        if let Some(origin) = origin {
            details = format!("{details} · by {}", origin.author);
        }
        let mut row = card(format!("installed-{id}"))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(icons::icon(IconName::Puzzle).small())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(plugin.name.clone()),
                    )
                    .child(
                        div()
                            .text_color(hex(color::TEXT_SECONDARY))
                            .child(format!("v{}", plugin.version)),
                    ),
            )
            .child(div().truncate().child(plugin.id.clone()))
            .child(plugin.description.clone())
            .child(div().text_color(hex(color::TEXT_SECONDARY)).child(details))
            .child(if verified {
                div()
                    .id(SharedString::from(format!("verified-{id}")))
                    .debug_selector({
                        let id = id.clone();
                        move || format!("verified-{id}")
                    })
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(hex(color::SUCCESS))
                    .child(icons::icon(IconName::ShieldCheck).small())
                    .child("SHA-256 verified")
                    .into_any_element()
            } else {
                trust_line().into_any_element()
            })
            .child(
                // One state control: the switch labels itself Enabled/Disabled.
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .child(if on && enabled { "Enabled" } else { "Disabled" }),
                    )
                    .child(
                        gpui_component::switch::Switch::new(SharedString::from(format!(
                            "enable-{id}"
                        )))
                        .checked(on)
                        .disabled(busy || !enabled)
                        .accessibility_label(format!("Enable {}", plugin.name))
                        .on_click(cx.listener(
                            move |this, checked: &bool, _, cx| {
                                this.toggle_community_plugin(id.clone(), *checked, cx);
                            },
                        )),
                    ),
            );
        let source = origin
            .map(|e| e.source_url.clone())
            .filter(|url| url.starts_with("https://github.com/"));
        if self.community.confirm_remove.as_deref() == Some(plugin.id.as_str()) {
            let cancel_id = remove.clone();
            row = row.child(
                div()
                    .id(SharedString::from(format!("confirm-remove-{remove}")))
                    .debug_selector({
                        let id = remove.clone();
                        move || format!("confirm-remove-{id}")
                    })
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(format!(
                        "Remove {}? Clips that use it show a placeholder until you install it \
                         again.",
                        plugin.name
                    ))
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new(SharedString::from(format!("remove-confirm-{remove}")))
                                    .small()
                                    .danger()
                                    .icon(icons::icon(IconName::Trash))
                                    .label("Remove plugin")
                                    .disabled(busy)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.remove_community_plugin(remove.clone(), cx);
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!(
                                    "remove-cancel-{cancel_id}"
                                )))
                                .small()
                                .ghost()
                                .label("Cancel")
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.community.confirm_remove = None;
                                        cx.notify();
                                    },
                                )),
                            ),
                    ),
            );
        } else {
            row = row.child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .when_some(source, |actions, url| {
                        actions.child(
                            Button::new(SharedString::from(format!("source-{}", plugin.id)))
                                .small()
                                .ghost()
                                .icon(icons::icon(IconName::ExternalLink))
                                .label("Source")
                                .on_click(move |_, _, cx| cx.open_url(&url)),
                        )
                    })
                    .child(
                        Button::new(SharedString::from(format!("remove-{remove}")))
                            .small()
                            .danger()
                            .icon(icons::icon(IconName::Trash))
                            .label("Remove")
                            .disabled(busy)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.community.confirm_remove = Some(remove.clone());
                                cx.notify();
                            })),
                    ),
            );
        }
        if let Some(index) = &self.community.index {
            if let Some(entry) = index
                .plugins
                .iter()
                .find(|e| e.id == plugin.id && e.newer_than(&plugin.version))
            {
                let entry = entry.clone();
                row = row.child(
                    style::primary_button(SharedString::from(format!("update-{}", entry.id)), cx)
                        .small()
                        .label(format!("Update available · {}", entry.version))
                        .disabled(busy || !enabled || entry.compatible().is_err())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.install_community_entry(entry.clone(), cx);
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
        row
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
                        .flex()
                        .gap_2()
                        .items_center()
                        .child(icons::icon(IconName::Puzzle).small())
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .font_weight(FontWeight::SEMIBOLD)
                                .child(entry.name.clone()),
                        )
                        .child(
                            div()
                                .text_color(hex(color::TEXT_SECONDARY))
                                .child(format!("v{}", entry.version)),
                        ),
                )
                .child(
                    div()
                        .text_color(hex(color::TEXT_SECONDARY))
                        .child(format!("by {}", entry.author)),
                )
                .child(entry.description.clone())
                .child(
                    div()
                        .text_color(hex(color::TEXT_SECONDARY))
                        .child(format!("{} · ABI {}", entry.license, entry.min_abi)),
                );
            if let Err(reason) = &compatibility {
                row = row.child(
                    div()
                        .text_color(hex(color::WARNING))
                        .child(format!("Incompatible: {reason}")),
                );
            }
            let verified = installed.is_some() && self.community.verified.contains(&entry.id);
            row = row.child(if verified {
                div()
                    .id(SharedString::from(format!("catalog-verified-{}", entry.id)))
                    .debug_selector({
                        let id = entry.id.clone();
                        move || format!("catalog-verified-{id}")
                    })
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_color(hex(color::SUCCESS))
                    .child(icons::icon(IconName::ShieldCheck).small())
                    .child("SHA-256 verified")
                    .into_any_element()
            } else {
                trust_line().into_any_element()
            });
            let mut actions = div().flex().flex_wrap().gap_2();
            let install_label = if update {
                "Update available"
            } else if installed.is_some() {
                "Installed"
            } else {
                "Install"
            };
            let install_disabled =
                self.community.busy || compatibility.is_err() || (installed.is_some() && !update);
            let install_id = SharedString::from(format!("install-{}", entry.id));
            let install_selector = format!("install-{}", entry.id);
            // Install is the primary action while it can be taken; afterwards it is a plain
            // disabled "Installed" label.
            let install = if install_disabled {
                Button::new(install_id)
            } else {
                style::primary_button(install_id, cx)
            };
            actions = actions.child(
                install
                    .small()
                    .debug_selector(move || install_selector.clone())
                    .label(install_label)
                    .disabled(install_disabled)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.install_community_entry(action.clone(), cx);
                    })),
            );
            if installed.is_some_and(|p| !p.enabled && p.version == entry.version) {
                let id = entry.id.clone();
                let selector = format!("catalog-enable-{id}");
                actions = actions.child(
                    style::primary_button(SharedString::from(selector.clone()), cx)
                        .debug_selector(move || selector.clone())
                        .small()
                        .label("Enable")
                        .disabled(self.community.busy || compatibility.is_err())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.toggle_community_plugin(id.clone(), true, cx);
                        })),
                );
            }
            if entry.source_url.starts_with("https://github.com/") {
                let source = entry.source_url.clone();
                actions = actions.child(
                    Button::new(SharedString::from(format!("source-{}", entry.id)))
                        .small()
                        .ghost()
                        .icon(icons::icon(IconName::ExternalLink))
                        .label("Source")
                        .on_click(move |_, _, cx| cx.open_url(&source)),
                );
            }
            body = body.child(row.child(actions));
        }
        if count == 0 {
            body = body.child("No community plugins match your search.");
        }
        body
    }
}

#[cfg(test)]
mod tests;

impl Editor {
    pub(super) fn missing_plugins_notice(&self, cx: &mut Context<Self>) -> Div {
        let ids = self.missing_plugin_ids();
        let names = ids.iter().cloned().collect::<Vec<_>>().join(", ");
        let mut notice = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_2()
            .p_2()
            .text_size(crate::style::type_size::XS)
            .bg(hex(color::RAISED))
            .debug_selector(|| "missing-plugins-notice".into())
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(format!("Missing plugins: {names}")),
            );
        if self.community_enabled()
            && let Some(index) = &self.community.index
        {
            for entry in index.plugins.iter().filter(|p| ids.contains(&p.id)).take(3) {
                let entry = entry.clone();
                notice = notice.child(
                    Button::new(SharedString::from(format!("notice-install-{}", entry.id)))
                        .small()
                        .label(format!("Install {}", entry.name))
                        .disabled(
                            self.community.busy
                                || entry.compatible().is_err()
                                || self.community.installed.iter().any(|p| p.id == entry.id),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.install_community_entry(entry.clone(), cx);
                        })),
                );
            }
        }
        notice.child(
            Button::new("missing-manage-plugins")
                .small()
                .ghost()
                .label("Manage plugins")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.dock.left_open = true;
                    this.select_left_tab(LeftTab::Plugins, cx);
                })),
        )
    }
}
