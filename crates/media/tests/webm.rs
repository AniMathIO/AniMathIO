//! WebM export (VP9 + Opus) round trips through the FilmCraft decoders.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::doc_markdown
)]

use std::sync::Arc;

use animathio_core::{FrameRate, Rounding, TICKS_PER_SECOND, Time};
use animathio_media::{
    AudioConfig, EncoderConfig, FilmCraftBackend, Matrix, MediaBackend, MediaKind, Quality,
    VideoConfig, WebmEncoder,
};
use filmcraft_matroska::{Demuxer, TrackKind};

fn synthetic_frame(i: u32, width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (width as usize, height as usize);
    let mut buf = vec![0_u8; w * h * 4];
    let phase = f64::from(i) * 0.05;
    for row in 0..h {
        for col in 0..w {
            let fx = col as f64 / w as f64;
            let fy = row as f64 / h as f64;
            let r = 128.0 + 100.0 * ((fx * 6.0 + phase) * std::f64::consts::TAU).sin();
            let g = 128.0 + 100.0 * ((fy * 6.0 + phase * 1.3) * std::f64::consts::TAU).sin();
            let b = 128.0 + 100.0 * (((fx + fy) * 3.0 + phase * 1.7) * std::f64::consts::TAU).sin();
            let p = (row * w + col) * 4;
            buf[p] = r.round() as u8;
            buf[p + 1] = g.round() as u8;
            buf[p + 2] = b.round() as u8;
            buf[p + 3] = 255;
        }
    }
    let sq = (w / 10).max(8);
    let x0 = (i as usize * 5) % (w - sq);
    let y0 = (i as usize * 3) % (h - sq);
    for row in y0..y0 + sq {
        for col in x0..x0 + sq {
            let p = (row * w + col) * 4;
            buf[p] = 255;
            buf[p + 1] = 255;
            buf[p + 2] = 255;
        }
    }
    buf
}

fn psnr_luma(a: &[u8], b: &[u8]) -> f64 {
    let n = (a.len() / 4).min(b.len() / 4);
    let luma =
        |p: &[u8]| 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]);
    let mse = (0..n)
        .map(|i| {
            let d = luma(&a[i * 4..]) - luma(&b[i * 4..]);
            d * d
        })
        .sum::<f64>()
        / n as f64;
    if mse == 0.0 {
        999.0
    } else {
        10.0 * (255.0_f64 * 255.0 / mse).log10()
    }
}

fn rate(num: u32, den: u32) -> FrameRate {
    FrameRate::new(num, den).unwrap()
}

const SAMPLE_RATE: usize = 48_000;

/// Interleaved stereo: a 440 Hz tone on the left (amplitude 0.5), 660 Hz on the right (0.25),
/// silent before `onset` seconds.
fn tone(seconds: f64, onset: f64) -> Vec<f32> {
    let frames = (seconds * SAMPLE_RATE as f64) as usize;
    let mut out = Vec::with_capacity(frames * 2);
    for n in 0..frames {
        let t = n as f64 / SAMPLE_RATE as f64;
        if t < onset {
            out.extend([0.0, 0.0]);
        } else {
            let phase = std::f64::consts::TAU * t;
            out.push((0.5 * (phase * 440.0).sin()) as f32);
            out.push((0.25 * (phase * 660.0).sin()) as f32);
        }
    }
    out
}

