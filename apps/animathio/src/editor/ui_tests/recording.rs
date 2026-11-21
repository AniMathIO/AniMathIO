//! Microphone recording flow: a scripted capture source stands in for the sound card, so the
//! panel, the level meter, Stop, Cancel and undo all run without a device.

use std::sync::Arc;
use std::time::Duration;

use animathio_audio::{SymphoniaSource, SyntheticCapture, SyntheticDevice};
use animathio_core::{ClipKind, TICKS_PER_SECOND};
use gpui::TestAppContext;

use super::fixtures::*;
use super::{Editor, Time, Ui, actions, test_env};

const TICK: Duration = Duration::from_millis(25);

fn env_with(capture: &SyntheticCapture) -> crate::editor_env::EditorEnv {
    let mut env = test_env();
    env.capture = Arc::new(capture.clone());
    env
}

fn desk_and_usb() -> SyntheticCapture {
    SyntheticCapture::new(vec![
        // 2 s of a half-scale tone, 0.1 s per editor tick.
        SyntheticDevice::tone("Desk mic", 48_000, 1, 2.0, 0.5, 4800),
        SyntheticDevice::tone("USB mic", 44_100, 2, 2.0, 0.25, 4410),
        SyntheticDevice::tone("Blocked mic", 48_000, 1, 1.0, 0.5, 4800).failing_open(
            "Microphone access was denied. Allow AniMathIO in your privacy settings.",
        ),
    ])
}

fn open_recorder(cx: &mut TestAppContext, capture: &SyntheticCapture) -> Ui {
    let (project, assets) = sample();
    let mut ui = Ui::open_with_env(cx, project, assets, env_with(capture));
    ui.dispatch(actions::RecordAudio);
    ui.settle();
    ui
}

fn wav_assets(editor: &Editor) -> usize {
    editor
        .project
        .assets()
        .values()
        .filter(|asset| asset.media_type == "audio/wav")
        .count()
}

fn audio_clips(editor: &Editor) -> Vec<(animathio_core::ClipId, Time, Time)> {
    editor
        .project
        .tracks()
        .iter()
        .flat_map(|track| track.clips().values())
        .filter(|clip| matches!(clip.kind, ClipKind::Audio(_)))
        .map(|clip| (clip.id, clip.range.start, clip.range.end))
        .collect()
}

#[gpui::test]
fn record_meter_stop_adds_one_undoable_asset_and_clip(cx: &mut TestAppContext) {
    let capture = desk_and_usb();
    let mut ui = open_recorder(cx, &capture);
    ui.read(|e| {
        let panel = e.record_panel.as_ref().expect("the recorder is open");
        assert_eq!(
            panel
                .devices
                .iter()
                .map(|d| d.name.as_str())
                .collect::<Vec<_>>(),
            ["Desk mic", "USB mic", "Blocked mic"],
            "devices are listed for explicit selection"
        );
        assert!(!panel.is_recording() && panel.message.is_none());
    });
    assert!(
        ui.cx.debug_bounds("record-panel").is_some(),
        "the card is drawn"
    );
    assert_eq!(capture.opened_streams(), 0, "listing opens no device");

    let before = ui.read(|e| e.project.clone());
    let assets_before = ui.read(|e| e.assets.len());
    ui.update(|e, _, cx| e.seek_to(Time(TICKS_PER_SECOND), cx));
    ui.update(|e, _, cx| e.start_recording(cx));
    assert_eq!(capture.live_streams(), 1);

    // The meter follows the signal: nothing before the first tick, then the 0.5 peak.
    ui.read(|e| assert_eq!(e.record_panel.as_ref().unwrap().shown, 0.0));
    ui.advance_clock(TICK);
    let (peak, shown, elapsed) = ui.read(|e| {
        let panel = e.record_panel.as_ref().unwrap();
        (panel.level.peak, panel.shown, panel.elapsed)
    });
    assert!((peak - 0.5).abs() < 0.01, "peak {peak}");
    assert!(
        shown > 0.8 && shown < 1.0,
        "-6 dB sits near the top: {shown}"
    );
    assert!(elapsed.0 > 0);
    assert!(ui.cx.debug_bounds("record-meter").is_some());
    for _ in 0..4 {
        ui.advance_clock(TICK);
    }
    let elapsed_later = ui.read(|e| e.record_panel.as_ref().unwrap().elapsed);
    assert!(
        elapsed_later > elapsed,
        "elapsed time grows while recording"
    );
    // Nothing touches the project until Stop.
    ui.read(|e| assert_eq!(e.project, before));

    ui.update(|e, _, cx| e.stop_recording(cx));
    assert_eq!(capture.live_streams(), 0, "Stop closes the device");
    ui.read(|e| {
        assert_eq!(e.assets.len(), assets_before + 1);
        let (id, asset) = e
            .project
            .assets()
            .iter()
            .find(|(_, asset)| asset.media_type == "audio/wav")
            .expect("the recording is registered as a WAV asset");
        assert_eq!(asset.name, "Recording 1.wav");
        let source = SymphoniaSource::from_bytes(e.assets[id].clone()).unwrap();
        assert_eq!(source.info().rate, 48_000);
        let clips = audio_clips(e);
        assert_eq!(clips.len(), 1, "one clip at the playhead");
        assert_eq!(clips[0].1, Time(TICKS_PER_SECOND));
        assert!(e.dirty && !e.record_panel.as_ref().unwrap().is_recording());
    });

    // One undo removes the asset and the clip together; redo brings both back.
    let after = ui.read(|e| e.project.clone());
    ui.dispatch(actions::Undo);
    ui.read(|e| assert_eq!(e.project, before));
    ui.dispatch(actions::Redo);
    ui.read(|e| assert_eq!(e.project, after));
}

