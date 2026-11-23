//! Community plugins in the running editor: the global switch and hot load/unload.
//!
//! The trust settings live in `community-plugins.json` (see `animathio_plugin_host::community`)
//! and are shared with `animathio-cli`. Changing them reconciles the plugin hosts in place:
//! newly enabled plugins load, disabled ones unload, the inspector's plugin list refreshes and
//! the preview re-renders, so clips of an unloaded plugin show the missing-plugin placeholder
//! and clips of a newly loaded one render, all without a restart.

use super::tool_panel;
use super::*;
use crate::style::{self, SegmentedOption, TextRole as _, color, hex};
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
    /// Cards whose Details are open (`installed:<id>` or `catalog:<id>`).
    pub details_open: BTreeSet<String>,
    /// Keyboard focus for the Installed | Browse choice, created on first render.
    pub tabs_focus: std::cell::OnceCell<Vec<FocusHandle>>,
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
            details_open: BTreeSet::new(),
            tabs_focus: std::cell::OnceCell::new(),
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

    /// Reconcile the hosts with the saved settings (the CLI may have changed them), then
    /// refresh the plugin list and render a frame against the reconciled preview host.
    pub(super) fn sync_community_plugins(&mut self, cx: &mut Context<Self>) {
        let Some(store) = self.community_store() else {
            return;
        };
        if let Some(host) = &mut self.plugin_host {
            let report = host.sync_community_plugins(&store);
            if report.changed() {
                self.plugins = super::support::plugin_descriptors(host);
                self.refresh_playback_effects();
            }
            for problem in report.problems {
                self.notices.push(crate::style::Toast::warning(problem));
            }
        }
        // Each background host can have a different loaded set (e.g. after a failed load).
        // Queue reconciliation before the frame even when the UI host was already current.
        self.worker.sync_community_plugins(store.clone());
        self.math_library.preview_host.sync_community_plugins(store);
        let time = self.current_render_time();
        self.request_render(time);
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

/// Width of the label column in a card's Details.
const DETAIL_LABEL_WIDTH: gpui::Rems = gpui::Rems(4.);

/// A caption line (`Caption`, `TEXT_MUTED`).
fn caption(text: impl Into<SharedString>) -> Div {
    div()
        .text_role(style::Role::Caption)
        .text_color(hex(color::TEXT_MUTED))
        .child(text.into())
}

/// A text link: accent text on a ghost button (`Source`, `Details`).
fn link_button(id: impl Into<ElementId>, label: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .child(div().text_color(hex(color::ACCENT)).child(label))
}

/// The card frame shared by installed, built-in and Browse cards.
fn card(id: String) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.clone()))
        .debug_selector(move || id.clone())
        .p_3()
        .rounded(px(style::radius::CARD))
        .border_1()
        .border_color(hex(color::BORDER))
        .bg(hex(color::RAISED))
        .text_role(style::Role::Label)
        .flex()
        .flex_col()
        .gap_1()
}

/// The first row of a card: the plugin icon, the name and, at the right, the state or action.
fn card_title(name: impl Into<SharedString>, trailing: impl IntoElement) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .child(
            icons::icon(IconName::Puzzle)
                .small()
                .text_color(hex(color::TEXT_SECONDARY)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_role(style::Role::Heading)
                .child(name.into()),
        )
        .child(trailing)
}

/// The rows behind a card's Details link: `(debug id suffix, label, value)`. The plugin id,
/// the package file and the ABI live here instead of on the card face.
fn detail_lines(key: &str, lines: Vec<(&'static str, &'static str, String)>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .pt_1()
        .children(lines.into_iter().map(|(suffix, label, value)| {
            let id = format!("plugin-{suffix}-{key}");
            let tooltip = value.clone();
            div()
                .id(SharedString::from(id.clone()))
                .debug_selector(move || id.clone())
                .flex()
                .gap_2()
                .text_role(style::Role::Caption)
                .text_color(hex(color::TEXT_SECONDARY))
                .child(
                    div()
                        .w(DETAIL_LABEL_WIDTH)
                        .flex_shrink_0()
                        .text_color(hex(color::TEXT_MUTED))
                        .child(label),
                )
                .child(div().flex_1().min_w_0().truncate().child(value))
                .tooltip(move |window, cx| {
                    gpui_component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx)
                })
        }))
}

/// "Community plugins run sandboxed and are not reviewed." (one sentence, wraps instead of
/// clipping).
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
                .child("Community plugins run sandboxed and are not reviewed."),
        )
}

/// The permanent caption under the Community plugins switch.
fn trust_caption(id: &'static str) -> Stateful<Div> {
    trust_line()
        .id(id)
        .debug_selector(move || id.into())
        .text_role(style::Role::Label)
        .text_color(hex(color::TEXT_SECONDARY))
}