/// Encode `frames` video frames and `audio` (fed one frame interval at a time, like the exporter).
fn encode(frame_rate: FrameRate, frames: u32, audio: Option<&[f32]>, crf: f32) -> Vec<u8> {
    let (width, height) = (160, 90);
    let mut video = VideoConfig::new(width, height, frame_rate);
    video.quality = Quality::Crf(crf);
    let config = audio.map_or_else(
        || EncoderConfig::video_only(video),
        |_| EncoderConfig::with_audio(video, AudioConfig::new(48_000, 2, 128_000)),
    );
    let mut encoder = WebmEncoder::new(config).expect("encoder");
    let mut fed = 0;
    for i in 0..frames {
        encoder
            .encode_video_frame(&synthetic_frame(i, width, height))
            .expect("video frame");
        if let Some(audio) = audio {
            let end = frame_rate
                .frame_to_time(i64::from(i) + 1, Rounding::Nearest)
                .unwrap();
            let end =
                (i128::from(end.0) * SAMPLE_RATE as i128 / i128::from(TICKS_PER_SECOND)) as usize;
            let end = if i + 1 == frames {
                audio.len() / 2
            } else {
                end.min(audio.len() / 2)
            };
            encoder
                .encode_audio(&audio[fed * 2..end * 2])
                .expect("audio");
            fed = end;
        }
    }
    encoder.finish().expect("finish")
}

fn first_cross(samples: &[f32], channels: usize, channel: usize, threshold: f32) -> Option<usize> {
    samples
        .chunks_exact(channels)
        .position(|frame| frame[channel].abs() > threshold)
}

#[test]
fn vp9_round_trip_frame_count_quality_and_timestamps() {
    for (num, den) in [(30, 1), (30_000, 1001), (25, 1)] {
        let frame_rate = rate(num, den);
        let frames = 45;
        let bytes = encode(frame_rate, frames, None, 23.0);
        let backend = FilmCraftBackend::new();
        let info = backend
            .probe_bytes("out.webm", Arc::from(bytes.clone()))
            .unwrap();
        assert_eq!(info.kind, MediaKind::Movie);
        assert_eq!(info.container, "WebM");
        assert_eq!(info.audio.len(), 0);
        let video = info.video.expect("video stream");
        assert_eq!((video.width, video.height), (160, 90));
        assert_eq!(video.codec, "VP9");
        assert_eq!(video.color.matrix, Matrix::Bt709, "tagged BT.709");
        assert_eq!(video.frame_rate, frame_rate, "{num}/{den} frame rate");
        let expected = frame_rate
            .frame_to_time(i64::from(frames), Rounding::Nearest)
            .unwrap();
        // The container stores whole milliseconds; the duration is within one frame.
        let one_frame = TICKS_PER_SECOND * i64::from(den) / i64::from(num);
        assert!(
            (info.duration.0 - expected.0).abs() <= one_frame,
            "{num}/{den}: duration {} vs {}",
            info.duration.0,
            expected.0
        );

        let decoder = backend
            .open_video_bytes("out.webm", Arc::from(bytes.clone()))
            .unwrap();
        for i in 0..frames {
            let time = frame_rate
                .frame_to_time(i64::from(i), Rounding::Floor)
                .unwrap();
            let decoded = decoder.frame_at(time).unwrap();
            let psnr = psnr_luma(&decoded.pixels, &synthetic_frame(i, 160, 90));
            assert!(psnr > 36.0, "{num}/{den} frame {i}: luma PSNR {psnr:.1} dB");
        }

        // Packet timestamps: one video packet per frame at the exact rational time (within the
        // half-millisecond tick), keyframes at the start and every two seconds.
        let mut demuxer = Demuxer::from_slice(&bytes).unwrap();
        assert!(demuxer.file().is_webm());
        let mut index = 0_u64;
        let mut keys = Vec::new();
        while let Some(packet) = demuxer.next_packet().unwrap() {
            assert_eq!(demuxer.tracks()[packet.track].kind, TrackKind::Video);
            let exact = i128::from(index) * i128::from(den) * 1_000_000_000 / i128::from(num);
            assert!(
                (i128::from(packet.pts_ns) - exact).abs() <= 500_000,
                "{num}/{den} packet {index}: {} ns vs {exact} ns",
                packet.pts_ns
            );
            if packet.keyframe {
                keys.push(index);
            }
            index += 1;
        }
        assert_eq!(index, u64::from(frames));
        assert_eq!(keys.first(), Some(&0));
        let interval = u64::from(num.div_ceil(den) * 2);
        assert!(keys.iter().all(|k| k % interval == 0), "keyframes {keys:?}");
    }
}

