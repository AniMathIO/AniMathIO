//! End-to-end check with the real `tiny` model. Ignored by default so CI never downloads a
//! model and needs no whisper.cpp build. Run it with:
//!
//! ```text
//! cargo test -p animathio-transcribe --features whisper --test whisper_tiny -- --ignored --nocapture
//! ```
//!
//! The first run downloads and verifies `ggml-tiny.bin` (78 MB) into the model directory
//! (`ANIMATHIO_MODEL_DIR` overrides it). `data/speech.wav` was generated with
//! `espeak-ng -v en-us -s 140 -w raw.wav "Hello world. This is a test of automatic subtitles."`
//! and `ffmpeg -i raw.wav -ar 16000 -ac 1 speech.wav`.
#![cfg(feature = "whisper")]

use animathio_core::Time;
use animathio_transcribe::{
    Cancel, ModelSize, SplitOptions, TranscribeOptions, mono_16k_from_bytes, split_segments,
    transcribe,
};
use std::sync::{Arc, atomic::AtomicBool};

#[test]
#[ignore = "downloads the tiny Whisper model and runs inference"]
fn tiny_model_transcribes_generated_speech() {
    let cancel: Cancel = Arc::new(AtomicBool::new(false));
    animathio_transcribe::models::download(ModelSize::Tiny, &cancel, &mut |_| {}).unwrap();

    let samples = mono_16k_from_bytes(include_bytes!("data/speech.wav").to_vec()).unwrap();
    assert!(
        (samples.len() / 16_000).abs_diff(4) <= 1,
        "{}",
        samples.len()
    );
    let options = TranscribeOptions {
        model: ModelSize::Tiny,
        language: Some("en".into()),
        threads: None,
    };
    let transcript = transcribe(&samples, Time(0), &options, &cancel, Arc::new(|_| {})).unwrap();
    let text = transcript
        .segments
        .iter()
        .map(|s| s.text.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    println!("{transcript:#?}");
    // The tiny model mangles espeak speech ("the low world ... sub titles"), so match loosely.
    assert!(text.contains("world") && text.contains("sub"), "{text}");
    assert_eq!(transcript.language, "en");
    let cues = split_segments(&transcript.segments, &SplitOptions::default());
    assert!(
        cues.last().expect("at least one cue").end.0
            <= Time(5 * animathio_core::TICKS_PER_SECOND).0
    );
}

#[test]
#[ignore = "downloads the tiny Whisper model"]
fn cancelling_before_start_returns_cancelled() {
    let cancel: Cancel = Arc::new(AtomicBool::new(true));
    let samples = vec![0.1_f32; 16_000];
    let result = transcribe(
        &samples,
        Time(0),
        &TranscribeOptions::default(),
        &cancel,
        Arc::new(|_| {}),
    );
    assert!(matches!(
        result,
        Err(animathio_transcribe::Error::Cancelled)
    ));
}
