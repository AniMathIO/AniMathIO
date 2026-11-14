//! Automatic-cut review panel and background analysis. Drafts are invalidated on
//! edits or setting changes; applying enabled cuts is one command history entry.
use super::*;
use crate::style;
use animathio_control::{
    AutocutParams, AutocutProposal, autocut_command, propose_autocuts_cancellable, timecode,
};
use gpui_component::{
    Disableable,
    slider::{Slider, SliderEvent, SliderState},
};

pub(super) struct Review {
    cancel: animathio_transcribe::Cancel,
    identity: Arc<()>,
    params: AutocutParams,
    pub(super) proposal: Option<AutocutProposal>,
    sliders: Vec<Entity<SliderState>>,
    _subscriptions: Vec<Subscription>,
    generation: u64,
    busy: bool,
    error: Option<String>,
}

impl Drop for Review {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Editor {
    pub(super) fn open_autocut(&mut self, cx: &mut Context<Self>) {
        let params = AutocutParams {
            clip: self.selected,
            range: self
                .marks
                .range(animathio_export::duration(&self.project).0),
            ..AutocutParams::default()
        };
        let mut sliders = Vec::new();
        let mut subscriptions = Vec::new();
        for (i, min, max, step, value) in [
            (0, -80., -10., 1., -40.),
            (1, 0.1, 3., 0.1, 0.5),
            (2, 0., 0.5, 0.01, 0.1),
        ] {
            let state = cx.new(|_| {
                SliderState::new()
                    .min(min)
                    .max(max)
                    .step(step)
                    .default_value(value)
            });
            subscriptions.push(
                cx.subscribe(
                    &state,
                    move |this, _, event: &SliderEvent, cx| match event {
                        SliderEvent::Change(value) => {
                            if let Some(review) = this.autocut_review.as_mut() {
                                let value = f64::from(value.start());
                                #[allow(
                                    clippy::cast_possible_truncation,
                                    clippy::cast_precision_loss
                                )]
                                match i {
                                    0 => review.params.silence_db = value,
                                    1 => {
                                        review.params.min_silence =
                                            Time((value * TICKS_PER_SECOND as f64).round() as i64);
                                    }
                                    _ => {
                                        let padding =
                                            Time((value * TICKS_PER_SECOND as f64).round() as i64);
                                        review.params.padding_before = padding;
                                        review.params.padding_after = padding;
                                    }
                                }
                                review
                                    .cancel
                                    .store(true, std::sync::atomic::Ordering::Relaxed);
                                review.proposal = None;
                                review.generation = review.generation.wrapping_add(1);
                                review.busy = false;
                                cx.notify();
                            }
                        }
                        SliderEvent::Release(_) => this.analyse_autocut(cx),
                    },
                ),
            );
            sliders.push(state);
        }
        self.autocut_review = Some(Review {
            cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            identity: Arc::new(()),
            params,
            proposal: None,
            sliders,
            _subscriptions: subscriptions,
            generation: 0,
            busy: false,
            error: None,
        });
        self.analyse_autocut(cx);
    }

    fn analyse_autocut(&mut self, cx: &mut Context<Self>) {
        let Some(review) = self.autocut_review.as_mut() else {
            return;
        };
        review
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        review.cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel = review.cancel.clone();
        let identity = review.identity.clone();
        review.generation = review.generation.wrapping_add(1);
        let generation = review.generation;
        review.busy = true;
        review.error = None;
        review.proposal = None;
        let params = review.params.clone();
        let project = self.project.clone();
        let assets = self.assets.clone();
        let audio_generation = self.audio_generation;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    propose_autocuts_cancellable(&project, &assets, &params, &cancel)
                })
                .await;
            let _ = this.update(cx, |editor, cx| {
                let Some(review) = editor.autocut_review.as_mut() else {
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

    pub(super) fn invalidate_autocut(&mut self) {
        if let Some(review) = self.autocut_review.as_mut() {
            review
                .cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
            review.proposal = None;
            review.generation = review.generation.wrapping_add(1);
            review.busy = false;
            review.error = Some("Project changed; analyse again".into());
        }
    }

    pub(super) fn autocut_panel(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let review = self.autocut_review.as_ref()?;
        if self.is_start_screen() {
            return None;
        }
        let theme = cx.theme().clone();
        let mut panel = div()
            .id("autocut-review")
            .absolute()
            .left(px(12.))
            .top(px(56.))
            .w(px(380.))
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
                    .child(icons::icon(IconName::Scissors).small())
                    .child("Automatic cuts"),
            )
            .child(div().text_xs().text_color(theme.muted_foreground).child(
                if review.params.clip.is_some() {
                    "Selected clip and linked companions"
                } else if review.params.track.is_some() {
                    "Selected track mix and linked companions"
                } else {
                    "Mixed project audio / in-out range"
                },
            ));
        if self.selected.is_some() {
            panel = panel.child(
                Button::new("autocut-scope")
                    .compact()
                    .label("Change audio: clip / track / project")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let selected = this.selected.and_then(|id| this.find_clip(id));
                        let audio_id = selected.and_then(|c| {
                            if matches!(c.kind, animathio_core::ClipKind::Video(_)) {
                                this.project
                                    .tracks()
                                    .iter()
                                    .flat_map(|t| t.clips().values())
                                    .find(|a| {
                                        matches!(a.kind, animathio_core::ClipKind::Audio(_))
                                            && c.link_group.is_some()
                                            && a.link_group == c.link_group
                                    })
                                    .map(|a| a.id)
                            } else {
                                Some(c.id)
                            }
                        });
                        let track = audio_id.and_then(|id| {
                            this.project
                                .tracks()
                                .iter()
                                .find(|t| t.clips().contains_key(&id))
                                .map(Track::id)
                        });
                        if let Some(r) = this.autocut_review.as_mut() {
                            if r.params.clip.take().is_some() {
                                r.params.track = track;
                            } else if r.params.track.take().is_none() {
                                r.params.clip = this.selected;
                            }
                        }
                        this.analyse_autocut(cx);
                    })),
            );
        }
        let labels = [
            format!("Silence threshold: {:.0} dBFS", review.params.silence_db),
            format!("Minimum pause: {}", timecode(review.params.min_silence)),
            format!("Speech padding: {}", timecode(review.params.padding_before)),
        ];
        for (label, state) in labels.into_iter().zip(&review.sliders) {
            panel = panel
                .child(div().text_xs().child(label))
                .child(Slider::new(state).horizontal());
        }
        let keep = review.params.keep_short_pauses;
        let all = review.params.all_unlocked_tracks;
        let scoped = review.params.clip.is_some() || review.params.track.is_some();
        let fillers = review.params.fillers;
        panel = panel.child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    Button::new("autocut-short-pauses")
                        .compact()
                        .label(if keep {
                            "Keep short pauses: on"
                        } else {
                            "Keep short pauses: off"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(r) = this.autocut_review.as_mut() {
                                r.params.keep_short_pauses = !r.params.keep_short_pauses;
                            }
                            this.analyse_autocut(cx);
                        })),
                )
                .child(
                    Button::new("autocut-all-tracks")
                        .compact()
                        .disabled(!scoped)
                        .label(if all || !scoped {
                            "All unlocked tracks"
                        } else {
                            "Selected + linked tracks"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(r) = this.autocut_review.as_mut() {
                                r.params.all_unlocked_tracks = !r.params.all_unlocked_tracks;
                            }
                            this.analyse_autocut(cx);
                        })),
                )
                .child(
                    Button::new("autocut-fillers")
                        .compact()
                        .label(if fillers {
                            "Fillers + repeats: on"
                        } else {
                            "Fillers + repeats: off"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(r) = this.autocut_review.as_mut() {
                                r.params.fillers = !r.params.fillers;
                            }
                            this.analyse_autocut(cx);
                        })),
                )
                .child(
                    Button::new("autocut-language")
                        .compact()
                        .label(format!(
                            "Language: {}",
                            review.params.language.as_deref().unwrap_or("auto")
                        ))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(r) = this.autocut_review.as_mut() {
                                r.params.language = match r.params.language.as_deref() {
                                    None => Some("en".into()),
                                    Some("en") => Some("hu".into()),
                                    _ => None,
                                };
                            }
                            this.analyse_autocut(cx);
                        })),
                ),
        );
        if fillers && !animathio_transcribe::engine_available() {
            panel = panel.child(
                div()
                    .text_xs()
                    .child("This build has no speech engine. Rebuild with the whisper feature."),
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
            let enabled: Vec<_> = proposal.cuts.iter().filter(|c| c.enabled).collect();
            let saved = Time(
                enabled
                    .iter()
                    .map(|c| c.range.end.0 - c.range.start.0)
                    .sum(),
            );
            panel = panel.child(div().text_xs().child(format!(
                "{} of {} cuts / {} saved",
                enabled.len(),
                proposal.cuts.len(),
                timecode(saved)
            )));
            let mut list = div()
                .id("autocut-list")
                .max_h(px(170.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap_1();
            for (i, cut) in proposal.cuts.iter().enumerate() {
                let start = cut.range.start;
                list = list.child(
                    div()
                        .flex()
                        .gap_1()
                        .items_center()
                        .child(
                            Button::new(("autocut-toggle", i))
                                .ghost()
                                .compact()
                                .icon(icons::icon(if cut.enabled {
                                    IconName::Check
                                } else {
                                    IconName::Minus
                                }))
                                .label(format!(
                                    "{} - {}  {:?} ({:.0}%)",
                                    timecode(cut.range.start),
                                    timecode(cut.range.end),
                                    cut.reason,
                                    cut.confidence * 100.
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(p) = this
                                        .autocut_review
                                        .as_mut()
                                        .and_then(|r| r.proposal.as_mut())
                                    {
                                        p.cuts[i].enabled = !p.cuts[i].enabled;
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new(("autocut-seek", i))
                                .ghost()
                                .compact()
                                .icon(icons::icon(IconName::Play))
                                .tooltip("Preview cut start")
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
                .is_some_and(|p| p.cuts.iter().any(|c| c.enabled));
        panel = panel.child(
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    Button::new("autocut-analyse")
                        .compact()
                        .label("Analyse")
                        .disabled(busy)
                        .on_click(cx.listener(|this, _, _, cx| this.analyse_autocut(cx))),
                )
                .when(fillers && animathio_transcribe::engine_available(), |row| {
                    row.child(
                        Button::new("autocut-download")
                            .compact()
                            .label("Download model & analyse")
                            .disabled(busy)
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(r) = this.autocut_review.as_mut() {
                                    r.params.download_model = true;
                                }
                                this.analyse_autocut(cx);
                            })),
                    )
                })
                .child(
                    Button::new("autocut-cancel")
                        .compact()
                        .label("Cancel")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.autocut_review = None;
                            cx.notify();
                        })),
                )
                .child(
                    style::primary_button("autocut-apply", cx)
                        .compact()
                        .label("Apply")
                        .disabled(!can_apply)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let proposal = this
                                .autocut_review
                                .as_ref()
                                .and_then(|r| r.proposal.clone());
                            if let Some(p) = proposal {
                                match autocut_command(&this.project, &p) {
                                    Ok(command) => {
                                        if this.try_execute(&command, cx) {
                                            this.autocut_review = None;
                                        }
                                    }
                                    Err(error) => this.error = Some(error.to_string()),
                                }
                            }
                            cx.notify();
                        })),
                ),
        );
        Some(panel)
    }
}