#[test]
fn opus_audio_round_trip_level_alignment_and_duration() {
    let audio = tone(2.0, 0.5);
    let bytes = encode(rate(30, 1), 60, Some(&audio), 23.0);
    let backend = FilmCraftBackend::new();
    let info = backend
        .probe_bytes("out.webm", Arc::from(bytes.clone()))
        .unwrap();
    assert_eq!(info.audio.len(), 1);
    assert_eq!(info.audio[0].sample_rate, 48_000);
    assert_eq!(info.audio[0].channels, 2);
    assert!(
        info.audio[0].codec.to_lowercase().contains("opus"),
        "{:?}",
        info.audio[0]
    );
    assert!(info.video.is_some());
    // 2 s of video and audio, within one frame.
    assert!(
        (info.duration.0 - 2 * TICKS_PER_SECOND).abs() <= TICKS_PER_SECOND / 30,
        "duration {}",
        info.duration.0
    );

    let decoder = backend
        .open_audio_bytes("out.webm", Arc::from(bytes.clone()))
        .unwrap();
    let decoded = decoder.samples(Time(0), SAMPLE_RATE * 2).unwrap();
    assert_eq!(decoded.len(), SAMPLE_RATE * 2 * 2);

    // The first 0.5 s are silent and the tone starts where it was written (the pre-skip is
    // honoured): the first loud sample is within 3 ms of the onset, on both channels.
    for channel in 0..2 {
        let onset = first_cross(&decoded, 2, channel, 0.05).expect("tone present");
        let drift_ms = (onset as f64 - 0.5 * SAMPLE_RATE as f64) / 48.0;
        assert!(
            drift_ms.abs() < 3.0,
            "channel {channel} onset off by {drift_ms:.2} ms"
        );
    }
    // Levels per channel (RMS of a sine is amplitude / sqrt 2) over the steady part, and the
    // waveform follows the input closely at zero lag.
    let (from, to) = (SAMPLE_RATE, SAMPLE_RATE * 2 - 2000);
    for (channel, amplitude) in [(0, 0.5_f64), (1, 0.25)] {
        let (mut signal, mut error, mut energy) = (0.0_f64, 0.0_f64, 0.0_f64);
        for n in from..to {
            let want = f64::from(audio[n * 2 + channel]);
            let got = f64::from(decoded[n * 2 + channel]);
            signal += want * want;
            error += (want - got) * (want - got);
            energy += got * got;
        }
        let count = (to - from) as f64;
        let rms = (energy / count).sqrt();
        let expected = amplitude / std::f64::consts::SQRT_2;
        assert!(
            (rms - expected).abs() < expected * 0.1,
            "channel {channel} RMS {rms:.4} vs {expected:.4}"
        );
        let snr = 10.0 * (signal / error).log10();
        assert!(snr > 15.0, "channel {channel} SNR {snr:.1} dB");
    }
}

#[test]
fn packets_are_interleaved_in_presentation_order_with_opus_timing() {
    let audio = tone(1.0, 0.0);
    let bytes = encode(rate(30, 1), 30, Some(&audio), 30.0);
    let mut demuxer = Demuxer::from_slice(&bytes).unwrap();
    let delay = demuxer
        .tracks()
        .iter()
        .find(|t| t.kind == TrackKind::Audio)
        .map(|t| t.codec_delay_ns)
        .unwrap();
    assert_eq!(
        delay, 2_500_000,
        "Opus pre-skip of 120 samples as CodecDelay"
    );
    let mut last = i64::MIN / 2;
    let (mut video, mut opus) = (0, 0);
    let mut first_audio = None;
    while let Some(packet) = demuxer.next_packet().unwrap() {
        let kind = demuxer.tracks()[packet.track].kind;
        // File order is ascending in stored time (presentation time plus the codec delay).
        let stored = packet.pts_ns
            + if kind == TrackKind::Audio {
                delay as i64
            } else {
                0
            };
        assert!(stored >= last - 500_000, "{kind:?} packet out of order");
        last = last.max(stored);
        match kind {
            TrackKind::Video => video += 1,
            TrackKind::Audio => {
                opus += 1;
                first_audio.get_or_insert(packet.pts_ns);
            }
            _ => unreachable!(),
        }
    }
    assert_eq!(video, 30);
    assert_eq!(opus, 50, "one 20 ms packet per 960 samples");
    assert_eq!(first_audio, Some(-2_500_000));
}

