//! Editor actions and the shared, platform-aware keymap.

use super::*;

actions!(
    editor,
    [
        ShuttleReverse,
        ShuttleStop,
        ShuttleForward,
        PreviousEdit,
        NextEdit,
        GoToIn,
        GoToOut,
        MarkIn,
        MarkOut,
        ClearMarks,
        SelectTool,
        BladeTool,
        RippleTool,
        HandTool,
        RippleDelete,
        ToggleRippleScope,
        SelectAllClips,
        ClearSelection,
        CopyClips,
        CutClips,
        PasteClips,
        DuplicateClips,
        BackTenFrames,
        ForwardTenFrames,
        Preferences,
        ImportV1,
        ImportManim,
        WriteOn,
        NewProject,
        OpenProject,
        SaveProject,
        ImportMedia,
        ExportVideo,
        ToggleExportQueue,
        ProjectSettings,
        Undo,
        Redo,
        DeleteSelection,
        SplitClip,
        TogglePlayback,
        PreviousFrame,
        NextFrame,
        GoToStart,
        GoToEnd,
        ToggleLoop,
        ToggleSnapping,
        ZoomIn,
        ZoomOut,
        ZoomToFit,
        LargerTracks,
        SmallerTracks,
        ToggleMediaPool,
        ToggleInspector,
        ToggleTimeline,
        ToggleGpuPreview,
        ShowEffects,
        FitViewer,
        ToggleSafeAreas,
        KeyboardShortcuts,
        About
    ]
);

#[derive(Clone, Debug, Default, PartialEq, gpui::Action)]
#[action(namespace = editor, no_json)]
pub struct OpenRecent {
    pub path: PathBuf,
}

pub fn init(cx: &mut App) {
    let bindings: Vec<_> = keymap()
        .into_iter()
        .map(|(keys, action, context)| {
            let predicate = binding_context(action.as_ref(), context);
            KeyBinding::load(
                keys,
                action,
                Some(std::rc::Rc::new(predicate)),
                false,
                None,
                cx,
            )
            .expect("valid platform keybinding")
        })
        .collect();
    cx.bind_keys(bindings);
}

fn binding_context(action: &dyn Action, panel: &str) -> KeyBindingContextPredicate {
    let context = if panel != "Editor"
        || action.partial_eq(&Undo)
        || action.partial_eq(&Redo)
        || action.partial_eq(&KeyboardShortcuts)
    {
        format!("{panel} && !Input")
    } else {
        panel.to_owned()
    };
    KeyBindingContextPredicate::parse(&context).expect("valid context")
}

