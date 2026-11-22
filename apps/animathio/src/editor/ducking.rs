//! "Auto duck…" panel: pick the voice source and music tracks, tune depth and ramps, review
//! the ducked spans on the timeline, then apply one undoable batch of gain keyframes.

use super::tool_panel;
use super::*;
use crate::style::{self, TextRole as _};
use animathio_control::{DuckParams, DuckProposal, duck_command, propose_duck, timecode};
use gpui_component::{
    Disableable,
    slider::{SliderEvent, SliderState},
};

pub(super) fn duck_clip_label(project: &Project, track: &Track, clip: &Clip) -> String {
    format!(
        "{} / {} ({})",
        clip_label(project, clip),
        track.name(),
        crate::timecode::format_timecode(clip.range.start, project.frame_rate())
    )
}

/// Music tracks to suggest when Auto duck opens: every unlocked track that holds audio clips
/// but neither the voice clips nor their linked companions (a video's own audio). The panel
/// previews the result before anything is written.
/// Whether `clip` is (or is linked to) one of the voice clips, so it cannot also be music.
pub(super) fn is_voice_clip(project: &Project, voice_clips: &[ClipId], clip: &Clip) -> bool {
    voice_clips.contains(&clip.id)
        || clip
            .link_group
            .is_some_and(|group| voice_clips.contains(&group))
        || project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .any(|v| {
                voice_clips.contains(&v.id)
                    && v.link_group.is_some()
                    && v.link_group == clip.link_group
            })
}

pub(super) fn default_music_tracks(project: &Project, voice_clips: &[ClipId]) -> Vec<TrackId> {
    project
        .tracks()
        .iter()
        .filter(|t| {
            !t.is_locked()
                && t.clips()
                    .values()
                    .any(|c| matches!(c.kind, ClipKind::Audio(_)))
                && !t
                    .clips()
                    .values()
                    .any(|c| is_voice_clip(project, voice_clips, c))
        })
        .map(Track::id)
        .collect()
}

/// One sentence describing what applying `proposal` will do.
pub(super) fn duck_summary(
    params: &DuckParams,
    proposal: &DuckProposal,
    project: &Project,
) -> String {
    let keyframes: usize = proposal.clips.iter().map(|c| c.keys.len()).sum();
    if proposal.voice.is_empty() {
        return "No voice found. Lower the voice threshold or widen the analyzed range.".into();
    }
    if proposal.clips.is_empty() {
        return format!(
            "Found {} voice region{}, but no chosen music clip overlaps the ducked spans.",
            proposal.voice.len(),
            plural(proposal.voice.len())
        );
    }
    let ducked = lowered_ticks(project, proposal);
    #[allow(clippy::cast_precision_loss)]
    let seconds = ducked as f64 / TICKS_PER_SECOND as f64;
    format!(
        "Music drops by {:.1} dB under {} voice region{} (music lowered for {seconds:.1} s, ramps included). Writes {} keyframe{} on {} clip{} as one undo step.",
        params.depth_db,
        proposal.voice.len(),
        plural(proposal.voice.len()),
        keyframes,
        plural(keyframes),
        proposal.clips.len(),
        plural(proposal.clips.len()),
    )
}

