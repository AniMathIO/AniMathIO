//! Quiet three-second device-clock measurement. No device is needed by normal tests.
#![allow(clippy::cast_precision_loss)] // Tone amplitudes and diagnostic seconds only.
use animathio_audio::{AudioPool, Pcm, Player};
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, Project, TICKS_PER_SECOND, Time, TimeRange,
    Track, TrackId,
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (rate, channels) = Player::default_output_format()?;
    let pcm = Pcm::new(
        rate,
        1,
        (0..rate * 3)
            .map(|i| (i as f32 * std::f32::consts::TAU * 440.0 / rate as f32).sin() * 0.01)
            .collect(),
    )?;
    let id = AssetId::of(b"generated quiet 440 Hz tone");
    let mut project = Project::default();
    Command::AddAsset {
        id: id.clone(),
        asset: Asset {
            name: "tone".into(),
            media_type: "audio/pcm".into(),
        },
    }
    .apply(&mut project)?;
    Command::AddTrack {
        track: Track::new(TrackId(0), "tone"),
        index: 0,
    }
    .apply(&mut project)?;
    Command::AddClip {
        track: TrackId(0),
        clip: Clip::new(
            ClipId(0),
            ClipKind::Audio(id.clone()),
            TimeRange::new(Time(0), Time(3 * TICKS_PER_SECOND))?,
        ),
    }
    .apply(&mut project)?;
    let mut pool = AudioPool::default();
    pool.insert(id, &pcm)?;
    let mut player = Player::new(Arc::new(project), Arc::new(pool.prepare(rate, channels)?))?;
    let start = Instant::now();
    player.play()?;
    let limit = Duration::from_secs(3);
    while start.elapsed() < limit && !player.has_error() {
        std::thread::sleep(Duration::from_millis(10).min(limit.saturating_sub(start.elapsed())));
    }
    let wall = start.elapsed();
    let clock = player.now();
    player.pause()?;
    let media_seconds = clock.0 as f64 / TICKS_PER_SECOND as f64;
    println!(
        "{rate} Hz, {channels} channels: wall={:.6}s audio={media_seconds:.6}s drift={:+.3}ms (callback-quantized, latency-corrected)",
        wall.as_secs_f64(),
        (media_seconds - wall.as_secs_f64()) * 1000.0
    );
    Ok(())
}
