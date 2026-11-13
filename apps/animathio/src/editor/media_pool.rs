//! Media pool.

use super::*;

impl Editor {
    pub(super) fn add_asset_at_playhead(&mut self, asset_id: AssetId, cx: &mut Context<Self>) {
        let Some(asset) = self.project.assets().get(&asset_id).cloned() else {
            return;
        };
        let kind = if asset.media_type.starts_with("video/") {
            ClipKind::Video(asset_id.clone())
        } else if asset.media_type.starts_with("image/") {
            ClipKind::Image(asset_id.clone())
        } else if asset.media_type.starts_with("audio/") {
            ClipKind::Audio(asset_id.clone())
        } else {
            self.error = Some(format!("cannot place {}", asset.media_type));
            cx.notify();
            return;
        };
        let Some(track) = self.project.tracks().first().map(Track::id) else {
            self.error = Some("no track to place the clip".into());
            cx.notify();
            return;
        };
        let start = self.position;
        let Some(end) = start.0.checked_add(DEFAULT_MEDIA_TICKS) else {
            return;
        };
        let Ok(range) = TimeRange::new(start, Time(end)) else {
            return;
        };
        let clip = Clip::new(self.next_clip_id(), kind, range);
        self.place_media_clip(track, clip, cx);
    }

    /// Start dragging an item from the media pool.
    pub(super) fn arm_pool_drag(&mut self, drag: PoolDrag, cx: &mut Context<Self>) {
        self.pool_drag = Some(drag);
        cx.notify();
    }

    /// Finish a media-pool drag at a window position over the timeline.
    pub(super) fn drop_on_timeline(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        self.snap_line = None;
        let Some(drag) = self.pool_drag.take() else {
            return;
        };
        let (x, y) = self.timeline_relative(position);
        let layout = self.current_layout();
        // Releasing outside the track area cancels the drag.
        if y < layout.ruler_height || !self.timeline_bounds.contains(&position) {
            cx.notify();
            return;
        }
        let time = timeline::drop_start(&layout, &self.project, x, self.position, self.snapping);
        let row = ((y - layout.ruler_height) / layout.row_height.max(1.0)) as usize;
        let track = self
            .project
            .tracks()
            .get(row)
            .map(Track::id)
            .or_else(|| self.project.tracks().first().map(Track::id));
        let Some(track) = track else {
            self.error = Some("no track to place the clip".into());
            cx.notify();
            return;
        };
        match drag {
            PoolDrag::Asset(asset_id) => self.drop_asset(asset_id, track, time, cx),
            PoolDrag::Plugin(plugin_id) => self.drop_plugin(plugin_id, track, time, cx),
            PoolDrag::Effect { plugin, effect } => {
                let hit = timeline::hit_test(&layout, &self.project, x, y);
                if let Some(clip) = hit.map(|hit| hit.clip).or(self.selected) {
                    self.add_effect_to_clip(clip, &plugin, &effect, cx);
                } else {
                    self.error = Some("drop an effect on a clip".into());
                    cx.notify();
                }
            }
        }
    }

    /// Place an asset clip at an explicit timeline position and track.
    pub(super) fn drop_asset(
        &mut self,
        asset_id: AssetId,
        track: TrackId,
        time: Time,
        cx: &mut Context<Self>,
    ) {
        let Some(asset) = self.project.assets().get(&asset_id).cloned() else {
            return;
        };
        let kind = if asset.media_type.starts_with("video/") {
            ClipKind::Video(asset_id.clone())
        } else if asset.media_type.starts_with("image/") {
            ClipKind::Image(asset_id.clone())
        } else if asset.media_type.starts_with("audio/") {
            ClipKind::Audio(asset_id.clone())
        } else {
            self.error = Some(format!("cannot place {}", asset.media_type));
            cx.notify();
            return;
        };
        let Some(end) = time.0.checked_add(DEFAULT_MEDIA_TICKS) else {
            return;
        };
        let Ok(range) = TimeRange::new(time, Time(end)) else {
            return;
        };
        let clip = Clip::new(self.next_clip_id(), kind, range);
        self.place_media_clip(track, clip, cx);
    }

