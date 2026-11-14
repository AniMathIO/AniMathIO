//! Media pool.

use super::*;
use crate::style::{color, hex};
use gpui_component::input::{Input, InputEvent, InputState};

#[derive(Default)]
pub(super) struct MediaView {
    list: bool,
    query: String,
    kind: &'static str,
    search: Option<Entity<InputState>>,
    subscription: Option<Subscription>,
    thumbnails: BTreeMap<AssetId, Option<MediaThumbnail>>,
    hovered: Option<(AssetId, usize)>,
}

struct MediaThumbnail {
    frames: Vec<Arc<RenderImage>>,
    detail: String,
    duration: Time,
    peaks: Arc<[f32]>,
}

fn thumbnail_image(image: image::DynamicImage) -> Arc<RenderImage> {
    let mut image = image.thumbnail(320, 180).to_rgba8();
    for pixel in image.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Arc::new(RenderImage::new([image::Frame::new(image)]))
}

fn load_thumbnail(name: &str, media_type: &str, bytes: Vec<u8>) -> MediaThumbnail {
    use animathio_media::MediaBackend;
    let mut result = MediaThumbnail {
        frames: Vec::new(),
        detail: "Preview unavailable".into(),
        duration: Time(0),
        peaks: Arc::from([]),
    };
    if media_type.starts_with("image/") {
        if let Ok(image) = image::load_from_memory(&bytes) {
            result.detail = format!("{} x {}", image.width(), image.height());
            result.frames.push(thumbnail_image(image));
        }
    } else if media_type.starts_with("audio/") {
        use animathio_audio::AudioSource;
        if let Ok(source) = SymphoniaSource::from_bytes_with_limits(
            bytes,
            animathio_audio::DecodeLimits {
                max_duration: Duration::from_secs(600),
                max_samples: 48_000 * 2 * 600,
            },
        ) && let Ok(pcm) = source.decode_all()
        {
            let wave = Waveform::from_pcm(&pcm, 96);
            result.duration = Time(wave.duration_ticks);
            result.peaks = wave.peaks.into();
            result.detail = format!("{} Hz / {} ch", pcm.rate(), pcm.channels());
        }
    } else if media_type.starts_with("video/") {
        let backend = animathio_media::FilmCraftBackend::new();
        let bytes: Arc<[u8]> = bytes.into();
        if let Ok(info) = backend.probe_bytes(name, bytes.clone()) {
            result.duration = info.duration;
            if let Some(video) = info.video {
                result.detail = format!("{} x {}", video.width, video.height);
            }
            if let Ok(decoder) = backend.open_video_bytes(name, bytes) {
                for index in 0..8 {
                    let time = Time((i128::from(info.duration.0) * index / 8) as i64);
                    if let Ok(frame) = decoder.frame_at(time)
                        && let Some(image) = image::RgbaImage::from_raw(
                            frame.width,
                            frame.height,
                            frame.pixels.to_vec(),
                        )
                    {
                        result
                            .frames
                            .push(thumbnail_image(image::DynamicImage::ImageRgba8(image)));
                    }
                }
            }
        }
    }
    result
}

