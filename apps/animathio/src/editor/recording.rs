//! "Record audio" panel in the Media library: pick a microphone, record with a live level
//! meter, and turn the take into a WAV asset.
//!
//! Capture lives outside the project model (`animathio_audio::Recorder`, opened through the
//! environment's `CaptureSource`). Stopping a take registers it exactly like an imported file:
//! the WAV bytes enter the asset store and one undoable `Command::Batch` adds the asset and,
//! when asked, an audio clip at the playhead position captured when Record was pressed. The
//! encode, hash and decode check run on a background task ("Saving recording…"); cancelling
//! or replacing the project while it runs discards the take. Cancelling drops the audio and
//! closes the device; the project is untouched until Stop.

use super::*;
use crate::style::{self, color, hex};
use animathio_audio::{InputDevice, Level, Recorder};
use gpui_component::select::{Select, SelectEvent, SelectState};
use gpui_component::switch::Switch;
use gpui_component::{Disableable, IndexPath};
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

/// The device menu entry that records from the system's default input.
const DEFAULT_DEVICE: &str = "System default";

/// The meter shows peaks from this level (dB relative to full scale) up to 0 dB.
const METER_FLOOR_DB: f32 = -60.0;

/// How far the displayed meter falls per 25 ms poll, so brief peaks stay readable.
const METER_FALL: f32 = 0.04;

/// Position of a peak on the meter, 0 (at or below -60 dB) to 1 (full scale).
pub(super) fn meter_fraction(peak: f32) -> f32 {
    if peak <= 0.0 {
        return 0.0;
    }
    ((20.0 * peak.log10() - METER_FLOOR_DB) / -METER_FLOOR_DB).clamp(0.0, 1.0)
}

/// A take whose audio is waiting for its background save: the WAV encode, hash and decode
/// check run off the interface thread, then the registering batch is applied here. Dropping
/// it (cancel, a new project, closing the editor) discards the take safely.
pub(super) struct SavingTake {
    /// Identifies this take, so a save landing after the panel was replaced (a new project, then
    /// a second take) can never register the wrong audio.
    token: u64,
    /// Length of the recording.
    duration: Time,
    /// Also place an audio clip at `position` when the batch applies.
    add_to_timeline: bool,
    /// Where the clip goes: the playhead position captured when Record was pressed.
    position: Time,
}

/// Source of [`SavingTake::token`]s.
static NEXT_SAVE_TOKEN: AtomicU64 = AtomicU64::new(1);

/// State of the open recorder panel.
pub(super) struct RecordPanel {
    pub(super) devices: Vec<InputDevice>,
    /// The chosen input's id, `None` for the system default.
    pub(super) device: Option<String>,
    select: Entity<SelectState<Vec<String>>>,
    /// The take in progress; `None` while idle.
    pub(super) session: Option<Recorder>,
    /// A finished take waiting for its off-thread save; `None` when nothing is pending.
    pub(super) saving: Option<SavingTake>,
    /// Frames the input dropped during this take (a full buffer while the app was busy).
    pub(super) dropped: u64,
    /// Where a new clip goes: the playhead captured when Record was pressed.
    pub(super) position: Time,
    pub(super) level: Level,
    /// The meter as drawn: the latest peak, falling slowly.
    pub(super) shown: f32,
    pub(super) elapsed: Time,
    /// Also place an audio clip at the playhead when the take stops.
    pub(super) add_to_timeline: bool,
    pub(super) listing: bool,
    /// The last problem (device error, permission), shown in the panel.
    pub(super) message: Option<String>,
    _subscription: Subscription,
}

impl RecordPanel {
    pub(super) fn is_recording(&self) -> bool {
        self.session.is_some()
    }

    /// A take is being captured or is waiting for its save: the panel stays open and the
    /// device choices stay frozen.
    pub(super) fn is_busy(&self) -> bool {
        self.session.is_some() || self.saving.is_some()
    }
}

/// The asset's file name: `Recording 1.wav`, `Recording 2.wav`, ... skipping names in use.
fn recording_name(project: &Project) -> String {
    let mut number = 1_u64;
    loop {
        let name = format!("Recording {number}.wav");
        if !project.assets().values().any(|asset| asset.name == name) {
            return name;
        }
        number += 1;
    }
}

