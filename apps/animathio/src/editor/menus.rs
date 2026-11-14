//! Title bar, application menus, context menus and status.

use super::actions::*;
use super::*;
use crate::style::{self, color};
use gpui_component::{
    TitleBar, WindowExt, global_state::GlobalState, menu::AppMenuBar, status_bar::StatusBar,
};

/// A track context-menu entry's handler.
type TrackMenuAction = Box<dyn Fn(&mut Editor, &mut Window, &mut Context<Editor>)>;

impl Editor {
    pub(super) fn refresh_menus(&mut self, cx: &mut Context<Self>) {
        let signature = format!(
            "{:?}/{:?}/{:?}/{:?}/{:?}/{:?}/{}",
            self.recent.paths,
            self.selected_ids(),
            self.ripple_scope,
            self.clipboard.is_some(),
            self.dock,
            self.preview.gpu,
            self.export_queue_open
        );
        if self.menu_signature == signature {
            return;
        }
        self.menu_signature = signature;
        let recent = Menu::new("Open Recent")
            .items(self.recent.paths.iter().map(|path| {
                MenuItem::action(
                    path.display().to_string(),
                    OpenRecent { path: path.clone() },
                )
            }))
            .disabled(self.recent.paths.is_empty());
        let none_selected = self.selected.is_none();
        let menus = vec![
            Menu::new("File").items([
                MenuItem::action("New project", NewProject),
                MenuItem::action("Open…", OpenProject),
                MenuItem::submenu(recent),
                MenuItem::action("Save", SaveProject),
                MenuItem::separator(),
                MenuItem::action("Import media…", ImportMedia),
                MenuItem::action("Import v1…", ImportV1),
                MenuItem::action("Import Manim scene…", ImportManim),
                MenuItem::action("Export…", ExportVideo),
                MenuItem::separator(),
                MenuItem::action("Project settings…", ProjectSettings),
                MenuItem::action("Preferences…", Preferences),
            ]),
            Menu::new("Edit").items([
                MenuItem::action("Undo", Undo),
                MenuItem::action("Redo", Redo),
                MenuItem::separator(),
                MenuItem::action("Cut", CutClips).disabled(none_selected),
                MenuItem::action("Copy", CopyClips).disabled(none_selected),
                MenuItem::action("Paste at playhead", PasteClips),
                MenuItem::action("Duplicate", DuplicateClips).disabled(none_selected),
                MenuItem::separator(),
                MenuItem::action("Select all clips", SelectAllClips),
                MenuItem::action("Deselect", ClearSelection).disabled(none_selected),
                MenuItem::action("Delete selection", DeleteSelection).disabled(none_selected),
            ]),
            Menu::new("View").items([
                MenuItem::action("Media pool", ToggleMediaPool).checked(self.dock.left_open),
                MenuItem::action("Inspector", ToggleInspector).checked(self.dock.right_open),
                MenuItem::action("Timeline", ToggleTimeline).checked(self.dock.bottom_open),
                MenuItem::separator(),
                MenuItem::action("Export queue", ToggleExportQueue).checked(self.export_queue_open),
                MenuItem::action("GPU preview", ToggleGpuPreview).checked(self.preview.gpu),
                MenuItem::action("Zoom to fit", ZoomToFit),
            ]),
            Menu::new("Playback").items([
                MenuItem::action("Shuttle reverse", ShuttleReverse),
                MenuItem::action("Stop", ShuttleStop),
                MenuItem::action("Shuttle forward", ShuttleForward),
                MenuItem::action("Previous frame", PreviousFrame),
                MenuItem::action("Next frame", NextFrame),
                MenuItem::action("Previous edit", PreviousEdit),
                MenuItem::action("Next edit", NextEdit),
                MenuItem::action("Start", GoToStart),
                MenuItem::action("End", GoToEnd),
                MenuItem::action("Set in", MarkIn),
                MenuItem::action("Set out (exclusive)", MarkOut),
                MenuItem::action("Clear in/out", ClearMarks),
                MenuItem::action("Go to in", GoToIn),
                MenuItem::action("Go to out", GoToOut),
                MenuItem::action("Loop", ToggleLoop),
            ]),
            Menu::new("Tools").items([
                MenuItem::action("Select", SelectTool),
                MenuItem::action("Blade", BladeTool),
                MenuItem::action("Ripple", RippleTool),
                MenuItem::action("Hand", HandTool),
                MenuItem::separator(),
                MenuItem::action("Ripple all tracks", ToggleRippleScope)
                    .checked(self.ripple_scope == RippleScope::AllTracks),
            ]),
            Menu::new("Clip").items([
                MenuItem::action("Split at playhead", SplitClip).disabled(none_selected),
                MenuItem::action("Add effect", ShowEffects),
                MenuItem::action("Ripple delete selection or range", RippleDelete),
                MenuItem::action("Write on…", WriteOn),
                MenuItem::action("Generate subtitles…", GenerateSubtitles),
                MenuItem::action("Automatic cuts…", AutomaticCuts),
            ]),
            Menu::new("Help").items([
                MenuItem::action("Keyboard shortcuts…", KeyboardShortcuts),
                MenuItem::separator(),
                MenuItem::action("About AniMathIO", About),
            ]),
        ];
        GlobalState::global_mut(cx).set_app_menus(menus.into_iter().map(Menu::owned).collect());
        self.app_menu.update(cx, AppMenuBar::reload);
    }