    fn place_media_clip(&mut self, track: TrackId, mut clip: Clip, cx: &mut Context<Self>) {
        use animathio_media::MediaBackend;
        let mut commands = Vec::new();
        if matches!(clip.kind, ClipKind::Audio(_)) {
            clip.params.insert("gain".into(), 1.0.into());
            clip.params.insert("muted".into(), 0.0.into());
        }
        if let ClipKind::Video(id) = &clip.kind
            && let Some(bytes) = self.assets.get(id)
            && let Ok(info) =
                animathio_media::FilmCraftBackend::new().probe_bytes("media", bytes.clone().into())
            && !info.audio.is_empty()
        {
            let audio_track = TrackId(
                self.project
                    .tracks()
                    .iter()
                    .map(|t| t.id().0)
                    .max()
                    .unwrap_or(0)
                    + 1,
            );
            clip.link_group = Some(clip.id);
            let mut audio = Clip::new(
                ClipId(clip.id.0 + 1),
                ClipKind::Audio(id.clone()),
                clip.range,
            );
            audio.link_group = clip.link_group;
            audio.params.insert("gain".into(), 1.0.into());
            audio.params.insert("muted".into(), 0.0.into());
            commands.push(Command::AddTrack {
                track: Track::new(audio_track, "Audio"),
                index: self.project.tracks().len(),
            });
            commands.push(Command::AddClip {
                track: audio_track,
                clip: audio,
            });
        }
        commands.push(Command::AddClip { track, clip });
        self.execute(&Command::Batch { commands }, cx);
    }

    /// Place a plugin clip at an explicit timeline position and track.
    pub(super) fn drop_plugin(
        &mut self,
        plugin_id: String,
        track: TrackId,
        time: Time,
        cx: &mut Context<Self>,
    ) {
        let Some(descriptor) = self.plugins.iter().find(|plugin| plugin.id == plugin_id) else {
            return;
        };
        let models = descriptor.models();
        let (numeric, opaque) = inspector::default_params(&models);
        let plugin_id = descriptor.id.clone();
        let Some(end) = time.0.checked_add(DEFAULT_MEDIA_TICKS) else {
            return;
        };
        let Ok(range) = TimeRange::new(time, Time(end)) else {
            return;
        };
        let mut clip = Clip::new(
            self.next_clip_id(),
            ClipKind::Plugin {
                plugin_id,
                params: opaque,
            },
            range,
        );
        clip.params = numeric;
        self.execute(&Command::AddClip { track, clip }, cx);
    }

    /// Effects offered by the loaded effect plugins, in plugin then listing order.
    pub(super) fn available_effects(&self) -> Vec<animathio_plugin_host::EffectDescriptor> {
        self.plugin_host
            .as_ref()
            .map(PluginHost::effects)
            .unwrap_or_default()
    }

    /// Display name for an effect, falling back to its id when the plugin is not loaded.
    pub(super) fn effect_label(&self, plugin: &str, effect: &str) -> String {
        self.available_effects()
            .into_iter()
            .find(|e| e.plugin_id == plugin && e.effect_id == effect)
            .map_or_else(|| effect.to_owned(), |e| e.name)
    }

    /// Add a plugin effect to the selected clip's chain.
    pub(super) fn add_effect_to_selected(
        &mut self,
        plugin: &str,
        effect: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected else {
            self.error = Some("select a clip to add an effect".into());
            cx.notify();
            return;
        };
        self.add_effect_to_clip(id, plugin, effect, cx);
    }

