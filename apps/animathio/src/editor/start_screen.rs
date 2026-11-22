//! Project launcher: a start page (heading over a faint graph-paper grid, the entry actions,
//! canvas presets, templates and recent projects), not a landing page.

use super::*;
use crate::style::{self, Alpha, Role, TextRole as _, color, tint};
use animathio_control::ExportPreset;
use gpui_component::menu::{DropdownMenu as _, PopupMenuItem};

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

const LOGO_SIZE: Rems = Rems(1.75);
/// The box a canvas card draws its aspect outline in (6rem x 4rem).
const OUTLINE_BOX_HEIGHT: Rems = Rems(4.0);
const TEMPLATES_MIN_WIDTH: Rems = Rems(24.);
const RECENT_WIDTH: Rems = Rems(21.);

/// A mix of two `0xRRGGBB` tokens: `t` = 0 is `from`, 1 is `to`.
fn mix(from: u32, to: u32, t: f32) -> Hsla {
    let channel = |shift: u32| {
        let (a, b) = (
            ((from >> shift) & 0xff) as f32,
            ((to >> shift) & 0xff) as f32,
        );
        (a + (b - a) * t).round() as u32
    };
    style::hex((channel(16) << 16) | (channel(8) << 8) | channel(0))
}

/// Graph paper behind the heading block: a 1 px line every 1rem in the border colour at the
/// edge step. It is static and painted with the block, so it costs two loops of quads.
fn graph_paper() -> impl IntoElement {
    canvas(
        |_, _, _| {},
        |bounds, (), window, _| {
            let step = f32::from(window.rem_size()).max(8.);
            let line = tint(color::BORDER, Alpha::Edge);
            let (width, height) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let mut x = 0.;
            while x <= width {
                window.paint_quad(fill(
                    Bounds::new(
                        point(bounds.origin.x + px(x), bounds.origin.y),
                        size(px(1.), bounds.size.height),
                    ),
                    line,
                ));
                x += step;
            }
            let mut y = 0.;
            while y <= height {
                window.paint_quad(fill(
                    Bounds::new(
                        point(bounds.origin.x, bounds.origin.y + px(y)),
                        size(bounds.size.width, px(1.)),
                    ),
                    line,
                ));
                y += step;
            }
        },
    )
    .absolute()
    .size_full()
}

impl Editor {
    pub(super) fn is_start_screen(&self) -> bool {
        !self.canvas_chosen
            && self.path.is_none()
            && self.project.tracks().is_empty()
            && self.project.assets().is_empty()
    }

