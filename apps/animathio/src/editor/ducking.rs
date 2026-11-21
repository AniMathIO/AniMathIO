//! "Auto duck…" panel: pick the voice source and music tracks, tune depth and ramps, review
//! the ducked spans on the timeline, then apply one undoable batch of gain keyframes.
use super::*;
use crate::style;
use animathio_control::{DuckParams, DuckProposal, duck_command, propose_duck, timecode};
use gpui_component::{
    Disableable,
    slider::{Slider, SliderEvent, SliderState},
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
pub(super) fn default_music_tracks(project: &Project, voice_clips: &[ClipId]) -> Vec<TrackId> {
    let is_voice = |clip: &Clip| {
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
    };
    project
        .tracks()
        .iter()
        .filter(|t| {
            !t.is_locked()
                && t.clips()
                    .values()
                    .any(|c| matches!(c.kind, ClipKind::Audio(_)))
                && !t.clips().values().any(is_voice)
        })
        .map(Track::id)
        .collect()
}

/// One sentence describing what applying `proposal` will do.
pub(super) fn duck_summary(params: &DuckParams, proposal: &DuckProposal) -> String {
    let keyframes: usize = proposal.clips.iter().map(|c| c.keys.len()).sum();
    if proposal.voice.is_empty() {
        return "No voice found. Lower the voice threshold or widen the analysed range.".into();
    }
    if proposal.clips.is_empty() {
        return format!(
            "Found {} voice region{}, but no chosen music clip overlaps the ducked spans.",
            proposal.voice.len(),
            plural(proposal.voice.len())
        );
    }
    let ducked: i64 = proposal.ducked.iter().map(|r| r.end.0 - r.start.0).sum();
    #[allow(clippy::cast_precision_loss)]
    let seconds = ducked as f64 / TICKS_PER_SECOND as f64;
    format!(
        "Music drops by {:.1} dB under {} voice region{} ({seconds:.1} s ducked). Writes {} keyframe{} on {} clip{} as one undo step.",
        params.depth_db,
        proposal.voice.len(),
        plural(proposal.voice.len()),
        keyframes,
        plural(keyframes),
        proposal.clips.len(),
        plural(proposal.clips.len()),
    )
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
                    review.error = Some("Project changed; analyse again".into());
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
            review.error = Some("Project changed; analyse again".into());
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
        timeline::display_tracks(&self.project)
            .into_iter()
            .enumerate()
            .filter(|(_, t)| {
                review.params.music_tracks.contains(&t.id())
                    || t.clips()
                        .keys()
                        .any(|id| review.params.music_clips.contains(id))
            })
            .flat_map(|(row, _)| proposal.ducked.iter().map(move |range| (*range, row)))
            .collect()
    }

    pub(super) fn duck_panel(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let review = self.duck_review.as_ref()?;
        if self.is_start_screen() {
            return None;
        }
        let theme = cx.theme().clone();
        let mut panel = div()
            .id("duck-review")
            .debug_selector(|| "duck-review".into())
            .absolute()
            .left(px(12.))
            .top(px(56.))
            .w(px(400.))
            .max_h((window.viewport_size().height - px(68.)).max(px(100.)))
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .rounded(px(style::radius::PANEL))
            .shadow_lg()
            .occlude()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(icons::icon(IconName::Volume2).small())
                    .child("Auto duck"),
            )
            .child(div().text_size(crate::style::type_size::XS).text_color(theme.muted_foreground).child(
                "Lower the music while someone speaks. Choose the voice, choose what to duck, check the shaded spans on the timeline, then Apply. Edit > Undo reverts it in one step.",
            ));
        let audio_tracks: Vec<(TrackId, String)> = self
            .project
            .tracks()
            .iter()
            .filter(|t| {
                t.clips()
                    .values()
                    .any(|c| matches!(c.kind, animathio_core::ClipKind::Audio(_)))
            })
            .map(|t| (t.id(), t.name().to_owned()))
            .collect();
        let chips = |panel: Stateful<Div>, title: &str, music: bool, cx: &mut Context<Self>| {
            let mut row = div().flex().flex_wrap().gap_1().items_center();
            for (id, name) in &audio_tracks {
                let on = if music {
                    review.params.music_tracks.contains(id)
                } else {
                    review.params.voice_tracks.contains(id)
                };
                let id = *id;
                row = row.child(
                    Button::new((
                        if music { "duck-music" } else { "duck-voice" },
                        id.0 as usize,
                    ))
                    .compact()
                    .icon(icons::icon(if on {
                        IconName::Check
                    } else {
                        IconName::Minus
                    }))
                    .label(name.clone())
                    .on_click(cx.listener(move |this, _, _, cx| {
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
                    })),
                );
            }
            panel
                .child(
                    div()
                        .text_size(crate::style::type_size::XS)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(title.to_owned()),
                )
                .child(row)
        };
        panel = chips(panel, "1. Voice: tracks", false, cx);
        let selected_audio = self.selected.is_some_and(|id| {
            self.find_clip(id).is_some_and(|c| {
                matches!(
                    c.kind,
                    animathio_core::ClipKind::Audio(_) | animathio_core::ClipKind::Video(_)
                )
            })
        });
        let clip_voice = !review.params.voice_clips.is_empty();
        panel = panel.child(
            Button::new("duck-voice-clip")
                .compact()
                .disabled(!selected_audio && !clip_voice)
                .icon(icons::icon(if clip_voice {
                    IconName::Check
                } else {
                    IconName::Minus
                }))
                .label("Use the selected clip as the voice")
                .on_click(cx.listener(|this, _, _, cx| {
                    let selected = this.selected;
                    if let Some(r) = this.duck_review.as_mut() {
                        if r.params.voice_clips.is_empty() {
                            r.params.voice_clips = selected.into_iter().collect();
                        } else {
                            r.params.voice_clips.clear();
                        }
                    }
                    this.analyse_duck(cx);
                })),
        );
        panel = chips(panel, "2. Music: tracks to duck", true, cx);
        let mut clip_targets = div().flex().flex_wrap().gap_1();
        for track in self.project.tracks() {
            for clip in track
                .clips()
                .values()
                .filter(|c| matches!(c.kind, animathio_core::ClipKind::Audio(_)))
            {
                let id = clip.id;
                let on = review.params.music_clips.contains(&id);
                clip_targets = clip_targets.child(
                    Button::new(("duck-music-clip", id.0 as usize))
                        .compact()
                        .debug_selector(move || format!("duck-target-{}", id.0))
                        .label(duck_clip_label(&self.project, track, clip))
                        .tooltip(format!("Clip {}", id.0))
                        .icon(icons::icon(if on {
                            IconName::Check
                        } else {
                            IconName::Minus
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(r) = this.duck_review.as_mut() {
                                if r.params.music_clips.contains(&id) {
                                    r.params.music_clips.retain(|c| *c != id);
                                } else {
                                    r.params.music_clips.push(id);
                                }
                            }
                            this.analyse_duck(cx);
                        })),
                );
            }
        }
        panel = panel
            .child(
                div()
                    .text_size(crate::style::type_size::XS)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("2. Music: individual clips to duck"),
            )
            .child(clip_targets);
        let labels = [
            format!("Duck depth: -{:.1} dB", review.params.depth_db),
            format!("Attack: {}", timecode(review.params.attack)),
            format!("Release: {}", timecode(review.params.release)),
            format!("Minimum gap to merge: {}", timecode(review.params.min_gap)),
            format!("Voice threshold: {:.0} dBFS", review.params.threshold_db),
        ];
        let timing_open = review.timing_open;
        for (index, (label, state)) in labels.into_iter().zip(&review.sliders).enumerate() {
            if index == 1 {
                panel = panel.child(
                    Button::new("duck-timing")
                        .debug_selector(|| "duck-timing".into())
                        .ghost()
                        .compact()
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
                );
            }
            if index == 0 || timing_open {
                panel = panel
                    .child(div().text_size(crate::style::type_size::XS).child(label))
                    .child(Slider::new(state).horizontal());
            }
        }
        let replace = review.params.replace_existing;
        panel = panel.child(
            Button::new("duck-replace")
                .compact()
                .icon(icons::icon(if replace {
                    IconName::Check
                } else {
                    IconName::Minus
                }))
                .label("Replace existing gain keyframes")
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(r) = this.duck_review.as_mut() {
                        r.params.replace_existing = !r.params.replace_existing;
                    }
                    this.analyse_duck(cx);
                })),
        );
        if !review.ready() {
            panel = panel.child(
                div()
                    .text_size(crate::style::type_size::XS)
                    .text_color(theme.muted_foreground)
                    .child("Choose a voice source and at least one music track or clip."),
            );
        }
        if let Some(error) = &review.error {
            panel = panel.child(
                div()
                    .text_size(crate::style::type_size::XS)
                    .text_color(theme.danger)
                    .child(error.clone()),
            );
        }
        if review.busy {
            panel = panel.child(
                div()
                    .text_size(crate::style::type_size::XS)
                    .child("Analysing audio..."),
            );
        }
        if let Some(proposal) = &review.proposal {
            panel = panel
                .child(
                    div()
                        .text_size(crate::style::type_size::XS)
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("3. Preview"),
                )
                .child(
                    div()
                        .id("duck-summary")
                        .debug_selector(|| "duck-summary".into())
                        .text_size(crate::style::type_size::XS)
                        .child(duck_summary(&review.params, proposal)),
                )
                .child(
                    div()
                        .text_size(crate::style::type_size::XS)
                        .text_color(theme.muted_foreground)
                        .child(
                            "Shaded spans on the music tracks show where the level is lowered. The play button beside a voice region moves the playhead to where that region starts.",
                        ),
                );
            let mut list = div()
                .id("duck-list")
                .flex_shrink_0()
                .max_h(px(120.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1();
            for (i, range) in proposal.voice.iter().enumerate() {
                let start = range.start;
                list = list.child(
                    div()
                        .flex()
                        .gap_1()
                        .items_center()
                        .child(div().text_size(crate::style::type_size::XS).child(format!(
                            "{} - {}",
                            timecode(range.start),
                            timecode(range.end)
                        )))
                        .child(
                            Button::new(("duck-seek", i))
                                .ghost()
                                .compact()
                                .icon(icons::icon(IconName::Play))
                                .tooltip("Move the playhead to the start of this voice region")
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.seek_to(start, cx)),
                                ),
                        ),
                );
            }
            panel = panel.child(list);
        }
        let busy = review.busy;
        let can_apply = !busy
            && review
                .proposal
                .as_ref()
                .is_some_and(|p| !p.clips.is_empty());
        panel = panel.child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    Button::new("duck-analyse")
                        .compact()
                        .label("Analyse")
                        .disabled(busy || !review.ready())
                        .on_click(cx.listener(|this, _, _, cx| this.analyse_duck(cx))),
                )
                .child(
                    Button::new("duck-cancel")
                        .debug_selector(|| "duck-cancel".into())
                        .compact()
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.duck_review = None;
                            cx.notify();
                        })),
                )
                .child(
                    style::primary_button("duck-apply", cx)
                        .compact()
                        .label("Apply")
                        .disabled(!can_apply)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.apply_duck(cx);
                        })),
                ),
        );
        Some(panel)
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

    #[core::prelude::v1::test]
    fn summary_states_depth_regions_keyframes_and_undo() {
        let params = DuckParams::default();
        assert_eq!(
            duck_summary(&params, &proposal(vec![range(1, 2), range(4, 6)], 1)),
            "Music drops by 12.0 dB under 2 voice regions (3.0 s ducked). Writes 2 keyframes on 1 clip as one undo step."
        );
        assert!(duck_summary(&params, &proposal(vec![range(1, 2)], 2)).contains("on 2 clips"));
    }

    #[core::prelude::v1::test]
    fn summary_explains_empty_proposals() {
        let params = DuckParams::default();
        assert!(duck_summary(&params, &proposal(vec![], 0)).starts_with("No voice found"));
        let none = duck_summary(&params, &proposal(vec![range(1, 2)], 0));
        assert!(none.contains("1 voice region,") && none.contains("no chosen music clip"));
    }
}
