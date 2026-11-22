//! Title bar, application menus, context menus and status.

use super::actions::*;
use super::*;
use crate::style::{self, color, tokens::TextRole as _};
use gpui_component::{
    TitleBar, WindowExt, global_state::GlobalState, menu::AppMenuBar, status_bar::StatusBar,
};

/// Title bar height in rems (the kit `TitleBar` has no default height).
const TITLE_BAR_HEIGHT_REMS: f32 = 3.067;
/// Minimum width of the app-menu area, in rems, so the centred project title
/// never overlaps the menus.
const MENU_AREA_MIN_WIDTH_REMS: f32 = 28.667;
/// Context-menu width in rems.
const CONTEXT_MENU_WIDTH_REMS: f32 = 16.0;

/// A track context-menu entry's handler.
type TrackMenuAction = Box<dyn Fn(&mut Editor, &mut Window, &mut Context<Editor>)>;

/// An entry of the clip context menu: icon, label, the bound shortcut action (if any) and the
/// handler.
type ClipMenuEntry = (IconName, &'static str, Option<Box<dyn Action>>, ClipAction);

/// The clip context-menu entries. Every entry that has a keyboard shortcut names its action so
/// the menu can show the real binding next to the label.
fn clip_menu_entries() -> Vec<ClipMenuEntry> {
    vec![
        (
            IconName::Scissors,
            "Split at playhead",
            Some(Box::new(SplitClip)),
            Editor::split_clip,
        ),
        (
            IconName::CopyPlus,
            "Duplicate",
            Some(Box::new(DuplicateClips)),
            Editor::duplicate_clip,
        ),
        (
            IconName::Copy,
            "Copy",
            Some(Box::new(CopyClips)),
            Editor::copy_clip,
        ),
        (
            IconName::Scissors,
            "Cut",
            Some(Box::new(CutClips)),
            Editor::cut_clip,
        ),
        (
            IconName::Trash,
            "Delete",
            Some(Box::new(DeleteSelection)),
            Editor::delete_clip,
        ),
        (
            IconName::Scissors,
            "Link or unlink",
            Some(Box::new(UnlinkClips)),
            Editor::unlink_clip,
        ),
        (
            IconName::FoldHorizontal,
            "Ripple delete",
            Some(Box::new(RippleDelete)),
            Editor::ripple_delete_clip,
        ),
        (
            IconName::ArrowLeftToLine,
            "Ripple trim previous to playhead",
            Some(Box::new(RippleTrimPrevious)),
            Editor::ripple_trim_clip_previous,
        ),
        (
            IconName::ArrowRightToLine,
            "Ripple trim next to playhead",
            Some(Box::new(RippleTrimNext)),
            Editor::ripple_trim_clip_next,
        ),
        (
            IconName::Blend,
            "Default transition (1 s fades)",
            Some(Box::new(DefaultTransitions)),
            Editor::default_transition_clip,
        ),
        (
            IconName::TrendingUp,
            "Fade in 1 s",
            None,
            Editor::fade_in_one_second,
        ),
        (
            IconName::TrendingDown,
            "Fade out 1 s",
            None,
            Editor::fade_out_one_second,
        ),
        (IconName::Eraser, "Clear fades", None, Editor::clear_fades),
    ]
}

impl Editor {
    pub(super) fn refresh_menus(&mut self, cx: &mut Context<Self>) {
        let signature = format!(
            "{:?}/{:?}/{:?}/{:?}/{:?}/{:?}/{}/{}",
            self.recent.paths,
            self.selected_ids(),
            self.ripple_scope,
            self.clipboard.is_some(),
            self.dock,
            self.preview.gpu,
            self.export_queue_open,
            self.is_start_screen()
        );
        if self.menu_signature == signature {
            return;
        }
        self.menu_signature = signature;
        let recent = Menu::new("Open recent")
            .items(self.recent.paths.iter().map(|path| {
                MenuItem::action(
                    path.display().to_string(),
                    OpenRecent { path: path.clone() },
                )
            }))
            .disabled(self.recent.paths.is_empty());
        let none_selected = self.selected.is_none();
        let mut menus = vec![
            Menu::new("File").items([
                MenuItem::action("New project", NewProject),
                MenuItem::action("Open…", OpenProject),
                MenuItem::submenu(recent),
                MenuItem::action("Save", SaveProject),
                MenuItem::action("Save as template…", SaveTemplate),
                MenuItem::separator(),
                MenuItem::action("Import media…", ImportMedia),
                MenuItem::action("Record audio…", RecordAudio),
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
                MenuItem::action("Library", ToggleMediaPool).checked(self.dock.left_open),
                MenuItem::action("Media", FocusLibrary),
                MenuItem::action("Elements", ShowElements),
                MenuItem::action("Titles", ShowTitles),
                MenuItem::action("Effects", ShowEffects),
                MenuItem::action("Plugins", ShowPlugins),
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
                MenuItem::action("Previous marker", PreviousMarker),
                MenuItem::action("Next marker", NextMarker),
                MenuItem::action("Start", GoToStart),
                MenuItem::action("End", GoToEnd),
                MenuItem::action("Set in", MarkIn),
                MenuItem::action("Set out", MarkOut),
                MenuItem::action("Clear in/out", ClearMarks),
                MenuItem::action("Go to in", GoToIn),
                MenuItem::action("Go to out", GoToOut),
                MenuItem::action("Loop", ToggleLoop),
            ]),
            Menu::new("Tools").items([
                MenuItem::action("Select", SelectTool),
                MenuItem::action("Blade", BladeTool),
                MenuItem::action("Ripple", RippleTool),
                MenuItem::action("Rate stretch", RateStretchTool),
                MenuItem::action("Slip", SlipTool),
                MenuItem::action("Slide", SlideTool),
                MenuItem::action("Rolling edit", RollingTool),
                MenuItem::action("Hand", HandTool),
                MenuItem::separator(),
                MenuItem::action("Ripple all tracks", ToggleRippleScope)
                    .checked(self.ripple_scope == RippleScope::AllTracks),
                MenuItem::action("Markers follow ripple", ToggleMarkerRipple)
                    .checked(self.marker_ripple != animathio_core::MarkerRipple::Stay),
            ]),
            Menu::new("Clip").items([
                MenuItem::action("Split at playhead", SplitClip).disabled(none_selected),
                MenuItem::action("Link or unlink clips", UnlinkClips).disabled(none_selected),
                MenuItem::action("Default video transition", DefaultVideoTransition)
                    .disabled(none_selected),
                MenuItem::action("Default audio transition", DefaultAudioTransition)
                    .disabled(none_selected),
                MenuItem::action("Ripple trim previous edit to playhead", RippleTrimPrevious),
                MenuItem::action("Ripple trim next edit to playhead", RippleTrimNext),
                MenuItem::action("Speed/Duration…", SpeedDuration).disabled(none_selected),
                MenuItem::action("Insert frame hold segment…", InsertFrameHold)
                    .disabled(none_selected),
                MenuItem::action("Add effect", ShowEffects),
                MenuItem::action("Ripple delete selection or range", RippleDelete),
                MenuItem::action("Write on…", WriteOn),
                MenuItem::action("Generate subtitles…", GenerateSubtitles),
                MenuItem::action("Auto reframe…", AutoReframe),
                MenuItem::action("Split at scene changes…", SceneCuts),
                MenuItem::action("Automatic cuts…", AutomaticCuts),
                MenuItem::action("Auto duck…", AutoDuck),
                MenuItem::action("Learn denoise from selection", LearnNoise),
                MenuItem::action("Add marker at playhead", AddMarker),
                MenuItem::action("Markers and beat sync…", ShowMarkers),
            ]),
            Menu::new("Help").items([
                MenuItem::action("Editor guide", OpenEditorGuide),
                MenuItem::action("Keyboard shortcuts…", KeyboardShortcuts),
                MenuItem::action("Report an issue", ReportIssue),
                MenuItem::separator(),
                MenuItem::action("About AniMathIO", About),
            ]),
        ];
        if self.is_start_screen() {
            // No project is open yet: Playback, Tools and Clip have nothing to act on.
            menus.retain(|menu| !matches!(menu.name.as_ref(), "Playback" | "Tools" | "Clip"));
        }
        GlobalState::global_mut(cx).set_app_menus(menus.into_iter().map(Menu::owned).collect());
        self.app_menu.update(cx, AppMenuBar::reload);
    }

    /// The project name shown in the title bar and the window title.
    fn project_title(&self) -> String {
        self.path.as_ref().and_then(|p| p.file_stem()).map_or_else(
            || {
                if self.is_start_screen() {
                    "AniMathIO".to_owned()
                } else {
                    "Untitled project".to_owned()
                }
            },
            |n| n.to_string_lossy().into_owned(),
        )
    }

    /// The window title: the project name, a bullet while there are unsaved changes, then the
    /// app name (`AniMathIO` alone on the launcher).
    pub(super) fn window_title(&self) -> String {
        if self.is_start_screen() {
            return "AniMathIO".to_owned();
        }
        format!(
            "{}{} - AniMathIO",
            if self.dirty { "• " } else { "" },
            self.project_title()
        )
    }

    pub(super) fn menu_bar(&self, window: &Window, cx: &mut Context<Self>) -> TitleBar {
        let title = self.project_title();
        let rem = window.rem_size();
        let close_view = cx.entity().downgrade();
        TitleBar::new()
            .h(rem * TITLE_BAR_HEIGHT_REMS)
            .on_close_window(move |_, window, cx| {
                let _ = close_view.update(cx, |this, cx| {
                    this.replace_project(ReplaceProject::CloseWindow, window, cx);
                });
            })
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .gap_3()
                    .w_full()
                    .h_full()
                    .px_2()
                    .child(img("brand.svg").size(style::size::ROW_MD).flex_shrink_0())
                    .child(
                        div()
                            .flex_1()
                            .min_w(rem * MENU_AREA_MIN_WIDTH_REMS)
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
                                .text_role(style::Role::Body)
                                .text_color(style::hex(color::TEXT_SECONDARY))
                                .when(self.dirty && !self.is_start_screen(), |title| {
                                    title.child(
                                        div()
                                            .id("title-unsaved")
                                            .debug_selector(|| "title-unsaved".into())
                                            .size(px(5.))
                                            .rounded_full()
                                            .bg(style::hex(color::ACCENT))
                                            .tooltip(|window, cx| {
                                                gpui_component::tooltip::Tooltip::new(
                                                    "Unsaved changes",
                                                )
                                                .build(window, cx)
                                            }),
                                    )
                                })
                                .child(div().truncate().child(title)),
                        )
                    })
                    .child(self.mcp_status_chip())
                    .when(!self.is_start_screen(), |row| {
                        row.child(
                            style::primary_button("title-export", cx)
                                .debug_selector(|| "title-export".into())
                                .small()
                                .mr_2()
                                .icon(icons::icon(IconName::Upload))
                                .label("Export")
                                .tooltip_with_action("Export video", &ExportVideo, Some("Editor"))
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_export_dialog(window, cx);
                                })),
                        )
                    }),
            )
    }

    /// The MCP status chip (see `docs/design.md` "Window frame and status
    /// bar"): a chip that needs attention turns `WARNING`, and the error is
    /// on its tooltip.
    fn mcp_status_chip(&self) -> Stateful<Div> {
        let listening = self.control.as_ref().is_some_and(|c| c.server.is_some());
        let error = self.control.as_ref().and_then(|c| c.error.clone());
        let label = if listening {
            "MCP listening"
        } else if error.is_some() {
            "MCP unavailable"
        } else {
            "MCP off"
        };
        let chip = style::chip("mcp-status", None, label)
            .when(listening, |chip| {
                chip.text_color(style::hex(color::SUCCESS))
            })
            .when(error.is_some(), |chip| {
                chip.text_color(style::hex(color::WARNING))
            });
        match error {
            Some(error) => chip.tooltip(move |window, cx| {
                gpui_component::tooltip::Tooltip::new(error.clone()).build(window, cx)
            }),
            None => chip,
        }
    }

    pub(super) fn flush_notices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(error) = self.error.take() {
            self.error_sequence = self.error_sequence.wrapping_add(1);
            self.last_error = Some(error.clone());
            self.notices.push(style::Toast::error(error));
        }
        for notice in self.notices.drain(..) {
            window.push_notification(notice, cx);
        }
    }

    pub(super) fn context_menu_dropdown(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Div> {
        let target = self.context_menu.clone()?;
        let pos = self.context_menu_pos?;
        // Keep the menu inside the window: slide it up when it would run off the bottom.
        let rows = match &target {
            ContextMenu::Clip { .. } => {
                clip_menu_entries().len() + self.available_effects().len() + 2
            }
            ContextMenu::Track { .. } => 5,
            ContextMenu::Keyframe(_) => 6,
        };
        let viewport = window.viewport_size();
        let height = (rows as f32 * 30.0 + 16.0) * style::ui_scale();
        let top = f32::from(pos.y)
            .min(f32::from(viewport.height) - height)
            .max(8.0);
        let left = f32::from(pos.x)
            .min(f32::from(viewport.width) - f32::from(window.rem_size() * 16.0) - 8.0)
            .max(8.0);
        let mut menu = div()
            .absolute()
            .left(px(left))
            .top(px(top))
            .w(window.rem_size() * CONTEXT_MENU_WIDTH_REMS)
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
                let actions = clip_menu_entries();
                for (icon, label, shortcut, action) in actions {
                    menu = menu.child(context_item(
                        icon,
                        label,
                        shortcut.as_deref(),
                        cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                            action(this, id, cx);
                            this.context_menu = None;
                            cx.notify();
                        }),
                        window,
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
                        None,
                        cx.listener(move |this: &mut Self, _: &ClickEvent, _, cx| {
                            this.add_effect_to_clip(id, &plugin, &effect, cx);
                            this.context_menu = None;
                            cx.notify();
                        }),
                        window,
                        cx,
                    ));
                }
            }
            ContextMenu::Track { id } => {
                let tracks = timeline::display_tracks(&self.project);
                let position = tracks.iter().position(|track| track.id() == id);
                let (first, last) =
                    position.map_or((0, 0), |row| super::clip_ops::display_section(&tracks, row));
                let actions: Vec<(IconName, &str, bool, TrackMenuAction)> = vec![
                    (
                        IconName::AudioWaveform,
                        "Audio effects",
                        true,
                        Box::new(move |this, _, cx| {
                            this.clear_selection(cx);
                            this.audio_inspector_track = Some(id);
                        }),
                    ),
                    (
                        IconName::Pencil,
                        "Rename",
                        true,
                        Box::new(move |this, window, cx| this.start_track_rename(id, window, cx)),
                    ),
                    (
                        IconName::ArrowUp,
                        "Move up",
                        position.is_some_and(|index| index > first),
                        Box::new(move |this, _, cx| this.move_track_by(id, -1, cx)),
                    ),
                    (
                        IconName::ArrowDown,
                        "Move down",
                        position.is_some_and(|index| index < last),
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
                        None,
                        cx.listener(move |this: &mut Self, _: &ClickEvent, window, cx| {
                            this.context_menu = None;
                            action(this, window, cx);
                            cx.notify();
                        }),
                        window,
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
                        None,
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
                        window,
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
    ///
    /// While the launcher is shown no project or canvas has been chosen, so
    /// the placeholder resolution and duration chips are hidden.
    pub(super) fn status_summary_visible(&self) -> bool {
        !self.is_start_screen()
    }

    pub(super) fn status_line(&self, cx: &mut Context<Self>) -> StatusBar {
        let (width, height) = project_size(&self.project);
        let duration = timecode::format_timecode(
            Time(timeline::project_duration(&self.project)),
            self.project.frame_rate(),
        );
        let start_screen = self.is_start_screen();
        let autosave = if self.preferences.autosave_seconds == 0 {
            "Autosave off".to_owned()
        } else if self.save_pending {
            "Saving…".to_owned()
        } else if self.path.is_some() {
            format!("Autosave every {}s", self.preferences.autosave_seconds)
        } else {
            "Save to enable autosave".to_owned()
        };
        // An explicit height (the chips are centred in it): the export queue panel sits on top of
        // exactly this bar.
        let mut status = StatusBar::new()
            .flex_shrink_0()
            .h(style::size::STATUS_BAR)
            .py_0()
            .px_3();
        if self.status_summary_visible() {
            status = status
                .left(
                    style::chip(
                        "status-canvas",
                        Some(IconName::Monitor),
                        format!("{width} x {height}"),
                    )
                    .tabular(),
                )
                .left(
                    style::chip(
                        "status-fps",
                        Some(IconName::Clapperboard),
                        frame_rate_label(self.project.frame_rate()),
                    )
                    .tabular(),
                )
                .left(style::chip("status-duration", Some(IconName::Clock), duration).tabular());
        }
        if !start_screen {
            // Autosave and the export queue belong to an open project.
            status = status.right(style::chip(
                "status-autosave",
                Some(IconName::ShieldCheck),
                autosave,
            ));
        }
        status = status.right(self.renderer_chip());
        if !start_screen {
            status = status.right(self.export_queue_button(cx));
            // No placeholder when idle: an empty child would add a gap after the queue chip, so
            // the export queue panel could not line up with the chip's right edge.
            if let Some(button) = self.subtitle_status_button(cx) {
                status = status.right(button);
            }
        }
        status
    }

    /// The renderer actually used for the preview, not just the one requested.
    pub(super) fn renderer_label(&self) -> &'static str {
        use crate::render_worker::ActiveBackend;
        match (self.preview.gpu, self.worker.active_backend()) {
            (true, ActiveBackend::CpuFallback) => "CPU preview (fallback)",
            (true, _) => "GPU preview",
            (false, _) => "CPU preview",
        }
    }

    fn renderer_chip(&self) -> Stateful<Div> {
        let label = self.renderer_label();
        let tooltip = match label {
            "GPU preview" => {
                "The preview is drawn on the GPU. Export always uses the CPU renderer."
            }
            "CPU preview (fallback)" => {
                "The GPU was requested but is unavailable, so the preview is drawn on the CPU."
            }
            _ => "The preview is drawn on the CPU. Switch to the GPU in Preferences or View.",
        };
        style::chip("status-renderer", Some(IconName::Cpu), label).tooltip(move |window, cx| {
            gpui_component::tooltip::Tooltip::new(tooltip).build(window, cx)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[core::prelude::v1::test]
    fn clip_menu_shortcuts_are_bound_in_the_keymap() {
        let bound: Vec<_> = super::super::actions::keymap()
            .into_iter()
            .filter(|(_, _, context)| *context == "Timeline")
            .map(|(_, action, _)| action)
            .collect();
        let entries = clip_menu_entries();
        let shown = entries
            .iter()
            .filter(|(_, _, action, _)| action.is_some())
            .count();
        assert!(
            shown >= 6,
            "Cut, Copy, Duplicate, Delete and friends show keys"
        );
        for (_, label, action, _) in &entries {
            if let Some(action) = action {
                assert!(
                    bound.iter().any(|b| b.partial_eq(action.as_ref())),
                    "{label} names an unbound action"
                );
            }
        }
    }
}