/// One undoable batch registering a recorded asset and, with `placement` (clip id and start),
/// an audio clip of the recording's length on the first unlocked audio-only track that is free
/// there, or on a new "Audio" track. An empty unlocked track named "Audio" (the name the app
/// gives its audio tracks) counts as free; other empty tracks stay visual, since a video track
/// may be waiting for its first clip.
pub(super) fn recording_command(
    project: &Project,
    id: AssetId,
    name: String,
    duration: Time,
    placement: Option<(ClipId, Time)>,
) -> Result<Command, String> {
    let mut commands = Vec::new();
    if !project.assets().contains_key(&id) {
        commands.push(Command::AddAsset {
            id: id.clone(),
            asset: Asset {
                name,
                media_type: "audio/wav".into(),
            },
        });
    }
    if let Some((clip_id, start)) = placement {
        let end = start
            .0
            .checked_add(duration.0.max(1))
            .ok_or("the clip would end past the supported timeline")?;
        let range = TimeRange::new(start, Time(end)).map_err(|error| error.0)?;
        let free = |track: &Track| {
            let audio_only = track.name() == "Audio"
                || (!track.clips().is_empty()
                    && track
                        .clips()
                        .values()
                        .all(|clip| matches!(clip.kind, ClipKind::Audio(_))));
            !track.is_locked()
                && audio_only
                && !track
                    .clips()
                    .values()
                    .any(|clip| clip.range.start < range.end && range.start < clip.range.end)
        };
        let track = if let Some(track) = project.tracks().iter().find(|track| free(track)) {
            track.id()
        } else {
            let next = project
                .tracks()
                .iter()
                .map(|track| track.id().0)
                .max()
                .map_or(1, |max| max.saturating_add(1));
            commands.push(Command::AddTrack {
                track: Track::new(TrackId(next), "Audio"),
                index: project.tracks().len(),
            });
            TrackId(next)
        };
        let mut clip = Clip::new(clip_id, ClipKind::Audio(id), range);
        clip.params.insert("gain".into(), 1.0.into());
        clip.params.insert("muted".into(), 0.0.into());
        commands.push(Command::AddClip { track, clip });
    }
    Ok(Command::Batch { commands })
}

