//! Dialogs.

use super::*;

impl Editor {
    pub(super) fn settings_dialog(&self, cx: &mut Context<Self>) -> Div {
        let width = self.project.width();
        let height = self.project.height();
        let fps = frame_rate_label(self.project.frame_rate());
        let background = self.project.background();
        let hex = format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            background[0], background[1], background[2], background[3]
        );
        let swatch = fields::to_hsla(background);
        div()
            .absolute()
            .top(px(52.))
            .right(px(12.))
            .w(px(280.))
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(self.settings_field("Width", width.to_string(), EditTarget::SettingsWidth, cx))
            .child(self.settings_field(
                "Height",
                height.to_string(),
                EditTarget::SettingsHeight,
                cx,
            ))
            .child(self.settings_field("Frame rate", fps, EditTarget::SettingsFps, cx))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .child(div().w(px(16.)).h(px(12.)).bg(swatch))
                    .child(self.settings_field(
                        "Background",
                        hex,
                        EditTarget::SettingsBackground,
                        cx,
                    )),
            )
            .child(
                div()
                    .cursor_pointer()
                    .px_2()
                    .child("Close")
                    .id("settings-close")
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        this.settings_open = false;
                        cx.notify();
                    })),
            )
    }

    pub(super) fn settings_field(
        &self,
        label: &str,
        value: String,
        target: EditTarget,
        cx: &mut Context<Self>,
    ) -> Div {
        let display = if let Some(edit) = &self.editing
            && edit.target == target
        {
            edit.buffer.clone()
        } else {
            value.clone()
        };
        div()
            .flex()
            .flex_row()
            .justify_between()
            .px_2()
            .py_1()
            .child(label.to_owned())
            .child(
                div()
                    .cursor_pointer()
                    .child(display)
                    .id("settings-field")
                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                        this.editing = Some(TextEdit {
                            target: target.clone(),
                            buffer: value.clone(),
                        });
                        cx.notify();
                    })),
            )
    }

    pub(super) fn open_export_dialog(&mut self, cx: &mut Context<Self>) {
        if self.export_task.is_some() {
            return;
        }
        let Some(output) = rfd::FileDialog::new()
            .add_filter("MP4 video", &["mp4"])
            .set_file_name("video.mp4")
            .save_file()
        else {
            return;
        };
        // Export never overwrites silently: ask before replacing an existing file.
        if output.exists() {
            self.confirm_overwrite = Some(output);
            cx.notify();
            return;
        }
        self.start_export(output, false, cx);
    }

    /// Spawn the export worker; the export dialog shows progress and carries the
    /// confirm-overwrite prompt.
    pub(super) fn start_export(
        &mut self,
        output: PathBuf,
        overwrite: bool,
        cx: &mut Context<Self>,
    ) {
        if self.export_task.is_some() {
            return;
        }
        let params = animathio_control::ExportParams {
            output,
            from: Time(0),
            to: None,
            settings: animathio_export::Settings::default(),
            overwrite,
        };
        match crate::export_worker::ExportTask::spawn(
            self.project.clone(),
            self.assets.clone(),
            params,
            None,
        ) {
            Ok(task) => self.export_task = Some(task),
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    pub(super) fn toggle_settings(&mut self, cx: &mut Context<Self>) {
        self.settings_open = !self.settings_open;
        cx.notify();
    }

    pub(super) fn export_dialog(&self, cx: &mut Context<Self>) -> Div {
        use std::sync::atomic::Ordering;
        let Some(task) = &self.export_task else {
            let Some(path) = &self.confirm_overwrite else {
                return div();
            };
            return div()
                .absolute()
                .top(px(60.))
                .left(px(30.))
                .w(px(280.))
                .p_3()
                .bg(rgb(0x1f2430))
                .flex()
                .flex_col()
                .gap_2()
                .child("Replace existing file?")
                .child(
                    div()
                        .text_color(rgb(0x8a94a6))
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
        let width = if total == 0 {
            0.0
        } else {
            240.0 * completed as f32 / total as f32
        };
        div()
            .absolute()
            .top(px(60.))
            .left(px(30.))
            .w(px(280.))
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
                div()
                    .w(px(240.))
                    .h(px(8.))
                    .bg(cx.theme().muted)
                    .child(div().w(px(width)).h(px(8.)).bg(cx.theme().progress_bar)),
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
