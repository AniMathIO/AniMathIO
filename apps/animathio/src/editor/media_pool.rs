//! Media pool.

use super::*;
use crate::style::{self, color, hex, tokens::TextRole as _};
use gpui_component::input::{Input, InputEvent, InputState};

/// Right padding of the Library body, in rems: the width of the scrollbar track.
const SCROLLBAR_LANE_REMS: f32 = 1.1;

/// Search text for the Effects tab's built-in Write on button.
const WRITE_ON_SEARCH: &str = "Animations Write on LaTeX";

/// Shown at the top of the Effects tab while no clip is selected; one line
/// each (see `docs/design.md` "Writing").
pub(super) const EFFECTS_HINT: &str = "Select a clip, then click an effect.";
const EFFECTS_HINT_DRAG: &str = "Or drag one onto a clip.";

/// The Media tab's kind filter (see `style::segmented`); the values are the
/// `media_type` prefixes `media_matches` filters by.
static KIND_FILTERS: [style::SegmentedOption; 4] = [
    style::SegmentedOption::new("filter-All", "All", None),
    style::SegmentedOption::new("filter-Video", "Video", None),
    style::SegmentedOption::new("filter-Images", "Images", None),
    style::SegmentedOption::new("filter-Audio", "Audio", None),
];
const KIND_VALUES: [&str; 4] = ["", "video/", "image/", "audio/"];

