//! Timeline markers and beat/marker sync: adding, jumping, editing and dragging markers,
//! plus a panel that lists them and runs beat detection, "markers at beats" and cut sync.
//! Every edit is one `Command` (or one `Command::Batch`), so each is a single undo step.

use super::tool_panel;
use super::*;
use crate::style::{self, SegmentedOption, TextRole as _, color};
use animathio_control::{
    BeatDetectParams, BeatDetection, BeatMarkerOptions, BeatSelection, SyncCutsParams, SyncMode,
    SyncSource, analyze_beats, markers_at_beats_command, select_beats, sync_cuts_command,
};
use animathio_core::{Marker, MarkerColor};
use gpui_component::{
    Disableable, WindowExt,
    input::{Input, InputState},
};

/// Pixel width of a marker flag on the ruler.
const FLAG_W: f32 = 10.0;
const FLAG_H: f32 = 9.0;
/// Marker colour swatch in the edit dialog, and the colour dot in the panel list.
const SWATCH: gpui::Rems = gpui::Rems(1.333);
const DOT: gpui::Rems = gpui::Rems(0.667);

/// The colour of a palette entry as a paint colour.
pub(super) fn paint_color(color: MarkerColor) -> Hsla {
    let [r, g, b] = color.rgb();
    style::hex((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b))
}

/// The audio analysis a control call needs before it can run, if any.
pub(super) fn beat_analysis_params(call: &Call) -> Option<BeatDetectParams> {
    match call {
        Call::BeatsAddMarkers { params } => Some(params.detect.clone()),
        Call::SyncCutsToBeats { params } if params.source.needs_beats() => {
            Some(params.detect.clone())
        }
        _ => None,
    }
}

/// State of the markers and beat-sync panel.
pub(super) struct Panel {
    identity: Arc<()>,
    generation: u64,
    detection: Option<BeatDetection>,
    /// Tracks the beats were analysed from; any edit to them invalidates the beats.
    analysed_tracks: Vec<Track>,
    busy: bool,
    error: Option<String>,
    notice: Option<String>,
    every: u32,
    downbeats_only: bool,
    max_shift_ms: i64,
    source: SyncSource,
    /// Analyse the selected clip instead of the whole mix.
    selected_only: bool,
    /// Scroll position of the panel body.
    scroll: ScrollHandle,
    /// Keyboard focus for the segmented controls (one handle per option).
    scope_focus: Vec<FocusHandle>,
    every_focus: Vec<FocusHandle>,
    source_focus: Vec<FocusHandle>,
    shift_focus: Vec<FocusHandle>,
}

/// The "Max shift" choices in milliseconds.
const MAX_SHIFTS_MS: [i64; 3] = [100, 250, 500];

impl Panel {
    /// The beat placement and sync settings, for the UI tests: (every Nth beat, downbeats only,
    /// max shift in ms).
    #[cfg(test)]
    pub(super) fn settings(&self) -> (u32, bool, i64) {
        (self.every, self.downbeats_only, self.max_shift_ms)
    }

    fn new(cx: &mut Context<Editor>) -> Self {
        Self {
            identity: Arc::new(()),
            generation: 0,
            detection: None,
            analysed_tracks: Vec::new(),
            busy: false,
            error: None,
            notice: None,
            every: 1,
            downbeats_only: false,
            max_shift_ms: 250,
            source: SyncSource::Beats,
            selected_only: false,
            scroll: ScrollHandle::new(),
            scope_focus: tool_panel::focus_handles(2, cx),
            every_focus: tool_panel::focus_handles(4, cx),
            source_focus: tool_panel::focus_handles(3, cx),
            shift_focus: tool_panel::focus_handles(3, cx),
        }
    }
}

