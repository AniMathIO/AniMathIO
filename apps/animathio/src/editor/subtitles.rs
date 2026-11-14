//! Generate subtitles: dialog, job queue slot, progress popover and control calls.
//! Mirrors the export queue: a status-bar button opens a popover with progress and cancel.

use super::*;
use crate::style;
use crate::subtitle_job::{Phase, State, SubtitleJob};
use animathio_control::{SubtitleGenerateParams, SubtitleResult, subtitle_command};
use animathio_transcribe::{ModelSize, models};
use gpui_component::Disableable;
use gpui_component::WindowExt;
use gpui_component::form::{Field as FormField, Form};
use gpui_component::progress::Progress;
use gpui_component::select::{Select, SelectState};

/// Languages offered in the dialog; `auto` lets Whisper detect it.
const LANGUAGES: [(&str, &str); 21] = [
    ("auto", "Auto-detect"),
    ("en", "English"),
    ("es", "Spanish"),
    ("de", "German"),
    ("fr", "French"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("cs", "Czech"),
    ("hu", "Hungarian"),
    ("ro", "Romanian"),
    ("sv", "Swedish"),
    ("tr", "Turkish"),
    ("uk", "Ukrainian"),
    ("ru", "Russian"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh", "Chinese"),
];

fn model_label(size: ModelSize) -> String {
    let info = size.info();
    format!(
        "{} ({}, {})",
        info.name,
        models::display_size(info.bytes),
        if size.is_installed() {
            "installed"
        } else {
            "downloads once"
        }
    )
}

impl Editor {
    pub(super) fn open_subtitles_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .subtitle_job
            .as_ref()
            .is_some_and(|j| j.state == State::Running)
        {
            self.subtitle_panel_open = true;
            cx.notify();
            return;
        }
        let model_items: Vec<String> = ModelSize::ALL.into_iter().map(model_label).collect();
        let model = cx.new(|cx| {
            SelectState::new(
                model_items,
                Some(gpui_component::IndexPath::default().row(1)),
                window,
                cx,
            )
        });
        let language = cx.new(|cx| {
            SelectState::new(
                LANGUAGES
                    .iter()
                    .map(|(_, name)| (*name).to_owned())
                    .collect::<Vec<_>>(),
                Some(gpui_component::IndexPath::default()),
                window,
                cx,
            )
        });
        let mut ranges = vec!["Whole project".to_owned()];
        let clip_range = self.selected.is_some();
        if clip_range {
            ranges.push("Selected clip".to_owned());
        }
        let marked = self.marks.marked();
        if marked {
            ranges.push("In/out range".to_owned());
        }
        let range_default = usize::from(clip_range);
        let range = cx.new(|cx| {
            SelectState::new(
                ranges,
                Some(gpui_component::IndexPath::default().row(range_default)),
                window,
                cx,
            )
        });
        let editor = cx.entity();
        let engine = animathio_transcribe::engine_available();
        window.open_dialog(cx, move |dialog, _, cx| {
            let (editor, model, language, range) = (
                editor.clone(),
                model.clone(),
                language.clone(),
                range.clone(),
            );
            let note = if engine {
                "Speech recognition runs on this computer; nothing is uploaded. A model that is \
                 not installed yet is downloaded once from Hugging Face and checked with \
                 SHA-256 when you press Generate."
            } else {
                "This build of AniMathIO has no speech engine. Rebuild it with the `whisper` \
                 cargo feature to enable automatic subtitles."
            };
            dialog
                .title(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .text_size(px(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(
                            icons::icon(IconName::Captions)
                                .text_color(style::hex(style::color::ACCENT)),
                        )
                        .child("Generate subtitles"),
                )
                .width(px(520.))
                .child(
                    div()
                        .mb_3()
                        .text_size(px(12.))
                        .text_color(style::hex(style::color::TEXT_SECONDARY))
                        .child(note),
                )
                .child(
                    Form::new()
                        .child(FormField::new().label("Model").child(Select::new(&model)))
                        .child(
                            FormField::new()
                                .label("Language")
                                .child(Select::new(&language)),
                        )
                        .child(FormField::new().label("Audio").child(Select::new(&range))),
                )
                .footer(
                    div()
                        .flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("subtitles-cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            style::primary_button("subtitles-generate", cx)
                                .label("Generate")
                                .disabled(!engine)
                                .on_click(move |_, window, cx| {
                                    let size = ModelSize::ALL
                                        .get(model.read(cx).selected_index(cx).map_or(1, |i| i.row))
                                        .copied()
                                        .unwrap_or_default();
                                    let code = LANGUAGES
                                        .get(
                                            language
                                                .read(cx)
                                                .selected_index(cx)
                                                .map_or(0, |i| i.row),
                                        )
                                        .map_or("auto", |(code, _)| code);
                                    let choice = range
                                        .read(cx)
                                        .selected_value()
                                        .cloned()
                                        .unwrap_or_default();
                                    editor.update(cx, |this, cx| {
                                        this.start_subtitles_from_dialog(size, code, &choice, cx);
                                    });
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
    }

    fn start_subtitles_from_dialog(
        &mut self,
        size: ModelSize,
        language: &str,
        range_choice: &str,
        cx: &mut Context<Self>,
    ) {
        let mut params = SubtitleGenerateParams {
            model: size.info().name.to_owned(),
            language: (language != "auto").then(|| language.to_owned()),
            // The dialog's Generate button is the explicit consent to download.
            download_model: true,
            ..SubtitleGenerateParams::default()
        };
        match range_choice {
            "Selected clip" => params.clip = self.selected,
            "In/out range" => {
                params.range = self
                    .marks
                    .range(animathio_export::duration(&self.project).0);
            }
            _ => {}
        }
        let label = format!(
            "{} model, {}",
            size.info().name,
            LANGUAGES
                .iter()
                .find(|(code, _)| *code == language)
                .map_or("auto-detect", |(_, name)| name)
        );
        if let Err(error) = self.start_subtitle_job(params, label, None, cx) {
            self.error = Some(error);
        }
        cx.notify();
    }

    /// Start the worker. `reply` is a control client waiting for the result.
    pub(super) fn start_subtitle_job(
        &mut self,
        params: SubtitleGenerateParams,
        label: String,
        reply: Option<std::sync::mpsc::SyncSender<Result<Value, ControlError>>>,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self
            .subtitle_job
            .as_ref()
            .is_some_and(|j| j.state == State::Running)
        {
            return Err("subtitles are already being generated".into());
        }
        let job = SubtitleJob::spawn(
            self.project.clone(),
            self.assets.clone(),
            params,
            label,
            self.path.clone(),
            reply,
        )?;
        self.subtitle_job = Some(job);
        self.subtitle_panel_open = true;
        cx.notify();
        Ok(())
    }

    /// Poll the worker; apply a finished result as one undoable batch. Returns whether
    /// anything changed.
    pub(super) fn pump_subtitle_job(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(job) = self.subtitle_job.as_mut() else {
            return false;
        };
        if job.state != State::Running {
            return false;
        }
        let Some(result) = job.poll() else {
            return false;
        };
        let same_project = job.project_path == self.path;
        let reply = job.reply.take();
        let outcome = result.and_then(|draft| {
            if !same_project {
                return Err(ControlError::Other(
                    "another project was opened; the subtitles were discarded".into(),
                ));
            }
            if draft.cues.is_empty() {
                return Ok(SubtitleResult {
                    language: draft.language,
                    added: None,
                    cues: Vec::new(),
                });
            }
            let (command, added) = subtitle_command(&self.project, &draft.cues)?;
            if !self.try_execute(&command, cx) {
                return Err(ControlError::Other(
                    self.error
                        .take()
                        .unwrap_or_else(|| "could not add subtitles".into()),
                ));
            }
            Ok(SubtitleResult {
                language: draft.language,
                added: Some(added),
                cues: draft.cues.iter().map(Into::into).collect(),
            })
        });
        let (state, toast) = match &outcome {
            Ok(result) if result.added.is_some() => {
                let message = format!(
                    "Added {} subtitles ({})",
                    result.cues.len(),
                    result.language
                );
                (State::Done(message.clone()), style::Toast::success(message))
            }
            Ok(_) => {
                let message = "No speech found in the selected audio".to_owned();
                (State::Done(message.clone()), style::Toast::info(message))
            }
            Err(error) if error.to_string() == "cancelled" => (
                State::Cancelled,
                style::Toast::info("Subtitle generation cancelled"),
            ),
            Err(error) => (
                State::Failed(error.to_string()),
                style::Toast::error(format!("Subtitles failed: {error}")),
            ),
        };
        if let Some(job) = self.subtitle_job.as_mut() {
            job.state = state;
        }
        self.notices.push(toast);
        if let Some(reply) = reply {
            let _ = reply.send(outcome.and_then(|r| Ok(serde_json::to_value(r)?)));
        }
        cx.notify();
        true
    }

    /// Status-bar button; hidden until a job exists.
    pub(super) fn subtitle_status_button(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let job = self.subtitle_job.as_ref()?;
        let running = job.state == State::Running;
        let label = if running {
            format!("Subtitles {:.0}%", job.fraction() * 100.0)
        } else {
            "Subtitles".to_owned()
        };
        Some(
            div()
                .id("status-subtitles")
                .flex()
                .flex_row()
                .items_center()
                .gap_1()
                .px_2()
                .h(px(22.))
                .border_1()
                .border_color(cx.theme().border)
                .bg(style::hex(style::color::PANEL))
                .cursor_pointer()
                .rounded(px(style::radius::CONTROL))
                .hover(|this| this.bg(cx.theme().muted))
                .text_color(if running {
                    cx.theme().primary
                } else {
                    cx.theme().muted_foreground
                })
                .child(icons::icon(IconName::Captions).small())
                .child(label)
                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                    this.subtitle_panel_open = !this.subtitle_panel_open;
                    cx.notify();
                })),
        )
    }

    pub(super) fn subtitle_panel(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let job = self.subtitle_job.as_ref()?;
        if !self.subtitle_panel_open || self.is_start_screen() {
            return None;
        }
        let theme = cx.theme().clone();
        let (icon, color) = match &job.state {
            State::Running => (IconName::LoaderCircle, theme.primary),
            State::Done(_) => (IconName::CircleCheck, theme.success),
            State::Failed(_) => (IconName::CircleAlert, theme.danger),
            State::Cancelled => (IconName::Ban, theme.muted_foreground),
        };
        let phase = match job.phase() {
            _ if job.cancelling && job.state == State::Running => "Cancelling...".to_owned(),
            Phase::Preparing => "Preparing audio and model...".to_owned(),
            Phase::Downloading { downloaded, total } => format!(
                "Downloading model: {} of {}",
                models::display_size(downloaded),
                models::display_size(total)
            ),
            Phase::Transcribing(percent) => format!("Transcribing: {percent}%"),
        };
        let detail = match &job.state {
            State::Running => format!("{phase} ({} s)", job.elapsed_secs()),
            State::Done(message) => message.clone(),
            State::Failed(_) => "Failed".to_owned(),
            State::Cancelled => "Cancelled".to_owned(),
        };
        let running = job.state == State::Running;
        let mut row = div()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .rounded(px(style::radius::CONTROL))
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(icons::icon(icon).small().text_color(color))
                    .child(div().flex_1().min_w_0().truncate().child(job.label.clone()))
                    .child(
                        Button::new("subtitles-job-cancel")
                            .ghost()
                            .compact()
                            .icon(icons::icon(IconName::X))
                            .tooltip(if running { "Cancel" } else { "Dismiss" })
                            .disabled(job.cancelling && running)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                if running {
                                    if let Some(job) = this.subtitle_job.as_mut() {
                                        job.request_cancel();
                                    }
                                } else {
                                    this.subtitle_job = None;
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(detail),
            );
        if running {
            row = row.child(
                Progress::new("subtitle-progress")
                    .value(job.fraction() * 100.0)
                    .loading(matches!(job.phase(), Phase::Preparing)),
            );
        }
        if let State::Failed(error) = &job.state {
            row = row.child(
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(error.clone()),
            );
        }
        Some(
            div()
                .id("subtitles-panel")
                .absolute()
                .right(px(8.))
                .bottom(px(32.))
                .w(px(440.))
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
                        .flex_row()
                        .items_center()
                        .gap_2()
                        .child(icons::icon(IconName::Captions).small())
                        .child(div().flex_1().child("Subtitles"))
                        .child(
                            Button::new("subtitles-panel-close")
                                .ghost()
                                .compact()
                                .icon(icons::icon(IconName::X))
                                .tooltip("Close")
                                .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                    this.subtitle_panel_open = false;
                                    cx.notify();
                                })),
                        ),
                )
                .child(row),
        )
    }
}
