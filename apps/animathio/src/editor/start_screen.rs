//! Project launcher, shown until a project is opened or started.
use super::*;
use animathio_control::ExportPreset;

impl Editor {
    pub(super) fn is_start_screen(&self) -> bool {
        self.path.is_none() && self.project.tracks().is_empty() && self.project.assets().is_empty()
    }
    pub(super) fn start_screen(&self, cx: &mut Context<Self>) -> Div {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(cx.theme().background)
            .on_drop::<ExternalPaths>(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                let track = this.ensure_track(cx);
                this.import_paths(paths.paths(), Some((track, Time(0))), cx);
            }))
            .child(
                div()
                    .w(px(650.))
                    .p_6()
                    .rounded_lg()
                    .bg(cx.theme().popover)
                    .border_1()
                    .border_color(cx.theme().border)
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(div().text_2xl().child("AniMathIO"))
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child("Create a project"),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .children(ExportPreset::ALL.map(|preset| {
                                let (width, height) = preset.resolution();
                                let label = match preset {
                                    ExportPreset::Youtube1080p => "1080p",
                                    ExportPreset::Youtube4k => "4K",
                                    ExportPreset::ShortsVertical => "9:16 vertical",
                                    ExportPreset::Square => "Square",
                                };
                                Button::new(preset.name())
                                    .label(label)
                                    .icon(icons::icon(IconName::Clapperboard))
                                    .tooltip(format!("{width} x {height}, 30 fps"))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.new_project(cx);
                                        if let Ok(command) = preset.command(&this.project) {
                                            this.execute(&command, cx);
                                        }
                                    }))
                            })),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                Button::new("start-open")
                                    .primary()
                                    .icon(icons::icon(IconName::FolderOpen))
                                    .label("Open…")
                                    .on_click(cx.listener(|this, _, _, cx| this.open(cx))),
                            )
                            .child(
                                Button::new("start-v1")
                                    .label("Import v1…")
                                    .on_click(cx.listener(|this, _, _, cx| this.import_legacy(cx))),
                            )
                            .child(Button::new("start-manim").label("Import Manim…").on_click(
                                cx.listener(|this, _, window, cx| {
                                    this.open_manim_import(window, cx)
                                }),
                            )),
                    )
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child("Recent projects"),
                    )
                    .child(
                        div()
                            .id("start-recent-list")
                            .max_h(px(260.))
                            .overflow_y_scroll()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .children(self.recent.paths.iter().enumerate().map(|(index, path)| {
                                let path = path.clone();
                                Button::new(("start-recent", index))
                                    .ghost()
                                    .label(path.display().to_string())
                                    .on_click(
                                        cx.listener(move |this, _, _, cx| {
                                            this.open_path(&path, cx)
                                        }),
                                    )
                            })),
                    )
                    .child(
                        div()
                            .text_color(cx.theme().muted_foreground)
                            .child("Drop media here to start"),
                    ),
            )
    }
}
