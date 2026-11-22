//! Modal workflows built from gpui-component dialogs, forms and inputs.
//!
//! Every modal shares one frame ([`dialog_frame`]): a title with a secondary-coloured icon,
//! an optional one-line description, a label-column body ([`style::form_row`]) and a footer
//! that stays pinned ([`footer_row`]): the form-wide error on the left, the actions on the
//! right with the primary one last. Widths are the three [`style::dialog_width`] steps.

use super::*;
use crate::style::{self, Role, TextRole as _};
use animathio_control::ExportPreset;
use gpui_component::dialog::{Cancel, Confirm, Dialog};
use gpui_component::form::Field as FormField;
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::select::{Select, SelectState};
use gpui_component::{IndexPath, WindowExt};

/// The label column of a [`style::form_row`] (captions align with the controls beside it).
const LABEL_COLUMN: Rems = Rems(8.5);
/// A hairline outline for a segmented control: its `BG` well would otherwise vanish on the
/// dialog surface.
fn outlined(frame: Div) -> Div {
    frame
        .rounded(px(style::radius::CONTROL))
        .border_1()
        .border_color(style::hex(style::color::BORDER))
}

/// A two-option segmented control in a form row.
const SEGMENT_WIDTH: Rems = Rems(12.);
/// A short numeric field with its unit (`s`, `%`, `frames`).
const SHORT_FIELD: Rems = Rems(9.);

/// The validation error of the dialog being built, so its footer can show it. [`open_dialog`]
/// sets it before every build; [`footer_row`] marks it as shown, and a dialog whose footer
/// does not use [`footer_row`] still gets the error appended to its body.
struct DialogError {
    message: Option<SharedString>,
    in_footer: std::cell::Cell<bool>,
}
impl gpui::Global for DialogError {}

/// Keep validation errors readable inside the modal, above its dimmed background.
/// Each dialog remembers the error sequence at first render, so old errors never leak in.
pub(super) fn open_dialog(
    window: &mut Window,
    cx: &mut Context<Editor>,
    builder: impl Fn(Dialog, &mut Window, &mut App) -> Dialog + 'static,
) {
    let editor = cx.entity();
    let baseline = std::cell::Cell::new(None);
    window.open_dialog(cx, move |dialog, window, cx| {
        let state = editor.read(cx);
        let sequence = state.error_sequence;
        let error = state.last_error.clone();
        let initial = baseline.get().unwrap_or(sequence);
        baseline.set(Some(initial));
        let error = (sequence != initial).then_some(error).flatten();
        cx.set_global(DialogError {
            message: error.clone().map(SharedString::from),
            in_footer: std::cell::Cell::new(false),
        });
        let dialog = builder(dialog, window, cx);
        let shown = cx
            .try_global::<DialogError>()
            .is_some_and(|state| state.in_footer.get());
        match error {
            Some(error) if !shown => dialog.child(validation_error(error)),
            _ => dialog,
        }
    });
}

/// The one-line form-wide error (alert icon, `DANGER` text).
fn validation_error(message: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id("dialog-validation-error")
        .debug_selector(|| "dialog-validation-error".into())
        .flex()
        .items_start()
        .gap_1()
        .text_role(Role::Label)
        .text_color(style::hex(style::color::DANGER))
        .child(
            icons::icon(IconName::CircleAlert)
                .small()
                .text_color(style::hex(style::color::DANGER)),
        )
        .child(div().min_w_0().child(message.into()))
}

/// A dialog title: the icon (`TEXT_SECONDARY`) and the name of the thing or the action, in
/// the title role.
pub(super) fn heading(icon: IconName, title: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_role(Role::Title)
        .child(icons::icon(icon).text_color(style::hex(style::color::TEXT_SECONDARY)))
        .child(title)
}

/// The shared dialog frame: one of the three widths ([`style::dialog_width`]), the title and
/// an optional line saying what the dialog does.
pub(super) fn dialog_frame(
    dialog: Dialog,
    width: f32,
    icon: IconName,
    title: &'static str,
    description: Option<&'static str>,
) -> Dialog {
    let dialog = dialog
        .title(heading(icon, title))
        .width(style::dialog_width::of(width));
    match description {
        Some(text) => dialog.child(dialog_description(text)),
        None => dialog,
    }
}

/// The line under a dialog title (body text, secondary colour).
pub(super) fn dialog_description(text: impl Into<SharedString>) -> Div {
    div()
        .mb_3()
        .text_role(Role::Body)
        .text_color(style::hex(style::color::TEXT_SECONDARY))
        .child(text.into())
}

/// The rows of a form body, one gap between them.
pub(super) fn form_body() -> Div {
    div().flex().flex_col().gap_2()
}

/// A caption under a field (help text, a summary): `CAPTION` in `TEXT_MUTED`, aligned with
/// the control column of a [`style::form_row`].
pub(super) fn field_caption(id: &'static str, text: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .pl(LABEL_COLUMN)
        .ml_2()
        .text_role(Role::Caption)
        .child(text.into())
}

type Choices = Entity<SelectState<Vec<String>>>;
pub(super) fn input(
    value: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).default_value(value))
}
fn choices(items: Vec<String>, selected: usize, window: &mut Window, cx: &mut App) -> Choices {
    cx.new(|cx| SelectState::new(items, Some(IndexPath::default().row(selected)), window, cx))
}
fn selected(state: &Choices, cx: &App) -> String {
    state.read(cx).selected_value().cloned().unwrap_or_default()
}
/// Preset entries of the project settings dialog: the delivery presets and a trailing "Custom".
fn settings_preset_labels() -> Vec<String> {
    ExportPreset::ALL
        .map(|p| p.label().to_owned())
        .into_iter()
        .chain(std::iter::once("Custom".to_owned()))
        .collect()
}

/// The preset entry matching a project's size and frame rate, else the trailing "Custom" entry.
pub(super) fn settings_preset_index(width: u32, height: u32, rate: FrameRate) -> usize {
    ExportPreset::ALL
        .iter()
        .position(|p| {
            p.resolution() == (width, height) && rate.numerator() == 30 && rate.denominator() == 1
        })
        .unwrap_or(ExportPreset::ALL.len())
}

/// A two-option segmented control in a form row (12rem wide), `selected` being 0 or 1.
fn two_way(
    id: &'static str,
    [first, second]: [(&'static str, &'static str); 2],
    focus: &[FocusHandle],
    selected: usize,
    on_select: impl Fn(usize, &mut Window, &mut App) + Clone + 'static,
) -> Div {
    outlined(div().w(SEGMENT_WIDTH)).child(
        style::segmented(
            id,
            &[
                style::SegmentedOption::new(first.0, first.1, None),
                style::SegmentedOption::new(second.0, second.1, None),
            ],
            focus,
            selected,
            None,
            true,
            on_select,
        )
        .p_0(),
    )
}

/// Frame rates offered in Project settings (a trailing "Custom…" reveals the rational field).
const RATE_CHOICES: [&str; 8] = [
    "23.976 fps",
    "24 fps",
    "25 fps",
    "29.97 fps",
    "30 fps",
    "50 fps",
    "59.94 fps",
    "60 fps",
];
const CUSTOM_RATE: &str = "Custom…";

