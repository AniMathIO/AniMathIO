//! Integration tests for the media runtime: probing, frame-exact seek, sequential-vs-random
//! decode equality, MP4 encode round-trip, and pool GC.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::many_single_char_names,
    clippy::similar_names,
    clippy::doc_markdown
)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use animathio_core::{AssetId, FrameRate, Rounding, Time};
use animathio_media::{
    EncoderConfig, FilmCraftBackend, Matrix, MediaBackend, MediaPool, MediaPoolConfig, Mp4Encoder,
    Quality, VideoConfig,
};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample.mp4")
}

fn rate30() -> FrameRate {
    FrameRate::new(30, 1).unwrap()
}

fn frame_time(frame: i64) -> Time {
    rate30().frame_to_time(frame, Rounding::Floor).unwrap()
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mut mse = 0.0_f64;
    for i in 0..n {
        let d = f64::from(a[i]) - f64::from(b[i]);
        mse += d * d;
    }
    mse /= n as f64;
    if mse == 0.0 {
        return 999.0;
    }
    10.0 * (255.0_f64 * 255.0 / mse).log10()
}

/// PSNR between our RGBA8 and FFmpeg's RGB24 output (alpha ignored).
fn psnr_rgba_rgb24(rgba: &[u8], rgb24: &[u8]) -> f64 {
    let n = (rgba.len() / 4).min(rgb24.len() / 3);
    let mut a = Vec::with_capacity(n * 3);
    for i in 0..n {
        a.extend_from_slice(&rgba[i * 4..i * 4 + 3]);
    }
    psnr(&a, &rgb24[..n * 3])
}

fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn ffmpeg_rgb_at(path: &Path, t_seconds: f64, width: usize, height: usize) -> Option<Vec<u8>> {
    let out = Command::new("ffmpeg")
        .args(["-v", "error", "-ss"])
        .arg(format!("{t_seconds:.6}"))
        .args(["-i"])
        .arg(path)
        .args([
            "-map",
            "0:v:0",
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let expected = width * height * 3;
    (out.stdout.len() == expected).then_some(out.stdout)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos());
        let dir = std::env::temp_dir().join(format!(
            "animathio-media-{tag}-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn probe_reports_streams() {
    let backend = FilmCraftBackend::new();
    let info = backend.probe(&fixture()).expect("probe");
    let video = info.video.expect("video stream");
    assert_eq!(video.width, 320);
    assert_eq!(video.height, 180);
    assert_eq!(video.frame_rate, rate30());
    assert!(!video.vfr);
    assert_eq!(info.audio.len(), 1);
    assert_eq!(info.audio[0].sample_rate, 48_000);
    // Two seconds at 30 fps.
    let two_seconds = Time(animathio_core::TICKS_PER_SECOND * 2);
    assert!((info.duration.0 - two_seconds.0).abs() < 1000);
}

#[test]
fn unspecified_colour_defaults_to_bt601() {
    let backend = FilmCraftBackend::new();
    let decoder = backend.open_video(&fixture()).expect("open video");
    let frame = decoder.frame_at(frame_time(10)).expect("decode");
    assert_eq!(frame.color.matrix, Matrix::Bt601);
    assert!(!frame.color.matrix_specified);
}

#[test]
fn frame_exact_seek_matches_ffmpeg() {
    if !ffmpeg_available() {
        eprintln!("skipping: ffmpeg not found");
        return;
    }
    let backend = FilmCraftBackend::new();
    let info = backend.probe(&fixture()).expect("probe");
    let video = info.video.expect("video");
    let decoder = backend.open_video(&fixture()).expect("open video");

    for frame in [0, 1, 5, 15, 30, 45, 59] {
        let ours = decoder.frame_at(frame_time(frame)).expect("decode");
        let t_seconds = frame as f64 / 30.0;
        let reference = ffmpeg_rgb_at(
            &fixture(),
            t_seconds,
            video.width as usize,
            video.height as usize,
        )
        .expect("ffmpeg reference frame");
        let score = psnr_rgba_rgb24(&ours.pixels, &reference);
        assert!(score > 40.0, "frame {frame}: PSNR {score:.1} dB");
    }
}

#[test]
fn sequential_matches_random_access() {
    let backend = FilmCraftBackend::new();
    let decoder = backend.open_video(&fixture()).expect("open video");
    let sequential: Vec<Vec<u8>> = (0..60)
        .map(|i| {
            decoder
                .frame_at(frame_time(i))
                .expect("decode")
                .pixels
                .to_vec()
        })
        .collect();
    // Random access to a shuffled set of frame indices must be byte-identical.
    for frame in [59, 0, 13, 40, 27, 7, 33, 21, 51, 3] {
        let ours = decoder.frame_at(frame_time(frame)).expect("decode");
        assert_eq!(&*ours.pixels, sequential[frame as usize]);
    }
}

#[test]
fn encode_round_trip_frame_count_and_psnr() {
    let width = 320_u32;
    let height = 180_u32;
    let frames = 60_u32;

    let mut encoder = Mp4Encoder::new(EncoderConfig::video_only(VideoConfig {
        width,
        height,
        frame_rate: rate30(),
        quality: Quality::Crf(18.0),
        threads: 2,
    }))
    .expect("encoder");
    for i in 0..frames {
        let rgba = synthetic_frame(i, width, height);
        encoder.encode_video_frame(&rgba).expect("encode frame");
    }
    let bytes = encoder.finish().expect("finish");

    let tmp = TempDir::new("roundtrip");
    let out = tmp.path().join("out.mp4");
    std::fs::write(&out, bytes).expect("write mp4");

    let backend = FilmCraftBackend::new();
    let decoder = backend.open_video(&out).expect("open encoded mp4");
    let mut count = 0;
    let mut worst = f64::INFINITY;
    for i in 0..frames {
        let Ok(decoded) = decoder.frame_at(frame_time(i64::from(i))) else {
            break;
        };
        count += 1;
        let original = synthetic_frame(i, width, height);
        // PSNR is measured on luma, matching the spike's YUV-domain measurement: 4:2:0 chroma
        // subsampling is a property of the format, not the codec.
        worst = worst.min(psnr_luma(&decoded.pixels, &original));
    }
    assert_eq!(count, frames as usize, "decoded frame count");
    assert!(worst > 40.0, "round-trip luma PSNR {worst:.1} dB");
}

#[test]
fn pool_gc_releases_all_assets() {
    let backend = Arc::new(FilmCraftBackend::new());
    let pool = MediaPool::new(backend, MediaPoolConfig::default());
    let bytes = std::fs::read(fixture()).expect("read fixture");
    let asset = AssetId::of(&bytes);

    pool.add(asset.clone(), fixture()).expect("add asset");
    let first = pool.frame_at(asset.clone(), frame_time(5)).expect("decode");
    assert_eq!(first.width, 320);
    let second = pool.frame_at(asset.clone(), frame_time(5)).expect("decode");
    assert!(Arc::ptr_eq(&first, &second), "cache must be shared");

    assert!(!pool.is_empty());
    pool.release(asset.clone());
    assert!(pool.is_empty(), "pool must be empty after release");

    pool.add(asset, fixture()).expect("re-add asset");
    assert!(!pool.is_empty());
    pool.clear();
    assert!(pool.is_empty(), "pool must be empty after clear");
}

fn psnr_luma(a: &[u8], b: &[u8]) -> f64 {
    let n = (a.len() / 4).min(b.len() / 4);
    if n == 0 {
        return 0.0;
    }
    let mut mse = 0.0_f64;
    for i in 0..n {
        let la = 0.2126 * f64::from(a[i * 4])
            + 0.7152 * f64::from(a[i * 4 + 1])
            + 0.0722 * f64::from(a[i * 4 + 2]);
        let lb = 0.2126 * f64::from(b[i * 4])
            + 0.7152 * f64::from(b[i * 4 + 1])
            + 0.0722 * f64::from(b[i * 4 + 2]);
        let d = la - lb;
        mse += d * d;
    }
    mse /= n as f64;
    if mse == 0.0 {
        return 999.0;
    }
    10.0 * (255.0_f64 * 255.0 / mse).log10()
}

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