fn keymap() -> Vec<(&'static str, Box<dyn Action>, &'static str)> {
    vec![
        ("j", Box::new(ShuttleReverse), "Timeline"),
        ("j", Box::new(ShuttleReverse), "Viewer"),
        ("k", Box::new(ShuttleStop), "Timeline"),
        ("k", Box::new(ShuttleStop), "Viewer"),
        ("l", Box::new(ShuttleForward), "Timeline"),
        ("l", Box::new(ShuttleForward), "Viewer"),
        ("up", Box::new(PreviousEdit), "Timeline"),
        ("up", Box::new(PreviousEdit), "Viewer"),
        ("down", Box::new(NextEdit), "Timeline"),
        ("down", Box::new(NextEdit), "Viewer"),
        ("shift-i", Box::new(GoToIn), "Timeline"),
        ("shift-i", Box::new(GoToIn), "Viewer"),
        ("shift-o", Box::new(GoToOut), "Timeline"),
        ("shift-o", Box::new(GoToOut), "Viewer"),
        ("i", Box::new(MarkIn), "Timeline"),
        ("i", Box::new(MarkIn), "Viewer"),
        ("o", Box::new(MarkOut), "Timeline"),
        ("o", Box::new(MarkOut), "Viewer"),
        ("alt-x", Box::new(ClearMarks), "Timeline"),
        ("alt-x", Box::new(ClearMarks), "Viewer"),
        ("a", Box::new(SelectTool), "Timeline"),
        ("b", Box::new(BladeTool), "Timeline"),
        ("t", Box::new(RippleTool), "Timeline"),
        ("h", Box::new(HandTool), "Timeline"),
        ("shift-delete", Box::new(RippleDelete), "Timeline"),
        ("shift-backspace", Box::new(RippleDelete), "Timeline"),
        ("secondary-a", Box::new(SelectAllClips), "Timeline"),
        ("escape", Box::new(ClearSelection), "Timeline"),
        ("secondary-c", Box::new(CopyClips), "Timeline"),
        ("secondary-x", Box::new(CutClips), "Timeline"),
        ("secondary-v", Box::new(PasteClips), "Timeline"),
        ("secondary-d", Box::new(DuplicateClips), "Timeline"),
        ("secondary-,", Box::new(Preferences), "Editor"),
        ("secondary-n", Box::new(NewProject), "Editor"),
        ("secondary-o", Box::new(OpenProject), "Editor"),
        ("secondary-s", Box::new(SaveProject), "Editor"),
        ("secondary-i", Box::new(ImportMedia), "Editor"),
        ("secondary-m", Box::new(ExportVideo), "Editor"),
        ("secondary-z", Box::new(Undo), "Editor"),
        ("secondary-shift-z", Box::new(Redo), "Editor"),
        ("delete", Box::new(DeleteSelection), "Timeline"),
        ("secondary-b", Box::new(SplitClip), "Timeline"),
        ("space", Box::new(TogglePlayback), "Viewer"),
        ("space", Box::new(TogglePlayback), "Timeline"),
        ("left", Box::new(PreviousFrame), "Viewer"),
        ("left", Box::new(PreviousFrame), "Timeline"),
        ("right", Box::new(NextFrame), "Viewer"),
        ("right", Box::new(NextFrame), "Timeline"),
        ("shift-left", Box::new(BackTenFrames), "Viewer"),
        ("shift-left", Box::new(BackTenFrames), "Timeline"),
        ("shift-right", Box::new(ForwardTenFrames), "Viewer"),
        ("shift-right", Box::new(ForwardTenFrames), "Timeline"),
        ("home", Box::new(GoToStart), "Viewer"),
        ("home", Box::new(GoToStart), "Timeline"),
        ("end", Box::new(GoToEnd), "Viewer"),
        ("end", Box::new(GoToEnd), "Timeline"),
        ("secondary-l", Box::new(ToggleLoop), "Viewer"),
        ("secondary-l", Box::new(ToggleLoop), "Timeline"),
        ("n", Box::new(ToggleSnapping), "Timeline"),
        ("secondary-=", Box::new(ZoomIn), "Timeline"),
        ("secondary--", Box::new(ZoomOut), "Timeline"),
        ("shift-z", Box::new(ZoomToFit), "Timeline"),
        ("secondary-alt-=", Box::new(LargerTracks), "Timeline"),
        ("secondary-alt--", Box::new(SmallerTracks), "Timeline"),
        ("shift-f", Box::new(FitViewer), "Viewer"),
        ("shift-f", Box::new(FitViewer), "Timeline"),
        ("shift-/", Box::new(KeyboardShortcuts), "Editor"),
        ("backspace", Box::new(DeleteSelection), "Timeline"),
    ]
}