impl Editor {
    /// Add a marker at the playhead (`M`).
    pub(super) fn add_marker_at_playhead(&mut self, cx: &mut Context<Self>) {
        let mut marker = Marker::new(self.project.next_marker_id(), self.position);
        if self
            .project
            .markers()
            .iter()
            .any(|m| m.time == self.position)
        {
            self.error = Some("A marker already exists at the playhead".into());
            cx.notify();
            return;
        }
        marker.color = MarkerColor::Blue;
        self.execute(&Command::AddMarker { marker }, cx);
    }

    /// Move the playhead to the previous (`-1`) or next (`1`) marker.
    pub(super) fn jump_marker(&mut self, direction: i32, cx: &mut Context<Self>) {
        let times = self.project.markers().iter().map(|m| m.time);
        let target = if direction < 0 {
            times.filter(|t| *t < self.position).max()
        } else {
            times.filter(|t| *t > self.position).min()
        };
        if let Some(time) = target {
            self.seek_to(time, cx);
        }
    }

    /// Run `beats.add_markers` or `timeline.sync_cuts_to_beats` against the live project and
    /// history as one undo step. `detection` comes from a background analysis.
    pub(super) fn apply_beat_call(
        &mut self,
        call: &Call,
        detection: Option<&BeatDetection>,
        cx: &mut Context<Self>,
    ) -> Result<serde_json::Value, ControlError> {
        let value = match call {
            Call::BeatsAddMarkers { params } => {
                let detection =
                    detection.ok_or_else(|| ControlError::Other("beat analysis missing".into()))?;
                serde_json::to_value(animathio_control::apply_beat_markers(
                    &mut self.project,
                    &mut self.history,
                    detection,
                    &params.options,
                )?)?
            }
            Call::SyncCutsToBeats { params } => {
                self.source_limits
                    .probe_missing(&self.project, &self.assets);
                serde_json::to_value(animathio_control::apply_sync_cuts(
                    &mut self.project,
                    &mut self.history,
                    detection,
                    params,
                    &self.source_limits,
                )?)?
            }
            _ => return Err(ControlError::Other("not a beat call".into())),
        };
        self.after_edit(cx);
        Ok(value)
    }

    pub(super) fn open_marker_panel(&mut self, cx: &mut Context<Self>) {
        self.marker_panel = Some(Panel::new(cx));
        cx.notify();
    }

    /// Drop analysed beats once the tracks they came from change.
    pub(super) fn invalidate_beats(&mut self) {
        let tracks = self.project.tracks();
        if let Some(panel) = self.marker_panel.as_mut()
            && panel.detection.is_some()
            && panel.analysed_tracks != tracks
        {
            panel.detection = None;
            panel.generation = panel.generation.wrapping_add(1);
            panel.busy = false;
            panel.notice = Some("Timeline changed; detect beats again".into());
        }
    }

