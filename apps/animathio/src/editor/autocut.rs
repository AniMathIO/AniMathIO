//! Automatic-cut review panel and background analysis. Drafts are invalidated on
//! edits or setting changes; applying enabled cuts is one command history entry.

use super::tool_panel;
use super::*;
use crate::style::{self, SegmentedOption};
use animathio_control::{
    AutocutParams, AutocutProposal, autocut_command, propose_autocuts_cancellable, timecode,
};
use gpui_component::{
    Disableable,
    slider::{SliderEvent, SliderState},
};

/// What the review analyses: the selected clip, its track's mix, or the whole project
/// (or the in/out range).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Scope {
    Clip,
    Track,
    Project,
}

impl Scope {
    const ALL: [Self; 3] = [Self::Clip, Self::Track, Self::Project];

    fn of(params: &AutocutParams) -> Self {
        if params.clip.is_some() {
            Self::Clip
        } else if params.track.is_some() {
            Self::Track
        } else {
            Self::Project
        }
    }

    /// Point `params` at this scope; `track` is the selection's audio track, if any.
    fn apply(
        self,
        params: &mut AutocutParams,
        selected: Option<ClipId>,
        track: Option<animathio_core::TrackId>,
    ) {
        params.clip = None;
        params.track = None;
        match self {
            Self::Clip => params.clip = selected,
            Self::Track => params.track = track,
            Self::Project => {}
        }
    }
}

pub(super) struct Review {
    cancel: animathio_transcribe::Cancel,
    identity: Arc<()>,
    pub(super) params: AutocutParams,
    pub(super) proposal: Option<AutocutProposal>,
    sliders: Vec<Entity<SliderState>>,
    _subscriptions: Vec<Subscription>,
    generation: u64,
    busy: bool,
    error: Option<String>,
    /// Scroll position of the panel body.
    scroll: ScrollHandle,
    /// Keyboard focus for the segmented controls (one handle per option).
    scope_focus: Vec<FocusHandle>,
    language_focus: Vec<FocusHandle>,
}

