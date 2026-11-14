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

    pub(super) fn start_screen(&mut self, cx: &mut Context<Self>) -> Div {
        // Static wash plus a slow light sweep. The sweep is skipped entirely
        // under "Reduce motion", leaving only the wash.
        let wash = div().absolute().inset_0().bg(linear_gradient(
            115.,
            linear_color_stop(style::hexa(color::ACCENT_FROM, 0.22), 0.),
            linear_color_stop(style::hexa(color::ACCENT_TO, 0.07), 1.),
        ));
        let sweep = |left: f32| {
            let half = |from: f32, to: f32, at: f32| {
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(relative(at))
                    .w(relative(0.5))
                    .bg(linear_gradient(
                        90.,
                        linear_color_stop(style::hexa(color::ACCENT_TO, from), 0.),
                        linear_color_stop(style::hexa(color::ACCENT_TO, to), 1.),
                    ))
            };
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(left))
                .w(relative(0.42))
                .child(half(0., 0.12, 0.))
                .child(half(0.12, 0., 0.5))
        };
        let glow: AnyElement = if self.preferences.reduce_motion {
            div()
                .absolute()
                .inset_0()
                .child(wash)
                .child(sweep(0.30))
                .into_any_element()
        } else {
            div()
                .absolute()
                .inset_0()
                .child(wash)
                .child(
                    sweep(0.).with_animation(
                        "launcher-glow",
                        Animation::new(Duration::from_secs(9))
                            .repeat()
                            .with_max_fps(24.),
                        |this, progress| this.left(relative(-0.42 + 1.42 * progress)),
                    ),
                )
                .into_any_element()
        };
        let hero = div()
            .relative()
            .flex()
            .items_center()
            .justify_between()
            .gap_6()
            .px_8()
            .py_6()
            .overflow_hidden()
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
                    .w(px(300.))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_3()
                    .rounded(px(12.))
                    .bg(style::hexa(color::BG, 0.7))
                    .border_1()
                    .border_color(style::hexa(color::ACCENT, 0.28))
                    .shadow_lg()
                    .child(
                        div()
                            .h(px(116.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(7.))
                            .bg(linear_gradient(
                                135.,
                                linear_color_stop(style::hexa(color::ACCENT_FROM, 0.30), 0.),
                                linear_color_stop(style::hexa(color::ACCENT_TO, 0.12), 1.),
                            ))
                            .child(img("brand.svg").size(px(84.))),
                    )
                    .children(
                        [
                            (color::CLIP_VIDEO, [0.0, 0.46], [0.5, 0.42]),
                            (color::ACCENT, [0.12, 0.30], [0.46, 0.26]),
                            (color::CLIP_AUDIO, [0.0, 0.78], [0.82, 0.18]),
                        ]
                        .map(|(tone, first, second)| {
                            div()
                                .relative()
                                .h(px(9.))
                                .rounded(px(3.))
                                .bg(style::hexa(color::BORDER, 0.5))
                                .children([first, second].map(|[left, width]| {
                                    div()
                                        .absolute()
                                        .top_0()
                                        .bottom_0()
                                        .left(relative(left))
                                        .w(relative(width))
                                        .rounded(px(3.))
                                        .bg(style::hexa(tone, 0.75))
                                }))
                        }),
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
                            ExportPreset::Portrait => ("4:5 Portrait", "1080 x 1350"),
                        };
                        // Fit the canvas shape in a 114 x 84 box (the slot is 96 tall).
                        let fit = (114. / width as f32).min(84. / height as f32);
                        let (preview_width, preview_height) =
                            (width as f32 * fit, height as f32 * fit);
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
                                    this.new_project_with_preset(preset, cx);
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
                                        div()
                                            .px_2()
                                            .py_px()
                                            .rounded_full()
                                            .bg(style::hexa(color::ACCENT, 0.14))
                                            .text_size(px(10.))
                                            .font_family(cx.theme().mono_font_family.clone())
                                            .text_color(style::hex(color::ACCENT))
                                            .child(match preset {
                                                ExportPreset::ShortsVertical => "9:16",
                                                ExportPreset::Square => "1:1",
                                                ExportPreset::Portrait => "4:5",
                                                _ => "16:9",
                                            }),
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
                                this.new_project_with_preset(preset, cx);
                            }))
                    }),
            );

        let presets = presets.flex_wrap();
        let recent = div()
            .id("start-recent-list")
            .max_h(px(154.))
            .overflow_y_scroll()
            .flex()
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
                    let track = this.project.tracks().first().map_or(TrackId(this.next_id), Track::id);
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
                        .child(self.template_cards(cx))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(16.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .child("Pick up where you left off"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .gap_2()
                                        .child(
                                            Button::new("start-open")
                                                .outline()
                                                .icon(icons::icon(IconName::FolderOpen))
                                                .label("Open project")
                                                .on_click(cx.listener(|this, _, _, cx| this.open(cx))),
                                        )
                                        .child(
                                            Button::new("start-v1")
                                                .outline()
                                                .icon(icons::icon(IconName::Import))
                                                .label("Import v1")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.import_legacy(cx);
                                                })),
                                        )
                                        .child(
                                            Button::new("start-manim")
                                                .outline()
                                                .icon(icons::icon(IconName::Code))
                                                .label("Import Manim")
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.open_manim_import(window, cx);
                                                })),
                                        ),
                                ),
                        )
                        .child(recent)
                        .child(
                            div()
                                .id("start-drop-zone")
                                .flex()
                                .items_center()
                                .justify_center()
                                .gap_3()
                                .h(px(92.))
                                .rounded(px(style::radius::PANEL))
                                .border_1()
                                .border_dashed()
                                .border_color(style::hexa(color::ACCENT, 0.35))
                                .bg(style::hexa(color::ACCENT, 0.04))
                                .text_color(style::hex(color::TEXT_SECONDARY))
                                .drag_over::<ExternalPaths>(|zone, _, _, _| {
                                    zone.border_color(style::hex(color::ACCENT))
                                        .bg(style::hexa(color::ACCENT, 0.14))
                                })
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .size(px(40.))
                                        .rounded_full()
                                        .bg(style::hexa(color::ACCENT, 0.16))
                                        .child(
                                            icons::icon(IconName::CloudUpload)
                                                .text_color(style::hex(color::ACCENT)),
                                        ),
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
    // Not `super::*`: it would import gpui's `test` attribute over the built-in.
    use super::modified_label;
    use std::time::{Duration, SystemTime};
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