/// Ticks during which some target clip is below full level: the ducked spans (ramps
/// included) trimmed to the ranges of the clips that receive keyframes, merged.
fn lowered_ticks(project: &Project, proposal: &DuckProposal) -> i64 {
    let mut spans: Vec<(i64, i64)> = Vec::new();
    for target in &proposal.clips {
        let Some(range) = project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .find(|c| c.id == target.clip)
            .map(|c| c.range)
        else {
            continue;
        };
        for r in &proposal.ducked {
            let (a, b) = (r.start.0.max(range.start.0), r.end.0.min(range.end.0));
            if b > a {
                spans.push((a, b));
            }
        }
    }
    spans.sort_unstable();
    let (mut total, mut end) = (0, i64::MIN);
    for (a, b) in spans {
        let a = a.max(end);
        if b > a {
            total += b - a;
            end = b;
        }
    }
    total
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

pub(super) struct DuckReview {
    identity: Arc<()>,
    pub(super) params: DuckParams,
    pub(super) proposal: Option<DuckProposal>,
    sliders: Vec<Entity<SliderState>>,
    _subscriptions: Vec<Subscription>,
    generation: u64,
    busy: bool,
    error: Option<String>,
    /// Whether the attack, release, gap and threshold sliders are shown.
    pub(super) timing_open: bool,
}

fn ticks(seconds: f64) -> Time {
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    Time((seconds * TICKS_PER_SECOND as f64).round() as i64)
}

impl DuckReview {
    fn ready(&self) -> bool {
        (!self.params.voice_clips.is_empty() || !self.params.voice_tracks.is_empty())
            && (!self.params.music_tracks.is_empty() || !self.params.music_clips.is_empty())
    }
    fn invalidate(&mut self) {
        self.proposal = None;
        self.generation = self.generation.wrapping_add(1);
        self.busy = false;
    }
}

impl Editor {
    pub(super) fn open_duck(&mut self, cx: &mut Context<Self>) {
        let defaults = DuckParams::default();
        let voice_clips: Vec<ClipId> = self
            .selected
            .filter(|id| {
                self.find_clip(*id).is_some_and(|clip| {
                    matches!(clip.kind, ClipKind::Audio(_) | ClipKind::Video(_))
                })
            })
            .into_iter()
            .collect();
        let params = DuckParams {
            music_tracks: if voice_clips.is_empty() {
                Vec::new()
            } else {
                default_music_tracks(&self.project, &voice_clips)
            },
            voice_clips,
            range: self
                .marks
                .range(animathio_export::duration(&self.project).0),
            ..defaults.clone()
        };
        let mut sliders = Vec::new();
        let mut subscriptions = Vec::new();
        let seconds = |t: Time| {
            #[allow(clippy::cast_precision_loss)]
            let s = t.0 as f64 / TICKS_PER_SECOND as f64;
            s
        };
        for (i, min, max, step, value) in [
            (0, 0., 30., 0.5, defaults.depth_db),
            (1, 0.05, 2., 0.05, seconds(defaults.attack)),
            (2, 0.1, 3., 0.05, seconds(defaults.release)),
            (3, 0., 2., 0.05, seconds(defaults.min_gap)),
            (4, -70., -20., 1., defaults.threshold_db),
        ] {
            let state = cx.new(|_| {
                SliderState::new()
                    .min(min as f32)
                    .max(max as f32)
                    .step(step as f32)
                    .default_value(value as f32)
            });
            subscriptions.push(
                cx.subscribe(
                    &state,
                    move |this, _, event: &SliderEvent, cx| match event {
                        SliderEvent::Change(value) => {
                            if let Some(review) = this.duck_review.as_mut() {
                                let value = f64::from(value.start());
                                match i {
                                    0 => review.params.depth_db = value,
                                    1 => review.params.attack = ticks(value),
                                    2 => review.params.release = ticks(value),
                                    3 => review.params.min_gap = ticks(value),
                                    _ => review.params.threshold_db = value,
                                }
                                review.invalidate();
                                cx.notify();
                            }
                        }
                        SliderEvent::Release(_) => this.analyse_duck(cx),
                    },
                ),
            );
            sliders.push(state);
        }
        self.duck_review = Some(DuckReview {
            identity: Arc::new(()),
            params,
            proposal: None,
            sliders,
            _subscriptions: subscriptions,
            generation: 0,
            busy: false,
            error: None,
            timing_open: false,
        });
        self.analyse_duck(cx);
    }

    /// After the voice clip changes, drop music picks that are now voice and, when the user
    /// has not picked any music, suggest the unlocked audio tracks again.
    fn refresh_music_defaults(&mut self) {
        let Some(review) = self.duck_review.as_mut() else {
            return;
        };
        let voice = review.params.voice_clips.clone();
        let project = &self.project;
        let is_voice = |id: &ClipId| {
            project
                .tracks()
                .iter()
                .flat_map(|t| t.clips().values())
                .find(|c| c.id == *id)
                .is_some_and(|c| is_voice_clip(project, &voice, c))
        };
        review.params.music_clips.retain(|id| !is_voice(id));
        let voice_track = |t: &TrackId| {
            project
                .tracks()
                .iter()
                .find(|track| track.id() == *t)
                .is_some_and(|track| {
                    track
                        .clips()
                        .values()
                        .any(|c| is_voice_clip(project, &voice, c))
                })
        };
        review.params.music_tracks.retain(|t| !voice_track(t));
        if review.params.music_tracks.is_empty()
            && review.params.music_clips.is_empty()
            && !voice.is_empty()
        {
            review.params.music_tracks = default_music_tracks(project, &voice);
        }
    }

    pub(super) fn apply_duck(&mut self, cx: &mut Context<Self>) {
        let proposal = self.duck_review.as_ref().and_then(|r| r.proposal.clone());
        if let Some(p) = proposal {
            match duck_command(&self.project, &p) {
                Ok(command) => {
                    if self.try_execute(&command, cx) {
                        self.duck_review = None;
                        let keyframes: usize = p.clips.iter().map(|c| c.keys.len()).sum();
                        self.notices.push(crate::style::Toast::info(format!(
                            "Ducked {} music clip{} ({keyframes} keyframes). Edit > Undo restores the previous levels.",
                            p.clips.len(),
                            plural(p.clips.len()),
                        )));
                    }
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        cx.notify();
    }

    pub(super) fn analyse_duck(&mut self, cx: &mut Context<Self>) {
        let Some(review) = self.duck_review.as_mut() else {
            return;
        };
        review.invalidate();
        review.error = None;
        if !review.ready() {
            cx.notify();
            return;
        }
        let identity = review.identity.clone();
        let generation = review.generation;
        review.busy = true;
        let params = review.params.clone();
        let project = self.project.clone();
        let assets = self.assets.clone();
        let audio_generation = self.audio_generation;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move { propose_duck(&project, &assets, &params) })
                .await;
            let _ = this.update(cx, |editor, cx| {
                let Some(review) = editor.duck_review.as_mut() else {
                    return;
                };
                if !Arc::ptr_eq(&identity, &review.identity) || generation != review.generation {
                    return;
                }
                review.busy = false;
                if audio_generation == editor.audio_generation {
                    match result {
                        Ok(proposal) => review.proposal = Some(proposal),
                        Err(error) => review.error = Some(error.to_string()),
                    }
                } else {
                    review.error = Some("Project changed; analyze again".into());
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub(super) fn invalidate_duck(&mut self) {
        if let Some(review) = self.duck_review.as_mut() {
            review.invalidate();
            review.error = Some("Project changed; analyze again".into());
        }
    }

    /// Ducked spans as `(range, track row)` for the timeline overlay.
    pub(super) fn duck_overlay(&self) -> Vec<(TimeRange, usize)> {
        let Some(review) = self.duck_review.as_ref() else {
            return Vec::new();
        };
        let Some(proposal) = review.proposal.as_ref() else {
            return Vec::new();
        };
        // Only the parts of the ducked spans that lie on a clip that receives keyframes.
        let mut overlay = Vec::new();
        for (row, track) in timeline::display_tracks(&self.project)
            .into_iter()
            .enumerate()
        {
            let whole_track = review.params.music_tracks.contains(&track.id());
            for clip in track.clips().values() {
                if !matches!(clip.kind, ClipKind::Audio(_))
                    || !(whole_track || review.params.music_clips.contains(&clip.id))
                {
                    continue;
                }
                for range in &proposal.ducked {
                    let (start, end) = (
                        range.start.0.max(clip.range.start.0),
                        range.end.0.min(clip.range.end.0),
                    );
                    if end > start {
                        overlay.push((
                            TimeRange {
                                start: Time(start),
                                end: Time(end),
                            },
                            row,
                        ));
                    }
                }
            }
        }
        overlay
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn duck_panel(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let review = self.duck_review.as_ref()?;
        if self.is_start_screen() {
            return None;
        }
        let entity = cx.entity();
        let audio_tracks: Vec<(TrackId, String, bool)> = self
            .project
            .tracks()
            .iter()
            .filter(|t| {
                t.clips()
                    .values()
                    .any(|c| matches!(c.kind, animathio_core::ClipKind::Audio(_)))
            })
            .map(|t| {
                let voice = review.params.voice_tracks.contains(&t.id())
                    || t.clips()
                        .values()
                        .any(|c| is_voice_clip(&self.project, &review.params.voice_clips, c));
                (t.id(), t.name().to_owned(), voice)
            })
            .collect();
        // Track rows: a check when the track is included, nothing when it is not.
        let track_rows = |music: bool, cx: &mut Context<Self>| -> Vec<AnyElement> {
            let mut rows = Vec::new();
            for (id, name, is_voice) in &audio_tracks {
                // Music is never the voice: hide tracks that hold the voice clip.
                if music && *is_voice {
                    continue;
                }
                let on = if music {
                    review.params.music_tracks.contains(id)
                } else {
                    review.params.voice_tracks.contains(id)
                };
                let id = *id;
                rows.push(
                    tool_panel::check_row(
                        (
                            if music { "duck-music" } else { "duck-voice" },
                            id.0 as usize,
                        ),
                        None,
                        on,
                        name.clone(),
                        cx.listener(move |this, _, _, cx| {
                            if let Some(r) = this.duck_review.as_mut() {
                                let (own, other) = if music {
                                    (&mut r.params.music_tracks, &mut r.params.voice_tracks)
                                } else {
                                    (&mut r.params.voice_tracks, &mut r.params.music_tracks)
                                };
                                if let Some(pos) = own.iter().position(|t| *t == id) {
                                    own.remove(pos);
                                } else {
                                    own.push(id);
                                    other.retain(|t| *t != id);
                                }
                            }
                            this.analyse_duck(cx);
                        }),
                    )
                    .into_any_element(),
                );
            }
            rows
        };

        // 1. Voice.
        let selected_audio = self.selected.is_some_and(|id| {
            self.find_clip(id).is_some_and(|c| {
                matches!(
                    c.kind,
                    animathio_core::ClipKind::Audio(_) | animathio_core::ClipKind::Video(_)
                )
            })
        });
        let clip_voice = !review.params.voice_clips.is_empty();
        let voice_clip_disabled = !selected_audio && !clip_voice;
        let mut voice = track_rows(false, cx);
        voice.push(
            tool_panel::check_row(
                "duck-voice-clip",
                Some("duck-voice-clip".into()),
                clip_voice,
                "Use the selected clip as the voice",
                cx.listener(move |this, _, _, cx| {
                    if voice_clip_disabled {
                        return;
                    }
                    let selected = this.selected;
                    if let Some(r) = this.duck_review.as_mut() {
                        if r.params.voice_clips.is_empty() {
                            r.params.voice_clips = selected.into_iter().collect();
                        } else {
                            r.params.voice_clips.clear();
                        }
                    }
                    this.refresh_music_defaults();
                    this.analyse_duck(cx);
                }),
            )
            .when(voice_clip_disabled, |row| row.opacity(0.5))
            .into_any_element(),
        );

        // 2. Music: tracks, then individual clips.
        let mut music = vec![tool_panel::note("Tracks").into_any_element()];
        music.extend(track_rows(true, cx));
        let mut clip_rows: Vec<AnyElement> = Vec::new();
        for track in self.project.tracks() {
            for clip in track
                .clips()
                .values()
                .filter(|c| matches!(c.kind, animathio_core::ClipKind::Audio(_)))
                .filter(|c| {
                    !review.params.voice_tracks.contains(&track.id())
                        && !is_voice_clip(&self.project, &review.params.voice_clips, c)
                })
            {
                let id = clip.id;
                clip_rows.push(
                    tool_panel::check_row(
                        ("duck-music-clip", id.0 as usize),
                        Some(format!("duck-target-{}", id.0)),
                        review.params.music_clips.contains(&id),
                        duck_clip_label(&self.project, track, clip),
                        cx.listener(move |this, _, _, cx| {
                            if let Some(r) = this.duck_review.as_mut() {
                                if r.params.music_clips.contains(&id) {
                                    r.params.music_clips.retain(|c| *c != id);
                                } else {
                                    r.params.music_clips.push(id);
                                }
                            }
                            this.analyse_duck(cx);
                        }),
                    )
                    .into_any_element(),
                );
            }
        }
        if !clip_rows.is_empty() {
            music.push(tool_panel::note("Individual clips").into_any_element());
            music.extend(clip_rows);
        }

        // Settings: the depth, the timing behind one disclosure, and the keyframe policy.
        let values = [
            format!("-{:.1} dB", review.params.depth_db),
            tool_panel::seconds_label(review.params.attack, 2),
            tool_panel::seconds_label(review.params.release, 2),
            tool_panel::seconds_label(review.params.min_gap, 2),
            format!("{:.0} dBFS", review.params.threshold_db),
        ];
        let labels = [
            "Duck depth",
            "Attack",
            "Release",
            "Minimum gap to merge",
            "Voice threshold",
        ];
        let timing_open = review.timing_open;
        let mut settings: Vec<AnyElement> = Vec::new();
        for (index, ((label, state), value)) in labels
            .into_iter()
            .zip(&review.sliders)
            .zip(values)
            .enumerate()
        {
            if index == 1 {
                settings.push(
                    div()
                        .flex()
                        .child(
                            Button::new("duck-timing")
                                .debug_selector(|| "duck-timing".into())
                                .ghost()
                                .small()
                                .icon(
                                    icons::icon(if timing_open {
                                        IconName::ChevronDown
                                    } else {
                                        IconName::ChevronRight
                                    })
                                    .xsmall(),
                                )
                                .label("Timing and voice threshold")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(r) = this.duck_review.as_mut() {
                                        r.timing_open = !r.timing_open;
                                    }
                                    cx.notify();
                                })),
                        )
                        .into_any_element(),
                );
            }
            if index == 0 || timing_open {
                settings.push(
                    tool_panel::slider_row(label, state, &self.focus_handle, value)
                        .into_any_element(),
                );
            }
        }
        settings.push(
            style::switch_row(
                "duck-replace",
                "Replace existing gain keyframes",
                None,
                review.params.replace_existing,
                move |checked, _, cx| {
                    let checked = *checked;
                    entity.update(cx, |this, cx| {
                        if let Some(r) = this.duck_review.as_mut() {
                            r.params.replace_existing = checked;
                        }
                        this.analyse_duck(cx);
                    });
                },
            )
            .into_any_element(),
        );

        let mut body = tool_panel::body()
            .child(tool_panel::note(
                "Lowers the music while someone speaks. Check the shaded spans on the timeline, then apply. Undo reverts it in one step.",
            ))
            .child(tool_panel::section("1. Voice", voice))
            .child(tool_panel::section("2. Music", music))
            .child(tool_panel::section("Settings", settings));
        if !review.ready() {
            body = body.child(tool_panel::note(
                "Choose a voice source and at least one music track or clip.",
            ));
        }
        if let Some(error) = &review.error {
            body = body.child(tool_panel::problem(error.clone()));
        }
        if review.busy {
            body = body.child(tool_panel::note("Analyzing audio…"));
        }
        if let Some(proposal) = &review.proposal {
            let mut preview: Vec<AnyElement> = vec![
                tool_panel::note(duck_summary(&review.params, proposal, &self.project))
                    .id("duck-summary")
                    .debug_selector(|| "duck-summary".into())
                    .text_color(style::hex(style::color::TEXT))
                    .into_any_element(),
                tool_panel::note(
                    "Shaded spans on the music tracks show where the level is lowered. The play button beside a voice region moves the playhead to where that region starts.",
                )
                .into_any_element(),
            ];
            let mut list = div().id("duck-list").flex().flex_col();
            for (i, range) in proposal.voice.iter().enumerate() {
                let start = range.start;
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .min_h(style::size::ROW)
                        .pl_2()
                        .pr_1()
                        .child(
                            div()
                                .flex_1()
                                .text_role(style::Role::Label)
                                .text_color(style::hex(style::color::TEXT_SECONDARY))
                                .tabular()
                                .child(format!(
                                    "{} - {}",
                                    timecode(range.start),
                                    timecode(range.end)
                                )),
                        )
                        .child(
                            Button::new(("duck-seek", i))
                                .ghost()
                                .xsmall()
                                .icon(icons::icon(IconName::Play))
                                .tooltip("Move the playhead to the start of this voice region")
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.seek_to(start, cx)),
                                ),
                        ),
                );
            }
            preview.push(list.into_any_element());
            body = body.child(tool_panel::section("3. Preview", preview));
        }

        let busy = review.busy;
        let can_apply = !busy
            && review
                .proposal
                .as_ref()
                .is_some_and(|p| !p.clips.is_empty());
        let footer = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_wrap()
            .justify_end()
            .gap_2()
            .child(
                style::secondary_button("duck-analyse", cx)
                    .small()
                    .label("Analyze")
                    .disabled(busy || !review.ready())
                    .on_click(cx.listener(|this, _, _, cx| this.analyse_duck(cx))),
            )
            .child(
                style::secondary_button("duck-cancel", cx)
                    .debug_selector(|| "duck-cancel".into())
                    .small()
                    .label("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.duck_review = None;
                        cx.notify();
                    })),
            )
            .child(
                style::primary_button("duck-apply", cx)
                    .debug_selector(|| "duck-apply".into())
                    .small()
                    .label("Apply")
                    .disabled(!can_apply)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.apply_duck(cx);
                    })),
            );
        Some(
            style::ToolPanel::new("duck-review", IconName::Volume2, "Auto duck")
                .bottom_height(self.dock.bottom_height)
                .on_close(cx.listener(|this, _, _, cx| {
                    this.duck_review = None;
                    cx.notify();
                }))
                .body(self.duck_scroll.clone(), body)
                .footer(footer)
                .render(window),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_control::ClipDuck;

    fn range(start: i64, end: i64) -> TimeRange {
        TimeRange {
            start: Time(start * TICKS_PER_SECOND),
            end: Time(end * TICKS_PER_SECOND),
        }
    }

    fn proposal(voice: Vec<TimeRange>, clips: usize) -> DuckProposal {
        DuckProposal {
            revision: String::new(),
            ducked: voice.clone(),
            voice,
            clips: (0..clips)
                .map(|i| ClipDuck {
                    clip: ClipId(i as u64),
                    track: TrackId(1),
                    keys: vec![
                        animathio_control::DuckKey {
                            time: Time(0),
                            gain: 1.0,
                        };
                        2
                    ],
                })
                .collect(),
            replace_existing: false,
        }
    }

    fn project_with_clip(seconds: i64) -> Project {
        use animathio_core::{Asset, AssetId, Command};
        let id = AssetId::of(b"music");
        let mut project = Project::default();
        Command::Batch {
            commands: vec![
                Command::AddAsset {
                    id: id.clone(),
                    asset: Asset {
                        name: "music.wav".into(),
                        media_type: "audio/wav".into(),
                    },
                },
                Command::AddTrack {
                    track: Track::new(TrackId(1), "Music"),
                    index: 0,
                },
                Command::AddClip {
                    track: TrackId(1),
                    clip: Clip::new(ClipId(0), ClipKind::Audio(id), range(0, seconds)),
                },
            ],
        }
        .apply(&mut project)
        .unwrap();
        project
    }

    #[core::prelude::v1::test]
    fn summary_states_depth_regions_keyframes_and_undo() {
        let params = DuckParams::default();
        // Ducked spans (with ramps) differ from the voice and run past the 5 s clip.
        let mut p = proposal(vec![range(1, 2), range(4, 6)], 1);
        p.ducked = vec![range(0, 3), range(4, 8)];
        assert_eq!(
            duck_summary(&params, &p, &project_with_clip(5)),
            "Music drops by 12.0 dB under 2 voice regions (music lowered for 4.0 s, ramps included). Writes 2 keyframes on 1 clip as one undo step."
        );
        assert!(
            duck_summary(
                &params,
                &proposal(vec![range(1, 2)], 2),
                &project_with_clip(5)
            )
            .contains("on 2 clips")
        );
    }

    #[core::prelude::v1::test]
    fn summary_explains_empty_proposals() {
        let params = DuckParams::default();
        assert!(
            duck_summary(&params, &proposal(vec![], 0), &Project::default())
                .starts_with("No voice found")
        );
        let none = duck_summary(
            &params,
            &proposal(vec![range(1, 2)], 0),
            &Project::default(),
        );
        assert!(none.contains("1 voice region,") && none.contains("no chosen music clip"));
    }
}
