//! Timeline markers and beat/marker sync: adding, jumping, editing and dragging markers,
//! plus a panel that lists them and runs beat detection, "markers at beats" and cut sync.
//! Every edit is one `Command` (or one `Command::Batch`), so each is a single undo step.
use super::*;
use crate::style;
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
}
impl Default for Panel {
    fn default() -> Self {
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
        self.marker_panel = Some(Panel::default());
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
        window.open_dialog(cx, move |dialog, window, cx| {
            if !focused.replace(true) {
                let focus = name.read(cx).focus_handle(cx);
                window.focus(&focus, cx);
            }
            let (name_save, note_save, color_save, editor_save) =
                (name.clone(), note.clone(), color.clone(), editor.clone());
            let editor_delete = editor.clone();
            let current = *color.read(cx);
            let swatches = MarkerColor::ALL.into_iter().enumerate().map(|(i, option)| {
                let color = color.clone();
                div()
                    .id(("marker-color", i))
                    .size(px(20.))
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
            dialog
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
                        .child(Input::new(&name))
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
                                    let name = name_save.read(cx).value().trim().to_owned();
                                    let note = note_save.read(cx).value().trim().to_owned();
                                    let color = *color_save.read(cx);
                                    editor_save.update(cx, |this, cx| {
                                        if let Some(mut updated) = this.project.marker(id).cloned()
                                        {
                                            updated.name = name;
                                            updated.note = note;
                                            updated.color = color;
                                            if this.project.marker(id) != Some(&updated) {
                                                this.execute(
                                                    &Command::UpdateMarker {
                                                        id,
                                                        marker: updated,
                                                    },
                                                    cx,
                                                );
                                            }
                                        }
                                    });
                                    window.close_dialog(cx);
                                }),
                        ),
                )
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

    pub(super) fn marker_panel(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        let panel = self.marker_panel.as_ref()?;
        if self.is_start_screen() {
            return None;
        }
        let theme = cx.theme().clone();
        let rate = self.project.frame_rate();
        let selected_count = self.selected_ids().len();
        let mut root = div()
            .id("marker-panel")
            .absolute()
            .right(px(12.))
            .top(px(56.))
            .w(px(360.))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .rounded(px(style::radius::PANEL))
            .shadow_lg()
            .occlude()
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(icons::icon(IconName::Flag).small())
                    .child("Markers and beats"),
            );
        // Beat detection.
        let detected = panel.detection.as_ref();
        root = root.child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    Button::new("beats-scope")
                        .compact()
                        .disabled(self.selected.is_none())
                        .label(if panel.selected_only {
                            "Audio: selected clip"
                        } else {
                            "Audio: project mix"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(p) = this.marker_panel.as_mut() {
                                p.selected_only = !p.selected_only;
                                p.detection = None;
                            }
                            cx.notify();
                        })),
                )
                .child(
                    style::primary_button("beats-detect", cx)
                        .compact()
                        .label("Detect beats")
                        .disabled(panel.busy)
                        .on_click(cx.listener(|this, _, _, cx| this.detect_beats(cx))),
                ),
        );
        if panel.busy {
            root = root.child(div().text_xs().child("Analysing audio..."));
        }
        if let Some(error) = &panel.error {
            root = root.child(
                div()
                    .text_xs()
                    .text_color(theme.danger)
                    .child(error.clone()),
            );
        }
        if let Some(notice) = &panel.notice {
            root = root.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(notice.clone()),
            );
        }
        if let Some(d) = detected {
            root = root.child(div().text_xs().child(format!(
                "{:.1} BPM, {} beats, {:.0}% confidence",
                d.bpm,
                d.beats.len(),
                d.confidence * 100.
            )));
        }
        let every = panel.every;
        let downbeats = panel.downbeats_only;
        let mut subset = div()
            .flex()
            .flex_wrap()
            .gap_1()
            .items_center()
            .child(div().text_xs().child("Beats:"));
        for n in [1_u32, 2, 4] {
            subset = subset.child(
                Button::new(("beats-every", n as usize))
                    .compact()
                    .label(if n == 1 {
                        "Every".to_owned()
                    } else {
                        format!("Every {n}")
                    })
                    .selected(!downbeats && every == n)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(p) = this.marker_panel.as_mut() {
                            p.every = n;
                            p.downbeats_only = false;
                        }
                        cx.notify();
                    })),
            );
        }
        subset = subset.child(
            Button::new("beats-downbeats")
                .compact()
                .label("Downbeats")
                .selected(downbeats)
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(p) = this.marker_panel.as_mut() {
                        p.downbeats_only = !p.downbeats_only;
                    }
                    cx.notify();
                })),
        );
        root = root.child(subset).child(
            Button::new("beats-add-markers")
                .compact()
                .icon(icons::icon(IconName::Flag))
                .label("Add markers at beats")
                .disabled(detected.is_none())
                .on_click(cx.listener(|this, _, _, cx| this.add_beat_markers(cx))),
        );
        // Cut sync.
        let source_label = match panel.source {
            SyncSource::Beats => "Sync to: beats",
            SyncSource::Markers => "Sync to: markers",
            SyncSource::Both => "Sync to: beats + markers",
        };
        let max_shift = panel.max_shift_ms;
        root = root.child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    Button::new("sync-source")
                        .compact()
                        .label(source_label)
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(p) = this.marker_panel.as_mut() {
                                p.source = match p.source {
                                    SyncSource::Beats => SyncSource::Markers,
                                    SyncSource::Markers => SyncSource::Both,
                                    SyncSource::Both => SyncSource::Beats,
                                };
                            }
                            cx.notify();
                        })),
                )
                .child(
                    Button::new("sync-shift")
                        .compact()
                        .label(format!("Max shift: {max_shift} ms"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(p) = this.marker_panel.as_mut() {
                                p.max_shift_ms = match p.max_shift_ms {
                                    100 => 250,
                                    250 => 500,
                                    _ => 100,
                                };
                            }
                            cx.notify();
                        })),
                ),
        );
        root = root.child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .child(
                    Button::new("sync-snap")
                        .compact()
                        .icon(icons::icon(IconName::Magnet))
                        .label("Snap selected cuts")
                        .disabled(selected_count == 0)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.sync_selected(SyncMode::Snap, cx)),
                        ),
                )
                .child(
                    Button::new("sync-split")
                        .compact()
                        .icon(icons::icon(IconName::Scissors))
                        .label("Split selected at targets")
                        .disabled(selected_count == 0)
                        .on_click(
                            cx.listener(|this, _, _, cx| this.sync_selected(SyncMode::Split, cx)),
                        ),
                ),
        );
        // Marker list.
        let mut list = div()
            .id("marker-list")
            .max_h(px(180.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1();
        for (i, marker) in self.project.markers().iter().enumerate() {
            let (id, time) = (marker.id, marker.time);
            let is_selected = self.marker_selection.contains(&id);
            let label = if marker.name.is_empty() {
                timecode::format_timecode(marker.time, rate)
            } else {
                format!(
                    "{}  {}",
                    timecode::format_timecode(marker.time, rate),
                    marker.name
                )
            };
            list = list.child(
                div()
                    .flex()
                    .gap_1()
                    .items_center()
                    .rounded(px(3.))
                    .when(is_selected, |row| row.bg(theme.accent))
                    .child(
                        div()
                            .size(px(10.))
                            .flex_shrink_0()
                            .rounded_full()
                            .bg(paint_color(marker.color)),
                    )
                    .child(
                        Button::new(("marker-seek", i))
                            .ghost()
                            .compact()
                            .label(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.clear_selection();
                                this.marker_selection.insert(id);
                                this.seek_to(time, cx);
                            })),
                    )
                    .child(
                        Button::new(("marker-edit", i))
                            .ghost()
                            .compact()
                            .icon(icons::icon(IconName::Pencil))
                            .tooltip("Edit marker")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.open_marker_dialog(id, window, cx);
                            })),
                    )
                    .child(
                        Button::new(("marker-remove", i))
                            .ghost()
                            .compact()
                            .icon(icons::icon(IconName::Trash))
                            .tooltip("Remove marker")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.execute(&Command::RemoveMarker { id }, cx);
                            })),
                    ),
            );
        }
        root = root
            .child(
                div()
                    .text_xs()
                    .child(format!("{} markers", self.project.markers().len())),
            )
            .child(list)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        Button::new("marker-add")
                            .compact()
                            .icon(icons::icon(IconName::Plus))
                            .label("Add at playhead")
                            .on_click(
                                cx.listener(|this, _, _, cx| this.add_marker_at_playhead(cx)),
                            ),
                    )
                    .child(
                        Button::new("marker-panel-close")
                            .compact()
                            .label("Close")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.marker_panel = None;
                                cx.notify();
                            })),
                    ),
            );
        Some(root)
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
                    point(left, bounds.top() + px(RULER_H - 5.0)),
                    point(right.max(left + px(2.0)), bounds.top() + px(RULER_H)),
                ),
                marker.color.opacity(0.55),
            ));
        }
        window.paint_quad(fill(
            Bounds::new(point(left, bounds.top()), size(px(1.0), px(height))),
            marker.color.opacity(0.35),
        ));
        window.paint_quad(fill(
            Bounds::new(point(left, bounds.top()), size(px(1.5), px(RULER_H - 4.0))),
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
            // A backing plate keeps the name legible over the timecode labels.
            #[allow(clippy::cast_precision_loss)]
            let plate = marker.name.chars().count() as f32 * 6.2 + 6.0;
            window.paint_quad(
                fill(
                    Bounds::new(
                        point(left + px(FLAG_W + 1.0), bounds.top() + px(1.0)),
                        size(px(plate), px(14.0)),
                    ),
                    style::hex(style::color::BG).opacity(0.9),
                )
                .corner_radii(px(2.0)),
            );
            paint_clip_label(
                marker.name.clone().into(),
                point(left + px(FLAG_W + 3.0), bounds.top() + px(1.0)),
                bounds.right(),
                font(snapshot.theme.font_family.clone()),
                window,
                cx,
            );
        }
    }
}