fn media_matches(name: &str, media_type: &str, query: &str, kind: &str) -> bool {
    name.to_lowercase().contains(&query.to_lowercase())
        && (kind.is_empty() || media_type.starts_with(kind))
}

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
        if let PoolDrag::Title(preset) = &drag {
            self.add_title_preset(preset, time, cx);
            return;
        }
        let Some(track) = track else {
            self.error = Some("no track to place the clip".into());
            cx.notify();
            return;
        };
        match drag {
            PoolDrag::Title(_) => unreachable!("titles create their own tracks"),
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
            .map_or_default(PluginHost::effects)
    }

    /// Display name for an effect, falling back to its id when the plugin is not loaded.
    pub(super) fn effect_label(&self, plugin: &str, effect: &str) -> String {
        self.available_effects()
            .into_iter()
            .chain(self.available_audio_effects())
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
        if self.is_audio_effect(plugin, effect) {
            self.add_audio_effect_to_clip(id, plugin, effect, cx);
            return;
        }
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

    pub(super) fn media_pool(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        self.media_view
            .thumbnails
            .retain(|id, _| self.project.assets().contains_key(id));
        if self.media_view.search.is_none() {
            let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search media…"));
            self.media_view.subscription =
                Some(cx.subscribe(&input, |this, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        this.media_view.query = input.read(cx).value().to_string();
                        cx.notify();
                    }
                }));
            self.media_view.search = Some(input);
        }
        // One worker processes missing cards in order, bounded to 128 cached assets.
        let missing: Vec<_> = self
            .project
            .assets()
            .iter()
            .filter(|(id, _)| !self.media_view.thumbnails.contains_key(*id))
            .take(128_usize.saturating_sub(self.media_view.thumbnails.len()))
            .filter(|(id, _)| self.assets.contains_key(*id))
            .map(|(id, asset)| (id.clone(), asset.clone()))
            .collect();
        if !missing.is_empty() {
            for (id, _) in &missing {
                self.media_view.thumbnails.insert(id.clone(), None);
            }
            let assets = self.assets.clone();
            cx.spawn(async move |this, cx| {
                for (id, asset) in missing {
                    let assets = assets.clone();
                    let load_id = id.clone();
                    let thumbnail = cx
                        .background_spawn(async move {
                            load_thumbnail(
                                &asset.name,
                                &asset.media_type,
                                assets.get(&load_id).cloned().unwrap_or_default(),
                            )
                        })
                        .await;
                    if this
                        .update(cx, |this, cx| {
                            if let Some(slot) = this.media_view.thumbnails.get_mut(&id) {
                                *slot = Some(thumbnail);
                                cx.notify();
                            }
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })
            .detach();
        }

        let tab_row = self.library_tabs(cx);

        let mut body = div()
            .flex_1()
            .w_full()
            .p_1()
            .id("media-body")
            .overflow_y_scroll();
        match self.left_tab {
            LeftTab::Titles => {
                body = body.child(self.title_cards(cx));
            }
            LeftTab::Assets => {
                let assets: Vec<_> = self
                    .project
                    .assets()
                    .iter()
                    .filter(|(_, asset)| {
                        media_matches(
                            &asset.name,
                            &asset.media_type,
                            &self.media_view.query,
                            self.media_view.kind,
                        )
                    })
                    .collect();
                if assets.is_empty() {
                    body = body.child(
                        div()
                            .p_4()
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap_3()
                            .child(
                                icons::icon(IconName::FolderOpen)
                                    .text_color(hex(color::TEXT_MUTED)),
                            )
                            .child(div().text_color(hex(color::TEXT)).child(
                                if self.project.assets().is_empty() {
                                    "Build your media library"
                                } else {
                                    "No matching media"
                                },
                            ))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(hex(color::TEXT_SECONDARY))
                                    .child("Drop video, images or audio here"),
                            )
                            .child(
                                Button::new("pool-import")
                                    .primary()
                                    .icon(icons::icon(IconName::Import))
                                    .label("Import media")
                                    .on_click(cx.listener(|this, _, _, cx| this.import_media(cx))),
                            ),
                    );
                }
                let mut cards = div().flex().flex_wrap().gap_2().w_full();
                for (id, asset) in assets {
                    let add_id = id.clone();
                    let drop_id = id.clone();
                    let hover_id = id.clone();
                    let thumb = self.media_view.thumbnails.get(id).and_then(Option::as_ref);
                    let icon = if asset.media_type.starts_with("video/") {
                        IconName::Video
                    } else if asset.media_type.starts_with("image/") {
                        IconName::Image
                    } else {
                        IconName::AudioWaveform
                    };
                    let tint = if asset.media_type.starts_with("video/") {
                        color::CLIP_VIDEO
                    } else if asset.media_type.starts_with("image/") {
                        color::CLIP_IMAGE
                    } else {
                        color::CLIP_AUDIO
                    };
                    let frame_index = self
                        .media_view
                        .hovered
                        .as_ref()
                        .filter(|(hover, _)| hover == id)
                        .map_or(0, |(_, frame)| *frame);
                    let frame = thumb
                        .and_then(|thumb| {
                            thumb
                                .frames
                                .get(frame_index.min(thumb.frames.len().saturating_sub(1)))
                        })
                        .cloned();
                    let hover_bounds =
                        std::rc::Rc::new(std::cell::Cell::new(Bounds::<Pixels>::default()));
                    let paint_bounds = hover_bounds.clone();
                    let mut preview = div()
                        .h(px(if self.media_view.list { 42. } else { 84. }))
                        .w_full()
                        .relative()
                        .overflow_hidden()
                        .bg(hex(color::BG))
                        .flex()
                        .items_center()
                        .justify_center();
                    if let Some(frame) = frame {
                        preview =
                            preview.child(img(frame).size_full().object_fit(ObjectFit::Contain));
                    } else if let Some(thumb) = thumb.filter(|t| !t.peaks.is_empty()) {
                        let peaks = thumb.peaks.clone();
                        preview = preview.child(
                            canvas(
                                |_, _, _| {},
                                move |bounds, (), window, _| {
                                    let width = f32::from(bounds.size.width) / peaks.len() as f32;
                                    for (index, peak) in peaks.iter().enumerate() {
                                        let height =
                                            f32::from(bounds.size.height) * peak.max(0.02) * 0.8;
                                        window.paint_quad(fill(
                                            Bounds::new(
                                                bounds.origin
                                                    + point(
                                                        px(index as f32 * width),
                                                        px((f32::from(bounds.size.height)
                                                            - height)
                                                            / 2.),
                                                    ),
                                                size(px((width - 1.).max(1.)), px(height)),
                                            ),
                                            hex(color::CLIP_AUDIO),
                                        ));
                                    }
                                },
                            )
                            .size_full(),
                        );
                    } else {
                        preview = preview.child(icons::icon(icon).text_color(hex(tint)));
                    }
                    let detail =
                        thumb.map_or_else(|| "Loading preview…".into(), |t| t.detail.clone());
                    let duration = thumb.map_or(Time(0), |t| t.duration);
                    preview = preview
                        .child(
                            div()
                                .absolute()
                                .top(px(4.))
                                .left(px(4.))
                                .px_1()
                                .rounded(px(3.))
                                .bg(hex(color::RAISED))
                                .text_size(px(9.))
                                .text_color(hex(tint))
                                .child(asset_kind_label(&asset.media_type)),
                        )
                        .when(duration.0 > 0, |p| {
                            p.child(
                                div()
                                    .absolute()
                                    .bottom(px(4.))
                                    .right(px(4.))
                                    .px_1()
                                    .bg(hex(color::RAISED))
                                    .text_size(px(10.))
                                    .child(timecode::format_timecode(
                                        duration,
                                        self.project.frame_rate(),
                                    )),
                            )
                        });
                    preview = preview.child(
                        canvas(
                            move |bounds, _, _| {
                                paint_bounds.set(bounds);
                            },
                            |_, (), _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    );
                    let card = div()
                        .id(SharedString::from(format!("asset-{}", id.as_str())))
                        .rounded(px(6.))
                        .overflow_hidden()
                        .border_1()
                        .border_color(hex(color::BORDER))
                        .bg(hex(color::RAISED))
                        .hover(|s| s.border_color(hex(color::ACCENT)).bg(hex(color::HOVER)))
                        .cursor_pointer()
                        .when(self.media_view.list, Styled::w_full)
                        .when(!self.media_view.list, |s| {
                            s.flex_1().min_w(px(104.)).max_w(px(176.))
                        })
                        .child(preview)
                        .child(
                            div()
                                .p_2()
                                .text_size(px(11.))
                                .child(
                                    div()
                                        .truncate()
                                        .text_color(hex(color::TEXT))
                                        .child(asset.name.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(10.))
                                        .text_color(hex(color::TEXT_SECONDARY))
                                        .child(detail),
                                ),
                        )
                        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                            // Scrub by horizontal position within the card; no continuous timer.
                            let bounds = hover_bounds.get();
                            let fraction = f32::from(event.position.x - bounds.origin.x)
                                / f32::from(bounds.size.width).max(1.);
                            let frame = (fraction.clamp(0., 0.999) * 8.) as usize;
                            let next = Some((hover_id.clone(), frame.min(7)));
                            if this.media_view.hovered != next {
                                this.media_view.hovered = next;
                                cx.notify();
                            }
                        }))
                        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                            if !hovered {
                                this.media_view.hovered = None;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.add_asset_at_playhead(add_id.clone(), cx);
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.arm_pool_drag(PoolDrag::Asset(drop_id.clone()), cx);
                            }),
                        );
                    cards = cards.child(card);
                }
                body = body.child(cards);
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
                let mut cards = div().flex().flex_wrap().gap_2().w_full().pt_1();
                for descriptor in &self.plugins {
                    let id = descriptor.id.clone();
                    let drop_id = id.clone();
                    let name = descriptor.name.clone();
                    let icon = if id.contains("latex") || id.contains("math") {
                        IconName::Sigma
                    } else if id.contains("graph") {
                        IconName::ChartNoAxesColumn
                    } else if id.contains("shape") {
                        IconName::Shapes
                    } else if id.contains("text") {
                        IconName::Type
                    } else {
                        IconName::Puzzle
                    };
                    cards = cards.child(
                        div()
                            .id(SharedString::from(format!("plugin-{id}")))
                            .flex_1()
                            .min_w(px(104.))
                            .max_w(px(176.))
                            .rounded(px(6.))
                            .overflow_hidden()
                            .border_1()
                            .border_color(hex(color::BORDER))
                            .bg(hex(color::RAISED))
                            .cursor_pointer()
                            .hover(|s| s.border_color(hex(color::ACCENT)).bg(hex(color::HOVER)))
                            .child(
                                div()
                                    .h(px(64.))
                                    .bg(hex(color::BG))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(
                                        icons::icon(icon)
                                            .size(px(26.))
                                            .text_color(hex(color::CLIP_PLUGIN)),
                                    ),
                            )
                            .child(
                                div()
                                    .p_2()
                                    .text_size(px(11.))
                                    .truncate()
                                    .text_color(hex(color::TEXT))
                                    .child(name),
                            )
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
                body = body.child(cards);
            }
            LeftTab::Effects => {
                let effects = self.available_effects();
                let audio_effects = self.available_audio_effects();
                let animations = self.animation_library_rows(cx);
                if effects.is_empty() && audio_effects.is_empty() && animations.is_empty() {
                    body = body.child(empty_state("No effect plugins loaded", cx));
                }
                let group = |label: &'static str| {
                    div()
                        .px_2()
                        .pt_2()
                        .pb_1()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(hex(color::TEXT_MUTED))
                        .child(label)
                };
                if !effects.is_empty() {
                    body = body.child(group("Video effects"));
                }
                let audio_start = effects.len();
                for (position, descriptor) in effects.into_iter().chain(audio_effects).enumerate() {
                    let audio = position >= audio_start;
                    if audio && position == audio_start {
                        body = body.child(group("Audio effects"));
                    }
                    let icon = if audio {
                        IconName::AudioWaveform
                    } else {
                        IconName::Sparkles
                    };
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
                            .child(icons::icon(icon).small())
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
                body = body.children(animations);
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
            .bg(hex(color::PANEL))
            .child(tab_row)
            .when(self.left_tab != LeftTab::Titles, |pane| {
                pane.child(
                    div()
                        .px_2()
                        .py_2()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div().flex_1().min_w_0().child(
                                Input::new(self.media_view.search.as_ref().expect("search state"))
                                    .small(),
                            ),
                        )
                        .child(
                            icon_button(
                                "pool-grid",
                                icons::icon(IconName::LayoutGrid),
                                "Thumbnail grid",
                                cx.listener(|this, _, _, cx| {
                                    this.media_view.list = false;
                                    cx.notify();
                                }),
                            )
                            .selected(!self.media_view.list),
                        )
                        .child(
                            icon_button(
                                "pool-list",
                                icons::icon(IconName::List),
                                "List view",
                                cx.listener(|this, _, _, cx| {
                                    this.media_view.list = true;
                                    cx.notify();
                                }),
                            )
                            .selected(self.media_view.list),
                        ),
                )
                .child(
                    div().px_2().pb_2().flex().gap_1().children(
                        [
                            ("", "All"),
                            ("video/", "Video"),
                            ("image/", "Images"),
                            ("audio/", "Audio"),
                        ]
                        .into_iter()
                        .map(|(kind, label)| {
                            Button::new(SharedString::from(format!("filter-{label}")))
                                .ghost()
                                .compact()
                                .label(label)
                                .selected(self.media_view.kind == kind)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.media_view.kind = kind;
                                    cx.notify();
                                }))
                        }),
                    ),
                )
            })
            .child(body)
    }

    pub(super) fn pool_drag_ghost(&self, cx: &mut Context<Self>) -> Option<Div> {
        let drag = self.pool_drag.as_ref()?;
        let (icon, label) = match drag {
            PoolDrag::Title(id) => (IconName::Type, id.clone()),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn media_search_combines_name_and_kind() {
        assert!(media_matches("Lesson.MP4", "video/mp4", "LESS", "video/"));
        assert!(!media_matches("Lesson.MP4", "video/mp4", "less", "audio/"));
        assert!(!media_matches("Lesson.MP4", "video/mp4", "image", ""));
        assert!(media_matches("Score.wav", "audio/wav", "", ""));
    }
    #[core::prelude::v1::test]
    fn thumbnails_bound_pixel_storage_and_preserve_aspect() {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::new(1920, 1080));
        let thumb = thumbnail_image(image);
        assert_eq!(thumb.size(0).width, gpui::DevicePixels(320));
        assert_eq!(thumb.size(0).height, gpui::DevicePixels(180));
    }
}
