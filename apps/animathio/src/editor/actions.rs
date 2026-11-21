//! Editor actions and the shared, platform-aware keymap.

use super::*;

/// The editor guide on the project site, opened by Help > Editor guide.
const EDITOR_GUIDE_URL: &str = "https://docs.animathio.com/docs/editor";
/// Where bug reports and feature requests go, opened by Help > Report an issue.
const ISSUES_URL: &str = "https://github.com/AniMathIO/AniMathIO/issues/new/choose";

/// The Help > About toast: the running version, not a hard-coded release.
pub(super) fn about_text() -> String {
    format!(
        "AniMathIO {}. A video editor for math animations.",
        env!("CARGO_PKG_VERSION")
    )
}

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
        MarkClip,
        MarkIn,
        MarkOut,
        ClearMarks,
        SelectTool,
        BladeTool,
        RippleTool,
        RateStretchTool,
        SlipTool,
        SlideTool,
        RollingTool,
        HandTool,
        RippleDelete,
        RippleTrimPrevious,
        RippleTrimNext,
        UnlinkClips,
        ToggleRippleScope,
        ToggleMarkerRipple,
        SelectAllClips,
        ClearSelection,
        CopyClips,
        CutClips,
        PasteClips,
        DuplicateClips,
        BackFiveFrames,
        ForwardFiveFrames,
        Preferences,
        ImportV1,
        ImportManim,
        WriteOn,
        GenerateSubtitles,
        AutomaticCuts,
        AutoDuck,
        LearnNoise,
        AddMarker,
        PreviousMarker,
        NextMarker,
        ShowMarkers,
        AutoReframe,
        SceneCuts,
        SpeedDuration,
        InsertFrameHold,
        NewProject,
        SaveTemplate,
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
        SplitAllTracks,
        TogglePlayback,
        NudgeLeft,
        NudgeRight,
        PreviousFrame,
        NextFrame,
        GoToStart,
        GoToEnd,
        ToggleLoop,
        ToggleSnapping,
        DefaultVideoTransition,
        DefaultAudioTransition,
        DefaultTransitions,
        ZoomIn,
        ZoomOut,
        ZoomToFit,
        LargerTracks,
        SmallerTracks,
        FocusLibrary,
        FocusTimeline,
        FocusViewer,
        FocusInspector,
        FocusEffects,
        ToggleMediaPool,
        ToggleInspector,
        ToggleTimeline,
        ToggleGpuPreview,
        ShowEffects,
        ShowElements,
        ShowTitles,
        ShowPlugins,
        FitViewer,
        ToggleSafeAreas,
        KeyboardShortcuts,
        OpenEditorGuide,
        ReportIssue,
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
        || [
            &FocusLibrary as &dyn Action,
            &FocusTimeline,
            &FocusViewer,
            &FocusInspector,
            &FocusEffects,
        ]
        .iter()
        .any(|focus| action.partial_eq(*focus))
    {
        format!("{panel} && !Input")
    } else {
        panel.to_owned()
    };
    KeyBindingContextPredicate::parse(&context).expect("valid context")
}