pub(super) fn field(label: &'static str, child: impl IntoElement) -> FormField {
    FormField::new().label(label).child(child)
}
pub(super) fn tagged_input(id: &'static str, child: impl IntoElement) -> impl IntoElement {
    div()
        .id(id)
        .flex_1()
        .min_w_0()
        .debug_selector(move || id.into())
        .child(child)
}

/// A form submission reports whether validation and application succeeded.
pub(super) type Submit = std::rc::Rc<dyn Fn(&mut App) -> bool>;

/// The pinned footer of every dialog: a form-wide validation error at the left (alert icon,
/// `DANGER`), the actions at the right. Pass the buttons in order: secondary actions, Cancel,
/// then the primary action last.
pub(super) fn footer_row(app: &App, buttons: impl IntoElement) -> Div {
    let error = app.try_global::<DialogError>().and_then(|state| {
        let message = state.message.clone()?;
        state.in_footer.set(true);
        Some(message)
    });
    div()
        .flex()
        .items_center()
        .gap_3()
        .pt_3()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .children(error.map(validation_error)),
        )
        .child(
            div()
                .flex()
                .items_center()
                .flex_shrink_0()
                .gap_2()
                .child(buttons),
        )
}

/// The Cancel button: it closes through the same action as Escape.
pub(super) fn cancel_button(id: &'static str, app: &App) -> Button {
    style::secondary_button(id, app)
        .label("Cancel")
        .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx))
}

pub(super) fn submit_button(label: &'static str, app: &App) -> Button {
    style::primary_button("dialog-apply", app)
        .debug_selector(|| "dialog-apply".into())
        .label(label)
        .on_click(|_, window, cx| {
            window.dispatch_action(Box::new(Confirm { secondary: false }), cx);
        })
}

/// Route custom footer buttons through the same Confirm/Cancel actions as Enter/Escape.
pub(super) fn submit_footer(label: &'static str, app: &App) -> Div {
    footer_row(
        app,
        div()
            .flex()
            .gap_2()
            .child(cancel_button("dialog-cancel", app))
            .child(submit_button(label, app)),
    )
}

/// The component closes exactly once on success; validation failure preserves the form.
pub(super) fn form_dialog(
    dialog: Dialog,
    label: &'static str,
    app: &App,
    submit: Submit,
) -> Dialog {
    dialog
        .footer(submit_footer(label, app))
        .on_ok(move |_, _, cx| submit(cx))
}

/// Window-aware submissions use this when they open another dialog or a native picker.
/// Return false if the handler already closed/replaced the dialog itself.
pub(super) fn on_enter(
    dialog: Dialog,
    handler: impl Fn(&mut Window, &mut App) -> bool + 'static,
) -> Dialog {
    dialog.on_ok(move |_, window, cx| handler(window, cx))
}

/// An action that replaces (or closes) the current project.
#[derive(Clone, Debug)]
pub(super) enum ReplaceProject {
    New,
    Open,
    Recent(PathBuf),
    ImportV1,
    CloseWindow,
}

impl Editor {
    /// Run `action`, first asking whether to discard unsaved changes or a recording still
    /// in progress (a take that would be dropped by New, Open, Import v1 or closing the
    /// window). Without this guard New, Open, Open Recent and Import v1 silently dropped
    /// the project (an untitled project has no autosave to recover from).
    pub(super) fn replace_project(
        &mut self,
        action: ReplaceProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let take_in_progress = self.take_in_progress();
        if !self.dirty && !take_in_progress {
            self.run_replace(action, window, cx);
            return;
        }
        let editor = cx.entity();
        let (title, label) = match action {
            ReplaceProject::CloseWindow => ("Quit without saving?", "Quit without saving"),
            _ => ("Discard unsaved changes?", "Discard changes"),
        };
        let detail = match (self.dirty, take_in_progress) {
            (true, true) => {
                "The current project has unsaved changes and a recording in progress that will be lost."
            }
            (true, false) => "The current project has unsaved changes that will be lost.",
            (false, true) => "A recording is in progress and will be lost.",
            (false, false) => unreachable!("guarded above"),
        };
        open_dialog(window, cx, move |dialog, _, cx| {
            let editor = editor.clone();
            let action = action.clone();
            let dialog = dialog_frame(
                dialog,
                style::dialog_width::SMALL,
                IconName::TriangleAlert,
                title,
                Some(detail),
            )
            .footer(footer_row(
                cx,
                div()
                    .flex()
                    .gap_2()
                    .child(cancel_button("dialog-cancel", cx))
                    .child(
                        style::destructive_button("dialog-apply", label, true, cx)
                            .debug_selector(|| "dialog-apply".into())
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(Confirm { secondary: false }), cx);
                            }),
                    ),
            ));
            on_enter(dialog, move |window, cx| {
                window.close_dialog(cx);
                editor.update(cx, |this, cx| this.run_replace(action.clone(), window, cx));
                false
            })
        });
    }

    fn run_replace(&mut self, action: ReplaceProject, window: &mut Window, cx: &mut Context<Self>) {
        match action {
            ReplaceProject::New => {
                window.focus(&self.focus_handle, cx);
                self.new_project_with_chooser(cx);
            }
            ReplaceProject::Open => self.open(cx),
            ReplaceProject::Recent(path) => self.open_path(&path, cx),
            ReplaceProject::ImportV1 => self.import_legacy(cx),
            ReplaceProject::CloseWindow => {
                self.dirty = false;
                self.window_tracker.persist();
                window.remove_window();
            }
        }
    }
}

/// Shorten `text` to at most `max` characters by replacing the middle with an ellipsis, so the
/// start of a path and its file name both stay visible.
pub(super) fn middle_truncate(text: &str, max: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max || max < 5 {
        return text.to_owned();
    }
    let keep = max - 1;
    let tail = (keep * 2).div_ceil(3);
    let head = keep - tail;
    let start: String = chars[..head].iter().collect();
    let end: String = chars[chars.len() - tail..].iter().collect();
    format!("{start}…{end}")
}

/// The one-line outcome of the export choices: `640 × 360, 30 fps, chapters on`.
pub(super) fn export_summary(
    size: (u32, u32),
    frame_rate: &str,
    markers: usize,
    format: animathio_export::Format,
) -> String {
    let chapters = if !format.supports_chapters() {
        "no chapters"
    } else if markers == 0 {
        "no chapters (no markers)"
    } else {
        "chapters on"
    };
    format!("{} × {}, {frame_rate}, {chapters}", size.0, size.1)
}

/// The segmented-control option of an export container.
fn format_option(format: animathio_export::Format) -> style::SegmentedOption {
    match format {
        animathio_export::Format::Mp4 => {
            style::SegmentedOption::new("export-format-mp4", "MP4", None)
        }
        animathio_export::Format::Webm => {
            style::SegmentedOption::new("export-format-webm", "WebM", None)
        }
    }
}

/// Outcome of submitting the Export dialog.
pub(super) enum Submitted {
    Invalid,
    Queued,
    ConfirmOverwrite,
}

