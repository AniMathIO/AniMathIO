//! Projects the UI tests open: the shared sample, a voice + music mix and a captioned clip.

use animathio_control::{CaptionStyleParams, subtitle_command};
use animathio_core::{
    Asset, Clip, ClipId, ClipKind, Command, Project, TICKS_PER_SECOND, Time, TimeRange, Track,
    TrackId,
};
use animathio_transcribe::Cue;

use super::*;

pub(super) const MS: i64 = TICKS_PER_SECOND / 1000;

/// The CLI/golden sample: image, text, shapes and a path over three seconds.
pub(super) fn sample() -> (Project, BTreeMap<AssetId, Vec<u8>>) {
    let sample = animathio_render::sample::build().expect("build sample project");
    (sample.project, sample.assets)
}

/// 48 kHz mono 16-bit WAV from `(amplitude, milliseconds)` segments of a 440 Hz tone.
pub(super) fn wav(parts: &[(f32, i64)]) -> Vec<u8> {
    let rate = 48_000_u32;
    let mut samples = Vec::new();
    for &(amplitude, ms) in parts {
        let count = u32::try_from(ms).expect("ms") * rate / 1000;
        for n in 0..count {
            let phase = 2.0 * std::f32::consts::PI * 440.0 * n as f32 / rate as f32;
            samples.push((amplitude * phase.sin() * f32::from(i16::MAX)) as i16);
        }
    }
    let data_len = u32::try_from(samples.len() * 2).expect("wav size");
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

/// Ids in [`voice_and_music`].
pub(super) const VOICE_TRACK: TrackId = TrackId(1);
pub(super) const MUSIC_TRACK: TrackId = TrackId(2);
pub(super) const VOICE_CLIP: ClipId = ClipId(10);
pub(super) const MUSIC_CLIP: ClipId = ClipId(11);

/// A 3 s voice (tone, 1 s of silence, tone) over a steady 3 s music bed, each on its own track.
pub(super) fn voice_and_music() -> (Project, BTreeMap<AssetId, Vec<u8>>) {
    let voice = wav(&[(0.5, 1000), (0.0, 1000), (0.5, 1000)]);
    let music = wav(&[(0.2, 3000)]);
    let (voice_id, music_id) = (AssetId::of(&voice), AssetId::of(&music));
    let whole = TimeRange::new(Time(0), Time(3000 * MS)).expect("range");
    let mut project = Project::default();
    Command::Batch {
        commands: vec![
            Command::AddAsset {
                id: voice_id.clone(),
                asset: Asset {
                    name: "voice.wav".into(),
                    media_type: "audio/wav".into(),
                },
            },
            Command::AddAsset {
                id: music_id.clone(),
                asset: Asset {
                    name: "music.wav".into(),
                    media_type: "audio/wav".into(),
                },
            },
            Command::AddTrack {
                track: Track::new(VOICE_TRACK, "Voice"),
                index: 0,
            },
            Command::AddTrack {
                track: Track::new(MUSIC_TRACK, "Music"),
                index: 1,
            },
            Command::AddClip {
                track: VOICE_TRACK,
                clip: Clip::new(VOICE_CLIP, ClipKind::Audio(voice_id.clone()), whole),
            },
            Command::AddClip {
                track: MUSIC_TRACK,
                clip: Clip::new(MUSIC_CLIP, ClipKind::Audio(music_id.clone()), whole),
            },
        ],
    }
    .apply(&mut project)
    .expect("voice + music project");
    (
        project,
        BTreeMap::from([(voice_id, voice), (music_id, music)]),
    )
}

/// Two caption cues (one with word timings) as one command, like Generate subtitles applies.
pub(super) fn caption_command(project: &Project) -> Command {
    let cue = |start: i64, end: i64, text: &str| {
        Cue::new(Time(start * MS), Time(end * MS), text.to_owned())
    };
    subtitle_command(
        project,
        &[cue(200, 1200, "Hello brave world"), cue(1500, 2500, "Plain cue")],
        &CaptionStyleParams::default(),
    )
    .expect("caption command")
    .0
}