    pub(super) fn menu_bar(&self, window: &Window, cx: &mut Context<Self>) -> TitleBar {
        let title = self.path.as_ref().and_then(|p| p.file_stem()).map_or_else(
            || {
                if self.is_start_screen() {
                    "Creative workspace".to_owned()
                } else {
                    "Untitled project".to_owned()
                }
            },
            |n| n.to_string_lossy().into_owned(),
        );
        TitleBar::new().h(px(46.)).child(
            div()
                .relative()
                .flex()
                .items_center()
                .gap_3()
                .w_full()
                .h_full()
                .px_2()
                .child(img("brand.svg").size(px(30.)).flex_shrink_0())
                .child(
                    div()
                        .flex_1()
                        .min_w(px(430.))
                        .h_full()
                        .child(self.app_menu.clone()),
                )
                .when(f32::from(window.viewport_size().width) >= 1120., |row| {
                    row.child(
                        div()
                            .absolute()
                            .left(relative(0.38))
                            .w(relative(0.24))
                            .h_full()
                            .flex()
                            .items_center()
                            .justify_center()
                            .gap_2()
                            .text_size(px(12.))
                            .text_color(style::hex(color::TEXT_SECONDARY))
                            .when(self.dirty, |title| {
                                title.child(
                                    div()
                                        .size(px(5.))
                                        .rounded_full()
                                        .bg(style::hex(color::ACCENT)),
                                )
                            })
                            .child(div().truncate().child(title)),
                    )
                })
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_2()
                        .py_1()
                        .rounded_full()
                        .bg(style::hex(color::RAISED))
                        .border_1()
                        .border_color(style::hex(color::BORDER))
                        .text_size(px(11.))
                        .text_color(style::hex(color::TEXT_SECONDARY))
                        .child(div().size(px(5.)).rounded_full().bg(style::hex(
                            if self.control.is_some() {
                                color::SUCCESS
                            } else {
                                color::TEXT_MUTED
                            },
                        )))
                        .child(if self.control.is_some() {
                            "MCP listening"
                        } else {
                            "MCP off"
                        }),
                )
                .child(
                    style::primary_button("title-export", cx)
                        .small()
                        .mr_2()
                        .icon(icons::icon(IconName::Upload))
                        .label("Export")
                        .tooltip_with_action("Export video", &ExportVideo, Some("Editor"))
                        .on_click(
                            cx.listener(|this, _, window, cx| this.open_export_dialog(window, cx)),
                        ),
                ),
        )
    }

    pub(super) fn flush_notices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(error) = self.error.take() {
            self.notices.push(style::Toast::error(error));
        }
        for notice in self.notices.drain(..) {
            window.push_notification(notice, cx);
        }
    }

    pub(super) fn context_menu_dropdown(&self, cx: &mut Context<Self>) -> Option<Div> {
        let target = self.context_menu.clone()?;
        let pos = self.context_menu_pos?;
        let mut menu = div()
            .absolute()
            .left(pos.x)
            .top(pos.y)
            .w(px(200.))
            .bg(cx.theme().popover)
            .border_1()
            .border_color(cx.theme().border)
            .p_2()
            .rounded(px(style::radius::PANEL))
            .flex()
            .flex_col()
            .shadow_lg();
        let close = cx.listener(|this: &mut Self, _: &ClickEvent, _, cx| {
            this.context_menu = None;
            cx.notify();
        });
        match target {
            ContextMenu::Clip { id } => {
                let actions: Vec<(IconName, &str, ClipAction)> = vec![
                    (IconName::Scissors, "Split at playhead", Self::split_clip),
                    (IconName::CopyPlus, "Duplicate", Self::duplicate_clip),
                    (IconName::Copy, "Copy", Self::copy_clip),
                    (IconName::Scissors, "Cut", Self::cut_clip),
                    (IconName::Trash, "Delete", Self::delete_clip),
                    (IconName::Scissors, "Unlink", Self::unlink_clip),
                    (
                        IconName::FoldHorizontal,
                        "Ripple delete",
                        Self::ripple_delete_clip,
                    ),
                    (
                        IconName::TrendingUp,
                        "Fade in 1 s",
                        Self::fade_in_one_second,
                    ),
                    (
                        IconName::TrendingDown,
                        "Fade out 1 s",
                        Self::fade_out_one_second,
                    ),
                    (IconName::Eraser, "Clear fades", Self::clear_fades),
                ];
                for (icon, label, action) in actions {
                    menu = menu.child(context_item(
                        icon,
                        label,
                        cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                            action(this, id, cx);
                            this.context_menu = None;
                            cx.notify();
                        }),
                        cx,
                    ));
                }
                menu = menu.child(
                    div()
                        .px_2()
                        .py_1()
                        .text_color(cx.theme().muted_foreground)
                        .child("Add effect"),
                );
                for descriptor in self.available_effects() {
                    let (plugin, effect) = (descriptor.plugin_id, descriptor.effect_id);
                    menu = menu.child(context_item(
                        IconName::Sparkles,
                        &descriptor.name,
                        cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                            this.add_effect_to_clip(id, &plugin, &effect, cx);
                            this.context_menu = None;
                            cx.notify();
                        }),
                        cx,
                    ));
                }
            }
            ContextMenu::Track { id } => {
                let count = self.project.tracks().len();
                let position = self
                    .project
                    .tracks()
                    .iter()
                    .position(|track| track.id() == id);
                let actions: Vec<(IconName, &str, bool, TrackMenuAction)> = vec![
                    (
                        IconName::Pencil,
                        "Rename",
                        true,
                        Box::new(move |this, window, cx| this.start_track_rename(id, window, cx)),
                    ),
                    (
                        IconName::ArrowUp,
                        "Move up",
                        position.is_some_and(|index| index > 0),
                        Box::new(move |this, _, cx| this.move_track_by(id, -1, cx)),
                    ),
                    (
                        IconName::ArrowDown,
                        "Move down",
                        position.is_some_and(|index| index + 1 < count),
                        Box::new(move |this, _, cx| this.move_track_by(id, 1, cx)),
                    ),
                ];
                for (icon, label, enabled, action) in actions {
                    if !enabled {
                        continue;
                    }
                    menu = menu.child(context_item(
                        icon,
                        label,
                        cx.listener(move |this: &mut Self, _: &ClickEvent, window, cx| {
                            this.context_menu = None;
                            action(this, window, cx);
                            cx.notify();
                        }),
                        cx,
                    ));
                }
            }
            ContextMenu::Keyframe(selection) => {
                let remove = selection.clone();
                menu = menu
                    .child(context_item(
                        IconName::DiamondMinus,
                        "Remove keyframe",
                        cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                            this.execute(
                                &Command::RemoveKeyframe {
                                    id: remove.clip_id,
                                    target: remove.target.clone(),
                                    time: remove.time,
                                },
                                cx,
                            );
                            this.selected_keyframe = None;
                            this.context_menu = None;
                            cx.notify();
                        }),
                        cx,
                    ))
                    .children(self.keyframe_menu(&selection, cx));
            }
        }
        menu = menu.child(
            div()
                .px_2()
                .py_1()
                .cursor_pointer()
                .hover(|this| this.bg(cx.theme().muted))
                .text_color(cx.theme().muted_foreground)
                .child("Close")
                .id("context-close")
                .on_click(close),
        );
        Some(menu)
    }

    /// Persistent project summary; transient results are notifications.
    pub(super) fn status_line(&self, cx: &mut Context<Self>) -> StatusBar {
        let (width, height) = project_size(&self.project);
        let duration = timecode::format_timecode(
            Time(timeline::project_duration(&self.project)),
            self.project.frame_rate(),
        );
        let chip = |icon, label: String| {
            div()
                .flex()
                .items_center()
                .gap_2()
                .px_2()
                .h(px(22.))
                .rounded(px(style::radius::CONTROL))
                .bg(style::hex(color::PANEL))
                .text_color(style::hex(color::TEXT_SECONDARY))
                .child(icons::icon(icon).small())
                .child(label)
        };
        let autosave = if self.preferences.autosave_seconds == 0 {
            "Autosave off".to_owned()
        } else if self.save_pending {
            "Saving…".to_owned()
        } else if self.path.is_some() {
            format!("Autosave every {}s", self.preferences.autosave_seconds)
        } else {
            "Save to enable autosave".to_owned()
        };
        StatusBar::new()
            .h(px(34.))
            .flex_shrink_0()
            .px_3()
            .left(chip(IconName::Monitor, format!("{width} x {height}")))
            .left(chip(
                IconName::Clapperboard,
                frame_rate_label(self.project.frame_rate()),
            ))
            .left(
                div()
                    .font_family(cx.theme().mono_font_family.clone())
                    .child(chip(IconName::Clock, duration)),
            )
            .right(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(icons::icon(IconName::ShieldCheck).small())
                    .child(autosave),
            )
            .right(chip(
                IconName::Cpu,
                if self.preview.gpu {
                    "GPU requested"
                } else {
                    "CPU preview"
                }
                .to_owned(),
            ))
            .right(self.export_queue_button(cx))
            .right(
                self.subtitle_status_button(cx)
                    .map_or_else(|| div().into_any_element(), IntoElement::into_any_element),
            )
    }
}