    /// The heading block: logo and name, the title line, what the page is for and the three
    /// entry actions (New project, Open project…, Import…), over the graph-paper grid.
    fn start_heading(cx: &mut Context<Self>) -> Div {
        let editor = cx.entity().downgrade();
        div()
            .relative()
            .overflow_hidden()
            .rounded(px(style::radius::PANEL))
            .border_1()
            .border_color(style::hex(color::BORDER))
            .bg(style::hex(color::PANEL))
            .child(graph_paper())
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .p_6()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(img("brand.svg").size(LOGO_SIZE))
                            .child(div().text_role(Role::Heading).child("AniMathIO")),
                    )
                    .child(div().text_role(Role::Display).child("Start a project"))
                    .child(
                        div()
                            .text_role(Role::Body)
                            .text_color(style::hex(color::TEXT_SECONDARY))
                            .child("Pick a canvas, open a project or import one."),
                    )
                    .child(
                        div()
                            .id("start-actions")
                            .debug_selector(|| "start-actions".into())
                            .flex()
                            .flex_wrap()
                            .items_center()
                            .gap_2()
                            .child(
                                style::primary_button("start-new", cx)
                                    .debug_selector(|| "start-new".into())
                                    .icon(icons::icon(IconName::Plus))
                                    .label("New project")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.new_project_with_preset(
                                            ExportPreset::Youtube1080p,
                                            cx,
                                        );
                                    })),
                            )
                            .child(
                                style::secondary_button("start-open", cx)
                                    .debug_selector(|| "start-open".into())
                                    .icon(icons::icon(IconName::FolderOpen))
                                    .label("Open project…")
                                    .on_click(cx.listener(|this, _, _, cx| this.open(cx))),
                            )
                            .child(
                                style::secondary_button("start-import", cx)
                                    .debug_selector(|| "start-import".into())
                                    .icon(icons::icon(IconName::Import))
                                    .label("Import…")
                                    .dropdown_caret(true)
                                    .dropdown_menu(move |menu, _, _| {
                                        let (v1, manim) = (editor.clone(), editor.clone());
                                        menu.item(
                                            PopupMenuItem::element(|_, _| {
                                                div()
                                                    .id("start-v1")
                                                    .debug_selector(|| "start-v1".into())
                                                    .child("AniMathIO 1.x project…")
                                            })
                                            .on_click(move |_, _, cx| {
                                                let _ = v1.update(cx, |this, cx| {
                                                    this.import_legacy(cx);
                                                });
                                            }),
                                        )
                                        .item(
                                            PopupMenuItem::element(|_, _| {
                                                div()
                                                    .id("start-manim")
                                                    .debug_selector(|| "start-manim".into())
                                                    .child("Manim scene…")
                                            })
                                            .on_click(move |_, window, cx| {
                                                let _ = manim.update(cx, |this, cx| {
                                                    this.open_manim_import(window, cx);
                                                });
                                            }),
                                        )
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .text_role(Role::Caption)
                            .child("Or drop video, images or audio anywhere on this page."),
                    ),
            )
    }

    /// One canvas preset card: the real aspect outline, its name, the ratio and the size.
    fn canvas_card(
        &self,
        index: usize,
        preset: ExportPreset,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let (width, height) = preset.resolution();
        let (name, ratio) = match preset {
            ExportPreset::Youtube1080p => ("1080p", "16:9"),
            ExportPreset::Youtube4k => ("4K", "16:9"),
            ExportPreset::ShortsVertical => ("Vertical", "9:16"),
            ExportPreset::Square => ("Square", "1:1"),
            ExportPreset::Portrait => ("Portrait", "4:5"),
        };
        // The outline fits a 6rem x 4rem box, scaled by the interface scale.
        let rem = 16. * style::ui_scale();
        let fit = (6. * rem / width as f32).min(4. * rem / height as f32);
        let (outline_width, outline_height) = (width as f32 * fit, height as f32 * fit);
        let progress = if self.preferences.reduce_motion {
            1.
        } else {
            (self.preset_hover_changed.elapsed().as_secs_f32()
                / (style::motion::BASE_MS as f32 / 1000.))
                .min(1.)
        };
        let eased = 1. - (1. - progress).powi(3);
        let amount = |hovered: bool| if hovered { 1. } else { 0. };
        let from = amount(self.previous_hovered_preset == Some(index));
        let to = amount(self.hovered_preset == Some(index));
        let hover = from + (to - from) * eased;
        div()
            .id(("start-preset", index))
            .debug_selector(move || format!("canvas-preset-{index}"))
            .focusable()
            .tab_index(0)
            .focus_visible(|this| this.border_color(style::hex(color::ACCENT)))
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
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .rounded(px(style::radius::CARD))
            .border_1()
            .border_color(mix(color::BORDER, color::ACCENT, hover))
            .bg(mix(color::RAISED, color::HOVER, hover))
            .cursor_pointer()
            .active(|this| this.bg(style::hex(color::PRESSED)))
            .child(
                div()
                    .h(OUTLINE_BOX_HEIGHT)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .w(px(outline_width))
                            .h(px(outline_height))
                            .rounded(px(style::radius::CONTROL))
                            .border_1()
                            .border_color(mix(color::BORDER_STRONG, color::ACCENT, hover))
                            .bg(style::hex(color::BG)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_role(Role::Heading).child(name))
                    .child(div().text_role(Role::Label).child(ratio)),
            )
            .child(
                div()
                    .text_role(Role::Caption)
                    .tabular()
                    .child(format!("{width} × {height}, 30 fps")),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.new_project_with_preset(preset, cx);
            }))
    }

    /// The recent projects, one row each (name, folder, age), or the empty state.
    fn recent_projects(&self, cx: &mut Context<Self>) -> Div {
        let list = div().flex().flex_col().gap_2();
        if self.recent.paths.is_empty() {
            return list.child(
                div()
                    .id("start-recent-empty")
                    .debug_selector(|| "start-recent-empty".into())
                    .rounded(px(style::radius::CARD))
                    .border_1()
                    .border_color(style::hex(color::BORDER))
                    .bg(style::hex(color::RAISED))
                    .child(style::empty_state(
                        Some(IconName::FolderOpen),
                        "Recent projects appear here.",
                        None,
                    )),
            );
        }
        list.children(
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
                        .flex()
                        .flex_col()
                        .gap_1()
                        .min_w_0()
                        .p_3()
                        .rounded(px(style::radius::CARD))
                        .border_1()
                        .border_color(style::hex(color::BORDER))
                        .bg(style::hex(color::RAISED))
                        .cursor_pointer()
                        .hover(|this| {
                            this.bg(style::hex(color::HOVER))
                                .border_color(style::hex(color::ACCENT))
                        })
                        .active(|this| this.bg(style::hex(color::PRESSED)))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    icons::icon(IconName::Clapperboard)
                                        .small()
                                        .text_color(style::hex(color::TEXT_SECONDARY)),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_role(Role::Heading)
                                        .child(name),
                                ),
                        )
                        .child(
                            div()
                                .truncate()
                                .text_role(Role::Label)
                                .child(path.display().to_string()),
                        )
                        .child(div().text_role(Role::Caption).child(modified))
                        .on_click(cx.listener(move |this, _, _, cx| this.open_path(&path, cx)))
                }),
        )
    }

    pub(super) fn start_screen(&mut self, cx: &mut Context<Self>) -> Div {
        let hover_pending = !self.preferences.reduce_motion
            && self.preset_hover_changed.elapsed().as_secs_f32()
                < style::motion::BASE_MS as f32 / 1000.;
        let presets = div().flex().flex_wrap().gap_3().children(
            ExportPreset::ALL
                .into_iter()
                .enumerate()
                .map(|(index, preset)| self.canvas_card(index, preset, cx)),
        );
        let section = |title: &'static str| div().text_role(Role::Heading).child(title);
        let heading = Self::start_heading(cx);
        let recent = self.recent_projects(cx);
        let templates = self.template_cards(cx);
        div()
            .relative()
            .flex_1()
            .min_h_0()
            .w_full()
            .child(
                canvas(
                    |_, _, _| {},
                    move |_, (), window, _| {
                        if hover_pending {
                            window.request_animation_frame();
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .id("launcher-scroll")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.launcher_scroll)
                    .px_8()
                    .py_6()
                    .on_drop::<ExternalPaths>(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                        let track = this
                            .project
                            .tracks()
                            .first()
                            .map_or(TrackId(this.next_id), Track::id);
                        this.import_paths(paths.paths(), Some((track, Time(0))), cx);
                    }))
                    .drag_over::<ExternalPaths>(|style, _, _, _| {
                        style.bg(tint(color::ACCENT, Alpha::Wash))
                    })
                    .child(
                        div()
                            .max_w(px(1120.))
                            .mx_auto()
                            .flex()
                            .flex_col()
                            .gap_6()
                            .child(heading)
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_3()
                                    .child(section("Canvas"))
                                    .child(presets),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_wrap()
                                    .items_start()
                                    .gap_6()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(TEMPLATES_MIN_WIDTH)
                                            .flex()
                                            .flex_col()
                                            .gap_3()
                                            .child(section("Templates"))
                                            .child(templates),
                                    )
                                    .child(
                                        div()
                                            .w(RECENT_WIDTH)
                                            .flex_shrink_0()
                                            .flex()
                                            .flex_col()
                                            .gap_3()
                                            .child(section("Recent projects"))
                                            .child(recent),
                                    ),
                            ),
                    ),
            )
            .child(gpui_component::scroll::Scrollbar::vertical(
                &self.launcher_scroll,
            ))
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