impl Drop for Review {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Editor {
    /// The track holding the selected clip's audio (a video clip resolves to its linked
    /// audio companion).
    fn selected_audio_track(&self) -> Option<animathio_core::TrackId> {
        let clip = self.selected.and_then(|id| self.find_clip(id))?;
        let audio = if matches!(clip.kind, animathio_core::ClipKind::Video(_)) {
            self.project
                .tracks()
                .iter()
                .flat_map(|t| t.clips().values())
                .find(|other| {
                    matches!(other.kind, animathio_core::ClipKind::Audio(_))
                        && clip.link_group.is_some()
                        && other.link_group == clip.link_group
                })
                .map(|other| other.id)?
        } else {
            clip.id
        };
        self.project
            .tracks()
            .iter()
            .find(|t| t.clips().contains_key(&audio))
            .map(Track::id)
    }

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
            scroll: ScrollHandle::new(),
            scope_focus: tool_panel::focus_handles(Scope::ALL.len(), cx),
            language_focus: tool_panel::focus_handles(3, cx),
        });
        self.analyse_autocut(cx);
    }

    pub(super) fn set_autocut_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        let track = self.selected_audio_track();
        let selected = self.selected;
        if let Some(review) = self.autocut_review.as_mut() {
            scope.apply(&mut review.params, selected, track);
        }
        self.analyse_autocut(cx);
    }

    /// Apply the enabled cuts of the current proposal as one undo step and close the panel.
    pub(super) fn apply_autocut(&mut self, cx: &mut Context<Self>) {
        let proposal = self
            .autocut_review
            .as_ref()
            .and_then(|r| r.proposal.clone());
        if let Some(p) = proposal {
            match autocut_command(&self.project, &p) {
                Ok(command) => {
                    if self.try_execute(&command, cx) {
                        self.autocut_review = None;
                    }
                }
                Err(error) => self.error = Some(error.to_string()),
            }
        }
        cx.notify();
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
                    review.error = Some("Project changed; analyze again".into());
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
            review.error = Some("Project changed; analyze again".into());
        }
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn autocut_panel(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let review = self.autocut_review.as_ref()?;
        if self.is_start_screen() {
            return None;
        }
        let entity = cx.entity();
        let fillers = review.params.fillers;
        let scoped = review.params.clip.is_some() || review.params.track.is_some();

        // What is analysed.
        let mut target = vec![
            tool_panel::note(if review.params.clip.is_some() {
                "Selected clip and linked companions"
            } else if review.params.track.is_some() {
                "Selected track mix and linked companions"
            } else {
                "Mixed project audio or the in and out range"
            })
            .into_any_element(),
        ];
        if self.selected.is_some() {
            let current = Scope::of(&review.params);
            let entity = entity.clone();
            target.insert(
                0,
                style::form_row(
                    "Analyze",
                    tool_panel::choice(
                        "autocut-scope",
                        &[
                            SegmentedOption::new("autocut-scope-Clip", "Clip", None),
                            SegmentedOption::new("autocut-scope-Track", "Track", None),
                            SegmentedOption::new("autocut-scope-Project", "Project", None),
                        ],
                        &review.scope_focus,
                        Scope::ALL.iter().position(|s| *s == current).unwrap_or(0),
                        move |index, _, cx| {
                            let scope = Scope::ALL[index.min(Scope::ALL.len() - 1)];
                            entity.update(cx, |this, cx| this.set_autocut_scope(scope, cx));
                        },
                    ),
                )
                .into_any_element(),
            );
        }

        // Detection: three sliders, each with its value.
        let values = [
            format!("{:.0} dBFS", review.params.silence_db),
            tool_panel::seconds_label(review.params.min_silence, 1),
            tool_panel::seconds_label(review.params.padding_before, 2),
        ];
        let detection: Vec<AnyElement> = ["Silence threshold", "Minimum pause", "Speech padding"]
            .into_iter()
            .zip(&review.sliders)
            .zip(values)
            .map(|((label, state), value)| {
                tool_panel::slider_row(label, state, value).into_any_element()
            })
            .collect();

        // Options: switches that re-run the analysis.
        let mut options = vec![
            style::switch_row(
                "autocut-short-pauses",
                "Keep short pauses",
                Some("Leave the natural breaths between words."),
                review.params.keep_short_pauses,
                {
                    let entity = entity.clone();
                    move |checked, _, cx| {
                        let checked = *checked;
                        entity.update(cx, |this, cx| {
                            if let Some(r) = this.autocut_review.as_mut() {
                                r.params.keep_short_pauses = checked;
                            }
                            this.analyse_autocut(cx);
                        });
                    }
                },
            )
            .into_any_element(),
            style::switch_row(
                "autocut-fillers",
                "Fillers and repeats",
                Some("Also cut filler words and repeated phrases."),
                fillers,
                {
                    let entity = entity.clone();
                    move |checked, _, cx| {
                        let checked = *checked;
                        entity.update(cx, |this, cx| {
                            if let Some(r) = this.autocut_review.as_mut() {
                                r.params.fillers = checked;
                            }
                            this.analyse_autocut(cx);
                        });
                    }
                },
            )
            .into_any_element(),
        ];
        if scoped {
            options.push(
                style::switch_row(
                    "autocut-all-tracks",
                    "Cut all unlocked tracks",
                    Some("Off cuts only the selection and its linked tracks."),
                    review.params.all_unlocked_tracks,
                    {
                        let entity = entity.clone();
                        move |checked, _, cx| {
                            let checked = *checked;
                            entity.update(cx, |this, cx| {
                                if let Some(r) = this.autocut_review.as_mut() {
                                    r.params.all_unlocked_tracks = checked;
                                }
                                this.analyse_autocut(cx);
                            });
                        }
                    },
                )
                .into_any_element(),
            );
        }
        let language_index = match review.params.language.as_deref() {
            Some("en") => 1,
            Some("hu") => 2,
            _ => 0,
        };
        options.push(
            style::form_row(
                "Language",
                tool_panel::choice(
                    "autocut-language",
                    &[
                        SegmentedOption::new("autocut-language-auto", "Auto", None),
                        SegmentedOption::new("autocut-language-en", "English", None),
                        SegmentedOption::new("autocut-language-hu", "Hungarian", None),
                    ],
                    &review.language_focus,
                    language_index,
                    {
                        let entity = entity.clone();
                        move |index, _, cx| {
                            entity.update(cx, |this, cx| {
                                if let Some(r) = this.autocut_review.as_mut() {
                                    r.params.language = match index {
                                        1 => Some("en".into()),
                                        2 => Some("hu".into()),
                                        _ => None,
                                    };
                                }
                                this.analyse_autocut(cx);
                            });
                        }
                    },
                ),
            )
            .into_any_element(),
        );
        if fillers && !animathio_transcribe::engine_available() {
            options.push(
                tool_panel::note(
                    "This build has no speech engine, so filler words cannot be found. Release installers include one; from source, build with the whisper feature.",
                )
                .into_any_element(),
            );
        }

        let mut body = tool_panel::body()
            .child(tool_panel::section("Target", target))
            .child(tool_panel::section("Detection", detection))
            .child(tool_panel::section("Options", options));
        if let Some(error) = &review.error {
            body = body.child(tool_panel::problem(error.clone()));
        }
        if review.busy {
            body = body.child(tool_panel::note("Analyzing audio…"));
        }
        if let Some(proposal) = &review.proposal {
            let enabled: Vec<_> = proposal.cuts.iter().filter(|c| c.enabled).collect();
            let saved = Time(
                enabled
                    .iter()
                    .map(|c| c.range.end.0 - c.range.start.0)
                    .sum(),
            );
            let mut rows: Vec<AnyElement> = vec![
                tool_panel::note(format!(
                    "{} of {} cuts, {} saved",
                    enabled.len(),
                    proposal.cuts.len(),
                    tool_panel::seconds_label(saved, 1)
                ))
                .into_any_element(),
            ];
            let mut list = div().id("autocut-list").flex().flex_col();
            for (i, cut) in proposal.cuts.iter().enumerate() {
                let start = cut.range.start;
                let reason = match cut.reason {
                    animathio_control::AutocutReason::Silence => "Pause",
                    animathio_control::AutocutReason::Filler => "Filler",
                    animathio_control::AutocutReason::Repeat => "Repeat",
                };
                list = list.child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            tool_panel::check_row(
                                ("autocut-toggle", i),
                                None,
                                cut.enabled,
                                format!(
                                    "{} - {}  {reason} ({:.0}%)",
                                    timecode(cut.range.start),
                                    timecode(cut.range.end),
                                    cut.confidence * 100.
                                ),
                                cx.listener(move |this, _, _, cx| {
                                    if let Some(p) = this
                                        .autocut_review
                                        .as_mut()
                                        .and_then(|r| r.proposal.as_mut())
                                    {
                                        p.cuts[i].enabled = !p.cuts[i].enabled;
                                    }
                                    cx.notify();
                                }),
                            )
                            .flex_1()
                            .min_w_0(),
                        )
                        .child(
                            Button::new(("autocut-seek", i))
                                .ghost()
                                .xsmall()
                                .icon(icons::icon(IconName::Play))
                                .tooltip("Preview cut start")
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.seek_to(start, cx)),
                                ),
                        ),
                );
            }
            rows.push(list.into_any_element());
            body = body.child(tool_panel::section("Proposed cuts", rows));
        }

        let busy = review.busy;
        let can_apply = !busy
            && review
                .proposal
                .as_ref()
                .is_some_and(|p| p.cuts.iter().any(|c| c.enabled));
        let footer = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_wrap()
            .justify_end()
            .gap_2()
            .child(
                style::secondary_button("autocut-analyse", cx)
                    .debug_selector(|| "autocut-analyse".into())
                    .small()
                    .label("Analyze")
                    .disabled(busy)
                    .on_click(cx.listener(|this, _, _, cx| this.analyse_autocut(cx))),
            )
            .when(fillers && animathio_transcribe::engine_available(), |row| {
                row.child(
                    style::secondary_button("autocut-download", cx)
                        .small()
                        .label("Download model and analyze")
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
                style::secondary_button("autocut-cancel", cx)
                    .debug_selector(|| "autocut-cancel".into())
                    .small()
                    .label("Cancel")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.autocut_review = None;
                        cx.notify();
                    })),
            )
            .child(
                style::primary_button("autocut-apply", cx)
                    .debug_selector(|| "autocut-apply".into())
                    .small()
                    .label("Apply")
                    .disabled(!can_apply)
                    .on_click(cx.listener(|this, _, _, cx| this.apply_autocut(cx))),
            );
        Some(
            style::ToolPanel::new("autocut-review", IconName::Scissors, "Automatic cuts")
                .bottom_height(self.dock.bottom_height)
                .on_close(cx.listener(|this, _, _, cx| {
                    this.autocut_review = None;
                    cx.notify();
                }))
                .body(review.scroll.clone(), body)
                .footer(footer)
                .render(window),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::TrackId;

    #[core::prelude::v1::test]
    fn scope_picker_selects_exactly_one_target_and_round_trips() {
        let mut params = AutocutParams {
            clip: Some(ClipId(3)),
            ..AutocutParams::default()
        };
        assert_eq!(Scope::of(&params), Scope::Clip);

        Scope::Track.apply(&mut params, Some(ClipId(3)), Some(TrackId(7)));
        assert_eq!((params.clip, params.track), (None, Some(TrackId(7))));
        assert_eq!(Scope::of(&params), Scope::Track);

        Scope::Project.apply(&mut params, Some(ClipId(3)), Some(TrackId(7)));
        assert_eq!((params.clip, params.track), (None, None));
        assert_eq!(Scope::of(&params), Scope::Project);

        Scope::Clip.apply(&mut params, Some(ClipId(3)), Some(TrackId(7)));
        assert_eq!((params.clip, params.track), (Some(ClipId(3)), None));

        // A selection without an audio track falls back to the project scope rather than
        // keeping a stale target.
        Scope::Track.apply(&mut params, Some(ClipId(3)), None);
        assert_eq!(Scope::of(&params), Scope::Project);
    }
}