/// Handlers live on the editor ancestor so menus and toolbars dispatch the
/// same commands. Text entry always keeps its editing/navigation keys.
pub(super) fn attach(root: Div, cx: &mut Context<Editor>) -> Div {
    root.on_action(cx.listener(|this, _: &NewProject, _window, cx| {
        this.new_project(cx);
    }))
    .on_action(cx.listener(|this, _: &OpenProject, _window, cx| {
        this.open(cx);
    }))
    .on_action(cx.listener(|this, _: &SaveProject, _window, cx| {
        this.save(cx);
    }))
    .on_action(cx.listener(|this, _: &ImportMedia, _window, cx| {
        this.import_media(cx);
    }))
    .on_action(cx.listener(|this, _: &ToggleExportQueue, _, cx| {
        this.export_queue_open = !this.export_queue_open;
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &ExportVideo, window, cx| {
        this.open_export_dialog(window, cx);
    }))
    .on_action(cx.listener(|this, _: &ProjectSettings, window, cx| {
        this.open_settings(window, cx);
    }))
    .on_action(cx.listener(|this, _: &Undo, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.undo(cx);
    }))
    .on_action(cx.listener(|this, _: &Redo, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.redo(cx);
    }))
    .on_action(cx.listener(|this, _: &DeleteSelection, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.delete_selected(cx);
    }))
    .on_action(cx.listener(|this, _: &SplitClip, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.split_at_playhead(cx);
    }))
    .on_action(cx.listener(|this, _: &TogglePlayback, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.toggle_play(cx);
    }))
    .on_action(cx.listener(|this, _: &PreviousFrame, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        if this.timeline_focused(window, cx) && this.nudge_selection(-1, cx) {
            return;
        }
        this.step_frame(-1, cx);
    }))
    .on_action(cx.listener(|this, _: &NextFrame, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        if this.timeline_focused(window, cx) && this.nudge_selection(1, cx) {
            return;
        }
        this.step_frame(1, cx);
    }))
    .on_action(cx.listener(|this, _: &BackTenFrames, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        if this.timeline_focused(window, cx) && this.nudge_selection(-10, cx) {
            return;
        }
        this.step_frame(-10, cx);
    }))
    .on_action(cx.listener(|this, _: &ForwardTenFrames, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        if this.timeline_focused(window, cx) && this.nudge_selection(10, cx) {
            return;
        }
        this.step_frame(10, cx);
    }))
    .on_action(cx.listener(|this, _: &SelectAllClips, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.select_all(cx);
    }))
    .on_action(cx.listener(|this, _: &ClearSelection, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.escape(cx);
    }))
    .on_action(cx.listener(|this, _: &CopyClips, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.copy_selection(cx);
    }))
    .on_action(cx.listener(|this, _: &CutClips, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.cut_selection(cx);
    }))
    .on_action(cx.listener(|this, _: &PasteClips, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.paste_clips(cx);
    }))
    .on_action(cx.listener(|this, _: &DuplicateClips, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.duplicate_selection(cx);
    }))
    .on_action(cx.listener(|this, _: &ToggleRippleScope, _window, cx| {
        this.toggle_ripple_scope(cx);
    }))
    .on_action(cx.listener(|this, _: &GoToStart, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.jump_to_start(cx);
    }))
    .on_action(cx.listener(|this, _: &GoToEnd, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.jump_to_end(cx);
    }))
    .on_action(cx.listener(|this, _: &ToggleLoop, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.toggle_loop(cx);
    }))
    .on_action(cx.listener(|this, _: &ToggleSnapping, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.toggle_snapping(cx);
    }))
    .on_action(cx.listener(|this, _: &ZoomIn, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.zoom_timeline(1.25, cx);
    }))
    .on_action(cx.listener(|this, _: &ZoomOut, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.zoom_timeline(0.8, cx);
    }))
    .on_action(cx.listener(|this, _: &ZoomToFit, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.timeline_zoom = 1.0;
        this.timeline_scroll = 0.0;
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &LargerTracks, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.set_track_height(this.dock.track_height.larger(), cx);
    }))
    .on_action(cx.listener(|this, _: &SmallerTracks, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.set_track_height(this.dock.track_height.smaller(), cx);
    }))
    .on_action(cx.listener(|this, _: &ToggleMediaPool, _window, cx| {
        this.toggle_panel(PanelSide::Left, cx);
    }))
    .on_action(cx.listener(|this, _: &ToggleInspector, _window, cx| {
        this.toggle_panel(PanelSide::Right, cx);
    }))
    .on_action(cx.listener(|this, _: &ToggleTimeline, _window, cx| {
        this.toggle_panel(PanelSide::Bottom, cx);
    }))
    .on_action(cx.listener(|this, _: &ToggleGpuPreview, _window, cx| {
        this.toggle_gpu_preview(cx);
    }))
    .on_action(cx.listener(|this, _: &ShowEffects, _window, cx| {
        this.select_left_tab(LeftTab::Effects, cx);
    }))
    .on_action(cx.listener(|this, _: &FitViewer, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.select_viewer_fit(ViewerFit::Contain, cx);
    }))
    .on_action(cx.listener(|this, _: &ToggleSafeAreas, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.toggle_safe_areas(cx);
    }))
    .on_action(cx.listener(|_this, _: &KeyboardShortcuts, window, cx| {
        Editor::show_shortcuts(window, cx);
    }))
    .on_action(cx.listener(|this, _: &About, _window, cx| {
        this.notices
            .push(gpui_component::notification::Notification::info(
                "AniMathIO 2.0",
            ));
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &ShuttleReverse, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.shuttle(-1, cx);
    }))
    .on_action(cx.listener(|this, _: &ShuttleStop, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.k_held = true;
        this.shuttle(0, cx);
    }))
    .on_action(cx.listener(|this, _: &ShuttleForward, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.shuttle(1, cx);
    }))
    .on_action(cx.listener(|this, _: &PreviousEdit, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.step_edit(-1, cx);
    }))
    .on_action(cx.listener(|this, _: &NextEdit, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.step_edit(1, cx);
    }))
    .on_action(cx.listener(|this, _: &GoToIn, _, cx| {
        if let Some(time) = this.marks.start {
            this.seek_to(time, cx);
        }
    }))
    .on_action(cx.listener(|this, _: &GoToOut, _, cx| {
        if let Some(time) = this.marks.end {
            this.seek_to(time, cx);
        }
    }))
    .on_action(cx.listener(|this, _: &MarkIn, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.marks.set_in(this.position);
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &MarkOut, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.marks.set_out(this.position);
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &ClearMarks, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.marks = InOut::default();
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &SelectTool, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.tool = Tool::Select;
        this.snap_line = None;
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &BladeTool, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.tool = Tool::Blade;
        this.snap_line = None;
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &RippleTool, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.tool = Tool::Ripple;
        this.snap_line = None;
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &HandTool, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.tool = Tool::Hand;
        this.snap_line = None;
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &RippleDelete, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.ripple_delete_selection(cx);
    }))
    .on_action(cx.listener(|this, _: &Preferences, window, cx| this.open_preferences(window, cx)))
    .on_action(cx.listener(|this, _: &ImportV1, _, cx| this.import_legacy(cx)))
    .on_action(cx.listener(|this, _: &ImportManim, window, cx| this.open_manim_import(window, cx)))
    .on_action(cx.listener(|this, _: &WriteOn, window, cx| this.open_write_on(window, cx)))
    .on_action(cx.listener(|this, action: &OpenRecent, _, cx| this.open_path(&action.path, cx)))
}