impl Editor {
    /// Open the recorder in the Media library and list the input devices.
    pub(super) fn open_recorder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dock.left_open = true;
        self.select_left_tab(LeftTab::Media, cx);
        if self.record_panel.is_none() {
            let select = cx.new(|cx| {
                SelectState::new(
                    vec![DEFAULT_DEVICE.to_owned()],
                    Some(IndexPath::default()),
                    window,
                    cx,
                )
            });
            let subscription =
                cx.subscribe(&select, |this, _, event: &SelectEvent<Vec<String>>, cx| {
                    if let SelectEvent::Confirm(Some(label)) = event {
                        let id = (label != DEFAULT_DEVICE).then(|| label.clone());
                        this.select_input_device(id, cx);
                    }
                });
            self.record_panel = Some(RecordPanel {
                devices: Vec::new(),
                device: None,
                select,
                session: None,
                saving: None,
                dropped: 0,
                position: Time(0),
                level: Level::default(),
                shown: 0.0,
                elapsed: Time(0),
                add_to_timeline: true,
                listing: false,
                message: None,
                _subscription: subscription,
            });
        }
        self.refresh_input_devices(window, cx);
        cx.notify();
    }

    /// Close the recorder. A take in progress or saving keeps the panel open: stop or cancel
    /// it first (and wait out the save).
    pub(super) fn close_recorder(&mut self, cx: &mut Context<Self>) {
        if self.record_panel.as_ref().is_some_and(RecordPanel::is_busy) {
            return;
        }
        self.record_panel = None;
        cx.notify();
    }

    /// Re-list the input devices on a worker (enumerating can take a moment on some systems).
    pub(super) fn refresh_input_devices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(panel) = self.record_panel.as_mut() else {
            return;
        };
        if panel.listing || panel.is_busy() {
            return;
        }
        panel.listing = true;
        let source = self.env.capture.clone();
        cx.spawn_in(window, async move |this, cx| {
            let listed = cx.background_spawn(async move { source.devices() }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                let Some(panel) = this.record_panel.as_mut() else {
                    return;
                };
                panel.listing = false;
                match listed {
                    Ok(devices) => {
                        // Keep the choice when it is still plugged in, else fall back to default.
                        if panel
                            .device
                            .as_ref()
                            .is_some_and(|id| !devices.iter().any(|device| &device.id == id))
                        {
                            panel.device = None;
                        }
                        let labels: Vec<String> = std::iter::once(DEFAULT_DEVICE.to_owned())
                            .chain(devices.iter().map(|device| device.id.clone()))
                            .collect();
                        let row = panel
                            .device
                            .as_ref()
                            .and_then(|id| labels.iter().position(|label| label == id))
                            .unwrap_or(0);
                        panel.devices = devices;
                        panel.select.update(cx, |select, cx| {
                            select.set_items(labels, window, cx);
                            select.set_selected_index(
                                Some(IndexPath::default().row(row)),
                                window,
                                cx,
                            );
                        });
                    }
                    Err(error) => panel.message = Some(error.to_string()),
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    /// Choose the input for the next take (`None` is the system default).
    pub(super) fn select_input_device(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if let Some(panel) = self.record_panel.as_mut()
            && !panel.is_recording()
        {
            panel.device = id;
            panel.message = None;
            cx.notify();
        }
    }

    /// Open the chosen input and start capturing. A failure (permission, busy or missing
    /// device, audio turned off) is shown in the panel and nothing is left open. The clip, if
    /// asked for, goes to the playhead position captured here, so moving the playhead during
    /// the take does not move it.
    pub(super) fn start_recording(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.record_panel.as_mut() else {
            return;
        };
        if panel.is_busy() {
            return;
        }
        panel.message = None;
        match Recorder::start(&*self.env.capture, panel.device.as_deref()) {
            Ok(recorder) => {
                panel.level = Level::default();
                panel.shown = 0.0;
                panel.elapsed = Time(0);
                panel.dropped = 0;
                panel.position = self.position;
                panel.session = Some(recorder);
            }
            Err(error) => panel.message = Some(error.to_string()),
        }
        cx.notify();
    }

    /// Poll the capture stream (called from the editor's 25 ms tick). Updates the meter and the
    /// elapsed time; a failed device or a full recording ends the take. Returns whether the view
    /// needs a redraw.
    pub(super) fn pump_recording(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(panel) = self.record_panel.as_mut() else {
            return false;
        };
        let Some(recorder) = panel.session.as_mut() else {
            return false;
        };
        let polled = recorder.poll();
        panel.level = recorder.level();
        panel.elapsed = recorder.duration();
        panel.shown = meter_fraction(panel.level.peak).max(panel.shown - METER_FALL);
        panel.dropped = recorder.dropped_frames();
        let full = recorder.is_full();
        match polled {
            Err(error) => {
                // Keep what the device delivered before it failed.
                self.finish_recording(Some(format!("The microphone stopped: {error}")), cx);
            }
            Ok(_) if full => {
                self.finish_recording(
                    Some("Reached the maximum recording length, so the take was stopped.".into()),
                    cx,
                );
            }
            Ok(_) => {}
        }
        true
    }

    /// Stop the take and register it (see [`Editor::finish_recording`]).
    pub(super) fn stop_recording(&mut self, cx: &mut Context<Self>) {
        self.finish_recording(None, cx);
    }

    /// Cancel the take: the audio is discarded, the device is closed and the project is
    /// untouched.
    pub(super) fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        let Some(panel) = self.record_panel.as_mut() else {
            return;
        };
        if let Some(recorder) = panel.session.take() {
            recorder.cancel();
            panel.level = Level::default();
            panel.shown = 0.0;
            panel.elapsed = Time(0);
            panel.dropped = 0;
            self.notices
                .push(crate::style::Toast::info("Recording discarded."));
            cx.notify();
        }
    }

    /// End the take: the device is closed at once and the WAV encode, hash and decode check
    /// move to a background task, so a long take never freezes the interface. The card shows
    /// "Saving recording…" until the task registers the asset and the clip (see
    /// [`Editor::finish_saving`]). `notice` is shown when the take ended by itself.
    fn finish_recording(&mut self, notice: Option<String>, cx: &mut Context<Self>) {
        let Some(panel) = self.record_panel.as_mut() else {
            return;
        };
        let Some(recorder) = panel.session.take() else {
            return;
        };
        panel.level = Level::default();
        panel.shown = 0.0;
        panel.elapsed = Time(0);
        panel.message = notice;
        let recording = match recorder.finish() {
            Ok(recording) => recording,
            Err(error) => {
                let message = match panel.message.take() {
                    Some(notice) => format!("{notice} {error}"),
                    None => error.to_string(),
                };
                panel.message = Some(message);
                cx.notify();
                return;
            }
        };
        panel.dropped = recording.dropped_frames;
        let token = NEXT_SAVE_TOKEN.fetch_add(1, AtomicOrdering::Relaxed);
        panel.saving = Some(SavingTake {
            token,
            duration: recording.duration(),
            add_to_timeline: panel.add_to_timeline,
            position: panel.position,
        });
        cx.spawn(async move |this, cx| {
            // Off the interface thread: `into_wav` copies the whole take, the hash reads it
            // all and the decode check opens it — seconds of work at the size cap.
            let prepared = cx
                .background_spawn(async move {
                    let wav = recording.into_wav();
                    let id = AssetId::of(&wav);
                    let source = SymphoniaSource::from_bytes(wav.clone()).ok();
                    (id, wav, source)
                })
                .await;
            let _ = this.update(cx, |this, cx| this.finish_saving(token, prepared, cx));
        })
        .detach();
        cx.notify();
    }

    /// Register a finished take whose background save landed: insert the WAV bytes into the
    /// asset store exactly like an imported file and execute one undoable `Command::Batch`
    /// (`AddAsset`, plus `AddTrack` and `AddClip` where the take started), so a single undo
    /// removes both. Cancelling, a new project or closing the editor dropped the panel and
    /// with it the take, so a save landing after that is discarded here.
    fn finish_saving(
        &mut self,
        token: u64,
        prepared: (AssetId, Vec<u8>, Option<SymphoniaSource>),
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.record_panel.as_mut() else {
            return;
        };
        if panel.saving.as_ref().map(|saving| saving.token) != Some(token) {
            // The panel this take belonged to is gone (a new project replaced it).
            return;
        }
        let saving = panel.saving.take().expect("checked above");
        let (id, wav, source) = prepared;
        let SavingTake {
            token: _,
            duration,
            add_to_timeline,
            position,
        } = saving;
        let placement = add_to_timeline.then(|| (self.next_clip_id(), position));
        let command = match recording_command(
            &self.project,
            id.clone(),
            recording_name(&self.project),
            duration,
            placement,
        ) {
            Ok(command) => command,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        let known = self.assets.contains_key(&id);
        if !known {
            if let Some(source) = &source {
                let _ = self.pool.insert(id.clone(), source);
            }
            Arc::make_mut(&mut self.assets).insert(id.clone(), wav);
        }
        if self.try_execute(&command, cx) {
            self.worker.load(self.assets.clone());
            self.notices
                .push(crate::style::Toast::info(if add_to_timeline {
                    "Recording added to the library and the timeline."
                } else {
                    "Recording added to the library. Click it or drag it to the timeline."
                }));
        } else if !known {
            Arc::make_mut(&mut self.assets).remove(&id);
            // The decode went into the pool for a clip that never existed: prune it.
            self.pool.collect(&self.project);
        }
        cx.notify();
    }

    /// Whether a take is being captured or its save is still pending: closing the editor asks
    /// instead of dropping the work silently.
    pub(super) fn take_in_progress(&self) -> bool {
        self.record_panel.as_ref().is_some_and(RecordPanel::is_busy)
    }

    /// The recorder card shown at the top of the Media library.
    pub(super) fn record_panel_view(&self, cx: &mut Context<Self>) -> Option<Div> {
        let panel = self.record_panel.as_ref()?;
        let recording = panel.is_recording();
        let saving = panel.saving.is_some();
        let disabled_reason = self.env.capture.disabled_reason().map(str::to_owned);
        let timecode = timecode::format_timecode(panel.elapsed, self.project.frame_rate());
        let db = panel.level.peak_db();
        let meter_color = if panel.level.clipped() {
            color::DANGER
        } else {
            color::CLIP_AUDIO
        };
        let status = if let Some(reason) = &disabled_reason {
            Some((reason.clone(), true))
        } else if let Some(message) = &panel.message {
            Some((message.clone(), true))
        } else if saving {
            Some(("Saving recording\u{2026}".to_owned(), false))
        } else if !panel.listing && panel.devices.is_empty() && !recording {
            Some((
                "No microphone was found. Plug one in and refresh the list.".to_owned(),
                false,
            ))
        } else {
            None
        };
        Some(
            div()
                .debug_selector(|| "record-panel".into())
                .m_1()
                .p_2()
                .flex()
                .flex_col()
                .gap_2()
                .rounded(px(6.))
                .border_1()
                .border_color(hex(color::BORDER))
                .bg(hex(color::RAISED))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(icons::icon(IconName::Mic).small())
                        .child(
                            div()
                                .flex_1()
                                .text_size(style::type_size::SM)
                                .font_weight(FontWeight::SEMIBOLD)
                                .child("Record audio"),
                        )
                        .child(
                            icon_button(
                                "record-close",
                                icons::icon(IconName::X),
                                "Close the recorder",
                                cx.listener(|this, _, _, cx| this.close_recorder(cx)),
                            )
                            .disabled(recording || saving),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .debug_selector(|| "record-device".into())
                                .flex_1()
                                .min_w_0()
                                .child(
                                    Select::new(&panel.select)
                                        .small()
                                        .placeholder("Input device")
                                        .disabled(recording || saving || disabled_reason.is_some()),
                                ),
                        )
                        .child(
                            icon_button(
                                "record-refresh",
                                icons::icon(IconName::RefreshCw),
                                "Refresh the input list",
                                cx.listener(|this, _, window, cx| {
                                    this.refresh_input_devices(window, cx);
                                }),
                            )
                            .disabled(recording || saving || disabled_reason.is_some()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            div()
                                .debug_selector(|| "record-meter".into())
                                .flex_1()
                                .h(px(8.))
                                .rounded(px(4.))
                                .bg(hex(color::BG))
                                .overflow_hidden()
                                .child(
                                    div().h_full().w(relative(panel.shown)).bg(hex(meter_color)),
                                ),
                        )
                        .child(
                            div()
                                .w(px(52.))
                                .text_size(style::type_size::XS)
                                .text_color(hex(color::TEXT_SECONDARY))
                                .child(if recording && db.is_finite() {
                                    format!("{db:.0} dB")
                                } else {
                                    "-- dB".to_owned()
                                }),
                        ),
                )
                .child(
                    div()
                        .debug_selector(|| "record-time".into())
                        .text_size(style::type_size::SM)
                        .text_color(hex(if recording {
                            color::TEXT
                        } else {
                            color::TEXT_SECONDARY
                        }))
                        .child(timecode),
                )
                .child(
                    div().debug_selector(|| "record-add".into()).child(
                        Switch::new("record-add-to-timeline")
                            .label("Add to timeline at the record position")
                            .checked(panel.add_to_timeline)
                            .disabled(recording)
                            .on_click(cx.listener(|this, checked, _, cx| {
                                if let Some(panel) = this.record_panel.as_mut() {
                                    panel.add_to_timeline = *checked;
                                    cx.notify();
                                }
                            })),
                    ),
                )
                .when_some(status, |card, (text, problem)| {
                    card.child(
                        div()
                            .debug_selector(|| "record-status".into())
                            .text_size(style::type_size::XS)
                            .text_color(hex(if problem {
                                color::DANGER
                            } else {
                                color::TEXT_SECONDARY
                            }))
                            .child(text),
                    )
                })
                .when(panel.dropped > 0, |card| {
                    card.child(
                        div()
                            .debug_selector(|| "record-dropped".into())
                            .text_size(style::type_size::XS)
                            .text_color(hex(color::WARNING))
                            .child("Some audio was lost because the app was busy, so the recording has gaps."),
                    )
                })
                .child(
                    div()
                        .flex()
                        .gap_1()
                        .when(!recording && !saving, |row| {
                            row.child(
                                Button::new("record-start")
                                    .primary()
                                    .compact()
                                    .icon(icons::icon(IconName::Mic))
                                    .label("Record")
                                    .disabled(disabled_reason.is_some())
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.start_recording(cx);
                                    })),
                            )
                        })
                        .when(recording, |row| {
                            row.child(
                                Button::new("record-stop")
                                    .primary()
                                    .compact()
                                    .icon(icons::icon(IconName::Square))
                                    .label("Stop")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.stop_recording(cx);
                                    })),
                            )
                            .child(
                                Button::new("record-cancel")
                                    .compact()
                                    .icon(icons::icon(IconName::X))
                                    .label("Cancel")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.cancel_recording(cx);
                                    })),
                            )
                        }),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_audio::{Recorder, SymphoniaSource, SyntheticCapture, SyntheticDevice};
    use animathio_core::{History, TICKS_PER_SECOND};

    fn base_project() -> Project {
        let mut project = Project::default();
        Command::AddTrack {
            track: Track::new(TrackId(1), "Video 1"),
            index: 0,
        }
        .apply(&mut project)
        .unwrap();
        project
    }

    fn take(seconds: f32) -> (Vec<u8>, Time) {
        let capture = SyntheticCapture::new(vec![SyntheticDevice::tone(
            "Mic", 48_000, 1, seconds, 0.5, 4800,
        )]);
        let mut recorder = Recorder::start(&capture, None).unwrap();
        for _ in 0..(seconds * 10.0) as usize {
            recorder.poll().unwrap();
        }
        let recording = recorder.finish().unwrap();
        let duration = recording.duration();
        (recording.into_wav(), duration)
    }

    #[core::prelude::v1::test]
    fn meter_maps_decibels_onto_the_bar() {
        assert_eq!(meter_fraction(0.0), 0.0);
        assert_eq!(meter_fraction(0.0005), 0.0, "below -60 dB is empty");
        assert!((meter_fraction(1.0) - 1.0).abs() < 1e-6);
        assert!((meter_fraction(0.5) - (1.0 - 6.0206 / 60.0)).abs() < 1e-3);
        assert_eq!(meter_fraction(4.0), 1.0, "over full scale is clamped");
    }

    #[core::prelude::v1::test]
    fn a_recording_becomes_one_undoable_asset_and_clip() {
        let mut project = base_project();
        let before = project.clone();
        let (wav, duration) = take(2.0);
        let id = AssetId::of(&wav);
        assert_eq!(duration, Time(2 * TICKS_PER_SECOND));
        let source = SymphoniaSource::from_bytes(wav).unwrap();
        assert_eq!((source.info().rate, source.info().frames), (48_000, 96_000));

        let command = recording_command(
            &project,
            id.clone(),
            "Recording 1.wav".into(),
            duration,
            Some((ClipId(7), Time(TICKS_PER_SECOND))),
        )
        .unwrap();
        let Command::Batch { commands } = &command else {
            panic!("one batch");
        };
        assert!(matches!(commands[0], Command::AddAsset { .. }));
        assert!(matches!(commands[1], Command::AddTrack { .. }));
        assert!(matches!(commands[2], Command::AddClip { .. }));
        let mut history = History::default();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(project.assets()[&id].media_type, "audio/wav");
        assert_eq!(project.assets()[&id].name, "Recording 1.wav");
        let audio = project
            .tracks()
            .iter()
            .find(|t| t.name() == "Audio")
            .unwrap();
        let clip = &audio.clips()[&ClipId(7)];
        assert_eq!(clip.kind, ClipKind::Audio(id.clone()));
        assert_eq!(clip.range.start, Time(TICKS_PER_SECOND));
        assert_eq!(clip.range.end, Time(3 * TICKS_PER_SECOND));
        assert_eq!(clip.params["gain"], 1.0.into());
        assert_eq!(clip.params["muted"], 0.0.into());

        // One undo removes the clip, its track and the asset; redo restores them.
        let after = project.clone();
        assert!(history.undo(&mut project).unwrap());
        assert_eq!(project, before);
        assert!(history.redo(&mut project).unwrap());
        assert_eq!(project, after);
    }

    #[core::prelude::v1::test]
    fn a_library_only_recording_adds_just_the_asset() {
        let mut project = base_project();
        let (wav, duration) = take(0.5);
        let id = AssetId::of(&wav);
        let command = recording_command(
            &project,
            id.clone(),
            "Recording 1.wav".into(),
            duration,
            None,
        )
        .unwrap();
        let mut history = History::default();
        history.execute(&mut project, &command).unwrap();
        assert!(project.assets().contains_key(&id));
        assert!(project.tracks().iter().all(|t| t.clips().is_empty()));
        history.undo(&mut project).unwrap();
        assert!(project.assets().is_empty());
    }

    #[core::prelude::v1::test]
    fn the_clip_reuses_a_free_audio_track_and_avoids_an_occupied_one() {
        let (wav, duration) = take(1.0);
        let id = AssetId::of(&wav);
        let other = AssetId::of(b"music");
        let mut project = base_project();
        for command in [
            Command::AddAsset {
                id: other.clone(),
                asset: Asset {
                    name: "music.wav".into(),
                    media_type: "audio/wav".into(),
                },
            },
            Command::AddTrack {
                track: Track::new(TrackId(2), "Music"),
                index: 1,
            },
            Command::AddClip {
                track: TrackId(2),
                clip: Clip::new(
                    ClipId(1),
                    ClipKind::Audio(other),
                    TimeRange::new(Time(0), Time(2 * TICKS_PER_SECOND)).unwrap(),
                ),
            },
        ] {
            command.apply(&mut project).unwrap();
        }
        // Free after the music clip: lands on the existing audio track.
        let free = recording_command(
            &project,
            id.clone(),
            "r".into(),
            duration,
            Some((ClipId(2), Time(2 * TICKS_PER_SECOND))),
        )
        .unwrap();
        free.apply(&mut project.clone()).unwrap();
        let Command::Batch { commands } = &free else {
            panic!()
        };
        assert!(
            matches!(&commands[1], Command::AddClip { track, .. } if *track == TrackId(2)),
            "{commands:?}"
        );
        // Overlapping the music clip: a new audio track instead.
        let busy = recording_command(
            &project,
            id,
            "r".into(),
            duration,
            Some((ClipId(2), Time(TICKS_PER_SECOND))),
        )
        .unwrap();
        let Command::Batch { commands } = &busy else {
            panic!()
        };
        assert!(
            matches!(&commands[1], Command::AddTrack { track, .. } if track.id() == TrackId(3))
        );
    }

    #[core::prelude::v1::test]
    fn an_empty_audio_track_is_reused_but_other_empty_tracks_are_not() {
        let (wav, duration) = take(0.5);
        let id = AssetId::of(&wav);
        let mut project = base_project();
        Command::AddTrack {
            track: Track::new(TrackId(2), "Audio"),
            index: 1,
        }
        .apply(&mut project)
        .unwrap();
        let command = recording_command(
            &project,
            id,
            "r".into(),
            duration,
            Some((ClipId(1), Time(0))),
        )
        .unwrap();
        let Command::Batch { commands } = &command else {
            panic!()
        };
        assert!(matches!(commands[0], Command::AddAsset { .. }));
        assert!(
            matches!(&commands[1], Command::AddClip { track, .. } if *track == TrackId(2)),
            "the user's empty Audio track takes the clip: {commands:?}"
        );
        assert_eq!(commands.len(), 2, "no track is added");

        // A renamed (or video) empty track stays visual: a new "Audio" track instead.
        Command::RemoveTrack { id: TrackId(2) }
            .apply(&mut project)
            .unwrap();
        Command::AddTrack {
            track: Track::new(TrackId(3), "Voiceover"),
            index: 1,
        }
        .apply(&mut project)
        .unwrap();
        let command = recording_command(
            &project,
            AssetId::of(b"other"),
            "r".into(),
            duration,
            Some((ClipId(2), Time(0))),
        )
        .unwrap();
        let Command::Batch { commands } = &command else {
            panic!()
        };
        assert!(
            matches!(&commands[1], Command::AddTrack { track, .. } if track.name() == "Audio"),
            "{commands:?}"
        );
    }

    #[core::prelude::v1::test]
    fn recording_names_skip_names_in_use() {
        let mut project = Project::default();
        assert_eq!(recording_name(&project), "Recording 1.wav");
        Command::AddAsset {
            id: AssetId::of(b"a"),
            asset: Asset {
                name: "Recording 1.wav".into(),
                media_type: "audio/wav".into(),
            },
        }
        .apply(&mut project)
        .unwrap();
        assert_eq!(recording_name(&project), "Recording 2.wav");
    }
}