#[derive(Default)]
pub(super) struct MediaView {
    list: bool,
    pub(super) tab_focus: Vec<FocusHandle>,
    kind_focus: Vec<FocusHandle>,
    pub(super) queries: [String; 5],
    kind: &'static str,
    pub(super) searches: [Option<Entity<InputState>>; 5],
    subscriptions: Vec<Subscription>,
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
        // Thumbnails stream the asset (bounded memory), so any length is fine.
        if let Ok(source) = SymphoniaSource::from_bytes_with_limits(
            bytes,
            animathio_audio::DecodeLimits {
                max_duration: Duration::from_hours(4),
                max_samples: animathio_audio::MAX_INTERLEAVED_SAMPLES,
            },
        ) && let Ok(mut reader) = source.open_reader()
            && let Ok(wave) = Waveform::from_reader(reader.as_mut(), 96)
        {
            let info = source.info();
            result.duration = Time(wave.duration_ticks);
            result.peaks = wave.peaks.into();
            result.detail = format!("{} Hz / {} ch", info.rate, info.channels);
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

/// Grid columns for a Library body: as many as fit `min_rems` wide cards in the
/// dock (the width model, so a resize never flashes a wrong layout). The grid
/// shrinks cards to fit, so an estimate that is a pixel off never clips one.
pub(super) fn library_columns(rem: gpui::Pixels, dock_width: f32, min_rems: f32) -> usize {
    columns_for(dock_width * style::ui_scale(), f32::from(rem), min_rems)
}

/// [`library_columns`] on scaled pixels: `n` cards and `n - 1` grid gaps (`gap_2`, half a rem)
/// fit the body (minus its left padding and the scrollbar lane) when
/// `n * min + (n - 1) * gap <= inner`, so a card never drops below `min_rems`.
fn columns_for(dock_px: f32, rem: f32, min_rems: f32) -> usize {
    let inner = dock_px - rem * (0.25 + SCROLLBAR_LANE_REMS);
    let gap = rem * 0.5;
    (((inner + gap) / (min_rems * rem + gap)).floor() as usize).max(1)
}

#[cfg(test)]
mod column_tests {
    use super::columns_for;

    /// The dock width in scaled pixels for a logical `width` at UI `scale` (rem = 16 * scale).
    fn columns(width: f32, scale: f32) -> usize {
        columns_for(width * scale, 16. * scale, 7.)
    }

    #[test]
    fn library_columns_follow_the_dock_width_at_both_scales() {
        for scale in [1., 1.5] {
            assert_eq!(columns(240., scale), 1, "240px at {scale}x");
            assert_eq!(columns(340., scale), 2, "340px at {scale}x");
            assert_eq!(columns(400., scale), 3, "400px at {scale}x");
        }
    }

    #[test]
    fn library_cards_never_go_below_the_minimum_width() {
        for scale in [1., 1.5] {
            let rem = 16. * scale;
            for width in (200..900).step_by(5) {
                let dock = width as f32 * scale;
                let n = columns_for(dock, rem, 7.);
                if n == 1 {
                    continue;
                }
                let inner = dock - rem * (0.25 + super::SCROLLBAR_LANE_REMS);
                let card = (inner - (n - 1) as f32 * rem * 0.5) / n as f32;
                assert!(
                    card >= 7. * rem - 0.01,
                    "{n} columns at {width}px, {scale}x: card {card} < {}",
                    7. * rem
                );
            }
        }
    }
}

/// Centre an empty state vertically in the Library body (see `docs/design.md`
/// "Empty states": the state fills the width; the wrapper gives it the height).
fn centred_empty_state(state: gpui::Div) -> gpui::Div {
    div()
        .min_h(relative(1.))
        .w_full()
        .flex()
        .flex_col()
        .justify_center()
        .child(state)
}

/// A ghost button that empties a Library tab's search (and the Media kind
/// filter, which can hide everything just as well).
fn clear_search_button(tab: LeftTab, cx: &mut Context<Editor>) -> Button {
    Button::new(SharedString::from(format!(
        "{}-clear-search",
        tab.label().to_lowercase()
    )))
    .ghost()
    .small()
    .label("Clear search")
    .on_click(cx.listener(move |this, _, window, cx| {
        let index = tab as usize;
        this.media_view.queries[index].clear();
        if tab == LeftTab::Media {
            this.media_view.kind = "";
        }
        if let Some(input) = this.media_view.searches[index].clone() {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        cx.notify();
    }))
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
            self.error = Some(format!(
                "AniMathIO cannot place {} media on the timeline.",
                asset.media_type
            ));
            cx.notify();
            return;
        };
        let Some(track) = timeline::display_tracks(&self.project)
            .first()
            .map(|track| track.id())
        else {
            self.error = Some(
                "There is no track to place the clip on. Use Add track in the timeline.".into(),
            );
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
        let track = selection::track_at(&self.project, row).or_else(|| {
            timeline::display_tracks(&self.project)
                .first()
                .map(|t| t.id())
        });
        if let PoolDrag::Math(kind) = &drag {
            self.add_math_default(*kind, time, cx);
            return;
        }
        if let PoolDrag::Title(preset) = &drag {
            self.add_title_preset(preset, time, cx);
            return;
        }
        if let PoolDrag::Plugin(plugin_id) = &drag {
            // Elements are overlays: the row under the pointer when free, else a free layer.
            let row_track = selection::track_at(&self.project, row);
            self.drop_plugin(plugin_id.clone(), row_track, time, cx);
            return;
        }
        let Some(track) = track else {
            self.error = Some(
                "There is no track to place the clip on. Use Add track in the timeline.".into(),
            );
            cx.notify();
            return;
        };
        match drag {
            PoolDrag::Math(_) => unreachable!("math is placed above"),
            PoolDrag::Title(_) => unreachable!("titles create their own tracks"),
            PoolDrag::Asset(asset_id) => self.drop_asset(asset_id, track, time, cx),
            PoolDrag::Plugin(_) => unreachable!("plugins are placed above"),
            PoolDrag::Effect { plugin, effect } => {
                let hit = timeline::hit_test(&layout, &self.project, x, y);
                if let Some(clip) = hit.map(|hit| hit.clip).or(self.selected) {
                    self.add_effect_to_clip(clip, &plugin, &effect, cx);
                } else {
                    self.error = Some("Drop the effect on a clip to apply it.".into());
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
            self.error = Some(format!(
                "AniMathIO cannot place {} media on the timeline.",
                asset.media_type
            ));
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
        let info = match &clip.kind {
            ClipKind::Video(id) => self.assets.get(id).and_then(|bytes| {
                animathio_media::FilmCraftBackend::new()
                    .probe_bytes("media", bytes.clone().into())
                    .ok()
            }),
            _ => None,
        };
        // Use the media's real length, as importing a file does, instead of the
        // placeholder length. Audio is already decoded in the pool.
        let length = match &clip.kind {
            ClipKind::Audio(id) => self.waveform_for(id).map(|wave| Time(wave.duration_ticks)),
            _ => info.as_ref().map(|info| info.duration),
        };
        if let Some(duration) = length.filter(|d| d.0 > 0)
            && let Some(end) = clip.range.start.0.checked_add(duration.0)
            && let Ok(range) = TimeRange::new(clip.range.start, Time(end))
        {
            clip.range = range;
        }
        // Never fail on an occupied slot: use the chosen row when it is free over the clip,
        // else a free layer, creating a track when none is. New ids are reserved in order.
        let mut next_track = self
            .project
            .tracks()
            .iter()
            .map(|t| t.id().0)
            .max()
            .map_or(1, |max| max.saturating_add(1));
        let mut added_tracks = 0;
        let range = clip.range;
        let free_on = |t: &Track| {
            !t.is_locked()
                && !t
                    .clips()
                    .values()
                    .any(|c| c.range.start < range.end && range.start < c.range.end)
        };
        let target = match &clip.kind {
            ClipKind::Audio(_) => {
                if self
                    .project
                    .tracks()
                    .iter()
                    .any(|t| t.id() == track && free_on(t))
                {
                    track
                } else if let Some(free) = self.project.tracks().iter().find(|t| {
                    free_on(t)
                        && !t.clips().is_empty()
                        && t.clips()
                            .values()
                            .all(|c| matches!(c.kind, ClipKind::Audio(_)))
                }) {
                    free.id()
                } else {
                    let id = TrackId(next_track);
                    next_track = next_track.saturating_add(1);
                    commands.push(Command::AddTrack {
                        track: Track::new(id, "Audio"),
                        index: self.project.tracks().len(),
                    });
                    added_tracks += 1;
                    id
                }
            }
            kind => {
                let label = if matches!(kind, ClipKind::Image(_)) {
                    "Image"
                } else {
                    "Video"
                };
                match animathio_control::visual_slot(&self.project, range, Some(track)) {
                    animathio_control::Slot::Existing(id) => id,
                    animathio_control::Slot::New(id) => {
                        next_track = next_track.max(id.0.saturating_add(1));
                        commands.push(Command::AddTrack {
                            track: Track::new(id, label),
                            index: self.project.tracks().len(),
                        });
                        added_tracks += 1;
                        id
                    }
                }
            }
        };
        if let ClipKind::Video(id) = &clip.kind
            && info.as_ref().is_some_and(|info| !info.audio.is_empty())
        {
            let audio_track = TrackId(next_track);
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
                index: self.project.tracks().len() + added_tracks,
            });
            commands.push(Command::AddClip {
                track: audio_track,
                clip: audio,
            });
        }
        commands.push(Command::AddClip {
            track: target,
            clip,
        });
        self.execute(&Command::Batch { commands }, cx);
    }

    /// Place a plugin clip at an explicit timeline position. `track` is the row it was dropped
    /// on; when that row is occupied the clip goes to a free layer above instead.
    pub(super) fn drop_plugin(
        &mut self,
        plugin_id: String,
        track: Option<TrackId>,
        time: Time,
        cx: &mut Context<Self>,
    ) {
        let Some(descriptor) = self
            .plugins
            .iter()
            .find(|plugin| plugin.id == plugin_id)
            .cloned()
        else {
            return;
        };
        self.place_plugin_clip(&descriptor, track, time, cx);
    }

    fn place_plugin_clip(
        &mut self,
        descriptor: &PluginDescriptor,
        track: Option<TrackId>,
        start: Time,
        cx: &mut Context<Self>,
    ) {
        let models = descriptor.models();
        let (mut numeric, mut opaque) = inspector::default_params(&models);
        inspector::element_defaults(&descriptor.id, &mut numeric, &mut opaque);
        let Some(end) = start.0.checked_add(DEFAULT_MEDIA_TICKS) else {
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
        let id = clip.id;
        let (commands, _) =
            animathio_control::place_visual_clip(&self.project, clip, &descriptor.name, track);
        if self.try_execute(&Command::Batch { commands }, cx) {
            self.select_only(id, cx);
            cx.notify();
        }
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
            self.error = Some("Select a clip first, then click an effect to apply it.".into());
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

    /// Add a plugin clip at the playhead on a free layer, creating a track when needed.
    pub(super) fn add_plugin_clip(
        &mut self,
        descriptor: &PluginDescriptor,
        cx: &mut Context<Self>,
    ) {
        self.place_plugin_clip(descriptor, None, self.position, cx);
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
        if self.media_view.tab_focus.is_empty() {
            self.media_view.tab_focus = LeftTab::ALL
                .into_iter()
                .map(|_| cx.focus_handle())
                .collect();
        }
        if self.media_view.kind_focus.is_empty() {
            self.media_view.kind_focus =
                (0..KIND_VALUES.len()).map(|_| cx.focus_handle()).collect();
        }
        let tab = self.left_tab;
        let index = tab as usize;
        if self.media_view.searches[index].is_none() {
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(format!("Search {}…", tab.label().to_lowercase()))
            });
            let subscription = cx.subscribe(&input, move |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.media_view.queries[index] = input.read(cx).value().to_string();
                    cx.notify();
                }
            });
            self.media_view.subscriptions.push(subscription);
            self.media_view.searches[index] = Some(input);
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
            // The scrollbar's track is drawn over the body's right edge; keep it off the cards.
            .pr(window.rem_size() * SCROLLBAR_LANE_REMS)
            .id(("media-body", self.left_tab as usize))
            .debug_selector(|| "library-body".into())
            .overflow_y_scroll()
            .track_scroll(&self.library_scroll[self.left_tab as usize]);
        let this_recording = self.record_panel.is_some();
        match self.left_tab {
            LeftTab::Elements => {
                // `w_full` gives the cards' percentage widths a definite parent
                // width, so a two-column row never runs past the Library edge.
                body = body
                    .child(div().w_full().child(self.math_cards(window.rem_size(), cx)))
                    .child(self.plugin_element_cards(cx));
            }
            LeftTab::Titles => {
                body = body.child(self.title_cards(window, cx));
            }
            LeftTab::Media => {
                if let Some(card) = self.record_panel_view(cx) {
                    body = body.child(card);
                }
                let assets: Vec<_> = self
                    .project
                    .assets()
                    .iter()
                    .filter(|(_, asset)| {
                        media_matches(
                            &asset.name,
                            &asset.media_type,
                            &self.media_view.queries[LeftTab::Media as usize],
                            self.media_view.kind,
                        )
                    })
                    .collect();
                // Grid: columns from the dock width (minimum card width 7rem),
                // so a card is never clipped at the panel edge (docs/design.md
                // "Library cards and lists"). The dock width model drives this
                // (see `docs/design.md` "UI scale"), so there is no resize flash.
                let columns = if self.media_view.list {
                    1
                } else {
                    library_columns(window.rem_size(), self.dock.left_width, 7.)
                };
                let rem = window.rem_size();
                if assets.is_empty() {
                    let state = if self.project.assets().is_empty() {
                        style::empty_state(
                            Some(IconName::FolderOpen),
                            "Imported media appears here.",
                            Some(
                                style::primary_button("pool-import", cx)
                                    .small()
                                    .icon(icons::icon(IconName::Import))
                                    .label("Import media…")
                                    .on_click(cx.listener(|this, _, _, cx| this.import_media(cx)))
                                    .into_any_element(),
                            ),
                        )
                    } else {
                        style::empty_state(
                            Some(IconName::Search),
                            "No media matches the search.",
                            Some(clear_search_button(LeftTab::Media, cx).into_any_element()),
                        )
                    };
                    body = body.child(centred_empty_state(state));
                }
                let mut cards = div().grid().grid_cols(columns as u16).gap_2();
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
                                    let total = f32::from(bounds.size.width);
                                    let width = total / peaks.len() as f32;
                                    for (index, peak) in peaks.iter().enumerate() {
                                        // Clip the bar to the card's inner width: the last
                                        // one is never cut off at the edge.
                                        let left = index as f32 * width;
                                        let bar = (width - 1.).max(1.).min(total - left);
                                        if bar <= 0. {
                                            continue;
                                        }
                                        let height =
                                            f32::from(bounds.size.height) * peak.max(0.02) * 0.8;
                                        window.paint_quad(fill(
                                            Bounds::new(
                                                bounds.origin
                                                    + point(
                                                        px(left),
                                                        px((f32::from(bounds.size.height)
                                                            - height)
                                                            / 2.),
                                                    ),
                                                size(px(bar), px(height)),
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
                            // Kind icon over the thumbnail, not a coloured word.
                            div()
                                .absolute()
                                .top(rem / 4.)
                                .left(rem / 4.)
                                .p_1()
                                .rounded(px(style::radius::CONTROL))
                                .bg(style::tint(color::BG, style::Alpha::Scrim))
                                .child(icons::icon(icon).xsmall().text_color(hex(color::TEXT))),
                        )
                        .when(duration.0 > 0, |p| {
                            p.child(
                                div()
                                    .absolute()
                                    .bottom(rem / 4.)
                                    .right(rem / 4.)
                                    .px_1()
                                    .rounded(px(style::radius::CONTROL))
                                    .bg(hex(color::RAISED))
                                    .text_role(style::Role::Caption)
                                    .tabular()
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
                    let mut card = div()
                        .id(SharedString::from(format!("asset-{}", id.as_str())))
                        .rounded(px(style::radius::CARD))
                        .overflow_hidden()
                        .border_1()
                        .border_color(hex(color::BORDER))
                        .bg(hex(color::RAISED))
                        .hover(|s| s.border_color(hex(color::ACCENT)).bg(hex(color::HOVER)))
                        .cursor_pointer()
                        .child(preview)
                        .child(
                            div()
                                .p_2()
                                .flex()
                                .flex_col()
                                .gap_0p5()
                                .child(
                                    div()
                                        .truncate()
                                        .text_role(style::Role::Label)
                                        .text_color(hex(color::TEXT))
                                        .child(asset.name.clone()),
                                )
                                .child(
                                    div()
                                        .truncate()
                                        .text_role(style::Role::Caption)
                                        .child(detail),
                                ),
                        );
                    card = card.w_full().min_w_0();
                    cards = cards.child(
                        card.on_mouse_move(cx.listener(
                            move |this, event: &MouseMoveEvent, _, cx| {
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
                            },
                        ))
                        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                            if !hovered {
                                this.media_view.hovered = None;
                                cx.notify();
                            }
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            // A click completes the press that armed a drag; drop the ghost.
                            this.pool_drag = None;
                            this.add_asset_at_playhead(add_id.clone(), cx);
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.arm_pool_drag(PoolDrag::Asset(drop_id.clone()), cx);
                            }),
                        ),
                    );
                }
                body = body.child(cards);
            }
            LeftTab::Plugins => {
                body = body.child(self.plugin_inventory(cx));
            }
            LeftTab::Effects => {
                let effects: Vec<_> = self
                    .available_effects()
                    .into_iter()
                    .filter(|e| {
                        self.library_matches(LeftTab::Effects, &format!("Video effects {}", e.name))
                    })
                    .collect();
                let audio_effects: Vec<_> = self
                    .available_audio_effects()
                    .into_iter()
                    .filter(|e| {
                        self.library_matches(LeftTab::Effects, &format!("Audio effects {}", e.name))
                    })
                    .collect();
                let animations = self.animation_library_rows(cx);
                let has_transitions = ["Video fades", "Audio fades", "Video + audio fades"]
                    .iter()
                    .any(|label| {
                        self.library_matches(LeftTab::Effects, &format!("Transitions {label}"))
                    });
                let searching = !self.media_view.queries[LeftTab::Effects as usize]
                    .trim()
                    .is_empty();
                let no_plugin_rows =
                    effects.is_empty() && audio_effects.is_empty() && animations.is_empty();
                // Transitions and Write on are built in, so they never count as loaded plugins.
                if searching {
                    if no_plugin_rows
                        && !has_transitions
                        && !self.library_matches(LeftTab::Effects, WRITE_ON_SEARCH)
                    {
                        body = body.child(centred_empty_state(
                            style::empty_state(
                                Some(IconName::Sparkles),
                                "No effects match the search.",
                                Some(clear_search_button(LeftTab::Effects, cx).into_any_element()),
                            )
                            .debug_selector(|| "effects-empty".into()),
                        ));
                    }
                } else if no_plugin_rows {
                    body = body.child(centred_empty_state(
                        style::empty_state(
                            Some(IconName::Puzzle),
                            "No effect plugins are loaded.",
                            Some(
                                Button::new("effects-open-plugins")
                                    .link()
                                    .small()
                                    .label("Open Plugins")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.select_left_tab(LeftTab::Plugins, cx);
                                    }))
                                    .into_any_element(),
                            ),
                        )
                        .debug_selector(|| "effects-empty".into()),
                    ));
                }
                if self.selected_ids().is_empty() {
                    body = body.child(
                        div()
                            .id("effects-hint")
                            .debug_selector(|| "effects-hint".into())
                            .px_2()
                            .py_1()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_role(style::Role::Label)
                                    .text_color(hex(color::TEXT_MUTED))
                                    .child(EFFECTS_HINT),
                            )
                            .child(
                                div()
                                    .text_role(style::Role::Label)
                                    .text_color(hex(color::TEXT_MUTED))
                                    .child(EFFECTS_HINT_DRAG),
                            ),
                    );
                }
                let group = |label: &'static str| {
                    div()
                        .px_2()
                        .pt_2()
                        .pb_1()
                        .text_role(style::Role::Heading)
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
                    let (plugin, effect) = (descriptor.plugin_id, descriptor.effect_id);
                    let icon = effect_icon(audio, &effect);
                    let (click_plugin, click_effect) = (plugin.clone(), effect.clone());
                    body = body.child(
                        div()
                            .id(SharedString::from(format!("effect-item-{plugin}-{effect}")))
                            .px_2()
                            .min_h(style::size::ROW)
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .hover(|this| this.bg(hex(color::HOVER)))
                            .gap_2()
                            .text_role(style::Role::Body)
                            .child(
                                icons::icon(icon)
                                    .small()
                                    .text_color(hex(color::TEXT_SECONDARY)),
                            )
                            .child(descriptor.name)
                            .debug_selector({
                                let key = format!("effect-item-{plugin}-{effect}");
                                move || key.clone()
                            })
                            .focusable()
                            .tab_index(0)
                            .focus_visible(|s| s.bg(hex(color::HOVER)))
                            .on_key_down({
                                let plugin = plugin.clone();
                                let effect = effect.clone();
                                cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                                    if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                        this.add_effect_to_selected(&plugin, &effect, cx);
                                        cx.stop_propagation();
                                    }
                                })
                            })
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
                let transitions: Vec<_> = [
                    (
                        "video",
                        "Video fades",
                        IconName::Film,
                        fades::TransitionKind::Video,
                    ),
                    (
                        "audio",
                        "Audio fades",
                        IconName::AudioWaveform,
                        fades::TransitionKind::Audio,
                    ),
                    (
                        "both",
                        "Video + audio fades",
                        IconName::Layers,
                        fades::TransitionKind::Both,
                    ),
                ]
                .into_iter()
                .filter(|(_, label, _, _)| {
                    self.library_matches(LeftTab::Effects, &format!("Transitions {label}"))
                })
                .collect();
                if !transitions.is_empty() {
                    body = body.child(group("Transitions"));
                }
                for (key, label, icon, kind) in transitions {
                    let key_down = cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                            this.apply_default_transition(None, kind, cx);
                            cx.stop_propagation();
                        }
                    });
                    body = body.child(
                        div()
                            .id(SharedString::from(format!("library-transition-{key}")))
                            .debug_selector(move || format!("library-transition-{key}"))
                            .px_2()
                            .min_h(style::size::ROW)
                            .cursor_pointer()
                            .hover(|this| this.bg(hex(color::HOVER)))
                            .focus_visible(|s| s.bg(hex(color::HOVER)))
                            .focusable()
                            .tab_index(0)
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_role(style::Role::Body)
                            .child(
                                icons::icon(icon)
                                    .small()
                                    .text_color(hex(color::TEXT_SECONDARY)),
                            )
                            .child(label)
                            .tooltip(|window, cx| {
                                gpui_component::tooltip::Tooltip::new(
                                    "Apply one-second fades to the selected clips",
                                )
                                .build(window, cx)
                            })
                            .on_key_down(key_down)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.apply_default_transition(None, kind, cx);
                            })),
                    );
                }
                body = body.children(animations);
                if self.library_matches(LeftTab::Effects, WRITE_ON_SEARCH) {
                    body = body.child(
                        Button::new("write-on-preset")
                            .debug_selector(|| "library-write-on".into())
                            .ghost()
                            .small()
                            .icon(icons::icon(IconName::PenLine))
                            .label("Write on…")
                            .tooltip("Animate the selected LaTeX clip")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_write_on(window, cx)),
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
            .on_drop::<ExternalPaths>(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                this.import_paths(paths.paths(), None, cx);
            }))
            .bg(hex(color::PANEL))
            .child(tab_row)
            .child(
                div()
                    .px_2()
                    .py_2()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div().flex_1().min_w_0().child(
                            Input::new(
                                self.media_view.searches[index]
                                    .as_ref()
                                    .expect("search state"),
                            )
                            .small(),
                        ),
                    )
                    .when(self.left_tab == LeftTab::Media, |row| {
                        row.child(
                            icon_button(
                                "pool-record",
                                icons::icon(IconName::Mic),
                                "Record audio from a microphone",
                                cx.listener(|this, _, window, cx| this.open_recorder(window, cx)),
                            )
                            .selected(this_recording),
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
                        )
                    }),
            )
            .when(self.left_tab == LeftTab::Media, |pane| {
                let selected_kind = KIND_VALUES
                    .iter()
                    .position(|kind| *kind == self.media_view.kind)
                    .unwrap_or(0);
                let editor = cx.entity();
                pane.child(
                    div()
                        .debug_selector(|| "media-kind-filters".into())
                        .px_2()
                        .pb_2()
                        .child(style::segmented(
                            "media-kind-filter",
                            &KIND_FILTERS,
                            &self.media_view.kind_focus,
                            selected_kind,
                            None,
                            true,
                            move |index, _window, cx| {
                                editor.update(cx, |this, cx| {
                                    this.media_view.kind = KIND_VALUES[index];
                                    cx.notify();
                                });
                            },
                        )),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .relative()
                    .child(body.h_full())
                    .child(gpui_component::scroll::Scrollbar::vertical(
                        &self.library_scroll[self.left_tab as usize],
                    )),
            )
    }

    pub(super) fn library_matches(&self, tab: LeftTab, text: &str) -> bool {
        text.to_lowercase().contains(
            self.media_view.queries[tab as usize]
                .trim()
                .to_lowercase()
                .as_str(),
        )
    }

    pub(super) fn element_kinds(&self) -> Vec<(usize, animathio_control::MathObjectKind)> {
        use animathio_control::MathObjectKind;
        let mut kinds: Vec<_> = MathObjectKind::ALL
            .into_iter()
            .enumerate()
            .filter(|(_, kind)| {
                self.library_matches(
                    LeftTab::Elements,
                    &format!("{} {}", math::element_group(*kind), kind.label()),
                )
            })
            .collect();
        kinds.sort_by_key(|(_, kind)| match math::element_group(*kind) {
            "Text & formulas" => 0,
            "Shapes & geometry" => 1,
            _ => 2,
        });
        kinds
    }

    /// First-party sources with a dedicated object card never appear a second time.
    /// Shapes retains its generic card for rectangle, star and arrow authoring.
    pub(super) fn element_plugin_sources(&self) -> Vec<&PluginDescriptor> {
        self.plugins
            .iter()
            .filter(|p| {
                !matches!(
                    p.id.as_str(),
                    "org.animathio.text" | "org.animathio.latex" | "org.animathio.graph"
                )
            })
            .filter(|p| {
                self.library_matches(
                    LeftTab::Elements,
                    &format!(
                        "{} {}",
                        p.name,
                        if p.id == "org.animathio.shapes" {
                            "Shapes & geometry"
                        } else {
                            "Plugins"
                        }
                    ),
                )
            })
            .collect()
    }

    fn plugin_element_cards(&self, cx: &mut Context<Self>) -> Div {
        let mut cards = div().flex().flex_col().gap_2().p_2();
        for (first_party, label) in [(true, "More shapes"), (false, "Plugins")] {
            let sources: Vec<_> = self
                .element_plugin_sources()
                .into_iter()
                .filter(|p| (p.id == "org.animathio.shapes") == first_party)
                .collect();
            if sources.is_empty() {
                continue;
            }
            cards = cards.child(div().text_role(style::Role::Heading).child(label));
            for descriptor in sources {
                let id = descriptor.id.clone();
                let drag = id.clone();
                let keyboard = id.clone();
                let add = id.clone();
                let add_id = id.clone();
                let selector = format!("element-plugin-{id}");
                cards = cards.child(
                    div()
                        .id(SharedString::from(selector.clone()))
                        .debug_selector(move || selector.clone())
                        .min_h(style::size::ROW)
                        .p_2()
                        .flex()
                        .items_center()
                        .rounded(px(style::radius::CARD))
                        .border_1()
                        .border_color(hex(color::BORDER))
                        .bg(hex(color::RAISED))
                        .hover(|s| s.border_color(hex(color::ACCENT)).bg(hex(color::HOVER)))
                        .focusable()
                        .tab_index(0)
                        .cursor_pointer()
                        .focus_visible(|s| s.border_color(hex(color::ACCENT)))
                        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                this.add_plugin_clip_by_id(&keyboard, cx);
                                cx.stop_propagation();
                            }
                        }))
                        .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                            this.pool_drag = None;
                            this.add_plugin_clip_by_id(&id, cx);
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.arm_pool_drag(PoolDrag::Plugin(drag.clone()), cx);
                            }),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    icons::icon(if first_party {
                                        IconName::Shapes
                                    } else {
                                        IconName::Puzzle
                                    })
                                    .small()
                                    .text_color(hex(color::TEXT_SECONDARY)),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .truncate()
                                        .text_role(style::Role::Label)
                                        .text_color(hex(color::TEXT))
                                        .child(descriptor.name.clone()),
                                )
                                .child(
                                    Button::new(SharedString::from(format!("add-element-{add}")))
                                        .debug_selector(move || format!("add-element-{add}"))
                                        .ghost()
                                        .small()
                                        .icon(icons::icon(IconName::Plus))
                                        .tooltip("Add with defaults at playhead")
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.pool_drag = None;
                                            this.add_plugin_clip_by_id(&add_id, cx);
                                            cx.stop_propagation();
                                        })),
                                ),
                        ),
                );
            }
        }
        cards
    }

    pub(super) fn pool_drag_ghost(&self, cx: &mut Context<Self>) -> Option<Div> {
        let drag = self.pool_drag.as_ref()?;
        let (icon, label) = match drag {
            PoolDrag::Math(kind) => (IconName::Shapes, kind.label().into()),
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

/// The Library icon of a built-in effect: one per effect, so the list reads at a glance.
pub(super) fn effect_icon(audio: bool, effect: &str) -> IconName {
    if audio {
        return match effect {
            "gain" => IconName::Volume2,
            "eq3" => IconName::SlidersVertical,
            "compressor" => IconName::Gauge,
            "gate" => IconName::ZapOff,
            "highpass" => IconName::Activity,
            "voice_cleanup" => IconName::Mic,
            "denoise" => IconName::AudioLines,
            _ => IconName::AudioWaveform,
        };
    }
    match effect {
        "grayscale" => IconName::Contrast,
        "sepia" => IconName::Coffee,
        "invert" => IconName::Blend,
        "saturate" => IconName::Droplet,
        "brightness" => IconName::Sun,
        "contrast" => IconName::SunMoon,
        "blur" => IconName::Aperture,
        "opacity" => IconName::Layers2,
        "slide" => IconName::MoveHorizontal,
        _ => IconName::Sparkles,
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