    /// Add a plugin effect, with the plugin's default parameters, to a clip's chain.
    pub(super) fn add_effect_to_clip(
        &mut self,
        id: ClipId,
        plugin: &str,
        effect: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(descriptor) = self
            .available_effects()
            .into_iter()
            .find(|e| e.plugin_id == plugin && e.effect_id == effect)
        else {
            self.error = Some(format!("effect {plugin}/{effect} is not available"));
            cx.notify();
            return;
        };
        let index = self.find_clip(id).map_or(0, |clip| clip.effects.len());
        let effect = descriptor.instance(animathio_core::EffectId(self.next_id));
        self.next_id = self.next_id.saturating_add(1);
        self.execute(&Command::AddEffect { id, index, effect }, cx);
    }

    /// Split a specific clip at the playhead.
    pub(super) fn add_plugin_clip(
        &mut self,
        descriptor: &PluginDescriptor,
        cx: &mut Context<Self>,
    ) {
        let models = descriptor.models();
        let (numeric, opaque) = inspector::default_params(&models);
        let Some(track) = self.project.tracks().first() else {
            self.error = Some("no track to place the clip".into());
            cx.notify();
            return;
        };
        let track = track.id();
        let start = self.position;
        let duration = DEFAULT_MEDIA_TICKS;
        let Some(end) = start.0.checked_add(duration) else {
            self.error = Some("clip end overflow".into());
            cx.notify();
            return;
        };
        let Ok(range) = TimeRange::new(start, Time(end)) else {
            self.error = Some("invalid clip range".into());
            cx.notify();
            return;
        };
        let mut clip = Clip::new(
            self.next_clip_id(),
            ClipKind::Plugin {
                plugin_id: descriptor.id.clone(),
                params: opaque,
            },
            range,
        );
        clip.params = numeric;
        self.execute(&Command::AddClip { track, clip }, cx);
    }

    pub(super) fn add_plugin_clip_by_id(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(descriptor) = self.plugins.iter().find(|plugin| plugin.id == id) else {
            return;
        };
        let descriptor = descriptor.clone();
        self.add_plugin_clip(&descriptor, cx);
    }