impl Editor {
    pub(super) fn open_export_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let duration = animathio_export::duration(&self.project).0;
        if duration <= 0 {
            self.error = Some("Nothing to export yet. Add a clip to the timeline first.".into());
            cx.notify();
            return;
        }
        let default_range = self
            .marks
            .range(duration)
            .unwrap_or_else(|| TimeRange::new(Time(0), Time(duration)).unwrap());
        let rate = self.project.frame_rate();
        let project_resolution = project_size(&self.project);
        let project_rate = frame_rate_label(rate);
        let mut presets = vec![format!(
            "Project settings ({} × {}, {project_rate})",
            project_resolution.0, project_resolution.1
        )];
        presets.extend(ExportPreset::ALL.map(|preset| preset.label().to_owned()));
        let preset = choices(presets, 0, window, cx);
        let format = std::rc::Rc::new(std::cell::Cell::new(animathio_export::Format::default()));
        let format_focus: Vec<FocusHandle> = animathio_export::Format::ALL
            .iter()
            .map(|_| cx.focus_handle())
            .collect();
        let range = choices(
            vec!["Whole project".into(), "In/out".into(), "Custom".into()],
            usize::from(self.marks.marked()),
            window,
            cx,
        );
        let from = input(
            timecode::format_timecode(default_range.start, rate),
            window,
            cx,
        );
        let to = input(
            timecode::format_timecode(default_range.end, rate),
            window,
            cx,
        );
        let output = input(
            self.next_export_path().to_string_lossy().into_owned(),
            window,
            cx,
        );
        // The dialog re-renders when a choice changes: Custom start/end appear only for the
        // Custom range, and the summary follows the preset and format.
        for select in [&preset, &range] {
            cx.subscribe_in(
                select,
                window,
                |_, _, _: &gpui_component::select::SelectEvent<Vec<String>>, window, _| {
                    window.refresh();
                },
            )
            .detach();
        }
        cx.subscribe_in(
            &output,
            window,
            |_, _, _: &gpui_component::input::InputEvent, window, _| window.refresh(),
        )
        .detach();
        let markers = self.project.markers().len();
        let editor = cx.entity();
        open_dialog(window, cx, move |dialog, _, cx| {
            let path_input = output.clone();
            let submit = {
                let (editor, preset, format, range, from, to, output) = (
                    editor.clone(),
                    preset.clone(),
                    format.clone(),
                    range.clone(),
                    from.clone(),
                    to.clone(),
                    output.clone(),
                );
                std::rc::Rc::new(move |front: bool, window: &mut Window, cx: &mut App| {
                    let preset_name = selected(&preset, cx);
                    let preset = ExportPreset::ALL
                        .into_iter()
                        .find(|p| p.label() == preset_name);
                    let container = format.get();
                    let choice = selected(&range, cx);
                    let from = timecode::parse_timecode(&from.read(cx).value(), rate);
                    let to = timecode::parse_timecode(&to.read(cx).value(), rate);
                    let output = PathBuf::from(output.read(cx).value().trim());
                    let outcome = editor.update(cx, |this, cx| {
                        this.submit_export(preset, container, &choice, from, to, output, front, cx)
                    });
                    match outcome {
                        Submitted::Invalid => {}
                        Submitted::Queued => window.close_dialog(cx),
                        Submitted::ConfirmOverwrite => {
                            window.close_dialog(cx);
                            editor.update(cx, |this, cx| this.open_overwrite_dialog(window, cx));
                        }
                    }
                })
            };
            let (queue_submit, now_submit) = (submit.clone(), submit);
            let enter_submit = now_submit.clone();
            let chosen_preset = ExportPreset::ALL
                .into_iter()
                .find(|p| p.label() == selected(&preset, cx));
            let container = format.get();
            let custom_range = selected(&range, cx) == "Custom";
            let (size, rate_label) = chosen_preset.map_or_else(
                || (project_resolution, project_rate.clone()),
                |p| (p.resolution(), "30 fps".to_owned()),
            );
            let format_note = match container {
                animathio_export::Format::Mp4 => {
                    "H.264 video and AAC audio. Plays everywhere; markers become chapters."
                }
                animathio_export::Format::Webm => {
                    "VP9 video and Opus audio for the web. Slower to encode than MP4."
                }
            };
            // Switching the container swaps a `.mp4`/`.webm` extension in the path with it, so
            // the file name never contradicts the selector (other names are left alone).
            let on_format = {
                let (format, output) = (format.clone(), output.clone());
                move |index: usize, window: &mut Window, cx: &mut App| {
                    let chosen = animathio_export::Format::ALL[index];
                    format.set(chosen);
                    output.update(cx, |state, cx| {
                        let current = PathBuf::from(state.value().trim());
                        if animathio_export::Format::for_path(&current)
                            .is_some_and(|other| other != chosen)
                        {
                            state.set_value(
                                current
                                    .with_extension(chosen.extension())
                                    .to_string_lossy()
                                    .into_owned(),
                                window,
                                cx,
                            );
                        }
                    });
                    window.refresh();
                }
            };
            let format_options = animathio_export::Format::ALL.map(format_option);
            let dialog = dialog_frame(
                dialog,
                style::dialog_width::MEDIUM,
                IconName::Clapperboard,
                "Export video",
                Some("Render the project to a video file."),
            )
            .child(
                form_body()
                    .child(style::form_row("Preset", Select::new(&preset)))
                    .child(style::form_row(
                        "Format",
                        outlined(div())
                            .id("export-format-select")
                            .debug_selector(|| "export-format-select".into())
                            .w(SEGMENT_WIDTH)
                            .child(
                                style::segmented(
                                    "export-format",
                                    &format_options,
                                    &format_focus,
                                    animathio_export::Format::ALL
                                        .iter()
                                        .position(|f| *f == container)
                                        .unwrap_or(0),
                                    None,
                                    true,
                                    on_format,
                                )
                                .p_0(),
                            ),
                    ))
                    .child(field_caption("export-format-note", format_note))
                    .child(style::form_row(
                        "File",
                        div()
                            .flex()
                            .gap_2()
                            .child(tagged_input("export-output", Input::new(&output)))
                            .child(style::secondary_button("export-browse", cx).label("Browse…").on_click(
                                move |_, window, cx| {
                                    let current = PathBuf::from(
                                        path_input.read(cx).value().to_string(),
                                    );
                                    let mut picker = rfd::FileDialog::new()
                                        .add_filter(
                                            match container {
                                                animathio_export::Format::Mp4 => "MP4 video",
                                                animathio_export::Format::Webm => "WebM video",
                                            },
                                            &[container.extension()],
                                        )
                                        .set_file_name(format!(
                                            "video.{}",
                                            container.extension()
                                        ));
                                    if let Some(parent) = current
                                        .parent()
                                        .filter(|p| !p.as_os_str().is_empty())
                                    {
                                        picker = picker.set_directory(parent);
                                    }
                                    if let Some(path) = picker.save_file() {
                                        path_input.update(cx, |state, cx| {
                                            state.set_value(
                                                path.to_string_lossy().into_owned(),
                                                window,
                                                cx,
                                            );
                                        });
                                    }
                                },
                            )),
                    ))
                    .child(style::form_row("Range", Select::new(&range)))
                    .when(custom_range, |form| {
                        form.child(style::form_row(
                            "Start",
                            div()
                                .flex()
                                .child(tagged_input("export-custom-start", Input::new(&from))),
                        ))
                        .child(style::form_row(
                            "End",
                            div()
                                .flex()
                                .child(tagged_input("export-custom-end", Input::new(&to))),
                        ))
                        .child(field_caption(
                            "export-custom-hint",
                            "Timecodes are hours:minutes:seconds:frames. The end frame is not included.",
                        ))
                    })
                    .child(field_caption(
                        "export-summary",
                        export_summary(size, &rate_label, markers, container),
                    )),
            )
            .footer(footer_row(
                cx,
                div()
                    .flex()
                    .gap_2()
                    .child(
                        style::secondary_button("export-add-to-queue", cx)
                            .label("Add to queue")
                            .on_click(move |_, window, cx| queue_submit(false, window, cx)),
                    )
                    .child(
                        style::secondary_button("dialog-cancel", cx)
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        style::primary_button("export-now", cx)
                            .debug_selector(|| "export-now".into())
                            .label("Export")
                            .on_click(move |_, window, cx| now_submit(true, window, cx)),
                    ),
            ));
            // Enter exports now, like the primary button.
            on_enter(dialog, move |window, cx| {
                enter_submit(true, window, cx);
                false
            })
        });
    }

    /// A free `<project>.mp4`, `<project>-2.mp4`, ... (`video` for an unsaved
    /// project) in the default export folder, so repeated queueing does not
    /// collide with earlier jobs or files.
    fn next_export_path(&self) -> PathBuf {
        let folder = self.preferences.export_dir(self.path.as_deref());
        let stem = self
            .path
            .as_deref()
            .and_then(Path::file_stem)
            .map_or_else(|| "video".to_owned(), |s| s.to_string_lossy().into_owned());
        let candidate = |n: u32| {
            if n == 1 {
                folder.join(format!("{stem}.mp4"))
            } else {
                folder.join(format!("{stem}-{n}.mp4"))
            }
        };
        (1..=9999)
            .map(candidate)
            .find(|path| !path.exists() && !self.export_queue.targets(path))
            .unwrap_or_else(|| candidate(1))
    }

    #[allow(clippy::too_many_arguments)] // Dialog fields, passed straight through.
    pub(super) fn submit_export(
        &mut self,
        preset: Option<ExportPreset>,
        format: animathio_export::Format,
        choice: &str,
        from: Option<Time>,
        to: Option<Time>,
        output: PathBuf,
        front: bool,
        cx: &mut Context<Self>,
    ) -> Submitted {
        let duration = animathio_export::duration(&self.project).0;
        let range = match choice {
            "In/out" => self.marks.range(duration),
            "Custom" => from
                .zip(to)
                .and_then(|(from, to)| TimeRange::new(from, to).ok())
                .filter(|r| r.start < r.end && r.start.0 >= 0 && r.end.0 <= duration),
            _ => TimeRange::new(Time(0), Time(duration)).ok(),
        };
        let Some(range) = range else {
            self.error = Some(match choice {
                "In/out" => "The in/out range is empty or outside the project. Move the I and O \
                                 marks in the timeline, or choose Whole project."
                    .to_owned(),
                "Custom" => "Enter a start before the end, both inside the project, as \
                                 HH:MM:SS:FF."
                    .to_owned(),
                _ => "The project has nothing to export.".to_owned(),
            });
            cx.notify();
            return Submitted::Invalid;
        };
        if output.as_os_str().is_empty() || output.file_name().is_none() {
            self.error = Some("Choose an output file".into());
            cx.notify();
            return Submitted::Invalid;
        }
        // Exporting over the open project file would destroy it.
        if let Some(project) = &self.path
            && let Err(error) = animathio_control::ensure_distinct_from_project(&output, project)
        {
            self.error = Some(error.to_string());
            cx.notify();
            return Submitted::Invalid;
        }
        // `clip.webm` with MP4 selected (or the reverse) would fail only after queueing.
        if let Err(error) = format.check_path(&output) {
            self.error = Some(error.to_string());
            cx.notify();
            return Submitted::Invalid;
        }
        if self.export_queue.targets(&output) {
            self.error = Some("That file is already in the export queue".into());
            cx.notify();
            return Submitted::Invalid;
        }
        let spec = match self.export_spec(output.clone(), preset, format, range) {
            Ok(spec) => spec,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return Submitted::Invalid;
            }
        };
        if output.exists() {
            self.pending_export = Some((spec, front));
            cx.notify();
            return Submitted::ConfirmOverwrite;
        }
        self.enqueue_export(spec, front, cx);
        Submitted::Queued
    }

    /// Snapshot the project (with the preset applied) into a queueable job.
    /// Later edits and delivery presets never touch the open project.
    fn export_spec(
        &self,
        output: PathBuf,
        preset: Option<ExportPreset>,
        format: animathio_export::Format,
        range: TimeRange,
    ) -> Result<crate::export_queue::JobSpec, String> {
        let mut project = self.project.clone();
        let mut settings = animathio_export::Settings::default();
        if let Some(preset) = preset {
            preset
                .command(&project)
                .and_then(|command| command.apply(&mut project).map_err(ControlError::from))
                .map_err(|error| error.to_string())?;
            settings = preset.export_settings();
        }
        settings.format = format;
        let rate = self.project.frame_rate();
        let summary = format!(
            "{}, {} to {}",
            preset.map_or("Project settings", ExportPreset::label),
            timecode::format_timecode(range.start, rate),
            timecode::format_timecode(range.end, rate),
        );
        Ok(crate::export_queue::JobSpec {
            project,
            assets: self.assets.clone(),
            params: animathio_control::ExportParams {
                output,
                from: range.start,
                to: Some(range.end),
                settings,
                overwrite: false,
            },
            summary,
        })
    }

    pub(super) fn enqueue_export(
        &mut self,
        spec: crate::export_queue::JobSpec,
        front: bool,
        cx: &mut Context<Self>,
    ) {
        self.export_queue.enqueue(spec, front);
        self.export_queue_open = true;
        self.pump_export_queue();
        if !front {
            self.notices
                .push(crate::style::Toast::info("Added to export queue"));
        }
        cx.notify();
    }

    /// Poll the queue: collect a finished job, start the next, and turn results
    /// into toasts. Returns whether anything changed.
    pub(super) fn pump_export_queue(&mut self) -> bool {
        use crate::export_queue::Event;
        let assets_and_spawn = |spec: crate::export_queue::JobSpec| {
            crate::export_worker::ExportTask::spawn(spec.project, spec.assets, spec.params, None)
        };
        let events = self.export_queue.pump(assets_and_spawn);
        let changed = !events.is_empty();
        for event in events {
            let notice = match event {
                Event::Finished { name, .. } => {
                    crate::style::Toast::success(format!("Exported {name}"))
                }
                Event::Failed { name, error, .. } => {
                    crate::style::Toast::error(format!("Could not export {name}: {error}"))
                }
                Event::Cancelled { name, .. } => {
                    crate::style::Toast::info(format!("Cancelled export of {name}"))
                }
            };
            self.notices.push(notice);
        }
        changed
    }

    /// The "Replace existing file?" step for a job that targets an existing file.
    fn open_overwrite_dialog(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((spec, _)) = &self.pending_export else {
            return;
        };
        let path = spec.params.output.display().to_string();
        let editor = cx.entity();
        open_dialog(window, cx, move |dialog, _, cx| {
            let cancel = editor.clone();
            let dialog = dialog_frame(
                dialog,
                style::dialog_width::SMALL,
                IconName::TriangleAlert,
                "Replace existing file?",
                Some("This file already exists. Exporting replaces it."),
            )
            .child(
                div()
                    .id("overwrite-path")
                    .debug_selector(|| "overwrite-path".into())
                    .text_role(Role::Label)
                    .text_color(style::hex(style::color::TEXT_SECONDARY))
                    .child(middle_truncate(&path, 64)),
            )
            .on_close(move |_, _, cx| {
                cancel.update(cx, |this, cx| {
                    this.pending_export = None;
                    cx.notify();
                });
            })
            .footer(footer_row(
                cx,
                div()
                    .flex()
                    .gap_2()
                    .child(cancel_button("overwrite-cancel", cx))
                    .child(
                        style::destructive_button("overwrite-replace", "Replace", true, cx)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(Confirm { secondary: false }), cx);
                            }),
                    ),
            ));
            let replace = editor.clone();
            on_enter(dialog, move |_, cx| {
                replace.update(cx, |this, cx| {
                    if let Some((mut spec, front)) = this.pending_export.take() {
                        spec.params.overwrite = true;
                        this.enqueue_export(spec, front, cx);
                    }
                });
                true
            })
        });
    }

    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Bound to a name so the style guard does not mistake the call for a colour literal.
        let to_color = fields::to_hsla;
        let (current_width, current_height) = project_size(&self.project);
        let rate = self.project.frame_rate();
        let width = input(current_width.to_string(), window, cx);
        let height = input(current_height.to_string(), window, cx);
        let rate_index = RATE_CHOICES
            .iter()
            .position(|label| parse_fps(label.trim_end_matches(" fps")).is_ok_and(|r| r == rate))
            .unwrap_or(RATE_CHOICES.len());
        let rates = choices(
            RATE_CHOICES
                .iter()
                .map(|label| (*label).to_owned())
                .chain(std::iter::once(CUSTOM_RATE.to_owned()))
                .collect(),
            rate_index,
            window,
            cx,
        );
        let fps = input(
            format!("{}/{}", rate.numerator(), rate.denominator()),
            window,
            cx,
        );
        let background = self.project.background();
        let color = cx.new(|cx| {
            gpui_component::color_picker::ColorPickerState::new(window, cx)
                .default_value(to_color(background))
        });
        let background_hex = format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            background[0], background[1], background[2], background[3]
        );
        let presets = choices(
            settings_preset_labels(),
            settings_preset_index(current_width, current_height, rate),
            window,
            cx,
        );
        // Choosing a preset sets the size at once (there is no separate "use" button); the
        // trailing "Custom" entry leaves the fields alone.
        {
            let (width, height) = (width.clone(), height.clone());
            cx.subscribe_in(
                &presets,
                window,
                move |_,
                      _,
                      event: &gpui_component::select::SelectEvent<Vec<String>>,
                      window,
                      cx| {
                    let gpui_component::select::SelectEvent::Confirm(Some(label)) = event else {
                        return;
                    };
                    if let Some(preset) = ExportPreset::ALL.into_iter().find(|p| p.label() == label)
                    {
                        let (preset_width, preset_height) = preset.resolution();
                        width.update(cx, |state, cx| {
                            state.set_value(preset_width.to_string(), window, cx);
                        });
                        height.update(cx, |state, cx| {
                            state.set_value(preset_height.to_string(), window, cx);
                        });
                    }
                    window.refresh();
                },
            )
            .detach();
        }
        // Programmatic preset fills use set_value, which only notifies the view.
        // A Change event here comes from editing the field.
        for field in [&width, &height] {
            let presets = presets.downgrade();
            cx.subscribe_in(
                field,
                window,
                move |_, _, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::Change) {
                        let custom = IndexPath::new(ExportPreset::ALL.len());
                        let _ = presets.update(cx, |state, cx| {
                            if state.selected_index(cx) != Some(custom) {
                                state.set_selected_index(Some(custom), window, cx);
                                window.refresh();
                            }
                        });
                    }
                },
            )
            .detach();
        }
        cx.subscribe_in(
            &rates,
            window,
            |_, _, _: &gpui_component::select::SelectEvent<Vec<String>>, window, _| {
                window.refresh();
            },
        )
        .detach();
        let editor = cx.entity();
        open_dialog(window, cx, move |dialog, _, cx| {
            let (width_field, height_field, fps_field) =
                (width.clone(), height.clone(), fps.clone());
            let (rate_select, color_picker) = (rates.clone(), color.clone());
            let editor = editor.clone();
            let custom_rate = selected(&rates, cx) == CUSTOM_RATE;
            let submit: Submit = std::rc::Rc::new(move |cx: &mut App| {
                let size = width_field
                    .read(cx)
                    .value()
                    .trim()
                    .parse::<u32>()
                    .ok()
                    .zip(height_field.read(cx).value().trim().parse::<u32>().ok())
                    .filter(|(width, height)| *width > 0 && *height > 0);
                let choice = selected(&rate_select, cx);
                let rate = if choice == CUSTOM_RATE {
                    parse_fps(fps_field.read(cx).value().trim()).ok()
                } else {
                    parse_fps(choice.trim_end_matches(" fps")).ok()
                };
                let background = {
                    let rgba = Rgba::from(
                        color_picker
                            .read(cx)
                            .value()
                            .unwrap_or_else(|| to_color(background)),
                    );
                    [rgba.r, rgba.g, rgba.b, rgba.a]
                        .map(|v| (v * 255.).round().clamp(0., 255.) as u8)
                };
                editor.update(cx, |this, cx| {
                    let Some(((width, height), frame_rate)) = size.zip(rate) else {
                        this.error = Some(
                            "Enter a width and height in pixels and a frame rate, such as 30 or 30000/1001."
                                .into(),
                        );
                        cx.notify();
                        return false;
                    };
                    let command = Command::SetSettings {
                        width,
                        height,
                        frame_rate,
                        background,
                        duration: this.project.duration(),
                    };
                    this.try_execute(&command, cx)
                })
            });
            let px_unit = || div().pr_1().text_role(Role::Label).child("px");
            let dialog = dialog_frame(
                dialog,
                style::dialog_width::MEDIUM,
                IconName::Settings2,
                "Project settings",
                Some("Canvas size, frame rate and background color of this project."),
            )
            .child(
                form_body()
                    .child(style::form_row(
                        "Preset",
                        tagged_input(
                            "project-size-preset",
                            div()
                                .debug_selector(|| {
                                    format!("project-size-preset-{}", selected(&presets, cx))
                                })
                                .child(Select::new(&presets)),
                        ),
                    ))
                    .child(style::form_row(
                        "Size",
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(tagged_input(
                                "project-width",
                                Input::new(&width).suffix(px_unit()),
                            ))
                            .child(
                                div()
                                    .text_role(Role::Body)
                                    .text_color(style::hex(style::color::TEXT_SECONDARY))
                                    .child("×"),
                            )
                            .child(tagged_input(
                                "project-height",
                                Input::new(&height).suffix(px_unit()),
                            )),
                    ))
                    .child(style::form_row("Frame rate", Select::new(&rates)))
                    .when(custom_rate, |form| {
                        form.child(style::form_row(
                            "Custom rate",
                            tagged_input("project-fps", Input::new(&fps)),
                        ))
                        .child(field_caption(
                            "project-fps-hint",
                            "A number such as 30, or a fraction such as 30000/1001.",
                        ))
                    })
                    .child(style::form_row(
                        "Background",
                        // Swatch plus hex like the inspector's colour rows; the bordered
                        // frame keeps a near-black background visible on the dialog.
                        div()
                            .id("settings-background")
                            .debug_selector(|| "settings-background".into())
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap_2()
                            .child(
                                div()
                                    .rounded(px(style::radius::CONTROL))
                                    .border_1()
                                    .border_color(style::hex(style::color::BORDER_STRONG))
                                    .p_0p5()
                                    .child(gpui_component::color_picker::ColorPicker::new(&color)),
                            )
                            .child(
                                div()
                                    .text_role(Role::Label)
                                    .tabular()
                                    .child(background_hex.clone()),
                            ),
                    )),
            );
            form_dialog(dialog, "Apply", cx, submit)
        });
    }

    pub(super) fn open_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let autosave = input(self.preferences.autosave_seconds.to_string(), window, cx);
        let folder = {
            let value = self
                .preferences
                .export_folder
                .to_string_lossy()
                .into_owned();
            let placeholder = if self.path.is_some() {
                "Next to the project".to_owned()
            } else {
                "Your Videos folder".to_owned()
            };
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        let scale_values: Vec<String> = std::iter::once("Desktop default".to_owned())
            .chain((80..=200).step_by(10).map(|n| format!("{n}%")))
            .collect();
        let scale_index = self
            .preferences
            .ui_scale
            .map_or(0, |n| ((n.clamp(80, 200) - 80) / 10 + 1) as usize);
        let scale = choices(scale_values, scale_index, window, cx);
        // The two-state settings: segmented controls and switches share one cell each with
        // the submit handler, since the controls only report changes.
        let gpu = std::rc::Rc::new(std::cell::Cell::new(self.preview.gpu));
        let compact = std::rc::Rc::new(std::cell::Cell::new(self.preferences.compact));
        let reduce_motion = std::rc::Rc::new(std::cell::Cell::new(self.preferences.reduce_motion));
        let check_updates = std::rc::Rc::new(std::cell::Cell::new(self.preferences.check_updates));
        let gpu_focus: Vec<FocusHandle> = (0..2).map(|_| cx.focus_handle()).collect();
        let density_focus: Vec<FocusHandle> = (0..2).map(|_| cx.focus_handle()).collect();
        let community_on = crate::editor_env::community_plugins_enabled();
        let editor = cx.entity();
        open_dialog(window, cx, move |dialog, _, cx| {
            let scale_submit = scale.clone();
            let autosave_submit = autosave.clone();
            let folder_submit = folder.clone();
            let (gpu_submit, compact_submit) = (gpu.clone(), compact.clone());
            let (motion_submit, check_submit) = (reduce_motion.clone(), check_updates.clone());
            let editor = editor.clone();
            let open_plugins = editor.clone();
            let folder_browse = folder.clone();
            let submit = std::rc::Rc::new(move |cx: &mut App| {
                let Ok(seconds) = autosave_submit.read(cx).value().trim().parse::<u64>() else {
                    editor.update(cx, |this, cx| {
                        this.error =
                            Some("Enter whole seconds for autosave, or 0 to turn it off.".into());
                        cx.notify();
                    });
                    return false;
                };
                let folder = PathBuf::from(folder_submit.read(cx).value().to_string());
                let gpu = gpu_submit.get();
                let compact = compact_submit.get();
                let reduce_motion = motion_submit.get();
                let check_updates = check_submit.get();
                editor.update(cx, |this, cx| {
                    let preferences = crate::preferences::Preferences {
                        autosave_seconds: seconds,
                        export_folder: folder,
                        compact,
                        ui_scale: selected(&scale_submit, cx)
                            .trim_end_matches('%')
                            .parse()
                            .ok(),
                        reduce_motion,
                        check_updates,
                    };
                    if let Err(error) = preferences.save() {
                        this.error = Some(format!("Could not save preferences: {error}"));
                        cx.notify();
                        return false;
                    }
                    this.preferences = preferences;
                    // Saving also re-reads the plugin settings, so changes made with
                    // `animathio-cli plugins` while the editor runs are picked up.
                    this.sync_community_plugins(cx);
                    if gpu != this.preview.gpu {
                        this.toggle_gpu_preview(cx);
                    }
                    style::set_ui_scale(this.preferences.effective_scale());
                    cx.set_reduce_motion(reduce_motion);
                    gpui_component::Theme::update(cx, |theme| {
                        theme.font_size =
                            px((if compact { 15. } else { 16. })
                                * this.preferences.effective_scale());
                        theme.motion.duration_fast = Duration::from_millis(if reduce_motion {
                            0
                        } else {
                            style::motion::FAST_MS
                        });
                        theme.motion.duration_normal = Duration::from_millis(if reduce_motion {
                            0
                        } else {
                            style::motion::BASE_MS
                        });
                        theme.motion.duration_slow = theme.motion.duration_normal;
                    });
                    cx.notify();
                    true
                })
            });
            let dialog = dialog_frame(
                dialog,
                style::dialog_width::MEDIUM,
                IconName::SlidersHorizontal,
                "Preferences",
                None,
            )
            .child(
                div()
                    .mt_2()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(style::form_group(
                        "General",
                        [
                            style::form_row(
                                "Autosave every",
                                div()
                                    .id("preferences-autosave")
                                    .debug_selector(|| "preferences-autosave".into())
                                    .w(SHORT_FIELD)
                                    .child(
                                        Input::new(&autosave)
                                            .suffix(div().pr_1().text_role(Role::Label).child("s")),
                                    ),
                            )
                            .into_any_element(),
                            field_caption("preferences-autosave-help", "0 turns autosave off.")
                                .into_any_element(),
                            style::form_row(
                                "Export folder",
                                div()
                                    .flex()
                                    .gap_2()
                                    .child(tagged_input(
                                        "preferences-export-folder",
                                        Input::new(&folder),
                                    ))
                                    .child(
                                        style::secondary_button(
                                            "preferences-export-folder-browse",
                                            cx,
                                        )
                                        .debug_selector(|| {
                                            "preferences-export-folder-browse".into()
                                        })
                                        .label("Browse…")
                                        .on_click(
                                            move |_, window, cx| {
                                                let mut picker = rfd::FileDialog::new();
                                                let current = PathBuf::from(
                                                    folder_browse
                                                        .read(cx)
                                                        .value()
                                                        .trim()
                                                        .to_owned(),
                                                );
                                                if current.is_dir() {
                                                    picker = picker.set_directory(current);
                                                }
                                                if let Some(path) = picker.pick_folder() {
                                                    folder_browse.update(cx, |state, cx| {
                                                        state.set_value(
                                                            path.to_string_lossy().into_owned(),
                                                            window,
                                                            cx,
                                                        );
                                                    });
                                                }
                                            },
                                        ),
                                    ),
                            )
                            .into_any_element(),
                        ],
                    ))
                    .child(style::form_group(
                        "Interface",
                        [
                            style::form_row("UI scale", Select::new(&scale)).into_any_element(),
                            style::form_row(
                                "Density",
                                two_way(
                                    "preferences-density",
                                    [
                                        ("preferences-density-compact", "Compact"),
                                        ("preferences-density-comfortable", "Comfortable"),
                                    ],
                                    &density_focus,
                                    usize::from(!compact.get()),
                                    {
                                        let compact = compact.clone();
                                        move |index, window, _| {
                                            compact.set(index == 0);
                                            window.refresh();
                                        }
                                    },
                                ),
                            )
                            .into_any_element(),
                            style::switch_row(
                                "preferences-reduce-motion",
                                "Reduce motion",
                                None,
                                reduce_motion.get(),
                                {
                                    let reduce_motion = reduce_motion.clone();
                                    move |checked, window, _| {
                                        reduce_motion.set(*checked);
                                        window.refresh();
                                    }
                                },
                            )
                            .into_any_element(),
                        ],
                    ))
                    .child(style::form_group(
                        "Preview",
                        [style::form_row(
                            "Renderer",
                            two_way(
                                "preferences-renderer",
                                [
                                    ("preferences-renderer-gpu", "GPU"),
                                    ("preferences-renderer-cpu", "CPU"),
                                ],
                                &gpu_focus,
                                usize::from(!gpu.get()),
                                {
                                    let gpu = gpu.clone();
                                    move |index, window, _| {
                                        gpu.set(index == 0);
                                        window.refresh();
                                    }
                                },
                            ),
                        )
                        .into_any_element()],
                    ))
                    .child(style::form_group(
                        "Plugins and updates",
                        [
                            style::switch_row(
                                "preferences-check-updates",
                                "Check for updates on startup",
                                None,
                                check_updates.get(),
                                {
                                    let check_updates = check_updates.clone();
                                    move |checked, window, _| {
                                        check_updates.set(*checked);
                                        window.refresh();
                                    }
                                },
                            )
                            .into_any_element(),
                            // One switch, one source of truth (CommunityStore): it lives in
                            // Library > Plugins, and this row only reports it and links there.
                            div()
                                .id("preferences-community-plugins")
                                .debug_selector(|| "preferences-community-plugins".into())
                                .child(style::form_row(
                                    "Community plugins",
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            div()
                                                .flex_1()
                                                .text_role(Role::Body)
                                                .child(if community_on { "On" } else { "Off" }),
                                        )
                                        .child(
                                            style::secondary_button("preferences-open-plugins", cx)
                                                .debug_selector(|| {
                                                    "preferences-open-plugins".into()
                                                })
                                                .label("Open Plugins")
                                                .on_click(move |_, window, cx| {
                                                    window.close_dialog(cx);
                                                    open_plugins.update(cx, |this, cx| {
                                                        this.dock.left_open = true;
                                                        this.select_left_tab(LeftTab::Plugins, cx);
                                                    });
                                                }),
                                        ),
                                ))
                                .child(field_caption(
                                    "preferences-community-help",
                                    "Third-party plugins run sandboxed and are not reviewed.",
                                ))
                                .into_any_element(),
                        ],
                    )),
            );
            form_dialog(dialog, "Save", cx, submit)
        });
    }
    pub(super) fn open_manim_import(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.finish_color_edit(cx);
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Manim Python script", &["py"])
            .pick_file()
        else {
            return;
        };
        let scene = cx.new(|cx| InputState::new(window, cx).placeholder("First scene in the file"));
        let editor = cx.entity();
        open_dialog(window, cx, move |dialog, _, cx| {
            let editor = editor.clone();
            let scene_submit = scene.clone();
            let path = path.clone();
            let submit: Submit = std::rc::Rc::new(move |cx: &mut App| {
                let scene = scene_submit.read(cx).value().trim().to_owned();
                editor.update(cx, |this, cx| {
                    let params = animathio_control::ImportManimParams {
                        path: Some(path.clone()),
                        source: None,
                        script_dir: None,
                        scene: (!scene.is_empty()).then_some(scene),
                        replace: false,
                    };
                    match this.handle_control_call(Call::ImportManim { params }, cx) {
                        Ok(_) => true,
                        Err(error) => {
                            this.error = Some(error.to_string());
                            cx.notify();
                            false
                        }
                    }
                })
            });
            let dialog = dialog_frame(
                dialog,
                style::dialog_width::MEDIUM,
                IconName::Code,
                "Import Manim scene",
                Some("Turns a Manim scene into clips on a new timeline."),
            )
            .child(form_body().child(style::form_row("Scene class", Input::new(&scene))));
            form_dialog(dialog, "Import", cx, submit)
        });
    }

    pub(super) fn open_write_on(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.filter(|id| self.find_clip(*id).is_some_and(|clip| matches!(&clip.kind, ClipKind::Plugin { plugin_id,.. } if plugin_id.contains("latex")))) else {
            self.error = Some("Select a LaTeX clip to use Write on.".into()); cx.notify(); return;
        };
        let duration = input("1", window, cx);
        let editor = cx.entity();
        open_dialog(window, cx, move |dialog, _, cx| {
            let duration_submit = duration.clone();
            let editor = editor.clone();
            let submit: Submit = std::rc::Rc::new(move |cx: &mut App| {
                let seconds = duration_submit.read(cx).value().parse::<f64>().ok();
                editor.update(cx, |this, cx| {
                    let Some(command) = this.find_clip(id).and_then(|clip| {
                        seconds.and_then(|seconds| editing::write_on_command(clip, seconds))
                    }) else {
                        this.error =
                            Some("Enter a duration in seconds that is longer than zero and fits within the clip.".into());
                        cx.notify();
                        return false;
                    };
                    this.try_execute(&command, cx)
                })
            });
            let dialog = dialog_frame(
                dialog,
                style::dialog_width::MEDIUM,
                IconName::WandSparkles,
                "Write on",
                Some("Reveals the equation as if it were being written."),
            )
            .child(form_body().child(style::form_row(
                "Duration",
                div().w(SHORT_FIELD).child(tagged_input(
                    "write-duration",
                    Input::new(&duration).suffix(div().pr_1().text_role(Role::Label).child("s")),
                )),
            )));
            form_dialog(dialog, "Apply", cx, submit)
        });
    }
}

