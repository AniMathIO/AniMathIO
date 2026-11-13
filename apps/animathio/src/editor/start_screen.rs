//! Project launcher. Decorative motion lives only here, never during playback.
use super::*;
use crate::style::{self, color};
use animathio_control::ExportPreset;

/// A stable, friendly age for the cached filesystem modification timestamp.
fn modified_label(modified: Option<SystemTime>) -> String {
    let Some(age) = modified.and_then(|time| time.elapsed().ok()) else {
        return "Modified time unavailable".to_owned();
    };
    match age.as_secs() {
        0..60 => "Modified just now".to_owned(),
        60..3600 => format!("Modified {} min ago", age.as_secs() / 60),
        3600..86400 => format!("Modified {} h ago", age.as_secs() / 3600),
        _ => format!("Modified {} days ago", age.as_secs() / 86400),
    }
}

impl Editor {
    pub(super) fn is_start_screen(&self) -> bool {
        self.path.is_none() && self.project.tracks().is_empty() && self.project.assets().is_empty()
    }

    pub(super) fn start_screen(&self, cx: &mut Context<Self>) -> Div {
        let glow = div()
            .absolute()
            .inset_0()
            .rounded(px(18.))
            .bg(linear_gradient(
                115.,
                linear_color_stop(style::hexa(color::ACCENT_FROM, 0.20), 0.),
                linear_color_stop(style::hexa(color::ACCENT_TO, 0.06), 1.),
            ))
            .with_animation(
                "launcher-glow",
                Animation::new(Duration::from_secs(6))
                    .repeat()
                    .with_max_fps(20.),
                |this, progress| {
                    this.opacity(0.65 + 0.35 * (progress * std::f32::consts::TAU).cos())
                },
            );
        let hero = div()
            .relative()
            .flex()
            .items_center()
            .justify_between()
            .gap_6()
            .px_8()
            .py_6()
            .rounded(px(18.))
            .border_1()
            .border_color(style::hexa(color::ACCENT, 0.22))
            .bg(style::hex(color::PANEL))
            .child(glow)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .child(img("brand.svg").size(px(44.)))
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("AniMathIO")
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(style::hex(color::TEXT_SECONDARY))
                                            .child("THE CREATIVE WORKSPACE"),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(px(38.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Make ideas move."),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(style::hex(color::TEXT_SECONDARY))
                            .child("A clear canvas for your next great explanation."),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .mt_1()
                            .text_size(px(11.))
                            .text_color(style::hex(color::TEXT_SECONDARY))
                            .child(icons::icon(IconName::Layers).small())
                            .child("Compose")
                            .child(div().w(px(16.)))
                            .child(icons::icon(IconName::WandSparkles).small())
                            .child("Animate")
                            .child(div().w(px(16.)))
                            .child(icons::icon(IconName::Clapperboard).small())
                            .child("Tell your story"),
                    ),
            )
            .child(
                div()
                    .relative()
                    .w(px(280.))
                    .h(px(168.))
                    .flex_shrink_0()
                    .rounded(px(12.))
                    .bg(style::hexa(color::BG, 0.6))
                    .border_1()
                    .border_color(style::hexa(color::ACCENT, 0.22))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(img("brand.svg").size(px(140.)))
                    .child(
                        div()
                            .absolute()
                            .bottom_3()
                            .left_4()
                            .right_4()
                            .flex()
                            .items_center()
                            .gap_1()
                            .children([color::CLIP_VIDEO, color::ACCENT, color::CLIP_AUDIO].map(
                                |tone| {
                                    div()
                                        .flex_1()
                                        .h(px(5.))
                                        .rounded_full()
                                        .bg(style::hexa(tone, 0.6))
                                },
                            )),
                    ),
            );

        let hover_progress = if self.preferences.reduce_motion {
            1.
        } else {
            (self.preset_hover_changed.elapsed().as_secs_f32()
                / (style::motion::BASE_MS as f32 / 1000.))
                .min(1.)
        };
        let hover_eased = 1. - (1. - hover_progress).powi(3);
        let presets = div()
            .flex()
            .gap_3()
            .children(
                ExportPreset::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, preset)| {
                        let (width, height) = preset.resolution();
                        let (label, detail) = match preset {
                            ExportPreset::Youtube1080p => ("1080p Full HD", "1920 x 1080"),
                            ExportPreset::Youtube4k => ("4K Ultra HD", "3840 x 2160"),
                            ExportPreset::ShortsVertical => ("9:16 Vertical", "1080 x 1920"),
                            ExportPreset::Square => ("Square", "1080 x 1080"),
                        };
                        let preview_width = if width >= height { 114. } else { 48. };
                        let preview_height = preview_width * height as f32 / width as f32;
                        let from = if self.previous_hovered_preset == Some(index) {
                            1.
                        } else {
                            0.
                        };
                        let to = if self.hovered_preset == Some(index) {
                            1.
                        } else {
                            0.
                        };
                        div()
                            .id(("start-preset", index))
                            .focusable()
                            .tab_index(0)
                            .focus_visible(|this| this.border_color(style::hex(color::ACCENT)))
                            .top(px(-3. * (from + (to - from) * hover_eased)))
                            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                                if *hovering || this.hovered_preset == Some(index) {
                                    this.previous_hovered_preset = this.hovered_preset;
                                    this.hovered_preset = hovering.then_some(index);
                                    this.preset_hover_changed = Instant::now();
                                    cx.notify();
                                }
                            }))
                            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.new_project(cx);
                                    if let Ok(command) = preset.command(&this.project) {
                                        this.execute(&command, cx);
                                    }
                                    cx.stop_propagation();
                                }
                            }))
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .p_4()
                            .rounded(px(style::radius::PANEL))
                            .border_1()
                            .border_color(style::hex(color::BORDER))
                            .bg(style::hex(color::PANEL))
                            .cursor_pointer()
                            .hover(|this| {
                                this.border_color(style::hex(color::ACCENT))
                                    .bg(style::hex(color::RAISED))
                                    .shadow_lg()
                            })
                            .active(|this| this.bg(style::hex(color::HOVER)))
                            .child(
                                div()
                                    .h(px(96.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .mb_3()
                                    .child(
                                        div()
                                            .relative()
                                            .w(px(preview_width))
                                            .h(px(preview_height))
                                            .rounded(px(5.))
                                            .border_1()
                                            .border_color(style::hexa(color::ACCENT, 0.6))
                                            .bg(linear_gradient(
                                                135.,
                                                linear_color_stop(
                                                    style::hexa(color::ACCENT_FROM, 0.26),
                                                    0.,
                                                ),
                                                linear_color_stop(
                                                    style::hexa(color::ACCENT_TO, 0.12),
                                                    1.,
                                                ),
                                            ))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                icons::icon(IconName::Play)
                                                    .text_color(style::hex(color::ACCENT)),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .child(
                                        div()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_size(px(14.))
                                            .child(label),
                                    )
                                    .child(
                                        icons::icon(IconName::ArrowUpRight)
                                            .small()
                                            .text_color(style::hex(color::TEXT_MUTED)),
                                    ),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_size(px(11.))
                                    .text_color(style::hex(color::TEXT_SECONDARY))
                                    .child(format!("{detail} / 30 fps")),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.new_project(cx);
                                if let Ok(command) = preset.command(&this.project) {
                                    this.execute(&command, cx);
                                }
                            }))
                    }),
            );

        let recent = div()
            .id("start-recent-list")
            .max_h(px(154.))
            .overflow_y_scroll()
            .flex()
            .flex_wrap()
            .gap_3()
            .when(self.recent.paths.is_empty(), |row| {
                row.child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap_3()
                        .p_4()
                        .rounded(px(style::radius::PANEL))
                        .bg(style::hex(color::PANEL))
                        .border_1()
                        .border_color(style::hex(color::BORDER))
                        .child(
                            icons::icon(IconName::FolderOpen)
                                .text_color(style::hex(color::TEXT_MUTED)),
                        )
                        .child(
                            div()
                                .text_color(style::hex(color::TEXT_SECONDARY))
                                .child("Your next project starts here.")
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .mt_1()
                                        .text_color(style::hex(color::TEXT_MUTED))
                                        .child(
                                            "Choose a canvas above, or open an existing project.",
                                        ),
                                ),
                        ),
                )
            })
            .children(
                self.recent
                    .paths
                    .iter()
                    .take(6)
                    .enumerate()
                    .map(|(index, path)| {
                        let modified = modified_label(self.recent.modified.get(path).copied());
                        let name = path.file_stem().map_or_else(
                            || "Untitled project".to_owned(),
                            |name| name.to_string_lossy().into_owned(),
                        );
                        let path = path.clone();
                        let keyboard_path = path.clone();
                        div()
                            .id(("start-recent", index))
                            .focusable()
                            .tab_index(0)
                            .focus_visible(|this| this.border_color(style::hex(color::ACCENT)))
                            .on_key_down(cx.listener(move |this, event: &KeyDownEvent, _, cx| {
                                if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                                    this.open_path(&keyboard_path, cx);
                                    cx.stop_propagation();
                                }
                            }))
                            .w(relative(0.32))
                            .min_w_0()
                            .p_3()
                            .rounded(px(style::radius::PANEL))
                            .bg(style::hex(color::PANEL))
                            .border_1()
                            .border_color(style::hex(color::BORDER))
                            .cursor_pointer()
                            .hover(|this| {
                                this.bg(style::hex(color::RAISED))
                                    .border_color(style::hexa(color::ACCENT, 0.6))
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .mb_2()
                                    .child(
                                        icons::icon(IconName::Clapperboard)
                                            .small()
                                            .text_color(style::hex(color::ACCENT)),
                                    )
                                    .child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(name),
                                    ),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(style::hex(color::TEXT_SECONDARY))
                                    .child(path.display().to_string()),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_size(px(10.))
                                    .text_color(style::hex(color::TEXT_MUTED))
                                    .child(modified),
                            )
                            .on_click(cx.listener(move |this, _, _, cx| this.open_path(&path, cx)))
                    }),
            );

        div().flex_1().min_h_0().w_full()
            .child(canvas(|_, _, _| {}, move |_, (), window, _| {
                if hover_progress < 1. { window.request_animation_frame(); }
            }).absolute().size_full())
            .child(
            div()
                .id("launcher-scroll")
                .size_full()
                .overflow_y_scroll()
                .px_8()
                .py_6()
                .on_drop::<ExternalPaths>(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                    let track = this.ensure_track(cx);
                    this.import_paths(paths.paths(), Some((track, Time(0))), cx);
                }))
                .drag_over::<ExternalPaths>(|style, _, _, _| {
                    style.bg(style::hexa(color::ACCENT, 0.08))
                })
                .child(
                    div()
                        .max_w(px(1120.))
                        .mx_auto()
                        .flex()
                        .flex_col()
                        .gap_5()
                        .child(hero)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Choose your canvas"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(style::hex(color::TEXT_SECONDARY))
                                        .child("A fresh start, in any format"),
                                ),
                        )
                        .child(presets)
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(
                                    Button::new("start-open")
                                        .icon(icons::icon(IconName::FolderOpen))
                                        .label("Open project…")
                                        .on_click(cx.listener(|this, _, _, cx| this.open(cx))),
                                )
                                .child(
                                    Button::new("start-v1")
                                        .icon(icons::icon(IconName::Import))
                                        .label("Import v1…")
                                        .on_click(
                                            cx.listener(|this, _, _, cx| this.import_legacy(cx)),
                                        ),
                                )
                                .child(
                                    Button::new("start-manim")
                                        .icon(icons::icon(IconName::Code))
                                        .label("Import Manim…")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.open_manim_import(window, cx)
                                        })),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(16.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Pick up where you left off"),
                        )
                        .child(recent)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap_3()
                                .p_4()
                                .rounded(px(style::radius::PANEL))
                                .border_1()
                                .border_color(style::hex(color::BORDER))
                                .bg(style::hex(color::PANEL))
                                .text_color(style::hex(color::TEXT_SECONDARY))
                                .child(
                                    icons::icon(IconName::CloudUpload)
                                        .text_color(style::hex(color::ACCENT)),
                                )
                                .child(
                                    div().child("Drop media anywhere to start").child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(style::hex(color::TEXT_MUTED))
                                            .child(
                                                "Video, images and audio. Your story, your tools.",
                                            ),
                                    ),
                                ),
                        ),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unavailable_or_future_modification_time_is_not_reported_as_recent() {
        assert_eq!(modified_label(None), "Modified time unavailable");
        assert_eq!(
            modified_label(Some(SystemTime::now() + Duration::from_secs(60))),
            "Modified time unavailable"
        );
        assert_eq!(
            modified_label(Some(SystemTime::now() - Duration::from_secs(120))),
            "Modified 2 min ago"
        );
    }
}