    pub(super) fn media_pool(&self, cx: &mut Context<Self>) -> Div {
        let tabs = [
            (LeftTab::Assets, "Assets", IconName::Images),
            (LeftTab::Plugins, "Plugins", IconName::Puzzle),
            (LeftTab::Effects, "Effects", IconName::Sparkles),
        ];
        let mut tab_row = div().flex().flex_row().w_full().bg(cx.theme().background);
        for (tab, label, icon) in tabs {
            let selected = self.left_tab == tab;
            tab_row = tab_row.child(
                Button::new(SharedString::from(format!("media-tab-{label}")))
                    .ghost()
                    .compact()
                    .icon(icons::icon(icon))
                    .label(label)
                    .selected(selected)
                    .toggled(selected)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.select_left_tab(tab, cx);
                    })),
            );
        }

        let mut body = div()
            .flex_1()
            .w_full()
            .p_1()
            .id("media-body")
            .overflow_y_scroll();
        match self.left_tab {
            LeftTab::Assets => {
                if self.project.assets().is_empty() {
                    body = body.child(empty_state("Drop media here", cx));
                }
                for (id, asset) in self.project.assets() {
                    let asset_id = id.clone();
                    let drop_id = id.clone();
                    let name = asset.name.clone();
                    let kind = asset_kind_label(&asset.media_type);
                    let icon = if asset.media_type.starts_with("video/") {
                        IconName::Video
                    } else if asset.media_type.starts_with("image/") {
                        IconName::Image
                    } else if asset.media_type.starts_with("audio/") {
                        IconName::AudioWaveform
                    } else {
                        IconName::File
                    };
                    body = body.child(
                        div()
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .hover(|this| this.bg(cx.theme().muted))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(icons::icon(icon).small())
                            .child(div().flex_1().child(name))
                            .child(div().text_color(cx.theme().muted_foreground).child(kind))
                            .id("asset-item")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.add_asset_at_playhead(asset_id.clone(), cx);
                            }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.arm_pool_drag(PoolDrag::Asset(drop_id.clone()), cx);
                                }),
                            ),
                    );
                }
            }
            LeftTab::Plugins => {
                body = body.child(
                    Button::new("write-on-preset")
                        .icon(icons::icon(IconName::PenLine))
                        .label("Write on…")
                        .tooltip("Animate the selected LaTeX clip")
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_write_on(window, cx)),
                        ),
                );
                if self.plugins.is_empty() {
                    body = body.child(empty_state("No clip plugins loaded", cx));
                }
                for descriptor in &self.plugins {
                    let id = descriptor.id.clone();
                    let drop_id = id.clone();
                    let name = descriptor.name.clone();
                    let icon = if id.contains("latex") || id.contains("math") {
                        IconName::Sigma
                    } else if id.contains("graph") {
                        IconName::ChartNoAxesColumn
                    } else {
                        IconName::Sparkles
                    };
                    body = body.child(
                        div()
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .hover(|this| this.bg(cx.theme().muted))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(icons::icon(icon).small())
                            .child(name)
                            .id("plugin-item")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.add_plugin_clip_by_id(&id, cx);
                            }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.arm_pool_drag(PoolDrag::Plugin(drop_id.clone()), cx);
                                }),
                            ),
                    );
                }
            }
            LeftTab::Effects => {
                let effects = self.available_effects();
                if effects.is_empty() {
                    body = body.child(empty_state("No effect plugins loaded", cx));
                }
                for descriptor in effects {
                    let (plugin, effect) = (descriptor.plugin_id, descriptor.effect_id);
                    let (click_plugin, click_effect) = (plugin.clone(), effect.clone());
                    body = body.child(
                        div()
                            .px_2()
                            .py_1()
                            .cursor_pointer()
                            .hover(|this| this.bg(cx.theme().muted))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(icons::icon(IconName::Sparkles).small())
                            .child(descriptor.name)
                            .id(SharedString::from(format!("effect-item-{plugin}-{effect}")))
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.add_effect_to_selected(&click_plugin, &click_effect, cx);
                            }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.arm_pool_drag(
                                        PoolDrag::Effect {
                                            plugin: plugin.clone(),
                                            effect: effect.clone(),
                                        },
                                        cx,
                                    );
                                }),
                            ),
                    );
                }
            }
        }
        div()
            .key_context("MediaPool")
            .track_focus(&self.panel_focus[3])
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.panel_focus[3], cx)),
            )
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .bg(cx.theme().popover)
            .on_drop::<ExternalPaths>(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.import_paths(paths.paths(), None, cx);
            }))
            .child(tab_row)
            .child(body)
    }

    pub(super) fn pool_drag_ghost(&self, cx: &mut Context<Self>) -> Option<Div> {
        let drag = self.pool_drag.as_ref()?;
        let (icon, label) = match drag {
            PoolDrag::Asset(id) => (
                IconName::Film,
                self.project
                    .assets()
                    .get(id)
                    .map_or_else(|| "media".to_owned(), |asset| asset.name.clone()),
            ),
            PoolDrag::Plugin(id) => (
                IconName::Sparkles,
                self.plugins
                    .iter()
                    .find(|plugin| &plugin.id == id)
                    .map_or_else(|| id.clone(), |plugin| plugin.name.clone()),
            ),
            PoolDrag::Effect { plugin, effect } => {
                (IconName::Sparkles, self.effect_label(plugin, effect))
            }
        };
        let over_timeline = self.timeline_bounds.contains(&self.pool_drag_pos);
        Some(
            div()
                .absolute()
                .left(self.pool_drag_pos.x + px(12.))
                .top(self.pool_drag_pos.y + px(12.))
                .px_2()
                .py_1()
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .bg(cx.theme().popover)
                .border_1()
                .border_color(if over_timeline {
                    cx.theme().drag_border
                } else {
                    cx.theme().border
                })
                .child(icons::icon(icon).small())
                .child(label),
        )
    }
}
