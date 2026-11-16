//! Generate the documentation capture fixture without opening a window or audio device.
#![allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
use animathio_core::{
    Asset, AssetId, Clip, ClipId, ClipKind, Command, Marker, MarkerColor, Time, TimeRange, Track,
    TrackId,
};
const MS: i64 = animathio_core::TICKS_PER_SECOND / 1000;
fn main() {
    let path = std::env::args().nth(1).expect("output .amio path");
    let mut sample = animathio_render::sample::build().expect("sample");
    for (id, name, parts) in [
        (100, "Voice", vec![(0.5, 1000), (0.0, 1000), (0.5, 1000)]),
        (101, "Music", vec![(0.2, 3000)]),
    ] {
        let bytes = wav(&parts);
        let asset_id = AssetId::of(&bytes);
        Command::Batch {
            commands: vec![
                Command::AddAsset {
                    id: asset_id.clone(),
                    asset: Asset {
                        name: format!("{name}.wav"),
                        media_type: "audio/wav".into(),
                    },
                },
                Command::AddTrack {
                    track: Track::new(TrackId(id), name),
                    index: usize::try_from(id - 100).expect("track index"),
                },
                Command::AddClip {
                    track: TrackId(id),
                    clip: Clip::new(
                        ClipId(id),
                        ClipKind::Audio(asset_id.clone()),
                        TimeRange::new(Time(0), Time(3000 * MS)).expect("range"),
                    ),
                },
            ],
        }
        .apply(&mut sample.project)
        .expect("audio tracks");
        sample.assets.insert(asset_id, bytes);
    }
    for (ms, name, color) in [
        (0, "Introduction", MarkerColor::Blue),
        (1000, "Pause begins", MarkerColor::Orange),
        (2000, "Continue explanation", MarkerColor::Green),
    ] {
        let mut marker = Marker::new(sample.project.next_marker_id(), Time(ms * MS));
        marker.name = name.into();
        marker.color = color;
        Command::AddMarker { marker }
            .apply(&mut sample.project)
            .expect("marker");
    }
    animathio_format::save_amio(path, &sample.project, &sample.assets).expect("save fixture");
}
fn wav(parts: &[(f32, i64)]) -> Vec<u8> {
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
