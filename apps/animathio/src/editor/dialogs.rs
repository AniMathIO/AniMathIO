//! Modal workflows built from gpui-component dialogs, forms and inputs.
use super::*;
use animathio_control::ExportPreset;
use gpui_component::form::{Field as FormField, Form};
use gpui_component::input::{Input, InputState, NumberInput};
use gpui_component::select::{Select, SelectState};
use gpui_component::{IndexPath, WindowExt};

type Choices = Entity<SelectState<Vec<String>>>;
fn input(value: impl Into<SharedString>, window: &mut Window, cx: &mut App) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).default_value(value))
}
fn choices(items: Vec<String>, selected: usize, window: &mut Window, cx: &mut App) -> Choices {
    cx.new(|cx| SelectState::new(items, Some(IndexPath::default().row(selected)), window, cx))
}
fn selected(state: &Choices, cx: &App) -> String {
    state.read(cx).selected_value().cloned().unwrap_or_default()
}
fn field(label: &'static str, child: impl IntoElement) -> FormField {
    FormField::new().label(label).child(child)
}
fn footer(
    label: &'static str,
    callback: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Div {
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
            Button::new("dialog-apply")
                .primary()
                .label(label)
                .on_click(callback),
        )
}

#[derive(Clone)]
pub(super) struct ExportOptions {
    preset: Option<ExportPreset>,
    range: TimeRange,
}