pub(super) fn keymap() -> Vec<(&'static str, Box<dyn Action>, &'static str)> {
    vec![
        ("shift-1", Box::new(FocusLibrary), "Editor"),
        ("shift-3", Box::new(FocusTimeline), "Editor"),
        ("shift-4", Box::new(FocusViewer), "Editor"),
        ("shift-5", Box::new(FocusInspector), "Editor"),
        ("shift-7", Box::new(FocusEffects), "Editor"),
        ("secondary-r", Box::new(SpeedDuration), "Timeline"),
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
        ("x", Box::new(MarkClip), "Timeline"),
        ("i", Box::new(MarkIn), "Timeline"),
        ("i", Box::new(MarkIn), "Viewer"),
        ("o", Box::new(MarkOut), "Timeline"),
        ("o", Box::new(MarkOut), "Viewer"),
        ("secondary-shift-x", Box::new(ClearMarks), "Timeline"),
        ("secondary-shift-x", Box::new(ClearMarks), "Viewer"),
        ("v", Box::new(SelectTool), "Timeline"),
        ("c", Box::new(BladeTool), "Timeline"),
        ("b", Box::new(RippleTool), "Timeline"),
        ("r", Box::new(RateStretchTool), "Timeline"),
        ("y", Box::new(SlipTool), "Timeline"),
        ("u", Box::new(SlideTool), "Timeline"),
        ("n", Box::new(RollingTool), "Timeline"),
        ("h", Box::new(HandTool), "Timeline"),
        ("shift-delete", Box::new(RippleDelete), "Timeline"),
        ("alt-backspace", Box::new(RippleDelete), "Timeline"),
        ("secondary-a", Box::new(SelectAllClips), "Timeline"),
        ("escape", Box::new(ClearSelection), "Timeline"),
        ("secondary-c", Box::new(CopyClips), "Timeline"),
        ("secondary-x", Box::new(CutClips), "Timeline"),
        ("secondary-v", Box::new(PasteClips), "Timeline"),
        ("secondary-shift-/", Box::new(DuplicateClips), "Timeline"),
        ("secondary-,", Box::new(Preferences), "Editor"),
        ("secondary-alt-n", Box::new(NewProject), "Editor"),
        ("secondary-o", Box::new(OpenProject), "Editor"),
        ("secondary-s", Box::new(SaveProject), "Editor"),
        ("secondary-i", Box::new(ImportMedia), "Editor"),
        ("secondary-m", Box::new(ExportVideo), "Editor"),
        ("secondary-z", Box::new(Undo), "Editor"),
        ("secondary-shift-z", Box::new(Redo), "Editor"),
        ("delete", Box::new(DeleteSelection), "Timeline"),
        ("secondary-k", Box::new(SplitClip), "Timeline"),
        ("secondary-shift-k", Box::new(SplitAllTracks), "Timeline"),
        ("secondary-l", Box::new(UnlinkClips), "Timeline"),
        ("secondary-g", Box::new(UnlinkClips), "Timeline"),
        ("q", Box::new(RippleTrimPrevious), "Timeline"),
        ("q", Box::new(RippleTrimPrevious), "Viewer"),
        ("w", Box::new(RippleTrimNext), "Timeline"),
        ("w", Box::new(RippleTrimNext), "Viewer"),
        ("secondary-d", Box::new(DefaultVideoTransition), "Timeline"),
        (
            "secondary-shift-d",
            Box::new(DefaultAudioTransition),
            "Timeline",
        ),
        ("shift-d", Box::new(DefaultTransitions), "Timeline"),
        ("space", Box::new(TogglePlayback), "Viewer"),
        ("space", Box::new(TogglePlayback), "Timeline"),
        ("alt-left", Box::new(NudgeLeft), "Timeline"),
        ("alt-right", Box::new(NudgeRight), "Timeline"),
        ("left", Box::new(PreviousFrame), "Viewer"),
        ("left", Box::new(PreviousFrame), "Timeline"),
        ("right", Box::new(NextFrame), "Viewer"),
        ("right", Box::new(NextFrame), "Timeline"),
        ("shift-left", Box::new(BackFiveFrames), "Viewer"),
        ("shift-left", Box::new(BackFiveFrames), "Timeline"),
        ("shift-right", Box::new(ForwardFiveFrames), "Viewer"),
        ("shift-right", Box::new(ForwardFiveFrames), "Timeline"),
        ("home", Box::new(GoToStart), "Viewer"),
        ("home", Box::new(GoToStart), "Timeline"),
        ("end", Box::new(GoToEnd), "Viewer"),
        ("end", Box::new(GoToEnd), "Timeline"),
        ("alt-l", Box::new(ToggleLoop), "Viewer"),
        ("alt-l", Box::new(ToggleLoop), "Timeline"),
        ("s", Box::new(ToggleSnapping), "Timeline"),
        ("m", Box::new(AddMarker), "Timeline"),
        ("m", Box::new(AddMarker), "Viewer"),
        ("shift-m", Box::new(NextMarker), "Timeline"),
        ("shift-m", Box::new(NextMarker), "Viewer"),
        ("secondary-shift-m", Box::new(PreviousMarker), "Timeline"),
        ("secondary-shift-m", Box::new(PreviousMarker), "Viewer"),
        ("=", Box::new(ZoomIn), "Timeline"),
        ("-", Box::new(ZoomOut), "Timeline"),
        ("\\", Box::new(ZoomToFit), "Timeline"),
        ("secondary-alt-=", Box::new(LargerTracks), "Timeline"),
        ("secondary-alt--", Box::new(SmallerTracks), "Timeline"),
        ("shift-f", Box::new(FitViewer), "Viewer"),
        ("shift-f", Box::new(FitViewer), "Timeline"),
        ("secondary-alt-k", Box::new(KeyboardShortcuts), "Editor"),
        ("backspace", Box::new(DeleteSelection), "Timeline"),
    ]
}