#[gpui::test]
fn cancel_discards_the_take_and_closes_the_device(cx: &mut TestAppContext) {
    let capture = desk_and_usb();
    let mut ui = open_recorder(cx, &capture);
    let before = ui.read(|e| e.project.clone());
    let assets_before = ui.read(|e| e.assets.len());
    ui.update(|e, _, cx| e.start_recording(cx));
    for _ in 0..3 {
        ui.advance_clock(TICK);
    }
    ui.read(|e| assert!(e.record_panel.as_ref().unwrap().elapsed.0 > 0));
    ui.update(|e, _, cx| e.cancel_recording(cx));
    assert_eq!(capture.live_streams(), 0);
    ui.read(|e| {
        let panel = e.record_panel.as_ref().unwrap();
        assert!(!panel.is_recording() && panel.elapsed.0 == 0 && panel.shown == 0.0);
        assert_eq!(e.project, before, "the project is untouched");
        assert_eq!(e.assets.len(), assets_before, "no audio was kept");
        assert!(!e.dirty);
    });
    // Further ticks have nothing to poll.
    ui.advance_clock(TICK);
    ui.read(|e| assert_eq!(e.project, before));
}

#[gpui::test]
fn the_library_only_option_registers_just_the_asset(cx: &mut TestAppContext) {
    let capture = desk_and_usb();
    let mut ui = open_recorder(cx, &capture);
    ui.update(|e, _, _| e.record_panel.as_mut().unwrap().add_to_timeline = false);
    let clips_before = ui.read(|e| audio_clips(e).len());
    ui.update(|e, _, cx| e.start_recording(cx));
    ui.advance_clock(TICK);
    ui.update(|e, _, cx| e.stop_recording(cx));
    ui.read(|e| {
        assert_eq!(wav_assets(e), 1);
        assert_eq!(audio_clips(e).len(), clips_before);
    });
    ui.dispatch(actions::Undo);
    ui.read(|e| assert_eq!(wav_assets(e), 0));
}

#[gpui::test]
fn the_chosen_device_is_the_one_recorded(cx: &mut TestAppContext) {
    let capture = desk_and_usb();
    let mut ui = open_recorder(cx, &capture);
    ui.update(|e, _, cx| e.select_input_device(Some("USB mic".into()), cx));
    ui.update(|e, _, cx| e.start_recording(cx));
    ui.advance_clock(TICK);
    ui.update(|e, _, cx| e.stop_recording(cx));
    ui.read(|e| {
        let (id, _) = e
            .project
            .assets()
            .iter()
            .find(|(_, asset)| asset.media_type == "audio/wav")
            .expect("the recorded asset");
        let source = SymphoniaSource::from_bytes(e.assets[id].clone()).unwrap();
        let info = source.info();
        assert_eq!((info.rate, info.channels), (44_100, 2));
    });
}