impl Editor {
    pub(super) fn open_export_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.export_task.is_some() {
            self.open_export_status(window, cx);
            return;
        }
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
        presets.extend(ExportPreset::ALL.map(|preset| preset.name().to_owned()));
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
            self.preferences
                .export_folder
                .join("video.mp4")
                .to_string_lossy()
                .into_owned(),
            window,
            cx,
        );
        let editor = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let path_input = output.clone();
            let editor = editor.clone();
            let preset_submit = preset.clone();
            let range_submit = range.clone();
            let from_submit = from.clone();
            let to_submit = to.clone();
            let output_submit = output.clone();
            dialog
                .title("Export video")
                .width(px(520.))
                .child(
                    Form::new()
                        .child(field("Preset", Select::new(&preset)))
                        .child(field(
                            "MP4 output",
                            div().flex().gap_2().child(Input::new(&output)).child(
                                Button::new("export-browse").label("Browse…").on_click(
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
                                ),
                            ),
                        ))
                        .child(field("Range", Select::new(&range)))
                        .child(field("Custom start (HH:MM:SS:FF)", Input::new(&from)))
                        .child(field("Custom end, exclusive", Input::new(&to))),
                )
                .footer(footer("Export", move |_, window, cx| {
                    let preset_name = selected(&preset_submit, cx);
                    let preset = ExportPreset::ALL
                        .into_iter()
                        .find(|p| p.name() == preset_name);
                    let choice = selected(&range_submit, cx);
                    let from = timecode::parse_timecode(&from_submit.read(cx).value(), rate);
                    let to = timecode::parse_timecode(&to_submit.read(cx).value(), rate);
                    let output = PathBuf::from(output_submit.read(cx).value().trim());
                    let valid = editor.update(cx, |this, cx| {
                        let duration = animathio_export::duration(&this.project).0;
                        let range = match choice.as_str() {
                            "In/out" => this.marks.range(duration),
                            "Custom" => from
                                .zip(to)
                                .and_then(|(from, to)| TimeRange::new(from, to).ok())
                                .filter(|r| {
                                    r.start < r.end && r.start.0 >= 0 && r.end.0 <= duration
                                }),
                            _ => TimeRange::new(Time(0), Time(duration)).ok(),
                        };
                        let Some(range) = range else {
                            this.error = Some("Choose a nonempty range within the project".into());
                            cx.notify();
                            return false;
                        };
                        if output.as_os_str().is_empty() || output.file_name().is_none() {
                            this.error = Some("Choose an output file".into());
                            cx.notify();
                            return false;
                        }
                        this.export_options = Some(ExportOptions { preset, range });
                        if output.exists() {
                            this.confirm_overwrite = Some(output);
                        } else {
                            this.start_export(output, false, cx);
                        }
                        cx.notify();
                        true
                    });
                    if valid {
                        window.close_dialog(cx);
                        editor.update(cx, |this, cx| this.open_export_status(window, cx));
                    }
                }))
        });
    }

    fn open_export_status(&self, window: &mut Window, cx: &mut Context<Self>) {
        let title = if self.export_task.is_some() {
            "Exporting video"
        } else {
            "Export video"
        };
        let editor = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let content = editor.update(cx, |this, cx| this.export_dialog(cx));
            let editor = editor.clone();
            dialog
                .title(title)
                .child(content)
                .on_close(move |_, _, cx| {
                    editor.update(cx, |this, cx| {
                        this.confirm_overwrite = None;
                        cx.notify();
                    });
                })
        });
    }

    pub(super) fn start_export(
        &mut self,
        output: PathBuf,
        overwrite: bool,
        cx: &mut Context<Self>,
    ) {
        if self.export_task.is_some() {
            return;
        }
        let mut project = self.project.clone();
        let mut settings = animathio_export::Settings::default();
        let range = self
            .export_options
            .as_ref()
            .map(|options| options.range)
            .or_else(|| self.marks.range(timeline::project_duration(&project)))
            .unwrap();
        if let Some(preset) = self
            .export_options
            .as_ref()
            .and_then(|options| options.preset)
        {
            match preset
                .command(&project)
                .and_then(|command| command.apply(&mut project).map_err(ControlError::from))
            {
                Ok(_) => settings = preset.export_settings(),
                Err(error) => {
                    self.error = Some(error.to_string());
                    return;
                }
            }
        }
        let params = animathio_control::ExportParams {
            output,
            from: range.start,
            to: Some(range.end),
            settings,
            overwrite,
        };
        match crate::export_worker::ExportTask::spawn(project, self.assets.clone(), params, None) {
            Ok(task) => self.export_task = Some(task),
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    pub(super) fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let width = input(self.project.width().to_string(), window, cx);
        let height = input(self.project.height().to_string(), window, cx);
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
            ExportPreset::ALL.map(|p| p.name().to_owned()).to_vec(),
            0,
            window,
            cx,
        );
        let editor = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let w = width.clone();
            let h = height.clone();
            let f = fps.clone();
            let c = color.clone();
            let editor = editor.clone();
            let pw = width.clone();
            let ph = height.clone();
            let preset = presets.clone();
            dialog
                .title("Project settings")
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
                                            .find(|p| p.name() == selected(&preset, cx))
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
                        .child(field("Width", NumberInput::new(&width)))
                        .child(field("Height", NumberInput::new(&height)))
                        .child(field(
                            "FPS (number or numerator/denominator)",
                            Input::new(&fps),
                        ))
                        .child(field(
                            "Background",
                            gpui_component::color_picker::ColorPicker::new(&color),
                        )),
                )
                .footer(footer("Apply", move |_, window, cx| {
                    let size = w
                        .read(cx)
                        .value()
                        .parse::<u32>()
                        .ok()
                        .zip(h.read(cx).value().parse::<u32>().ok());
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
                    let valid = editor.update(cx, |this, cx| {
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
                        match this.history.execute(&mut this.project, &command) {
                            Ok(()) => {
                                this.after_edit(cx);
                                true
                            }
                            Err(error) => {
                                this.error = Some(error.0);
                                cx.notify();
                                false
                            }
                        }
                    });
                    if valid {
                        window.close_dialog(cx);
                    }
                }))
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
        let editor = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let autosave_submit = autosave.clone();
            let folder_submit = folder.clone();
            let gpu_submit = gpu.clone();
            let density_submit = density.clone();
            let editor = editor.clone();
            dialog
                .title("Preferences")
                .child(
                    Form::new()
                        .child(field("Preview renderer", Select::new(&gpu)))
                        .child(field(
                            "Autosave seconds (0 disables)",
                            NumberInput::new(&autosave),
                        ))
                        .child(field("Default export folder", Input::new(&folder)))
                        .child(field("Theme density", Select::new(&density))),
                )
                .footer(footer("Save", move |_, window, cx| {
                    let Ok(seconds) = autosave_submit.read(cx).value().parse::<u64>() else {
                        return;
                    };
                    let folder = PathBuf::from(folder_submit.read(cx).value().to_string());
                    let gpu = selected(&gpu_submit, cx) == "GPU";
                    let compact = selected(&density_submit, cx) == "Compact";
                    let saved = editor.update(cx, |this, cx| {
                        let preferences = crate::preferences::Preferences {
                            autosave_seconds: seconds,
                            export_folder: folder,
                            compact,
                        };
                        if let Err(error) = preferences.save() {
                            this.error = Some(error);
                            cx.notify();
                            return false;
                        }
                        this.preferences = preferences;
                        if gpu != this.preview.gpu {
                            this.toggle_gpu_preview(cx);
                        }
                        cx.global_mut::<gpui_component::Theme>().font_size =
                            px(if compact { 12. } else { 14. });
                        cx.notify();
                        true
                    });
                    if saved {
                        window.close_dialog(cx);
                    }
                }))
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
        window.open_dialog(cx, move |dialog, _, _| {
            let editor = editor.clone();
            let scene_submit = scene.clone();
            let path = path.clone();
            dialog
                .title("Import Manim scene")
                .child(Form::new().child(field(
                    "Scene class (blank uses first Scene)",
                    Input::new(&scene),
                )))
                .footer(footer("Import", move |_, window, cx| {
                    let scene = scene_submit.read(cx).value().trim().to_owned();
                    let imported = editor.update(cx, |this, cx| {
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
                    });
                    if imported {
                        window.close_dialog(cx);
                    }
                }))
        });
    }

    pub(super) fn open_write_on(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selected.filter(|id| self.find_clip(*id).is_some_and(|clip| matches!(&clip.kind, ClipKind::Plugin { plugin_id,.. } if plugin_id.contains("latex")))) else {
            self.error = Some("Select a LaTeX clip to apply Write on".into()); cx.notify(); return;
        };
        let duration = input("1", window, cx);
        let editor = cx.entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let duration_submit = duration.clone();
            let editor = editor.clone();
            dialog
                .title("Write on")
                .child(Form::new().child(field(
                    "Reveal duration in seconds",
                    NumberInput::new(&duration),
                )))
                .footer(footer("Apply", move |_, window, cx| {
                    let seconds = duration_submit.read(cx).value().parse::<f64>().ok();
                    let applied = editor.update(cx, |this, cx| {
                        let Some(command) = this.find_clip(id).and_then(|clip| {
                            seconds.and_then(|seconds| editing::write_on_command(clip, seconds))
                        }) else {
                            this.error =
                                Some("Duration must be positive and fit within the clip".into());
                            cx.notify();
                            return false;
                        };
                        this.execute(&command, cx);
                        true
                    });
                    if applied {
                        window.close_dialog(cx);
                    }
                }))
        });
    }

    pub(super) fn export_dialog(&self, cx: &mut Context<Self>) -> Div {
        use std::sync::atomic::Ordering;
        let Some(task) = &self.export_task else {
            let Some(path) = &self.confirm_overwrite else {
                return div().child("Export finished. See the notification for the result.");
            };
            return div()
                .w_full()
                .p_3()
                .bg(cx.theme().popover)
                .flex()
                .flex_col()
                .gap_2()
                .child("Replace existing file?")
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child(path.display().to_string()),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_2()
                        .child(button(
                            "confirm-overwrite",
                            "Replace",
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                if let Some(path) = this.confirm_overwrite.take() {
                                    this.start_export(path, true, cx);
                                }
                                cx.notify();
                            }),
                        ))
                        .child(button(
                            "cancel-overwrite",
                            "Cancel",
                            cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.confirm_overwrite = None;
                                cx.notify();
                            }),
                        )),
                );
        };
        let completed = task.completed.load(Ordering::Relaxed);
        let total = task.total.load(Ordering::Relaxed);
        let percent = if total == 0 {
            0.0
        } else {
            100.0 * completed as f32 / total as f32
        };
        div()
            .w_full()
            .p_3()
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .flex()
            .flex_col()
            .gap_2()
            .child("Exporting MP4")
            .child(format!("{completed} / {total} frames"))
            .child(
                gpui_component::progress::Progress::new("export-progress")
                    .value(percent)
                    .loading(total == 0),
            )
            .child(button(
                "cancel-export",
                "Cancel",
                cx.listener(|this, _: &ClickEvent, _, cx| {
                    if let Some(task) = &this.export_task {
                        task.cancel.store(true, Ordering::Relaxed);
                    }
                    cx.notify();
                }),
            ))
    }
}