/// "SHA-256 verified" for a plugin installed from the registry in this session.
fn verified_line(id: String) -> Stateful<Div> {
    div()
        .id(SharedString::from(id.clone()))
        .debug_selector(move || id.clone())
        .flex()
        .items_center()
        .gap_1()
        .text_color(hex(color::SUCCESS))
        .child(icons::icon(IconName::ShieldCheck).xsmall())
        .child("SHA-256 verified")
}

impl Editor {
    /// Show or hide the Details of the card `key` (`installed:<id>` or `catalog:<id>`).
    fn toggle_details(&mut self, key: String, cx: &mut Context<Self>) {
        if !self.community.details_open.remove(&key) {
            self.community.details_open.insert(key);
        }
        cx.notify();
    }

    /// The "Details" link that opens and closes the disclosure under a card.
    fn details_link(&self, key: String, cx: &mut Context<Self>) -> Button {
        let open = self.community.details_open.contains(&key);
        let id = SharedString::from(format!("details-{key}"));
        let selector = id.to_string();
        link_button(id, if open { "Hide details" } else { "Details" })
            .debug_selector(move || selector.clone())
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_details(key.clone(), cx)))
    }

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
            let built_in = id.starts_with("org.animathio.");
            let origin = if built_in { "Built in" } else { "Third-party" };
            if !self.library_matches(
                LeftTab::Plugins,
                &format!(
                    "{} {} {} {} {} {}",
                    id, plugin.manifest.name, plugin.manifest.version, kinds, source_full, origin
                ),
            ) {
                continue;
            }
            let key = format!("installed:{id}");
            let open = self.community.details_open.contains(&key);
            let name = plugin.manifest.name.clone();
            let mut card = card(format!("installed-{id}"))
                .child(card_title(
                    plugin.manifest.name.clone(),
                    // Built-in plugins are always on; the switch shows the state.
                    div()
                        .id(SharedString::from(format!("builtin-note-{id}")))
                        .opacity(0.6)
                        .tooltip(|window, cx| {
                            gpui_component::tooltip::Tooltip::new("Built-in plugins are always on")
                                .build(window, cx)
                        })
                        .child(
                            gpui_component::switch::Switch::new(SharedString::from(format!(
                                "builtin-{id}"
                            )))
                            .checked(true)
                            .disabled(true)
                            .accessibility_label(format!("{name} is always on")),
                        ),
                ))
                .child(div().text_color(hex(color::TEXT_SECONDARY)).child(kinds))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().flex_1().min_w_0().child(caption(format!(
                            "{origin}, version {}",
                            plugin.manifest.version
                        ))))
                        .child(self.details_link(key, cx)),
                );
            if open {
                card = card.child(detail_lines(
                    &id,
                    vec![
                        ("id", "Plugin id", id.clone()),
                        ("source", "File", source),
                        ("abi", "ABI", plugin.manifest.api_version.clone()),
                    ],
                ));
            }
            body = body.child(card);
        }
        for failure in host.load_failures() {
            body = body.child(tool_panel::problem(failure.clone()));
        }
        body
    }
}