#[gpui::test]
fn a_permission_error_is_explained_and_nothing_stays_open(cx: &mut TestAppContext) {
    let capture = desk_and_usb();
    let mut ui = open_recorder(cx, &capture);
    ui.update(|e, _, cx| e.select_input_device(Some("Blocked mic".into()), cx));
    ui.update(|e, _, cx| e.start_recording(cx));
    ui.read(|e| {
        let panel = e.record_panel.as_ref().unwrap();
        assert!(!panel.is_recording());
        assert!(
            panel.message.as_deref().unwrap().contains("denied"),
            "{:?}",
            panel.message
        );
        assert_eq!(wav_assets(e), 0);
    });
    assert_eq!(capture.live_streams(), 0);
    assert!(
        ui.cx.debug_bounds("record-status").is_some(),
        "the message is drawn"
    );
}

#[gpui::test]
fn a_device_that_stops_mid_take_keeps_the_audio_it_delivered(cx: &mut TestAppContext) {
    let capture = SyntheticCapture::new(vec![
        SyntheticDevice::tone("Flaky mic", 48_000, 1, 2.0, 0.5, 4800)
            .failing_after(2, "the device was unplugged"),
    ]);
    let mut ui = open_recorder(cx, &capture);
    ui.update(|e, _, cx| e.start_recording(cx));
    for _ in 0..4 {
        ui.advance_clock(TICK);
    }
    ui.read(|e| {
        let panel = e.record_panel.as_ref().unwrap();
        assert!(!panel.is_recording(), "the failed take ended itself");
        assert!(panel.message.as_deref().unwrap().contains("unplugged"));
        assert_eq!(wav_assets(e), 1, "what was captured is kept");
        assert_eq!(audio_clips(e).len(), 1);
    });
    assert_eq!(capture.live_streams(), 0);
}

#[gpui::test]
fn starting_a_new_project_cancels_the_take(cx: &mut TestAppContext) {
    let capture = desk_and_usb();
    let mut ui = open_recorder(cx, &capture);
    ui.update(|e, _, cx| e.start_recording(cx));
    ui.advance_clock(TICK);
    assert_eq!(capture.live_streams(), 1);
    ui.update(|e, _, cx| e.new_project(cx));
    assert_eq!(
        capture.live_streams(),
        0,
        "the device closed with the project"
    );
    ui.read(|e| {
        assert!(e.record_panel.is_none());
        assert_eq!(wav_assets(e), 0);
    });
}

#[gpui::test]
fn the_headless_environment_never_opens_an_input_device(cx: &mut TestAppContext) {
    // The default test environment uses the null capture source, like ANIMATHIO_AUDIO=none.
    let (project, assets) = sample();
    let mut ui = Ui::open(cx, project, assets);
    ui.dispatch(actions::RecordAudio);
    ui.settle();
    ui.update(|e, _, cx| e.start_recording(cx));
    ui.read(|e| {
        let panel = e.record_panel.as_ref().expect("the panel still opens");
        assert!(panel.devices.is_empty() && !panel.is_recording());
        assert!(
            panel.message.as_deref().unwrap().contains("turned off"),
            "{:?}",
            panel.message
        );
        assert!(
            e.env.capture.disabled_reason().is_some(),
            "the source says it is disabled so the Record button is too"
        );
    });
    assert!(ui.cx.debug_bounds("record-status").is_some());
}

#[gpui::test]
fn closing_the_recorder_waits_for_a_take_in_progress(cx: &mut TestAppContext) {
    let capture = desk_and_usb();
    let mut ui = open_recorder(cx, &capture);
    ui.update(|e, _, cx| e.start_recording(cx));
    ui.update(|e, _, cx| e.close_recorder(cx));
    ui.read(|e| assert!(e.record_panel.is_some(), "a live take keeps the panel open"));
    ui.update(|e, _, cx| e.cancel_recording(cx));
    ui.update(|e, _, cx| e.close_recorder(cx));
    ui.read(|e| assert!(e.record_panel.is_none()));
}
