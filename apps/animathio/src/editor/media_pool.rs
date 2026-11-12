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
        self.execute(&Command::AddClip { track, clip }, cx);
    }

    /// Start dragging an item from the media pool.
    pub(super) fn arm_pool_drag(&mut self, drag: PoolDrag, cx: &mut Context<Self>) {
        self.pool_drag = Some(drag);
        cx.notify();
    }

    /// Finish a media-pool drag at a window position over the timeline.
    pub(super) fn drop_on_timeline(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
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
            PoolDrag::Effect(kind) => {
                let hit = timeline::hit_test(&layout, &self.project, x, y);
                if let Some(clip) = hit.map(|hit| hit.clip).or(self.selected) {
                    self.add_effect_to_clip(clip, kind, cx);
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
        self.execute(&Command::AddClip { track, clip }, cx);
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

    /// Add a built-in effect to the selected clip's chain.
    pub(super) fn add_effect_to_selected(
        &mut self,
        kind: animathio_core::EffectKind,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected else {
            self.error = Some("select a clip to add an effect".into());
            cx.notify();
            return;
        };
        self.add_effect_to_clip(id, kind, cx);
    }

    /// Add a built-in effect to a specific clip's chain.
    pub(super) fn add_effect_to_clip(
        &mut self,
        id: ClipId,
        kind: animathio_core::EffectKind,
        cx: &mut Context<Self>,
    ) {
        let index = self.find_clip(id).map_or(0, |clip| clip.effects.len());
        let effect =
            animathio_core::EffectInstance::new(animathio_core::EffectId(self.next_id), kind);
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
                let kinds = [
                    animathio_core::EffectKind::Grayscale,
                    animathio_core::EffectKind::Sepia,
                    animathio_core::EffectKind::Invert,
                    animathio_core::EffectKind::Saturate,
                    animathio_core::EffectKind::Brightness,
                    animathio_core::EffectKind::Contrast,
                    animathio_core::EffectKind::Blur,
                    animathio_core::EffectKind::Opacity,
                ];
                for kind in kinds {
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
                            .child(effect_label(kind))
                            .id("effect-item")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.add_effect_to_selected(kind, cx);
                            }))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                    this.arm_pool_drag(PoolDrag::Effect(kind), cx);
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
                this.import_paths(paths.paths(), None, cx)
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
            PoolDrag::Effect(kind) => (IconName::Sparkles, effect_label(*kind).to_owned()),
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