impl Editor {
    pub(super) fn open_speed_duration(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.or_else(|| self.selection.first().copied()) else {
            return;
        };
        let Some(clip) = self.find_clip(id).cloned() else {
            return;
        };
        let rate = self.project.frame_rate();
        let percent = input(format!("{:.4}", clip.time_map.speed.percent()), window, cx);
        let frames = rate
            .time_to_frame(
                Time(clip.range.end.0 - clip.range.start.0),
                Rounding::Nearest,
            )
            .unwrap_or(1);
        let duration = input(frames.to_string(), window, cx);
        // Two-option choices are segmented controls and a switch, sharing a cell with the
        // submit handler since the controls only report changes.
        let by_duration = std::rc::Rc::new(std::cell::Cell::new(false));
        let reverse = std::rc::Rc::new(std::cell::Cell::new(clip.time_map.reverse));
        let ripple = std::rc::Rc::new(std::cell::Cell::new(false));
        let mode_focus: Vec<FocusHandle> = (0..2).map(|_| cx.focus_handle()).collect();
        let direction_focus: Vec<FocusHandle> = (0..2).map(|_| cx.focus_handle()).collect();
        let maintain_pitch = std::rc::Rc::new(std::cell::Cell::new(clip.maintain_pitch));
        // Keys on the clip's own clock (a clip cut into the middle of a ramp keeps the
        // curve's clock running ahead of it): the Apply command rebuilds the clock from
        // the clip start, so show and compare them rebased.
        let local_ramps = clip
            .time_map
            .local_ramps(clip.range.end.0 - clip.range.start.0);
        let ramp_text = local_ramps
            .iter()
            .map(|p| {
                format!(
                    "{}:{:.4}",
                    rate.time_to_frame(p.time, Rounding::Nearest).unwrap_or(0),
                    p.speed.percent()
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let ramps = input(ramp_text.clone(), window, cx);
        let original_ramps = local_ramps;
        let editor = cx.entity();
        open_dialog(window, cx, move |dialog, _, cx| {
            let (percent, duration, mode, reverse, ripple, ramps) = (
                percent.clone(),
                duration.clone(),
                by_duration.clone(),
                reverse.clone(),
                ripple.clone(),
                ramps.clone(),
            );
            let maintain_pitch = maintain_pitch.clone();
            let editor = editor.clone();
            let ramp_text = ramp_text.clone();
            let original_ramps = original_ramps.clone();
            let clip = clip.clone();
            let form = form_body()
                .child(style::form_row(
                    "Change by",
                    two_way(
                        "speed-mode",
                        [("speed-mode-speed", "Speed"), ("speed-mode-duration", "Duration")],
                        &mode_focus,
                        usize::from(by_duration.get()),
                        {
                            let by_duration = by_duration.clone();
                            move |index, window, _| {
                                by_duration.set(index == 1);
                                window.refresh();
                            }
                        },
                    ),
                ))
                .child(style::form_row(
                    "Speed",
                    div().w(SHORT_FIELD).child(tagged_input(
                        "speed-percent",
                        Input::new(&percent)
                            .suffix(div().pr_1().text_role(Role::Label).child("%")),
                    )),
                ))
                .child(style::form_row(
                    "Duration",
                    div().w(SHORT_FIELD).child(
                        Input::new(&duration)
                            .suffix(div().pr_1().text_role(Role::Label).child("frames")),
                    ),
                ))
                .child(style::form_row(
                    "Direction",
                    two_way(
                        "speed-direction",
                        [
                            ("speed-direction-forward", "Forward"),
                            ("speed-direction-reverse", "Reverse"),
                        ],
                        &direction_focus,
                        usize::from(reverse.get()),
                        {
                            let reverse = reverse.clone();
                            move |index, window, _| {
                                reverse.set(index == 1);
                                window.refresh();
                            }
                        },
                    ),
                ))
                .child(style::switch_row(
                    "speed-maintain-pitch",
                    "Maintain pitch",
                    None,
                    maintain_pitch.get(),
                    {
                        let value = maintain_pitch.clone();
                        move |checked, window, _| {
                            value.set(*checked);
                            window.refresh();
                        }
                    },
                ))
                .child(style::switch_row(
                    "speed-ripple",
                    "Ripple trailing clips",
                    Some("Moves later clips to close or open the gap."),
                    ripple.get(),
                    {
                        let value = ripple.clone();
                        move |checked, window, _| {
                            value.set(*checked);
                            window.refresh();
                        }
                    },
                ))
                .child(style::form_row("Ramp keys", Input::new(&ramps)))
                .child(field_caption(
                    "speed-ramp-help",
                    "Pairs of frame:percent, such as 0:100, 30:50. Ramp keys override a constant speed; clear them to use Speed or Duration.",
                ));
            let submit: Submit = std::rc::Rc::new(move |cx: &mut App| {
                let result = (|| -> Result<Command, String> {
                    let speed = if mode.get() {
                        if !ramps.read(cx).value().trim().is_empty() {
                            return Err("Clear ramp keys before setting duration".into());
                        }
                        let count = duration
                            .read(cx)
                            .value()
                            .parse::<i64>()
                            .map_err(|_| "Enter a positive frame count")?;
                        if count <= 0 {
                            return Err("Enter a positive frame count".into());
                        }
                        let target = rate
                            .frame_to_time(count, Rounding::Nearest)
                            .map_err(|e| e.0)?
                            .0;
                        if target <= 0 {
                            return Err("Duration must span at least one tick".into());
                        }
                        let (a, b) = clip.source_bounds();
                        // Quantize the requested ratio to the model's millionth lattice.
                        let units = (i128::from(b.0 - a.0) * 1_000_000 + i128::from(target) / 2)
                            / i128::from(target);
                        animathio_core::ClipSpeed::new(
                            u32::try_from(units)
                                .map_err(|_| "Duration is outside the speed limits")?,
                            1_000_000,
                        )
                        .map_err(|e| e.0)?
                    } else {
                        retime::percent(&percent.read(cx).value())?
                    };
                    let keys = if ramps.read(cx).value().as_str() == ramp_text {
                        original_ramps.clone()
                    } else {
                        retime::ramps(&ramps.read(cx).value(), rate)?
                    };
                    // Only edit what changed: a locked linked companion must not reject an
                    // edit that leaves its own setting alone.
                    let reverse = reverse.get();
                    let ripple = ripple.get();
                    let mut commands = Vec::new();
                    if speed != clip.time_map.speed
                        || reverse != clip.time_map.reverse
                        || keys != original_ramps
                        || ripple
                    {
                        commands.push(Command::SetClipSpeed {
                            id,
                            speed,
                            reverse,
                            ramps: keys,
                            ripple,
                        });
                    }
                    if maintain_pitch.get() != clip.maintain_pitch {
                        commands.push(Command::SetClipMaintainPitch {
                            id,
                            maintain_pitch: maintain_pitch.get(),
                        });
                    }
                    match commands.len() {
                        0 => Err("Nothing to change. Edit a value first.".to_string()),
                        1 => Ok(commands.remove(0)),
                        _ => Ok(Command::Batch { commands }),
                    }
                })();
                editor.update(cx, |this, cx| match &result {
                    Ok(command) => this.try_execute(command, cx),
                    Err(error) => {
                        this.error = Some(error.clone());
                        cx.notify();
                        false
                    }
                })
            });
            let dialog = dialog_frame(
                dialog,
                style::dialog_width::MEDIUM,
                IconName::SlidersHorizontal,
                "Speed and duration",
                Some("Change how fast the clip plays or how long it lasts."),
            )
            .child(form);
            form_dialog(dialog, "Apply", cx, submit)
        });
    }
    pub(super) fn open_frame_hold(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.or_else(|| self.selection.first().copied()) else {
            return;
        };
        let at = self.position;
        let frames = input("30", window, cx);
        let editor = cx.entity();
        open_dialog(window, cx, move |dialog, _, cx| {
            let frames = frames.clone();
            let editor = editor.clone();
            let submit: Submit = std::rc::Rc::new({
                let (frames, editor) = (frames.clone(), editor.clone());
                move |cx: &mut App| {
                    let count = frames.read(cx).value().parse::<u32>().ok();
                    editor.update(cx, |this, cx| {
                        if let Some(frames) = count {
                            this.try_execute(&Command::InsertFrameHold { id, at, frames }, cx)
                        } else {
                            this.error = Some("Enter a whole number of frames, at least 1.".into());
                            cx.notify();
                            false
                        }
                    })
                }
            });
            let dialog = dialog_frame(
                dialog,
                style::dialog_width::MEDIUM,
                IconName::SlidersHorizontal,
                "Insert frame hold",
                Some(
                    "Holds the frame at the playhead and moves unlocked clips after it later. \
                     Linked audio is silent during the hold.",
                ),
            )
            .child(form_body().child(style::form_row(
                "Duration",
                div().w(SHORT_FIELD).child(tagged_input(
                    "hold-frames",
                    Input::new(&frames).suffix(div().pr_1().text_role(Role::Label).child("frames")),
                )),
            )));
            form_dialog(dialog, "Insert", cx, submit)
        });
    }
}