    fn detect_beats(&mut self, cx: &mut Context<Self>) {
        let selected = self.selected;
        let Some(panel) = self.marker_panel.as_mut() else {
            return;
        };
        panel.generation = panel.generation.wrapping_add(1);
        let generation = panel.generation;
        let identity = panel.identity.clone();
        panel.busy = true;
        panel.error = None;
        panel.notice = None;
        panel.detection = None;
        let params = BeatDetectParams {
            clip: selected.filter(|_| panel.selected_only),
            ..BeatDetectParams::default()
        };
        let project = self.project.clone();
        let assets = self.assets.clone();
        let audio_generation = self.audio_generation;
        cx.spawn(async move |this, cx| {
            let snapshot = project.clone();
            let result = cx
                .background_spawn(async move { analyze_beats(&project, &assets, &params) })
                .await;
            let _ = this.update(cx, |editor, cx| {
                let Some(panel) = editor.marker_panel.as_mut() else {
                    return;
                };
                if !Arc::ptr_eq(&identity, &panel.identity) || generation != panel.generation {
                    return;
                }
                panel.busy = false;
                if audio_generation != editor.audio_generation
                    && snapshot.tracks() != editor.project.tracks()
                {
                    panel.error = Some("Project changed; detect again".into());
                } else {
                    match result {
                        Ok(detection) => {
                            panel.analysed_tracks = snapshot.tracks().to_vec();
                            panel.detection = Some(detection);
                        }
                        Err(error) => {
                            panel.error = Some(
                                error
                                    .to_string()
                                    .trim_start_matches("invalid params: ")
                                    .to_owned(),
                            );
                        }
                    }
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Show a failure inside the panel (where the user is looking), or as the editor error when
    /// the panel is closed.
    fn panel_error(&mut self, message: &str) {
        let message = message.strip_prefix("invalid params: ").unwrap_or(message);
        match self.marker_panel.as_mut() {
            Some(panel) => {
                panel.error = Some(message.to_owned());
                panel.notice = None;
            }
            None => self.error = Some(message.to_owned()),
        }
    }

    fn beat_selection(&self) -> BeatSelection {
        self.marker_panel
            .as_ref()
            .map_or_else(BeatSelection::default, |p| BeatSelection {
                every: p.every,
                downbeats_only: p.downbeats_only,
            })
    }

    fn add_beat_markers(&mut self, cx: &mut Context<Self>) {
        let Some(detection) = self.marker_panel.as_ref().and_then(|p| p.detection.clone()) else {
            return;
        };
        if let Some(panel) = self.marker_panel.as_mut() {
            panel.error = None;
        }
        let options = BeatMarkerOptions {
            selection: self.beat_selection(),
            ..BeatMarkerOptions::default()
        };
        match markers_at_beats_command(&self.project, &detection, &options) {
            Ok((command, added)) => {
                if self.try_execute(&command, cx)
                    && let Some(panel) = self.marker_panel.as_mut()
                {
                    panel.notice = Some(format!("Added {added} markers"));
                }
            }
            Err(error) => self.panel_error(&error.to_string()),
        }
        cx.notify();
    }

    fn sync_selected(&mut self, mode: SyncMode, cx: &mut Context<Self>) {
        let clips: Vec<ClipId> = self.selected_ids().into_iter().collect();
        let Some(panel) = self.marker_panel.as_ref() else {
            return;
        };
        if clips.is_empty() {
            self.panel_error("Select the clips to sync first");
            cx.notify();
            return;
        }
        if panel.source.needs_beats() && panel.detection.is_none() {
            self.panel_error("Detect beats first, or sync to markers");
            cx.notify();
            return;
        }
        if let Some(panel) = self.marker_panel.as_mut() {
            panel.error = None;
        }
        let Some(panel) = self.marker_panel.as_ref() else {
            return;
        };
        let selection = self.beat_selection();
        let params = SyncCutsParams {
            clips,
            source: panel.source,
            mode,
            max_shift: Time(panel.max_shift_ms * TICKS_PER_SECOND / 1000),
            selection,
            ..SyncCutsParams::default()
        };
        let beats: Vec<Time> = panel
            .detection
            .as_ref()
            .filter(|_| params.source.needs_beats())
            .and_then(|d| select_beats(d, selection).ok())
            .map_or_default(|b| b.into_iter().map(|(_, t)| t).collect());
        let bpm = panel.detection.as_ref().map(|d| d.bpm);
        self.source_limits
            .probe_missing(&self.project, &self.assets);
        match sync_cuts_command(&self.project, &params, &beats, bpm, &self.source_limits) {
            Ok((command, result)) => {
                if self.try_execute(&command, cx)
                    && let Some(panel) = self.marker_panel.as_mut()
                {
                    panel.notice = Some(match mode {
                        SyncMode::Snap => format!(
                            "Moved {} cuts ({} skipped)",
                            result.shifted.len(),
                            result.skipped.len()
                        ),
                        SyncMode::Split => format!("Split at {} points", result.splits.len()),
                    });
                }
            }
            Err(error) => self.panel_error(&error.to_string()),
        }
        cx.notify();
    }

    /// Edit a marker's name, colour and note in a compact dialog (double-click on the ruler).
    pub(super) fn open_marker_dialog(
        &mut self,
        id: MarkerId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(marker) = self.project.marker(id).cloned() else {
            return;
        };
        let name = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Name")
                .default_value(marker.name.clone())
        });
        let note = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Note")
                .default_value(marker.note.clone())
        });
        // Type straight away: focus the name field the first time the dialog renders (the
        // dialog takes focus when it opens, so focusing earlier is overridden).
        let focused = std::rc::Rc::new(std::cell::Cell::new(false));
        let color = cx.new(|_| marker.color);
        let editor = cx.entity();
        let rate = self.project.frame_rate();
        super::dialogs::open_dialog(window, cx, move |dialog, window, cx| {
            if !focused.replace(true) {
                let focus = name.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            let (name_save, note_save, color_save, editor_save) =
                (name.clone(), note.clone(), color.clone(), editor.clone());
            let editor_delete = editor.clone();
            // The primary button and Enter share one save action.
            let save_marker: super::dialogs::Submit =
                std::rc::Rc::new(move |cx: &mut gpui::App| {
                    let name = name_save.read(cx).value().trim().to_owned();
                    let note = note_save.read(cx).value().trim().to_owned();
                    let color = *color_save.read(cx);
                    editor_save.update(cx, |this, cx| {
                        let Some(mut updated) = this.project.marker(id).cloned() else {
                            return false;
                        };
                        updated.name = name;
                        updated.note = note;
                        updated.color = color;
                        if this.project.marker(id) == Some(&updated) {
                            return true;
                        }
                        this.try_execute(
                            &Command::UpdateMarker {
                                id,
                                marker: updated,
                            },
                            cx,
                        )
                    })
                });
            let enter_save = save_marker.clone();
            let current = *color.read(cx);
            let swatches = MarkerColor::ALL.into_iter().enumerate().map(|(i, option)| {
                let color = color.clone();
                div()
                    .id(("marker-color", i))
                    .size(SWATCH)
                    .rounded_full()
                    .cursor_pointer()
                    .bg(paint_color(option))
                    .border_2()
                    .border_color(if option == current {
                        style::hex(style::color::TEXT)
                    } else {
                        gpui::transparent_black()
                    })
                    .on_click(move |_, window, cx| {
                        color.update(cx, |c, _| *c = option);
                        window.refresh();
                    })
            });
            let dialog = dialog
                .title(format!(
                    "Marker at {}",
                    timecode::format_timecode(marker.time, rate)
                ))
                .width(px(380.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(super::dialogs::tagged_input(
                            "marker-name",
                            Input::new(&name),
                        ))
                        .child(div().flex().gap_2().children(swatches))
                        .child(Input::new(&note)),
                )
                .footer(
                    div()
                        .flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new("marker-delete")
                                .label("Delete")
                                .icon(icons::icon(IconName::Trash))
                                .on_click(move |_, window, cx| {
                                    editor_delete.update(cx, |this, cx| {
                                        this.execute(&Command::RemoveMarker { id }, cx);
                                    });
                                    window.close_dialog(cx);
                                }),
                        )
                        .child(
                            Button::new("marker-cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            style::primary_button("marker-save", cx)
                                .label("Save")
                                .on_click(move |_, window, cx| {
                                    if save_marker(cx) {
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                );
            // Enter saves the marker, like the primary button.
            super::dialogs::on_enter(dialog, move |_, cx| enter_save(cx))
        });
    }

    /// The markers as painted on the ruler, with any drag in progress applied.
    pub(super) fn marker_paints(&self) -> Vec<MarkerPaint> {
        self.project
            .markers()
            .iter()
            .map(|marker| {
                let time = match &self.gesture {
                    Some(TimelineGesture::Marker(drag)) if drag.members.contains(&marker.id) => {
                        let shift = (drag.current.0 - drag.origin.0).max(
                            -drag
                                .members
                                .iter()
                                .filter_map(|id| self.project.marker(*id))
                                .map(|m| m.time.0)
                                .min()
                                .unwrap_or(0),
                        );
                        Time(marker.time.0 + shift)
                    }
                    _ => marker.time,
                };
                MarkerPaint {
                    time,
                    end: marker.duration.map(|d| Time(time.0.saturating_add(d.0))),
                    color: paint_color(marker.color),
                    name: marker.name.clone(),
                    selected: self.marker_selection.contains(&marker.id),
                }
            })
            .collect()
    }

    #[allow(clippy::too_many_lines)]
    pub(super) fn marker_panel(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let panel = self.marker_panel.as_ref()?;
        if self.is_start_screen() {
            return None;
        }
        let rate = self.project.frame_rate();
        let selected_count = self.selected_ids().len();
        let entity = cx.entity();
        let detected = panel.detection.as_ref();

        // Beats: where to listen, how to place markers, and the result of the analysis.
        let scope = tool_panel::choice(
            "beats-scope",
            &[
                SegmentedOption::new("beats-scope-mix", "Project mix", None),
                SegmentedOption::new("beats-scope-clip", "Selected clip", None),
            ],
            &panel.scope_focus,
            usize::from(panel.selected_only),
            {
                let entity = entity.clone();
                move |index, _, cx| {
                    entity.update(cx, |this, cx| {
                        let selected_only = index == 1;
                        if selected_only && this.selected.is_none() {
                            return;
                        }
                        if let Some(p) = this.marker_panel.as_mut()
                            && p.selected_only != selected_only
                        {
                            p.selected_only = selected_only;
                            p.detection = None;
                        }
                        cx.notify();
                    });
                }
            },
        );
        let every_index = if panel.downbeats_only {
            3
        } else {
            match panel.every {
                2 => 1,
                4 => 2,
                _ => 0,
            }
        };
        let place = tool_panel::choice(
            "beats-every",
            &[
                SegmentedOption::new("beats-every-1", "Every", None),
                SegmentedOption::new("beats-every-2", "Every 2", None),
                SegmentedOption::new("beats-every-4", "Every 4", None),
                SegmentedOption::new("beats-downbeats", "Downbeats", None),
            ],
            &panel.every_focus,
            every_index,
            {
                let entity = entity.clone();
                move |index, _, cx| {
                    entity.update(cx, |this, cx| {
                        if let Some(p) = this.marker_panel.as_mut() {
                            p.downbeats_only = index == 3;
                            // Downbeats keep the last "every Nth" choice for when it is turned off.
                            match index {
                                0 => p.every = 1,
                                1 => p.every = 2,
                                2 => p.every = 4,
                                _ => {}
                            }
                        }
                        cx.notify();
                    });
                }
            },
        );
        let mut beats = vec![
            style::form_row("Audio", scope).into_any_element(),
            tool_panel::stacked("Place markers on", place).into_any_element(),
            div()
                .flex()
                .child(
                    style::secondary_button("beats-detect", cx)
                        .debug_selector(|| "beats-detect".into())
                        .small()
                        .label("Detect beats")
                        .disabled(panel.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.detect_beats(cx))),
                )
                .into_any_element(),
        ];
        if panel.busy {
            beats.push(tool_panel::note("Analysing audio…").into_any_element());
        }
        if let Some(error) = &panel.error {
            beats.push(tool_panel::problem(error.clone()).into_any_element());
        }
        if let Some(notice) = &panel.notice {
            beats.push(tool_panel::note(notice.clone()).into_any_element());
        }
        if let Some(d) = detected {
            beats.push(
                tool_panel::note(format!(
                    "{:.1} BPM, {} beats, {:.0}% confidence",
                    d.bpm,
                    d.beats.len(),
                    d.confidence * 100.
                ))
                .into_any_element(),
            );
        }

        // Cut sync.
        let source_index = match panel.source {
            SyncSource::Beats => 0,
            SyncSource::Markers => 1,
            SyncSource::Both => 2,
        };
        let sync_source = tool_panel::choice(
            "sync-source",
            &[
                SegmentedOption::new("sync-source-beats", "Beats", None),
                SegmentedOption::new("sync-source-markers", "Markers", None),
                SegmentedOption::new("sync-source-both", "Both", None),
            ],
            &panel.source_focus,
            source_index,
            {
                let entity = entity.clone();
                move |index, _, cx| {
                    entity.update(cx, |this, cx| {
                        if let Some(p) = this.marker_panel.as_mut() {
                            p.source = match index {
                                1 => SyncSource::Markers,
                                2 => SyncSource::Both,
                                _ => SyncSource::Beats,
                            };
                        }
                        cx.notify();
                    });
                }
            },
        );
        let shift_index = MAX_SHIFTS_MS
            .iter()
            .position(|ms| *ms == panel.max_shift_ms)
            .unwrap_or(1);
        let sync_shift = tool_panel::choice(
            "sync-shift",
            &[
                SegmentedOption::new("sync-shift-100", "100 ms", None),
                SegmentedOption::new("sync-shift-250", "250 ms", None),
                SegmentedOption::new("sync-shift-500", "500 ms", None),
            ],
            &panel.shift_focus,
            shift_index,
            {
                let entity = entity.clone();
                move |index, _, cx| {
                    entity.update(cx, |this, cx| {
                        if let Some(p) = this.marker_panel.as_mut() {
                            p.max_shift_ms = MAX_SHIFTS_MS[index.min(MAX_SHIFTS_MS.len() - 1)];
                        }
                        cx.notify();
                    });
                }
            },
        );
        let sync = vec![
            style::form_row("Sync to", sync_source).into_any_element(),
            style::form_row("Max shift", sync_shift).into_any_element(),
            div()
                .flex()
                .flex_wrap()
                .gap_2()
                .child(
                    style::secondary_button("sync-snap", cx)
                        .debug_selector(|| "sync-snap".into())
                        .small()
                        .icon(icons::icon(IconName::Magnet))
                        .label("Snap selected cuts")
                        .disabled(selected_count == 0)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.sync_selected(SyncMode::Snap, cx)),
                        ),
                )
                .child(
                    style::secondary_button("sync-split", cx)
                        .debug_selector(|| "sync-split".into())
                        .small()
                        .icon(icons::icon(IconName::Scissors))
                        .label("Split at targets")
                        .disabled(selected_count == 0)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.sync_selected(SyncMode::Split, cx)),
                        ),
                )
                .into_any_element(),
        ];

        // Marker list: `ROW` rows, the whole row seeks, ghost icon buttons edit and remove.
        let mut list = div().id("marker-list").flex().flex_col();
        for (i, marker) in self.project.markers().iter().enumerate() {
            let (id, time) = (marker.id, marker.time);
            let is_selected = self.marker_selection.contains(&id);
            list = list.child(
                div()
                    .id(("marker-seek", i))
                    .flex()
                    .items_center()
                    .gap_2()
                    .min_h(style::size::ROW)
                    .pl_2()
                    .pr_1()
                    .rounded(px(style::radius::CONTROL))
                    .cursor_pointer()
                    .when(is_selected, |row| {
                        row.bg(style::tint(color::ACCENT, style::Alpha::Wash))
                    })
                    .hover(|row| row.bg(style::hex(color::HOVER)))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.clear_selection(cx);
                        this.marker_selection.insert(id);
                        this.seek_to(time, cx);
                    }))
                    .child(
                        div()
                            .size(DOT)
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(paint_color(marker.color)),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_role(style::Role::Label)
                            .text_color(style::hex(color::TEXT_SECONDARY))
                            .tabular()
                            .child(timecode::format_timecode(marker.time, rate)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_role(style::Role::Label)
                            .text_color(style::hex(color::TEXT))
                            .child(marker.name.clone()),
                    )
                    .child(
                        Button::new(("marker-edit", i))
                            .ghost()
                            .xsmall()
                            .icon(icons::icon(IconName::Pencil))
                            .tooltip("Edit marker")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_marker_dialog(id, window, cx);
                            })),
                    )
                    .child(
                        Button::new(("marker-remove", i))
                            .ghost()
                            .xsmall()
                            .icon(icons::icon(IconName::Trash))
                            .tooltip("Remove marker")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.execute(&Command::RemoveMarker { id }, cx);
                            })),
                    ),
            );
        }
        let marker_count = self.project.markers().len();
        let markers = if marker_count == 0 {
            style::empty_state(Some(IconName::Flag), "Markers you add appear here.", None)
                .into_any_element()
        } else {
            list.into_any_element()
        };

        let body = tool_panel::body()
            .child(tool_panel::section("Beats", beats))
            .child(tool_panel::section("Sync cuts", sync))
            .child(tool_panel::section(
                if marker_count == 0 {
                    "Markers".to_owned()
                } else {
                    format!("Markers ({marker_count})")
                },
                [markers],
            ));
        let footer = div()
            .flex()
            .flex_wrap()
            .items_center()
            .justify_end()
            .gap_2()
            .child(
                style::secondary_button("marker-add", cx)
                    .debug_selector(|| "marker-add".into())
                    .small()
                    .icon(icons::icon(IconName::Plus))
                    .label("Add at playhead")
                    .on_click(cx.listener(|this, _, _, cx| this.add_marker_at_playhead(cx))),
            )
            .child(
                style::primary_button("beats-add-markers", cx)
                    .debug_selector(|| "beats-add-markers".into())
                    .small()
                    .icon(icons::icon(IconName::Flag))
                    .label("Add markers at beats")
                    .disabled(detected.is_none())
                    .on_click(cx.listener(|this, _, _, cx| this.add_beat_markers(cx))),
            );
        Some(
            style::ToolPanel::new("marker-panel", IconName::Flag, "Markers and beats")
                .bottom_height(self.dock.bottom_height)
                .on_close(cx.listener(|this, _, _, cx| {
                    this.marker_panel = None;
                    cx.notify();
                }))
                .body(panel.scroll.clone(), body)
                .footer(footer)
                .render(window),
        )
    }
}

