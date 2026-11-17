//! Modal workflows built from gpui-component dialogs, forms and inputs.
use super::*;
use crate::style;
use animathio_control::ExportPreset;
use gpui_component::dialog::{Cancel, Confirm, Dialog};
use gpui_component::form::{Field as FormField, Form};
use gpui_component::input::{Input, InputState, NumberInput};
use gpui_component::select::{Select, SelectState};
use gpui_component::{IndexPath, WindowExt};

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
        let dialog = builder(dialog, window, cx);
        if sequence != initial
            && let Some(error) = error
        {
            dialog.child(
                div()
                    .id("dialog-validation-error")
                    .debug_selector(|| "dialog-validation-error".into())
                    .text_color(cx.theme().danger)
                    .child(error),
            )
        } else {
            dialog
        }
    });
}

pub(super) fn heading(icon: IconName, title: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_size(crate::style::type_size::LG)
        .font_weight(FontWeight::SEMIBOLD)
        .child(icons::icon(icon).text_color(style::hex(style::color::ACCENT)))
        .child(title)
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

/// Route custom footer buttons through the same Confirm/Cancel actions as Enter/Escape.
pub(super) fn submit_footer(label: &'static str, app: &App) -> Div {
    div()
        .flex()
        .gap_2()
        .justify_end()
        .child(
            Button::new("dialog-cancel")
                .label("Cancel")
                .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx)),
        )
        .child(
            style::primary_button("dialog-apply", app)
                .debug_selector(|| "dialog-apply".into())
                .label(label)
                .on_click(|_, window, cx| {
                    window.dispatch_action(Box::new(Confirm { secondary: false }), cx);
                }),
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
    /// Run `action`, first asking whether to discard unsaved changes. Without
    /// this guard New, Open, Open Recent and Import v1 silently dropped the
    /// project (an untitled project has no autosave to recover from).
    pub(super) fn replace_project(
        &mut self,
        action: ReplaceProject,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.dirty {
            self.run_replace(action, window, cx);
            return;
        }
        let editor = cx.entity();
        let (title, label) = match action {
            ReplaceProject::CloseWindow => ("Quit without saving?", "Quit without saving"),
            _ => ("Discard unsaved changes?", "Discard changes"),
        };
        open_dialog(window, cx, move |dialog, _, cx| {
            let editor = editor.clone();
            let action = action.clone();
            let dialog = dialog
                .title(heading(IconName::TriangleAlert, title))
                .child(
                    div()
                        .text_size(crate::style::type_size::SM)
                        .text_color(style::hex(style::color::TEXT_SECONDARY))
                        .child("The current project has unsaved changes that will be lost."),
                )
                .footer(submit_footer(label, cx));
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
            self.error = Some("Add content before exporting".into());
            cx.notify();
            return;
        }
        let default_range = self
            .marks
            .range(duration)
            .unwrap_or_else(|| TimeRange::new(Time(0), Time(duration)).unwrap());
        let rate = self.project.frame_rate();
        let mut presets = vec!["Project settings".to_owned()];
        presets.extend(ExportPreset::ALL.map(|preset| preset.label().to_owned()));
        let preset = choices(presets, 0, window, cx);
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
        let editor = cx.entity();
        open_dialog(window, cx, move |dialog, _, cx| {
            let path_input = output.clone();
            let submit = {
                let (editor, preset, range, from, to, output) = (
                    editor.clone(),
                    preset.clone(),
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
                    let choice = selected(&range, cx);
                    let from = timecode::parse_timecode(&from.read(cx).value(), rate);
                    let to = timecode::parse_timecode(&to.read(cx).value(), rate);
                    let output = PathBuf::from(output.read(cx).value().trim());
                    let outcome = editor.update(cx, |this, cx| {
                        this.submit_export(preset, &choice, from, to, output, front, cx)
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
            let dialog = dialog
                .title(heading(IconName::Clapperboard, "Export video"))
                .width(px(560.))
                .child(
                    div()
                        .mb_3()
                        .text_size(crate::style::type_size::SM)
                        .text_color(style::hex(style::color::TEXT_SECONDARY))
                        .child("The final cut. Render a video or add it to your queue."),
                )
                .child(
                    Form::new()
                        .child(field("Preset", Select::new(&preset)))
                        .child(field(
                            "MP4 output",
                            div()
                                .flex()
                                .gap_2()
                                .child(tagged_input("export-output", Input::new(&output)))
                                .child(Button::new("export-browse").label("Browse…").on_click(
                                    move |_, window, cx| {
                                        let current =
                                            PathBuf::from(path_input.read(cx).value().to_string());
                                        let mut picker = rfd::FileDialog::new()
                                            .add_filter("MP4 video", &["mp4"])
                                            .set_file_name("video.mp4");
                                        if let Some(parent) =
                                            current.parent().filter(|p| !p.as_os_str().is_empty())
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
                        .child(field("Range", Select::new(&range)))
                        .child(field("Custom start (HH:MM:SS:FF)", Input::new(&from)))
                        .child(field("Custom end, exclusive", Input::new(&to))),
                )
                .footer(
                    div()
                        .flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("dialog-cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("export-add-to-queue")
                                .icon(icons::icon(IconName::ListVideo))
                                .label("Add to queue")
                                .on_click(move |_, window, cx| queue_submit(false, window, cx)),
                        )
                        .child(
                            style::primary_button("export-now", cx)
                                .icon(icons::icon(IconName::Upload))
                                .label("Export now")
                                .on_click(move |_, window, cx| now_submit(true, window, cx)),
                        ),
                );
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
            self.error = Some("Choose a nonempty range within the project".into());
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
        if self.export_queue.targets(&output) {
            self.error = Some("That file is already in the export queue".into());
            cx.notify();
            return Submitted::Invalid;
        }
        let spec = match self.export_spec(output.clone(), preset, range) {
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
        let name = spec
            .params
            .output
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        self.export_queue.enqueue(spec, front);
        self.export_queue_open = true;
        self.pump_export_queue();
        if !front {
            self.notices.push(crate::style::Toast::info(format!(
                "Added {name} to the export queue"
            )));
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
                    crate::style::Toast::error(format!("Export of {name} failed: {error}"))
                }
                Event::Cancelled { name, .. } => {
                    crate::style::Toast::info(format!("Export of {name} cancelled"))
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
            let dialog =
                dialog
                    .title(heading(IconName::TriangleAlert, "Replace existing file?"))
                    .width(px(480.))
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child(path.clone()),
                    )
                    .on_close(move |_, _, cx| {
                        cancel.update(cx, |this, cx| {
                            this.pending_export = None;
                            cx.notify();
                        });
                    })
                    .footer(
                        div()
                            .flex()
                            .gap_2()
                            .justify_end()
                            .child(Button::new("overwrite-cancel").label("Cancel").on_click(
                                |_, window, cx| window.dispatch_action(Box::new(Cancel), cx),
                            ))
                            .child(
                                style::primary_button("overwrite-replace", cx)
                                    .label("Replace")
                                    .on_click(|_, window, cx| {
                                        window.dispatch_action(
                                            Box::new(Confirm { secondary: false }),
                                            cx,
                                        );
                                    }),
                            ),
                    );
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
        let (width, height) = project_size(&self.project);
        let width = input(width.to_string(), window, cx);
        let height = input(height.to_string(), window, cx);
        let fps = input(
            format!(
                "{}/{}",
                self.project.frame_rate().numerator(),
                self.project.frame_rate().denominator()
            ),
            window,
            cx,
        );
        let background = self.project.background();
        let color = cx.new(|cx| {
            gpui_component::color_picker::ColorPickerState::new(window, cx)
                .default_value(fields::to_hsla(background))
        });
        let presets = choices(
            ExportPreset::ALL.map(|p| p.label().to_owned()).to_vec(),
            0,
            window,
            cx,
        );
        let editor = cx.entity();
        open_dialog(window, cx, move |dialog, _, cx| {
            let w = width.clone();
            let h = height.clone();
            let f = fps.clone();
            let c = color.clone();
            let editor = editor.clone();
            let pw = width.clone();
            let ph = height.clone();
            let preset = presets.clone();
            let submit: Submit = std::rc::Rc::new(move |cx: &mut App| {
                let size = w
                    .read(cx)
                    .value()
                    .parse::<u32>()
                    .ok()
                    .zip(h.read(cx).value().parse::<u32>().ok())
                    .filter(|(width, height)| *width > 0 && *height > 0);
                let rate = parse_fps(f.read(cx).value().trim()).ok();
                let background = {
                    let rgba = Rgba::from(
                        c.read(cx)
                            .value()
                            .unwrap_or_else(|| fields::to_hsla(background)),
                    );
                    [rgba.r, rgba.g, rgba.b, rgba.a]
                        .map(|v| (v * 255.).round().clamp(0., 255.) as u8)
                };
                editor.update(cx, |this, cx| {
                    let Some(((width, height), frame_rate)) = size.zip(rate) else {
                        this.error = Some("Enter valid dimensions and frame rate".into());
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
            let dialog = dialog
                .title(heading(IconName::Settings2, "Project settings"))
                .width(px(460.))
                .child(
                    Form::new()
                        .child(field(
                            "Resolution preset",
                            div().flex().gap_2().child(Select::new(&presets)).child(
                                Button::new("apply-size").label("Use size").on_click(
                                    move |_, window, cx| {
                                        if let Some(p) = ExportPreset::ALL
                                            .into_iter()
                                            .find(|p| p.label() == selected(&preset, cx))
                                        {
                                            let (width, height) = p.resolution();
                                            pw.update(cx, |s, cx| {
                                                s.set_value(width.to_string(), window, cx);
                                            });
                                            ph.update(cx, |s, cx| {
                                                s.set_value(height.to_string(), window, cx);
                                            });
                                        }
                                    },
                                ),
                            ),
                        ))
                        .child(field(
                            "Width",
                            tagged_input("project-width", NumberInput::new(&width)),
                        ))
                        .child(field("Height", NumberInput::new(&height)))
                        .child(field(
                            "FPS (number or numerator/denominator)",
                            Input::new(&fps),
                        ))
                        .child(field(
                            "Background",
                            gpui_component::color_picker::ColorPicker::new(&color),
                        )),
                );
            form_dialog(dialog, "Apply", cx, submit)
        });
    }

    pub(super) fn open_preferences(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let autosave = input(self.preferences.autosave_seconds.to_string(), window, cx);
        let folder = input(
            self.preferences
                .export_folder
                .to_string_lossy()
                .into_owned(),
            window,
            cx,
        );
        let gpu = choices(
            vec!["GPU".into(), "CPU".into()],
            usize::from(!self.preview.gpu),
            window,
            cx,
        );
        let density = choices(
            vec!["Compact".into(), "Comfortable".into()],
            usize::from(!self.preferences.compact),
            window,
            cx,
        );
        let scale_values: Vec<String> = std::iter::once("Desktop default".to_owned())
            .chain((80..=200).step_by(10).map(|n| format!("{n}%")))
            .collect();
        let scale_index = self
            .preferences
            .ui_scale
            .map_or(0, |n| ((n.clamp(80, 200) - 80) / 10 + 1) as usize);
        let scale = choices(scale_values, scale_index, window, cx);
        let motion = choices(
            vec!["Full motion".into(), "Reduce motion".into()],
            usize::from(self.preferences.reduce_motion),
            window,
            cx,
        );
        let check_updates = std::rc::Rc::new(std::cell::Cell::new(self.preferences.check_updates));
        let community = std::rc::Rc::new(std::cell::Cell::new(
            crate::editor_env::community_plugins_enabled(),
        ));
        let editor = cx.entity();
        open_dialog(window, cx, move |dialog, _, cx| {
            let scale_submit = scale.clone();
            let motion_submit = motion.clone();
            let autosave_submit = autosave.clone();
            let folder_submit = folder.clone();
            let gpu_submit = gpu.clone();
            let density_submit = density.clone();
            let check_submit = check_updates.clone();
            let community_submit = community.clone();
            let editor = editor.clone();
            let submit = std::rc::Rc::new(move |cx: &mut App| {
                let Ok(seconds) = autosave_submit.read(cx).value().parse::<u64>() else {
                    editor.update(cx, |this, cx| {
                        this.error =
                            Some("Enter whole seconds for autosave (0 disables it)".into());
                        cx.notify();
                    });
                    return false;
                };
                let folder = PathBuf::from(folder_submit.read(cx).value().to_string());
                let gpu = selected(&gpu_submit, cx) == "GPU";
                let compact = selected(&density_submit, cx) == "Compact";
                let reduce_motion = selected(&motion_submit, cx) == "Reduce motion";
                let check_updates = check_submit.get();
                let community = community_submit.get();
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
                        this.error = Some(error);
                        cx.notify();
                        return false;
                    }
                    this.preferences = preferences;
                    // Saving also re-reads the plugin settings, so changes made with
                    // `animathio-cli plugins` while the editor runs are picked up.
                    if let Err(error) = this.set_community_plugins_enabled(community, cx) {
                        this.error = Some(error);
                    }
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
            let dialog = dialog
                .title(heading(IconName::SlidersHorizontal, "Preferences"))
                .child(
                    Form::new()
                        .child(field("Preview renderer", Select::new(&gpu)))
                        .child(field(
                            "Autosave seconds (0 disables)",
                            div()
                                .id("preferences-autosave")
                                .debug_selector(|| "preferences-autosave".into())
                                .child(NumberInput::new(&autosave)),
                        ))
                        .child(field("Default export folder", Input::new(&folder)))
                        .child(field("Interface: UI scale", Select::new(&scale)))
                        .child(field("Theme density", Select::new(&density)))
                        .child(field("Motion", Select::new(&motion)))
                        .child(field(
                            "Check for updates",
                            gpui_component::checkbox::Checkbox::new("preferences-check-updates")
                                .checked(check_updates.get())
                                .accessibility_label("Check for updates")
                                .on_change({
                                    let check_updates = check_updates.clone();
                                    move |checked, window, _| {
                                        check_updates.set(*checked);
                                        window.refresh();
                                    }
                                }),
                        ))
                        .child(field(
                            "Community plugins (third-party, sandboxed, not audited)",
                            gpui_component::checkbox::Checkbox::new(
                                "preferences-community-plugins",
                            )
                            .checked(community.get())
                            .accessibility_label("Community plugins")
                            .on_change({
                                let community = community.clone();
                                move |checked, window, _| {
                                    community.set(*checked);
                                    window.refresh();
                                }
                            }),
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
        let scene = input("", window, cx);
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
            let dialog = dialog
                .title(heading(IconName::Code, "Import Manim scene"))
                .child(Form::new().child(field(
                    "Scene class (blank uses first Scene)",
                    Input::new(&scene),
                )));
            form_dialog(dialog, "Import", cx, submit)
        });
    }

    pub(super) fn open_write_on(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.filter(|id| self.find_clip(*id).is_some_and(|clip| matches!(&clip.kind, ClipKind::Plugin { plugin_id,.. } if plugin_id.contains("latex")))) else {
            self.error = Some("Select a LaTeX clip to apply Write on".into()); cx.notify(); return;
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
                            Some("Duration must be positive and fit within the clip".into());
                        cx.notify();
                        return false;
                    };
                    this.try_execute(&command, cx)
                })
            });
            let dialog = dialog
                .title(heading(IconName::WandSparkles, "Write on"))
                .child(Form::new().child(field(
                    "Reveal duration in seconds",
                    tagged_input("write-duration", NumberInput::new(&duration)),
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
        let mode = choices(
            vec!["Speed (%)".into(), "Duration (frames)".into()],
            0,
            window,
            cx,
        );
        let reverse = choices(
            vec!["Forward".into(), "Reverse".into()],
            usize::from(clip.time_map.reverse),
            window,
            cx,
        );
        let ripple = choices(
            vec![
                "Leave trailing clips in place".into(),
                "Ripple trailing clips".into(),
            ],
            0,
            window,
            cx,
        );
        let maintain_pitch = std::rc::Rc::new(std::cell::Cell::new(clip.maintain_pitch));
        let ramp_text = clip
            .time_map
            .ramps
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
        let original_ramps = clip.time_map.ramps.clone();
        let editor = cx.entity();
        open_dialog(window, cx, move |dialog, _, cx| {
            let (percent, duration, mode, reverse, ripple, ramps) = (
                percent.clone(),
                duration.clone(),
                mode.clone(),
                reverse.clone(),
                ripple.clone(),
                ramps.clone(),
            );
            let maintain_pitch = maintain_pitch.clone();
            let editor = editor.clone();
            let ramp_text = ramp_text.clone();
            let original_ramps = original_ramps.clone();
            let clip = clip.clone();
            let form = Form::new()
                .child(field("Change", Select::new(&mode)))
                .child(field(
                    "Speed (%)",
                    tagged_input("speed-percent", Input::new(&percent)),
                ))
                .child(field("Duration (frames)", Input::new(&duration)))
                .child(field("Direction", Select::new(&reverse)))
                .child(field(
                    "Maintain pitch",
                    gpui_component::checkbox::Checkbox::new("speed-maintain-pitch")
                        .checked(maintain_pitch.get())
                        .on_change({
                            let value = maintain_pitch.clone();
                            move |checked, window, _| {
                                value.set(*checked);
                                window.refresh();
                            }
                        }),
                ))
                .child(field("Ripple", Select::new(&ripple)))
                .child(field("Ramp keys (frame:percent, …)", Input::new(&ramps)));
            let submit: Submit = std::rc::Rc::new(move |cx: &mut App| {
                let result = (|| -> Result<Command, String> {
                    let speed = if selected(&mode, cx) == "Duration (frames)" {
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
                    Ok(Command::Batch {
                        commands: vec![
                            Command::SetClipSpeed {
                                id,
                                speed,
                                reverse: selected(&reverse, cx) == "Reverse",
                                ramps: keys,
                                ripple: selected(&ripple, cx) == "Ripple trailing clips",
                            },
                            Command::SetClipMaintainPitch {
                                id,
                                maintain_pitch: maintain_pitch.get(),
                            },
                        ],
                    })
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
            let dialog = dialog.title(heading(IconName::SlidersHorizontal,"Speed/Duration")).width(px(540.)).child(form)
                .child(div().text_size(style::type_size::SM).child("Ramp keys override constant speed. Clear keys for constant speed or duration. Linked audio follows the Maintain pitch setting. Keyframes keep their timeline times. Overlaps are rejected."));
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
                            this.error = Some("Enter a positive frame count".into());
                            cx.notify();
                            false
                        }
                    })
                }
            });
            let dialog = dialog.title(heading(IconName::SlidersHorizontal,"Insert frame hold segment"))
                .child(field("Duration (project frames)",tagged_input("hold-frames", Input::new(&frames))))
                .child(div().text_size(style::type_size::SM).child("Inserts at the playhead and ripples unlocked trailing clips. Linked audio is silent during the hold."));
            form_dialog(dialog, "Insert", cx, submit)
        });
    }
}
