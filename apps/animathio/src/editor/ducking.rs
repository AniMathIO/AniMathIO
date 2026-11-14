//! "Auto duck…" panel: pick the voice source and music tracks, tune depth and ramps, review
//! the ducked spans on the timeline, then apply one undoable batch of gain keyframes.
use super::*;
use crate::style;
use animathio_control::{DuckParams, DuckProposal, duck_command, propose_duck, timecode};
use gpui_component::{
    Disableable,
    slider::{Slider, SliderEvent, SliderState},
};

pub(super) struct DuckReview {
    identity: Arc<()>,
    pub(super) params: DuckParams,
    pub(super) proposal: Option<DuckProposal>,
    sliders: Vec<Entity<SliderState>>,
    _subscriptions: Vec<Subscription>,
    generation: u64,
    busy: bool,
    error: Option<String>,
}

fn ticks(seconds: f64) -> Time {
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    Time((seconds * TICKS_PER_SECOND as f64).round() as i64)
}

impl DuckReview {
    fn ready(&self) -> bool {
        (!self.params.voice_clips.is_empty() || !self.params.voice_tracks.is_empty())
            && !self.params.music_tracks.is_empty()
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
        let params = DuckParams {
            voice_clips: self.selected.into_iter().collect(),
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
        self.project
            .tracks()
            .iter()
            .enumerate()
            .filter(|(_, t)| review.params.music_tracks.contains(&t.id()))
            .flat_map(|(row, _)| proposal.ducked.iter().map(move |range| (*range, row)))
            .collect()
    }

    pub(super) fn duck_panel(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let review = self.duck_review.as_ref()?;
        if self.is_start_screen() {
            return None;
        }
        let theme = cx.theme().clone();
        let mut panel = div()
            .id("duck-review")
            .absolute()
            .left(px(12.))
            .top(px(56.))
            .w(px(400.))
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
            .child(div().text_xs().text_color(theme.muted_foreground).child(
                "Lower the music under speech. Pick where the voice is and which tracks to duck.",
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
                .child(div().text_xs().child(title.to_owned()))
                .child(row)
        };
        panel = chips(panel, "Voice tracks", false, cx);
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
                .label("Selected clip is the voice")
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
        panel = chips(panel, "Music tracks to duck", true, cx);
        let labels = [
            format!("Duck depth: -{:.1} dB", review.params.depth_db),
            format!("Attack: {}", timecode(review.params.attack)),
            format!("Release: {}", timecode(review.params.release)),
            format!("Minimum gap to merge: {}", timecode(review.params.min_gap)),
            format!("Voice threshold: {:.0} dBFS", review.params.threshold_db),
        ];
        for (label, state) in labels.into_iter().zip(&review.sliders) {
            panel = panel
                .child(div().text_xs().child(label))
                .child(Slider::new(state).horizontal());
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
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Choose a voice source and at least one music track."),
            );
        }
        if let Some(error) = &review.error {
            panel = panel.child(
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(error.clone()),
            );
        }
        if review.busy {
            panel = panel.child(div().text_xs().child("Analysing audio..."));
        }
        if let Some(proposal) = &review.proposal {
            let keyframes: usize = proposal.clips.iter().map(|c| c.keys.len()).sum();
            panel = panel.child(div().text_xs().child(format!(
                "{} voice regions / {} music clips / {} keyframes",
                proposal.voice.len(),
                proposal.clips.len(),
                keyframes
            )));
            let mut list = div()
                .id("duck-list")
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
                        .child(div().text_xs().child(format!(
                            "{} - {}",
                            timecode(range.start),
                            timecode(range.end)
                        )))
                        .child(
                            Button::new(("duck-seek", i))
                                .ghost()
                                .compact()
                                .icon(icons::icon(IconName::Play))
                                .tooltip("Preview voice start")
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