/// Gap between a marker's name and the next marker's flag.
const NAME_GAP: f32 = 4.0;

/// The x of every marker flag, left to right.
fn sorted_flag_xs(layout: &TimelineLayout, snapshot: &TimelineSnapshot) -> Vec<f32> {
    let mut xs: Vec<f32> = snapshot
        .markers
        .iter()
        .map(|marker| timeline::time_to_x(layout, marker.time))
        .collect();
    xs.sort_by(f32::total_cmp);
    xs
}

/// The x of the first flag to the right of `x` (`flag_xs` sorted ascending).
fn next_flag_x(flag_xs: &[f32], x: f32) -> Option<f32> {
    let first_after = flag_xs.partition_point(|candidate| *candidate <= x + 0.5);
    flag_xs.get(first_after).copied()
}

/// `name`, or its longest prefix followed by "…" that fits in `available` (a name that would
/// be cut mid-letter by the next flag ends in an ellipsis instead).
fn fit_name(name: &str, available: Pixels, name_font: &Font, window: &mut Window) -> String {
    if label_width(name, name_font.clone(), window) <= available {
        return name.to_owned();
    }
    let boundaries: Vec<usize> = name.char_indices().map(|(i, _)| i).collect();
    // Binary search for the longest prefix (in characters) that still fits with the ellipsis.
    let (mut low, mut high) = (0, boundaries.len());
    while low < high {
        let middle = (low + high).div_ceil(2);
        let candidate = format!("{}…", &name[..boundaries[middle]]);
        if label_width(&candidate, name_font.clone(), window) <= available {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    if low == 0 {
        String::new()
    } else {
        format!("{}…", &name[..boundaries[low]])
    }
}

/// Paint ranges, lines and flags for every visible marker over the whole timeline.
pub(super) fn paint_markers(
    bounds: Bounds<Pixels>,
    layout: &TimelineLayout,
    snapshot: &TimelineSnapshot,
    height: f32,
    window: &mut Window,
    cx: &mut App,
) {
    let flag_xs = sorted_flag_xs(layout, snapshot);
    for marker in &snapshot.markers {
        let x = timeline::time_to_x(layout, marker.time);
        let end_x = marker.end.map(|end| timeline::time_to_x(layout, end));
        if x > layout.width || end_x.unwrap_or(x) < -FLAG_W - 200.0 {
            continue;
        }
        let left = bounds.origin.x + px(x);
        if let Some(end_x) = end_x {
            let right = bounds.origin.x + px(end_x.clamp(-1.0, layout.width));
            window.paint_quad(fill(
                Bounds::from_corners(
                    point(left, bounds.top() + px(ruler_height() - 5.0)),
                    point(right.max(left + px(2.0)), bounds.top() + px(ruler_height())),
                ),
                marker.color.opacity(0.55),
            ));
        }
        window.paint_quad(fill(
            Bounds::new(point(left, bounds.top()), size(px(1.0), px(height))),
            marker.color.opacity(0.35),
        ));
        window.paint_quad(fill(
            Bounds::new(
                point(left, bounds.top()),
                size(px(1.5), px(ruler_height() - 4.0)),
            ),
            marker.color,
        ));
        if marker.selected {
            // A light outline around the flag and a thicker line mark the selection.
            window.paint_quad(fill(
                Bounds::new(
                    point(left - px(1.0), bounds.top()),
                    size(px(3.5), px(height)),
                ),
                style::hex(style::color::TEXT).opacity(0.45),
            ));
            window.paint_quad(
                fill(
                    Bounds::new(
                        point(left - px(1.0), bounds.top() - px(1.0)),
                        size(px(FLAG_W + 2.0), px(FLAG_H + 2.0)),
                    ),
                    style::hex(style::color::TEXT),
                )
                .corner_radii(px(3.0)),
            );
        }
        window.paint_quad(
            fill(
                Bounds::new(point(left, bounds.top()), size(px(FLAG_W), px(FLAG_H))),
                marker.color,
            )
            .corner_radii(px(2.0)),
        );
        if !marker.name.is_empty() {
            // The name sits on the ruler's top row, beside the flag, and stops before the
            // next marker's flag so two neighbouring names never overlap.
            let name_left = left + px(FLAG_W + 3.0);
            let limit = next_flag_x(&flag_xs, x).map_or(bounds.right(), |next| {
                (bounds.origin.x + px(next - NAME_GAP)).min(bounds.right())
            });
            let name_font = font(snapshot.theme.font_family.clone());
            let name = fit_name(&marker.name, limit - name_left, &name_font, window);
            paint_clip_label(
                name.into(),
                point(name_left, bounds.top() + px(1.0)),
                limit,
                name_font,
                window,
                cx,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::next_flag_x;

    #[core::prelude::v1::test]
    fn a_name_stops_at_the_next_flag_to_its_right() {
        let flags = [10.0, 40.0, 40.2, 90.0];
        assert_eq!(next_flag_x(&flags, 10.0), Some(40.0));
        // Flags a fraction of a pixel apart count as one: the next one is the first beyond.
        assert_eq!(next_flag_x(&flags, 40.0), Some(90.0));
        assert_eq!(next_flag_x(&flags, 90.0), None);
        assert_eq!(next_flag_x(&[], 0.0), None);
    }
}