#[test]
fn partial_final_opus_packet_is_padded_not_dropped() {
    // 1.01 s: 48 480 samples is 50.5 packets, so the last one is half silence.
    let audio = tone(1.01, 0.0);
    let bytes = encode(rate(30, 1), 31, Some(&audio), 23.0);
    let mut demuxer = Demuxer::from_slice(&bytes).unwrap();
    let mut opus = 0;
    while let Some(packet) = demuxer.next_packet().unwrap() {
        if demuxer.tracks()[packet.track].kind == TrackKind::Audio {
            opus += 1;
        }
    }
    assert_eq!(opus, 51);
}

#[test]
fn constant_bitrate_mode_encodes() {
    let mut video = VideoConfig::new(160, 90, rate(30, 1));
    video.quality = Quality::CbrKbps(400);
    let mut encoder = WebmEncoder::new(EncoderConfig::video_only(video)).unwrap();
    for i in 0..60 {
        encoder
            .encode_video_frame(&synthetic_frame(i, 160, 90))
            .unwrap();
    }
    let bytes = encoder.finish().unwrap();
    let kbps = bytes.len() as f64 * 8.0 / 2.0 / 1000.0;
    assert!(kbps < 800.0, "400 kbit/s target produced {kbps:.0} kbit/s");
    FilmCraftBackend::new()
        .probe_bytes("cbr.webm", Arc::from(bytes))
        .unwrap();
}

#[test]
fn invalid_configurations_are_clear_errors() {
    let video = |w, h| VideoConfig::new(w, h, rate(30, 1));
    let message = |config: EncoderConfig| {
        WebmEncoder::new(config)
            .err()
            .expect("must fail")
            .to_string()
    };
    assert!(message(EncoderConfig::video_only(video(0, 0))).contains("positive"));
    assert!(message(EncoderConfig::video_only(video(161, 90))).contains("even"));
    assert!(message(EncoderConfig::video_only(video(8192, 8192))).contains("argument"));
    for crf in [f32::NAN, -1.0, 52.0] {
        let mut config = video(64, 64);
        config.quality = Quality::Crf(crf);
        assert!(message(EncoderConfig::video_only(config)).contains("CRF"));
    }
    let mut config = video(64, 64);
    config.quality = Quality::CbrKbps(0);
    assert!(message(EncoderConfig::video_only(config)).contains("bitrate"));
    let audio = |rate_hz, channels| {
        EncoderConfig::with_audio(video(64, 64), AudioConfig::new(rate_hz, channels, 128_000))
    };
    assert!(message(audio(44_100, 2)).contains("48 kHz"));
    assert!(message(audio(48_000, 6)).contains("mono or stereo"));
    assert!(message(audio(48_000, 0)).contains("argument"));
    // Mono works.
    let mut mono = WebmEncoder::new(audio(48_000, 1)).unwrap();
    mono.encode_video_frame(&synthetic_frame(0, 64, 64))
        .unwrap();
    mono.encode_audio(&vec![0.1; 1600]).unwrap();
    assert!(mono.encode_audio(&[f32::NAN]).is_err());
    let bytes = mono.finish().unwrap();
    let info = FilmCraftBackend::new()
        .probe_bytes("m.webm", Arc::from(bytes))
        .unwrap();
    assert_eq!(info.audio[0].channels, 1);
    // Wrong frame sizes and audio on a video-only encoder.
    let mut video_only = WebmEncoder::new(EncoderConfig::video_only(video(64, 64))).unwrap();
    assert!(video_only.encode_video_frame(&[0; 10]).is_err());
    assert!(video_only.encode_audio(&[0.0; 2]).is_err());
}