/// Handlers live on the editor ancestor so menus and toolbars dispatch the
/// same commands. Text entry always keeps its editing/navigation keys.
pub(super) fn attach(root: Div, cx: &mut Context<Editor>) -> Div {
    root.on_action(cx.listener(|_, _: &SaveTemplate, window, cx| {
        Editor::save_template_dialog(window, cx);
    }))
    .on_action(cx.listener(|this, _: &NewProject, window, cx| {
        this.replace_project(ReplaceProject::New, window, cx);
    }))
    .on_action(cx.listener(|this, _: &OpenProject, window, cx| {
        this.replace_project(ReplaceProject::Open, window, cx);
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
    .on_action(cx.listener(|this, _: &SplitAllTracks, _, cx| {
        let targets: Vec<_> = this
            .project
            .tracks()
            .iter()
            .filter(|track| !track.is_locked())
            .flat_map(|track| track.clips().values())
            .filter(|clip| clip.range.start < this.position && this.position < clip.range.end)
            .map(|clip| clip.id)
            .collect();
        let targets = selection::one_per_link_group(&this.project, &targets);
        let (commands, next) =
            selection::split_commands(&this.project, &targets, this.position, this.next_id);
        this.next_id = next;
        if let Some(command) = selection::batch(commands) {
            this.execute(&command, cx);
        } else {
            this.error = Some(super::commands::SPLIT_HINT.into());
            cx.notify();
        }
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
    .on_action(cx.listener(|this, _: &NudgeLeft, _, cx| {
        this.nudge_selection(-1, cx);
    }))
    .on_action(cx.listener(|this, _: &NudgeRight, _, cx| {
        this.nudge_selection(1, cx);
    }))
    .on_action(cx.listener(|this, _: &PreviousFrame, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.step_frame(-1, cx);
    }))
    .on_action(cx.listener(|this, _: &NextFrame, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.step_frame(1, cx);
    }))
    .on_action(cx.listener(|this, _: &BackFiveFrames, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.step_frame(-5, cx);
    }))
    .on_action(cx.listener(|this, _: &ForwardFiveFrames, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.step_frame(5, cx);
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
    .on_action(cx.listener(|this, _: &ToggleMarkerRipple, _window, cx| {
        this.toggle_marker_ripple(cx);
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
        this.animate_timeline_zoom(1.0, 0.0, cx);
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
    .on_action(cx.listener(|this, _: &FocusLibrary, window, cx| {
        this.dock.left_open = true;
        this.select_left_tab(LeftTab::Media, cx);
        window.focus(&this.panel_focus[3], cx);
    }))
    .on_action(cx.listener(|this, _: &FocusEffects, window, cx| {
        this.dock.left_open = true;
        this.select_left_tab(LeftTab::Effects, cx);
        window.focus(&this.panel_focus[3], cx);
    }))
    .on_action(cx.listener(|this, _: &FocusTimeline, window, cx| {
        this.dock.bottom_open = true;
        window.focus(&this.panel_focus[0], cx);
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &FocusViewer, window, cx| {
        window.focus(&this.panel_focus[1], cx);
    }))
    .on_action(cx.listener(|this, _: &FocusInspector, window, cx| {
        this.dock.right_open = true;
        window.focus(&this.panel_focus[2], cx);
        cx.notify();
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
        this.dock.left_open = true;
        this.select_left_tab(LeftTab::Effects, cx);
    }))
    .on_action(cx.listener(|this, _: &ShowElements, window, cx| {
        this.dock.left_open = true;
        this.select_left_tab(LeftTab::Elements, cx);
        window.focus(&this.panel_focus[3], cx);
    }))
    .on_action(cx.listener(|this, _: &ShowTitles, window, cx| {
        this.dock.left_open = true;
        this.select_left_tab(LeftTab::Titles, cx);
        window.focus(&this.panel_focus[3], cx);
    }))
    .on_action(cx.listener(|this, _: &ShowPlugins, window, cx| {
        this.dock.left_open = true;
        this.select_left_tab(LeftTab::Plugins, cx);
        window.focus(&this.panel_focus[3], cx);
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
    // Escape in an inspector field drops the unsaved edit and returns to the timeline. The
    // timeline's own Escape binding only applies once focus is there.
    .on_action(
        cx.listener(|this, _: &gpui_component::input::Escape, window, cx| {
            if this.fields.revert_focused_input(window, cx) {
                window.focus(&this.panel_focus[0], cx);
            } else {
                cx.propagate();
            }
        }),
    )
    .on_action(cx.listener(|_this, _: &OpenEditorGuide, _, cx| cx.open_url(EDITOR_GUIDE_URL)))
    .on_action(cx.listener(|_this, _: &ReportIssue, _, cx| cx.open_url(ISSUES_URL)))
    .on_action(cx.listener(|this, _: &About, _window, cx| {
        this.notices.push(crate::style::Toast::info(about_text()));
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
    .on_action(cx.listener(|this, _: &MarkClip, _, cx| {
        if let Some(clip) = this
            .project
            .tracks()
            .iter()
            .flat_map(|t| t.clips().values())
            .find(|clip| clip.range.start <= this.position && this.position < clip.range.end)
        {
            this.marks.set_in(clip.range.start);
            this.marks.set_out(clip.range.end);
            cx.notify();
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
    .on_action(cx.listener(|this, _: &RateStretchTool, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.tool = Tool::RateStretch;
        this.snap_line = None;
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &SlipTool, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.tool = Tool::Slip;
        this.snap_line = None;
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &SlideTool, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.tool = Tool::Slide;
        this.snap_line = None;
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &RollingTool, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.tool = Tool::Rolling;
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
    .on_action(cx.listener(|this, _: &UnlinkClips, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        if let Some(id) = this.selected {
            if this
                .find_clip(id)
                .is_some_and(|clip| clip.link_group.is_some())
            {
                this.unlink_clip(id, cx);
            } else {
                let ids = this.selected_ids();
                if ids.len() > 1 {
                    let commands = ids
                        .into_iter()
                        .map(|id| Command::SetClipLink {
                            id,
                            group: this.selected,
                        })
                        .collect();
                    this.execute(&Command::Batch { commands }, cx);
                }
            }
        }
    }))
    .on_action(cx.listener(|this, _: &RippleDelete, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.ripple_delete_selection(cx);
    }))
    .on_action(cx.listener(|this, _: &RippleTrimPrevious, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.ripple_trim_to_playhead(None, ripple::TrimSide::Previous, cx);
    }))
    .on_action(cx.listener(|this, _: &RippleTrimNext, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.ripple_trim_to_playhead(None, ripple::TrimSide::Next, cx);
    }))
    .on_action(cx.listener(|this, _: &DefaultVideoTransition, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.apply_default_transition(None, fades::TransitionKind::Video, cx);
    }))
    .on_action(cx.listener(|this, _: &DefaultAudioTransition, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.apply_default_transition(None, fades::TransitionKind::Audio, cx);
    }))
    .on_action(cx.listener(|this, _: &DefaultTransitions, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.apply_default_transition(None, fades::TransitionKind::Both, cx);
    }))
    .on_action(cx.listener(|this, _: &Preferences, window, cx| this.open_preferences(window, cx)))
    .on_action(cx.listener(|this, _: &ImportV1, window, cx| {
        this.replace_project(ReplaceProject::ImportV1, window, cx);
    }))
    .on_action(cx.listener(|this, _: &ImportManim, window, cx| this.open_manim_import(window, cx)))
    .on_action(cx.listener(|this, _: &WriteOn, window, cx| this.open_write_on(window, cx)))
    .on_action(
        cx.listener(|this, _: &SpeedDuration, window, cx| this.open_speed_duration(window, cx)),
    )
    .on_action(
        cx.listener(|this, _: &InsertFrameHold, window, cx| this.open_frame_hold(window, cx)),
    )
    .on_action(cx.listener(|this, _: &SceneCuts, _, cx| this.open_scenes(cx)))
    .on_action(cx.listener(|this, _: &AutoReframe, _, cx| this.open_reframe(cx)))
    .on_action(cx.listener(|this, _: &AutomaticCuts, _, cx| this.open_autocut(cx)))
    .on_action(cx.listener(|this, _: &LearnNoise, _, cx| this.learn_noise(cx)))
    .on_action(cx.listener(|this, _: &AutoDuck, _, cx| this.open_duck(cx)))
    .on_action(cx.listener(|this, _: &AddMarker, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.add_marker_at_playhead(cx);
    }))
    .on_action(cx.listener(|this, _: &NextMarker, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.jump_marker(1, cx);
    }))
    .on_action(cx.listener(|this, _: &PreviousMarker, window, cx| {
        if this.fields.any_focused(window, cx) {
            cx.propagate();
            return;
        }
        this.jump_marker(-1, cx);
    }))
    .on_action(cx.listener(|this, _: &ShowMarkers, _, cx| this.open_marker_panel(cx)))
    .on_action(cx.listener(|this, _: &GenerateSubtitles, window, cx| {
        this.open_subtitles_dialog(window, cx);
    }))
    .on_action(cx.listener(|this, action: &OpenRecent, window, cx| {
        this.replace_project(ReplaceProject::Recent(action.path.clone()), window, cx);
    }))
}

impl Editor {
    fn show_shortcuts(window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::{WindowExt, kbd::Kbd};
        let search = cx.new(|cx| {
            gpui_component::input::InputState::new(window, cx).placeholder("Search actions or keys")
        });
        cx.subscribe(
            &search,
            |_, _, _: &gpui_component::input::InputEvent, cx| cx.notify(),
        )
        .detach();
        let mut seen = BTreeSet::new();
        let rows: Vec<_> = keymap()
            .into_iter()
            .filter_map(|(keys, action, context)| {
                let name = action
                    .name()
                    .rsplit("::")
                    .next()
                    .unwrap_or(action.name())
                    .to_owned();
                seen.insert(name.clone())
                    .then_some((name, keys, action, context))
            })
            .collect();
        window.open_dialog(cx, move |dialog, window, cx| {
            let query = search.read(cx).value().to_lowercase();
            dialog
                .title("Keyboard shortcuts")
                .child(gpui_component::input::Input::new(&search))
                .child(
                    div()
                        .id("shortcut-list")
                        .max_h(px(500.))
                        .overflow_y_scroll()
                        .children(
                            rows.iter()
                                .filter(|(label, keys, _, _)| {
                                    label.to_lowercase().contains(&query.replace(' ', ""))
                                        || keys.replace("secondary", "ctrl").contains(&query)
                                        || keys.replace("secondary", "cmd").contains(&query)
                                })
                                .map(|(label, _, action, context)| {
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
                                }),
                        ),
                )
        });
    }

    pub(super) fn zoom_timeline(&mut self, factor: f32, cx: &mut Context<Self>) {
        let zoom = self
            .zoom_motion
            .as_ref()
            .map_or(self.timeline_zoom, |motion| motion.to.0);
        self.animate_timeline_zoom(
            (zoom * factor).clamp(1.0, 64.0),
            f32::from(self.timeline_bounds.size.width) / 2.0,
            cx,
        );
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
    fn bindings_do_not_conflict_in_the_same_context() {
        for os in [
            OperatingSystem::Mac,
            OperatingSystem::Linux,
            OperatingSystem::Windows,
        ] {
            let mut seen = BTreeSet::new();
            for (keys, _, context) in keymap() {
                let key = Keystroke::parse_with_os(keys, os).unwrap();
                assert!(
                    seen.insert((format!("{key:?}"), context)),
                    "duplicate {keys} in {context}"
                );
            }
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