impl Editor {
    pub(super) fn plugin_inventory(&self, cx: &mut Context<Self>) -> Div {
        let enabled = self.community_enabled();
        let busy = self.community.busy;
        let mut body = div()
            .flex()
            .flex_col()
            .gap_3()
            .p_2()
            // Leave the scrollbar lane free so a card never runs under it.
            .pr_4()
            .debug_selector(|| "plugin-inventory".into())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .text_role(style::Role::Heading)
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
            let focus = self
                .community
                .tabs_focus
                .get_or_init(|| tool_panel::focus_handles(2, cx));
            let entity = cx.entity();
            body = body.child(tool_panel::choice(
                "community-tabs",
                &[
                    SegmentedOption::new("community-installed", "Installed", None),
                    SegmentedOption::new("community-browse", "Browse", None),
                ],
                focus,
                usize::from(self.community.browsing),
                move |index, _, cx| {
                    entity.update(cx, |this, cx| {
                        if index == 1 {
                            if !this.community.fetching {
                                this.browse_community(cx);
                            }
                        } else {
                            this.community.browsing = false;
                            this.refresh_community_inventory(cx);
                            cx.notify();
                        }
                    });
                },
            ));
            if !self.community.browsing {
                body = body.child(
                    div()
                        .id("community-explanation")
                        .debug_selector(|| "community-explanation".into())
                        .child(caption(
                            "Each plugin runs with memory and time limits. Check its source before you install it.",
                        )),
                );
            }
        }
        if busy {
            body = body.child(tool_panel::note("Installing or updating…"));
        }
        if let Some(error) = &self.community.error {
            body = body.child(
                card("community-error".into())
                    .text_color(hex(color::DANGER))
                    .child(error.clone())
                    .when(enabled && self.community.browsing, |row| {
                        row.child(
                            div().flex().child(
                                style::secondary_button("community-retry", cx)
                                    .small()
                                    .label("Retry")
                                    .disabled(self.community.fetching)
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.browse_community(cx)),
                                    ),
                            ),
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
                "The plugin {id} is not loaded. Its clips show a placeholder until it is enabled."
            ));
            if enabled {
                if let Some(entry) = entry {
                    row = row.child(
                        div().flex().justify_end().child(
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
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.install_community_entry(entry.clone(), cx);
                                },
                            )),
                        ),
                    );
                } else {
                    row = row.child(
                        div().flex().child(
                            link_button(
                                SharedString::from(format!("missing-browse-{id}")),
                                "Find in Browse",
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.browse_community(cx))),
                        ),
                    );
                }
            }
            body = body.child(row);
        }
        if self.community.browsing && enabled {
            return body.child(self.community_catalog(cx));
        }
        if enabled && self.community.installed.is_empty() {
            body = body.child(
                div()
                    .id("community-empty")
                    .debug_selector(|| "community-empty".into())
                    .child(style::empty_state(
                        Some(IconName::Puzzle),
                        "No community plugins installed.",
                        Some(
                            style::secondary_button("community-empty-browse", cx)
                                .small()
                                .label("Browse")
                                .on_click(cx.listener(|this, _, _, cx| this.browse_community(cx)))
                                .into_any_element(),
                        ),
                    )),
            );
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

    /// An installed community plugin, laid out like the built-in cards: icon, name and the
    /// enable switch, the description, the version caption with its Details link, then its
    /// own controls.
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
        let key = format!("installed:{id}");
        let mut caption_text = format!("Community, version {}", plugin.version);
        if let Some(origin) = origin {
            caption_text = format!("{caption_text}, by {}", origin.author);
        }
        let mut row = card(format!("installed-{id}"))
            .child(card_title(
                plugin.name.clone(),
                // The one state control: the switch says whether the plugin is on.
                gpui_component::switch::Switch::new(SharedString::from(format!("enable-{id}")))
                    .checked(on && enabled)
                    .disabled(busy || !enabled)
                    .accessibility_label(format!("Enable {}", plugin.name))
                    .on_click(cx.listener({
                        let id = id.clone();
                        move |this, checked: &bool, _, cx| {
                            this.toggle_community_plugin(id.clone(), *checked, cx);
                        }
                    })),
            ))
            .child(
                div()
                    .text_color(hex(color::TEXT_SECONDARY))
                    .child(plugin.description.clone()),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(div().flex_1().min_w_0().child(caption(caption_text)))
                    .child(self.details_link(key.clone(), cx)),
            );
        if self.community.details_open.contains(&key) {
            row = row.child(detail_lines(
                &id,
                vec![
                    ("id", "Plugin id", plugin.id.clone()),
                    ("kinds", "Kinds", plugin.kinds.join(", ")),
                    ("abi", "ABI", plugin.api_version.clone()),
                ],
            ));
        }
        if verified {
            row = row.child(verified_line(format!("verified-{id}")));
        }
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
                    .gap_2()
                    .pt_1()
                    .child(format!(
                        "Remove {}? Clips that use it show a placeholder until you install it \
                         again.",
                        plugin.name
                    ))
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                style::secondary_button(
                                    SharedString::from(format!("remove-cancel-{cancel_id}")),
                                    cx,
                                )
                                .small()
                                .label("Cancel")
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.community.confirm_remove = None;
                                        cx.notify();
                                    },
                                )),
                            )
                            .child(
                                style::destructive_button(
                                    SharedString::from(format!("remove-confirm-{remove}")),
                                    "Remove plugin",
                                    true,
                                    cx,
                                )
                                .small()
                                .icon(icons::icon(IconName::Trash))
                                .disabled(busy)
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.remove_community_plugin(remove.clone(), cx);
                                    },
                                )),
                            ),
                    ),
            );
        } else {
            let mut update_button = None;
            if let Some(index) = &self.community.index
                && let Some(entry) = index
                    .plugins
                    .iter()
                    .find(|e| e.id == plugin.id && e.newer_than(&plugin.version))
            {
                let entry = entry.clone();
                update_button = Some(
                    style::primary_button(SharedString::from(format!("update-{}", entry.id)), cx)
                        .small()
                        .label(format!("Update to {}", entry.version))
                        .disabled(busy || !enabled || entry.compatible().is_err())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.install_community_entry(entry.clone(), cx);
                        })),
                );
            }
            row =
                row.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(div().flex().items_center().gap_1().when_some(
                            source,
                            |links, url| {
                                links.child(
                                    link_button(
                                        SharedString::from(format!("source-{}", plugin.id)),
                                        "Source",
                                    )
                                    .on_click(move |_, _, cx| cx.open_url(&url)),
                                )
                            },
                        ))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    style::destructive_button(
                                        SharedString::from(format!("remove-{remove}")),
                                        "Remove",
                                        false,
                                        cx,
                                    )
                                    .small()
                                    .disabled(busy)
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.community.confirm_remove = Some(remove.clone());
                                            cx.notify();
                                        },
                                    )),
                                )
                                .children(update_button),
                        ),
                );
        }
        if let Some(index) = &self.community.index
            && let Some(delisted) = index.delisted.iter().find(|d| d.id == plugin.id)
        {
            row = row.child(
                div()
                    .flex()
                    .items_start()
                    .gap_1()
                    .text_color(hex(color::WARNING))
                    .child(
                        div()
                            .pt_px()
                            .child(icons::icon(IconName::TriangleAlert).xsmall()),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(format!("Removed from the registry: {}", delisted.reason)),
                    ),
            );
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
            body = body.child(tool_panel::note("Fetching community index…"));
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
            let key = format!("catalog:{}", entry.id);
            let mut row = card(format!("catalog-{}", entry.id))
                .child(card_title(entry.name.clone(), div()))
                .child(caption(format!(
                    "By {}, version {}",
                    entry.author, entry.version
                )))
                .child(
                    div()
                        .text_color(hex(color::TEXT_SECONDARY))
                        .child(entry.description.clone()),
                );
            if self.community.details_open.contains(&key) {
                row = row.child(detail_lines(
                    &entry.id,
                    vec![
                        ("id", "Plugin id", entry.id.clone()),
                        ("license", "License", entry.license.clone()),
                        ("abi", "ABI", entry.min_abi.clone()),
                    ],
                ));
            }
            if let Err(reason) = &compatibility {
                row = row.child(
                    div()
                        .flex()
                        .items_start()
                        .gap_1()
                        .text_color(hex(color::WARNING))
                        .child(
                            div()
                                .pt_px()
                                .child(icons::icon(IconName::TriangleAlert).xsmall()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(format!("Incompatible: {reason}")),
                        ),
                );
            }
            if installed.is_some() && self.community.verified.contains(&entry.id) {
                row = row.child(verified_line(format!("catalog-verified-{}", entry.id)));
            }
            let install_label = if update {
                "Update"
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
            // disabled "Installed" button.
            let install = if install_disabled {
                style::secondary_button(install_id, cx)
            } else {
                style::primary_button(install_id, cx)
            };
            let mut actions = div().flex().items_center().gap_2().child(
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
            let mut links = div().flex().items_center().gap_1();
            links = links.child(self.details_link(key, cx));
            if entry.source_url.starts_with("https://github.com/") {
                let source = entry.source_url.clone();
                links = links.child(
                    link_button(SharedString::from(format!("source-{}", entry.id)), "Source")
                        .on_click(move |_, _, cx| cx.open_url(&source)),
                );
            }
            body = body.child(
                row.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .pt_1()
                        .child(links)
                        .child(actions),
                ),
            );
        }
        if count == 0 {
            body = body.child(tool_panel::note("No community plugins match your search."));
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
            .text_role(style::Role::Label)
            .text_color(hex(color::TEXT))
            .bg(hex(color::RAISED))
            .debug_selector(|| "missing-plugins-notice".into())
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_1()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div().pt_px().child(
                            icons::icon(IconName::TriangleAlert)
                                .xsmall()
                                .text_color(hex(color::WARNING)),
                        ),
                    )
                    .child(div().flex_1().min_w_0().child(format!(
                        "Some clips need plugins that are not loaded: {names}."
                    ))),
            );
        if self.community_enabled()
            && let Some(index) = &self.community.index
        {
            for entry in index.plugins.iter().filter(|p| ids.contains(&p.id)).take(3) {
                let entry = entry.clone();
                notice = notice.child(
                    style::secondary_button(
                        SharedString::from(format!("notice-install-{}", entry.id)),
                        cx,
                    )
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
            link_button("missing-manage-plugins", "Manage plugins").on_click(cx.listener(
                |this, _, _, cx| {
                    this.dock.left_open = true;
                    this.select_left_tab(LeftTab::Plugins, cx);
                },
            )),
        )
    }
}