impl Editor {
    fn show_shortcuts(window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::{WindowExt, kbd::Kbd};
        let mut seen = BTreeSet::new();
        let rows: Vec<_> = keymap()
            .into_iter()
            .filter_map(|(_, action, context)| {
                let name = action
                    .name()
                    .rsplit("::")
                    .next()
                    .unwrap_or(action.name())
                    .to_owned();
                seen.insert(name.clone()).then_some((name, action, context))
            })
            .collect();
        window.open_dialog(cx, move |dialog, window, _cx| {
            dialog.title("Keyboard shortcuts").child(
                div()
                    .id("shortcut-list")
                    .max_h(px(500.))
                    .overflow_y_scroll()
                    .children(rows.iter().map(|(label, action, context)| {
                        div()
                            .flex()
                            .justify_between()
                            .py_1()
                            .gap_4()
                            .child(label.clone())
                            .children(Kbd::binding_for_action(
                                action.as_ref(),
                                Some(context),
                                window,
                            ))
                    })),
            )
        });
    }

    pub(super) fn zoom_timeline(&mut self, factor: f32, cx: &mut Context<Self>) {
        self.timeline_zoom = (self.timeline_zoom * factor).clamp(1.0, 64.0);
        cx.notify();
    }

    pub(super) fn set_track_height(
        &mut self,
        height: crate::layout::TrackHeight,
        cx: &mut Context<Self>,
    ) {
        self.dock.track_height = height;
        self.dock.save();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn panel_shortcuts_leave_text_entry_to_the_input() {
        for (_, action, panel) in keymap()
            .into_iter()
            .filter(|(_, _, panel)| *panel != "Editor")
        {
            let predicate = binding_context(action.as_ref(), panel);
            let contexts = [
                KeyContext::parse("Editor").unwrap(),
                KeyContext::parse(panel).unwrap(),
            ];
            assert!(predicate.depth_of(&contexts).is_some());
            let mut typing = contexts.to_vec();
            typing.push(KeyContext::parse("Input").unwrap());
            assert!(predicate.depth_of(&typing).is_none());
        }
    }

    #[core::prelude::v1::test]
    fn shortcuts_parse_for_mac_linux_and_windows() {
        for os in [
            OperatingSystem::Mac,
            OperatingSystem::Linux,
            OperatingSystem::Windows,
        ] {
            for (keys, _, context) in keymap() {
                let key =
                    Keystroke::parse_with_os(keys, os).expect("valid shortcut on each platform");
                assert!(KeyBindingContextPredicate::parse(context).is_ok());
                if keys.starts_with("secondary-") {
                    assert_eq!(key.modifiers.platform, os == OperatingSystem::Mac);
                    assert_eq!(key.modifiers.control, os != OperatingSystem::Mac);
                }
            }
        }
    }
}
