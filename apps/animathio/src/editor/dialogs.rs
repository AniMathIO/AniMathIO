//! Modal workflows built from gpui-component dialogs, forms and inputs.
use super::*;
use crate::style;
use animathio_control::ExportPreset;
use gpui_component::form::{Field as FormField, Form};
use gpui_component::input::{Input, InputState, NumberInput};
use gpui_component::select::{Select, SelectState};
use gpui_component::{IndexPath, WindowExt};

fn heading(icon: IconName, title: &'static str) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2()
        .text_size(px(16.))
        .font_weight(FontWeight::SEMIBOLD)
        .child(icons::icon(icon).text_color(style::hex(style::color::ACCENT)))
        .child(title)
}

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
    app: &App,
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
            style::primary_button("dialog-apply", app)
                .label(label)
                .on_click(callback),
        )
}

/// Outcome of submitting the Export dialog.
enum Submitted {
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
            self.next_export_path().to_string_lossy().into_owned(),
            window,
            cx,
        );
        let editor = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
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
                        .find(|p| p.name() == preset_name);
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
            dialog
                .title(heading(IconName::Clapperboard, "Export video"))
                .width(px(560.))
                .child(
                    div()
                        .mb_3()
                        .text_size(px(12.))
                        .text_color(style::hex(style::color::TEXT_SECONDARY))
                        .child("The final cut. Render a video or add it to your queue."),
                )
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
                )
        });
    }

    /// A free `video.mp4`, `video-2.mp4`, ... in the default export folder, so
    /// repeated queueing does not collide with earlier jobs or files.
    fn next_export_path(&self) -> PathBuf {
        let folder = &self.preferences.export_folder;
        let candidate = |n: u32| {
            if n == 1 {
                folder.join("video.mp4")
            } else {
                folder.join(format!("video-{n}.mp4"))
            }
        };
        (1..=9999)
            .map(candidate)
            .find(|path| !path.exists() && !self.export_queue.targets(path))
            .unwrap_or_else(|| candidate(1))
    }

    #[allow(clippy::too_many_arguments)] // Dialog fields, passed straight through.
    fn submit_export(
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
            preset.map_or("Project settings", ExportPreset::name),
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
        window.open_dialog(cx, move |dialog, _, cx| {
            let (replace, cancel) = (editor.clone(), editor.clone());
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
                        .child(
                            Button::new("overwrite-cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            style::primary_button("overwrite-replace", cx)
                                .label("Replace")
                                .on_click(move |_, window, cx| {
                                    replace.update(cx, |this, cx| {
                                        if let Some((mut spec, front)) = this.pending_export.take()
                                        {
                                            spec.params.overwrite = true;
                                            this.enqueue_export(spec, front, cx);
                                        }
                                    });
                                    window.close_dialog(cx);
                                }),
                        ),
                )
        });
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
        window.open_dialog(cx, move |dialog, _, cx| {
            let w = width.clone();
            let h = height.clone();
            let f = fps.clone();
            let c = color.clone();
            let editor = editor.clone();
            let pw = width.clone();
            let ph = height.clone();
            let preset = presets.clone();
            dialog
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
                .footer(footer("Apply", cx, move |_, window, cx| {
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
        let motion = choices(
            vec!["Full motion".into(), "Reduce motion".into()],
            usize::from(self.preferences.reduce_motion),
            window,
            cx,
        );
        let editor = cx.entity();
        window.open_dialog(cx, move |dialog, _, cx| {
            let motion_submit = motion.clone();
            let autosave_submit = autosave.clone();
            let folder_submit = folder.clone();
            let gpu_submit = gpu.clone();
            let density_submit = density.clone();
            let editor = editor.clone();
            dialog
                .title(heading(IconName::SlidersHorizontal, "Preferences"))
                .child(
                    Form::new()
                        .child(field("Preview renderer", Select::new(&gpu)))
                        .child(field(
                            "Autosave seconds (0 disables)",
                            NumberInput::new(&autosave),
                        ))
                        .child(field("Default export folder", Input::new(&folder)))
                        .child(field("Theme density", Select::new(&density)))
                        .child(field("Motion", Select::new(&motion))),
                )
                .footer(footer("Save", cx, move |_, window, cx| {
                    let Ok(seconds) = autosave_submit.read(cx).value().parse::<u64>() else {
                        return;
                    };
                    let folder = PathBuf::from(folder_submit.read(cx).value().to_string());
                    let gpu = selected(&gpu_submit, cx) == "GPU";
                    let compact = selected(&density_submit, cx) == "Compact";
                    let reduce_motion = selected(&motion_submit, cx) == "Reduce motion";
                    let saved = editor.update(cx, |this, cx| {
                        let preferences = crate::preferences::Preferences {
                            autosave_seconds: seconds,
                            export_folder: folder,
                            compact,
                            reduce_motion,
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
                        cx.set_reduce_motion(reduce_motion);
                        gpui_component::Theme::update(cx, |theme| {
                            theme.font_size = px(if compact { 12. } else { 14. });
                            theme.motion.duration_fast = Duration::from_millis(if reduce_motion {
                                0
                            } else {
                                style::motion::FAST_MS
                            });
                            theme.motion.duration_normal =
                                Duration::from_millis(if reduce_motion {
                                    0
                                } else {
                                    style::motion::BASE_MS
                                });
                            theme.motion.duration_slow = theme.motion.duration_normal;
                        });
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
        window.open_dialog(cx, move |dialog, _, cx| {
            let editor = editor.clone();
            let scene_submit = scene.clone();
            let path = path.clone();
            dialog
                .title(heading(IconName::Code, "Import Manim scene"))
                .child(Form::new().child(field(
                    "Scene class (blank uses first Scene)",
                    Input::new(&scene),
                )))
                .footer(footer("Import", cx, move |_, window, cx| {
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
        window.open_dialog(cx, move |dialog, _, cx| {
            let duration_submit = duration.clone();
            let editor = editor.clone();
            dialog
                .title(heading(IconName::WandSparkles, "Write on"))
                .child(Form::new().child(field(
                    "Reveal duration in seconds",
                    NumberInput::new(&duration),
                )))
                .footer(footer("Apply", cx, move |_, window, cx| {
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
}
