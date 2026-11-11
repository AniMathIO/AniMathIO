//! AniMathIO 2.0 M0 media spike: can the FilmCraft codec crates be our media backend?
//!
//! Throws the FilmCraft `MediaSource` (Mp4Source / MkvSource) against an FFmpeg reference on a
//! small generated corpus, then encodes H.264 + AAC into an MP4 with the FilmCraft encoders and
//! `filmcraft-isobmff`. See FINDINGS.md for the written go/no-go.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use filmcraft_codecs::open_bytes as fc_open;
use filmcraft_frame::PixelData;
use filmcraft_media::{FrameRequest, MediaSource, SharedSource};
use filmcraft_time::{FrameRate, Tick};

// ---------------------------------------------------------------------------
// MediaBackend trait
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct StreamInfo {
    container: String,
    width: u32,
    height: u32,
    fps_num: i64,
    fps_den: i64,
    duration_secs: f64,
    has_audio: bool,
    audio_sample_rate: u32,
    audio_channels: u32,
    codec: String,
}

/// Minimal backend abstraction: open, describe, decode a video frame at a media time, decode an
/// audio range. The FilmCraft backend implements it; the FFmpeg reference shells out.
trait MediaBackend {
    fn open(path: &Path) -> Result<Self, String>
    where
        Self: Sized;
    fn stream_info(&self) -> &StreamInfo;
    /// Full-resolution RGBA8 of the frame displayed at media time `t` (seconds).
    fn decode_frame_at(&self, t: f64) -> Result<(u32, u32, Vec<u8>), String>;
    /// Interleaved f32 samples decoded from [`start`, start + dur), at `out_rate` Hz.
    fn decode_audio(&self, start: f64, dur: f64, out_rate: u32) -> Result<Vec<f32>, String>;
}

// ---------------------------------------------------------------------------
// FilmCraft backend
// ---------------------------------------------------------------------------

struct FilmcraftBackend {
    src: SharedSource,
    info: StreamInfo,
}

impl FilmcraftBackend {
    /// Codec-native 4:2:0 planes of the frame displayed at `t` (Y, U, V, 8-bit).
    fn decode_frame_yuv_at(&self, t: f64) -> Result<(u32, u32, Vec<u8>, Vec<u8>, Vec<u8>), String> {
        let f = self
            .src
            .video_frame(FrameRequest::full(Tick::from_seconds_f64(t)))
            .map_err(|e| format!("decode @ {t}: {e}"))?;
        match &f.data {
            PixelData::Yuv8 { planes, .. } => Ok((f.width, f.height, planes[0].as_ref().clone(), planes[1].as_ref().clone(), planes[2].as_ref().clone())),
            _ => Ok((f.width, f.height, f.luma8(), vec![], vec![])),
        }
    }
}

impl MediaBackend for FilmcraftBackend {
    fn open(path: &Path) -> Result<Self, String> {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("media")
            .to_string();
        let bytes: Arc<[u8]> = std::fs::read(path)
            .map_err(|e| format!("read {}: {e}", path.display()))?
            .into();
        let src = fc_open(&name, bytes).map_err(|e| format!("open: {e}"))?;
        let mi = src.info();
        let v = mi.video.as_ref();
        let fr = v.map(|v| v.frame_rate).unwrap_or_default();
        let a = mi.audio();
        let info = StreamInfo {
            container: mi.container.clone(),
            width: v.map(|v| v.width).unwrap_or(0),
            height: v.map(|v| v.height).unwrap_or(0),
            fps_num: fr.num,
            fps_den: fr.den,
            duration_secs: mi.duration.seconds(),
            has_audio: mi.has_audio(),
            audio_sample_rate: a.map(|a| a.sample_rate).unwrap_or(0),
            audio_channels: a.map(|a| a.channels).unwrap_or(0),
            codec: v.map(|v| v.codec.clone()).unwrap_or_else(|| a.map(|a| a.codec.clone()).unwrap_or_default()),
        };
        Ok(FilmcraftBackend { src, info })
    }

    fn stream_info(&self) -> &StreamInfo {
        &self.info
    }

    fn decode_frame_at(&self, t: f64) -> Result<(u32, u32, Vec<u8>), String> {
        let tick = Tick::from_seconds_f64(t);
        let f = self
            .src
            .video_frame(FrameRequest::full(tick))
            .map_err(|e| format!("decode @ {t}: {e}"))?;
        Ok((f.width, f.height, f.to_rgba8()))
    }

    fn decode_audio(&self, start: f64, dur: f64, out_rate: u32) -> Result<Vec<f32>, String> {
        let sr = self.info.audio_sample_rate;
        if sr == 0 {
            return Err("no audio stream".into());
        }
        let start_sample = (start * sr as f64).round() as i64;
        let frames = (dur * sr as f64).round() as usize;
        let buf = self
            .src
            .audio(start_sample, frames, out_rate)
            .map_err(|e| format!("audio decode @ {start}: {e}"))?;
        Ok(buf.interleaved())
    }
}

// ---------------------------------------------------------------------------
// FFmpeg reference (shell out)
// ---------------------------------------------------------------------------

fn ffmpeg_bin() -> &'static str {
    "/usr/bin/ffmpeg"
}

/// Decode the entire first video stream to planar 4:2:0 YUV (presentation order).
fn ffmpeg_dump_yuv420(path: &Path, w: u32, h: u32) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new(ffmpeg_bin());
    cmd.args(["-v", "error", "-i"])
        .arg(path)
        .args(["-map", "0:v:0", "-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("ffmpeg dump: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let frame = (w * h * 3 / 2) as usize;
    if out.stdout.len() % frame != 0 {
        return Err(format!(
            "ffmpeg dump not a whole number of frames ({} bytes, frame {frame})",
            out.stdout.len()
        ));
    }
    Ok(out.stdout)
}

/// Decode one video frame displayed at time `t` as planar 4:2:0 YUV (accurate input seek).
fn ffmpeg_frame_yuv_at(path: &Path, t: f64) -> Result<(u32, u32, Vec<u8>), String> {
    let (w, h) = ffprobe_video_size(path)?;
    let mut cmd = Command::new(ffmpeg_bin());
    cmd.args(["-v", "error", "-ss"])
        .arg(format!("{t:.6}"))
        .args(["-i"])
        .arg(path)
        .args(["-map", "0:v:0", "-frames:v", "1", "-f", "rawvideo", "-pix_fmt", "yuv420p", "-"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("ffmpeg frame: {}", String::from_utf8_lossy(&out.stderr)));
    }
    Ok((w, h, out.stdout))
}

fn ffprobe_video_size(path: &Path) -> Result<(u32, u32), String> {
    let mut cmd = Command::new("/usr/bin/ffprobe");
    cmd.args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height", "-of", "csv=p=0"])
        .arg(path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut parts = s.trim().split(',');
    let w: u32 = parts.next().and_then(|p| p.trim().parse().ok()).unwrap_or(0);
    let h: u32 = parts.next().and_then(|p| p.trim().parse().ok()).unwrap_or(0);
    Ok((w, h))
}

/// Frame presentation timestamps (seconds) of the first video stream, in decode order.
fn ffprobe_frame_pts(path: &Path) -> Result<Vec<f64>, String> {
    let mut cmd = Command::new("/usr/bin/ffprobe");
    cmd.args(["-v", "error", "-select_streams", "v:0", "-show_entries", "frame=pts_time", "-of", "csv=p=0"])
        .arg(path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse::<f64>().ok())
        .collect())
}

/// Decode an audio range as interleaved f32 at `out_rate` Hz (mono mixdown).
fn ffmpeg_audio(path: &Path, start: f64, dur: f64, out_rate: u32) -> Result<Vec<f32>, String> {
    let mut cmd = Command::new(ffmpeg_bin());
    cmd.args(["-v", "error", "-ss"])
        .arg(format!("{start:.6}"))
        .args(["-i"])
        .arg(path)
        .args(["-map", "0:a:0", "-f", "f32le", "-ac", "1", "-ar"])
        .arg(out_rate.to_string())
        .args(["-t"])
        .arg(format!("{dur:.6}"))
        .arg("-")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let out = cmd.output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("ffmpeg audio: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let bytes = out.stdout;
    let mut v = Vec::with_capacity(bytes.len() / 4);
    for c in bytes.chunks_exact(4) {
        v.push(f32::from_le_bytes([c[0], c[1], c[2], c[3]]));
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// Metrics helpers
// ---------------------------------------------------------------------------

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let n = a.len().min(b.len());
    if n == 0 {
        return 0.0;
    }
    let mut mse = 0.0f64;
    for i in 0..n {
        let d = a[i] as f64 - b[i] as f64;
        mse += d * d;
    }
    mse /= n as f64;
    if mse == 0.0 {
        return 999.0;
    }
    10.0 * (255.0f64 * 255.0 / mse).log10()
}

fn rgba_to_rgb(rgba: &[u8]) -> Vec<u8> {
    rgba.chunks_exact(4).flat_map(|p| [p[0], p[1], p[2]]).collect()
}

struct Rng(u64);
impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x % n.max(1)
    }
}

fn percentile(sorted: &mut Vec<f64>, p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

// ---------------------------------------------------------------------------
// Decode + seek test
// ---------------------------------------------------------------------------

struct VideoResult {
    file: String,
    codec: String,
    container: String,
    ok: bool,
    note: String,
    decode_fps: f64,
    seq_match: f64, // YUV PSNR dB of sequential decode vs ffmpeg
    seek_p50_ms: f64,
    seek_p95_ms: f64,
    cold_seek_ms: f64,
    seek_match_pct: f64,
}

fn test_video(path: &Path, vfr: bool) -> VideoResult {
    let file = path.file_name().unwrap().to_string_lossy().to_string();
    let mut r = VideoResult {
        file: file.clone(),
        codec: String::new(),
        container: String::new(),
        ok: false,
        note: String::new(),
        decode_fps: 0.0,
        seq_match: f64::NAN,
        seek_p50_ms: f64::NAN,
        seek_p95_ms: f64::NAN,
        cold_seek_ms: f64::NAN,
        seek_match_pct: f64::NAN,
    };

    let fc = match FilmcraftBackend::open(path) {
        Ok(fc) => fc,
        Err(e) => {
            r.note = format!("open failed: {e}");
            return r;
        }
    };
    let info = fc.stream_info().clone();
    r.codec = info.codec.clone();
    r.container = info.container.clone();
    if info.width == 0 {
        r.note = "no video stream reported".into();
        return r;
    }
    let (w, h) = (info.width, info.height);
    let frame_bytes = (w * h * 3 / 2) as usize;

    let mut rng = Rng(0x9e3779b97f4a7c15);

    if vfr {
        // VFR: frame timestamps come from ffprobe; accuracy checked per-timestamp against ffmpeg.
        let pts = match ffprobe_frame_pts(path) {
            Ok(p) if !p.is_empty() => p,
            Ok(_) => {
                r.note = "no frame pts".into();
                return r;
            }
            Err(e) => {
                r.note = format!("ffprobe pts: {e}");
                return r;
            }
        };
        let n = pts.len();
        // Sequential decode at each frame's real timestamp; time it (no ffmpeg in the timed loop).
        let t0 = Instant::now();
        for (i, &t) in pts.iter().enumerate() {
            if let Err(e) = fc.decode_frame_yuv_at(t) {
                r.note = format!("seq decode frame {i}: {e}");
                return r;
            }
        }
        let s = t0.elapsed().as_secs_f64();
        r.decode_fps = n as f64 / s;
        // Spot-check a few frames against ffmpeg (outside the timing).
        let mut last = f64::NAN;
        for (i, &t) in pts.iter().enumerate().step_by(10).take(8) {
            if let (Ok((_, _, y, u, v)), Ok((_, _, ref_yuv))) = (fc.decode_frame_yuv_at(t), ffmpeg_frame_yuv_at(path, t)) {
                let mut fc_planes = y;
                fc_planes.extend_from_slice(&u);
                fc_planes.extend_from_slice(&v);
                last = psnr(&fc_planes, &ref_yuv);
            }
        }
        r.seq_match = last;

        // 200 random-access seeks to exact frame boundaries (frame-exact seeking).
        let mut lats = Vec::with_capacity(200);
        let mut matched = 0usize;
        let mut compared = 0usize;
        for _ in 0..200 {
            let i = rng.below(n as u64) as usize;
            let t = pts[i];
            let t0 = Instant::now();
            let a = fc.decode_frame_yuv_at(t);
            let lat = t0.elapsed().as_secs_f64() * 1000.0;
            lats.push(lat);
            if let (Ok((_, _, y, u, v)), Ok((_, _, ref_yuv))) = (a, ffmpeg_frame_yuv_at(path, t)) {
                let mut fc_planes = y;
                fc_planes.extend_from_slice(&u);
                fc_planes.extend_from_slice(&v);
                compared += 1;
                if psnr(&fc_planes, &ref_yuv) >= 45.0 {
                    matched += 1;
                }
            }
        }
        r.seek_p50_ms = percentile(&mut lats, 0.5);
        r.seek_p95_ms = percentile(&mut lats, 0.95);
        r.seek_match_pct = if compared > 0 { matched as f64 * 100.0 / compared as f64 } else { f64::NAN };
        r.note = format!("{n} frames (VFR)");
    } else {
        // CFR: full ffmpeg reference dump, compare by frame index.
        let dump = match ffmpeg_dump_yuv420(path, w, h) {
            Ok(d) => d,
            Err(e) => {
                r.note = format!("ffmpeg dump failed: {e}");
                return r;
            }
        };
        let ref_frames = dump.len() / frame_bytes;
        let rate = FrameRate::new(info.fps_num, info.fps_den);
        let n_seq = ref_frames;
        let mut last = f64::NAN;
        let t0 = Instant::now();
        for i in 0..n_seq {
            let tick = rate.tick_of(i as i64);
            match fc.decode_frame_yuv_at(tick.seconds()) {
                Ok((_w, _h, y, u, v)) => {
                    if i % 10 == 0 {
                        let mut fc_planes = y;
                        fc_planes.extend_from_slice(&u);
                        fc_planes.extend_from_slice(&v);
                        let s = i * frame_bytes;
                        last = psnr(&fc_planes, &dump[s..s + frame_bytes]);
                    }
                }
                Err(e) => {
                    r.note = format!("seq decode frame {i}: {e}");
                    return r;
                }
            }
        }
        let s = t0.elapsed().as_secs_f64();
        r.decode_fps = n_seq as f64 / s;
        r.seq_match = last;

        // 200 random-access seeks.
        let mut lats = Vec::with_capacity(200);
        let mut matched = 0usize;
        let mut compared = 0usize;
        for _ in 0..200 {
            let i = rng.below(ref_frames as u64) as usize;
            let tick = rate.tick_of(i as i64);
            let t0 = Instant::now();
            let a = fc.decode_frame_yuv_at(tick.seconds());
            let lat = t0.elapsed().as_secs_f64() * 1000.0;
            lats.push(lat);
            if let Ok((_w, _h, y, u, v)) = a {
                let mut fc_planes = y;
                fc_planes.extend_from_slice(&u);
                fc_planes.extend_from_slice(&v);
                let s = i * frame_bytes;
                if s + frame_bytes <= dump.len() {
                    compared += 1;
                    if psnr(&fc_planes, &dump[s..s + frame_bytes]) >= 45.0 {
                        matched += 1;
                    }
                }
            }
        }
        r.seek_p50_ms = percentile(&mut lats, 0.5);
        r.seek_p95_ms = percentile(&mut lats, 0.95);
        r.seek_match_pct = if compared > 0 { matched as f64 * 100.0 / compared as f64 } else { f64::NAN };
        r.note = format!("{ref_frames} reference frames, {n_seq} decoded sequentially");
    }

    r.ok = r.seek_match_pct >= 99.0 && r.decode_fps > 0.0;

    // Cold random-access seek: a fresh backend's first decode at mid-clip (parses the GOP and
    // decodes forward from the preceding keyframe; no warm cache).
    if let Ok(fc2) = FilmcraftBackend::open(path) {
        let t_far = info.duration_secs * 0.5;
        let t0 = Instant::now();
        let _ = fc2.decode_frame_yuv_at(t_far);
        r.cold_seek_ms = t0.elapsed().as_secs_f64() * 1000.0;
    }
    r
}

// ---------------------------------------------------------------------------
// Audio test
// ---------------------------------------------------------------------------

struct AudioResult {
    file: String,
    codec: String,
    sample_rate: u32,
    channels: u32,
    ok: bool,
    note: String,
    rms_err_vs_ffmpeg: f64,
    best_lag_samples: i32,
    rms_at_best_lag: f64,
}

fn test_audio(path: &Path) -> AudioResult {
    let file = path.file_name().unwrap().to_string_lossy().to_string();
    let mut r = AudioResult {
        file: file.clone(),
        codec: String::new(),
        sample_rate: 0,
        channels: 0,
        ok: false,
        note: String::new(),
        rms_err_vs_ffmpeg: f64::NAN,
        best_lag_samples: 0,
        rms_at_best_lag: f64::NAN,
    };
    let fc = match FilmcraftBackend::open(path) {
        Ok(fc) => fc,
        Err(e) => {
            r.note = format!("open failed: {e}");
            return r;
        }
    };
    let info = fc.stream_info().clone();
    r.codec = info.codec.clone();
    r.sample_rate = info.audio_sample_rate;
    r.channels = info.audio_channels;
    if info.audio_sample_rate == 0 {
        r.note = "no audio stream".into();
        return r;
    }
    let out_rate = 48_000u32;
    // Start 1 s in, past the codec priming region.
    let dur = 1.0f64;
    let a = match fc.decode_audio(1.0, dur, out_rate) {
        Ok(a) => a,
        Err(e) => {
            r.note = format!("decode failed: {e}");
            return r;
        }
    };
    let b = match ffmpeg_audio(path, 1.0, dur, out_rate) {
        Ok(b) => b,
        Err(e) => {
            r.note = format!("ffmpeg ref failed: {e}");
            return r;
        }
    };
    let n = a.len().min(b.len());
    if n == 0 {
        r.note = "empty decode".into();
        return r;
    }
    let mut mse = 0.0f64;
    for i in 0..n {
        let d = a[i] as f64 - b[i] as f64;
        mse += d * d;
    }
    mse /= n as f64;
    r.rms_err_vs_ffmpeg = mse.sqrt();
    // Best integer sample lag of fc relative to ref (ref shifted by lag), and the residual there:
    // separates a constant timeline offset from an actual decode error.
    let mut best = (0i32, f64::INFINITY);
    for lag in -64i32..=64i32 {
        let mut acc = 0.0f64;
        let mut c = 0usize;
        for i in 0..n {
            let j = i as i32 + lag;
            if j >= 0 && (j as usize) < n {
                let d = a[i] as f64 - b[j as usize] as f64;
                acc += d * d;
                c += 1;
            }
        }
        let rms = (acc / c.max(1) as f64).sqrt();
        if rms < best.1 {
            best = (lag, rms);
        }
    }
    r.best_lag_samples = best.0;
    r.rms_at_best_lag = best.1;
    r.ok = r.rms_at_best_lag < 0.01;
    if r.note.is_empty() {
        r.note = format!("{n} samples compared at {out_rate} Hz, 1.0 s in");
    }
    r
}

// ---------------------------------------------------------------------------
// Encode
// ---------------------------------------------------------------------------

fn rgba_to_yuv420(rgba: &[u8], w: u32, h: u32) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let (w, h) = (w as usize, h as usize);
    let mut y = vec![0u8; w * h];
    let cw = w / 2;
    let ch = h / 2;
    let mut u = vec![0u8; cw * ch];
    let mut v = vec![0u8; cw * ch];
    for row in 0..h {
        for col in 0..w {
            let p = &rgba[(row * w + col) * 4..][..3];
            let (rf, gf, bf) = (p[0] as f64 / 255.0, p[1] as f64 / 255.0, p[2] as f64 / 255.0);
            let yv = 0.2126 * rf + 0.7152 * gf + 0.0722 * bf;
            let cb = (bf - yv) / 1.8556;
            let cr = (rf - yv) / 1.5748;
            y[row * w + col] = (16.0 + 219.0 * yv).round().clamp(0.0, 255.0) as u8;
            u[(row / 2) * cw + col / 2] = (128.0 + 224.0 * cb).round().clamp(0.0, 255.0) as u8;
            v[(row / 2) * cw + col / 2] = (128.0 + 224.0 * cr).round().clamp(0.0, 255.0) as u8;
        }
    }
    (y, u, v)
}

fn synthetic_frame(i: u32, w: u32, h: u32) -> Vec<u8> {
    let (w, h) = (w as usize, h as usize);
    let mut buf = vec![0u8; w * h * 4];
    let hue = (i as f64 * 3.0) % 360.0;
    for row in 0..h {
        for col in 0..w {
            let p = (row * w + col) * 4;
            let t = (col as f64 / w as f64 + row as f64 / h as f64) / 2.0;
            let v = ((t * 360.0 + hue) % 360.0) as u8;
            buf[p] = v;
            buf[p + 1] = (255 - v) as u8;
            buf[p + 2] = (v / 2) as u8;
            buf[p + 3] = 255;
        }
    }
    let sq = (w / 12).max(16);
    let x0 = (i as usize * 7) % (w - sq);
    let y0 = (i as usize * 5) % (h - sq);
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

fn encode_mp4(out: &Path) -> Result<String, String> {
    use filmcraft_aac::{Encoder as AacEnc, EncoderConfig as AacCfg};
    use filmcraft_h264enc::{EncoderConfig, PacketFormat, Preset, Profile, RateControl, YuvFrame};
    use filmcraft_isobmff::{AvcConfig, Brand, Mp4Writer, SampleEntry, TrackConfig, WriteSample, WriterOptions};

    let (w, h) = (1280u32, 720u32);
    let (fps_num, fps_den) = (30u32, 1u32);
    let frames = 300u32; // 10 s
    let (sr, chans) = (48_000u32, 2usize);

    let mut vcfg = EncoderConfig::new(w, h, fps_num, fps_den);
    vcfg.profile = Profile::High;
    vcfg.preset = Preset::Balanced;
    vcfg.rate = RateControl::Crf(23.0);
    vcfg.keyint = 60;
    vcfg.bframes = 2;
    vcfg.format = PacketFormat::LengthPrefixed;
    vcfg.threads = 2;
    let mut enc = filmcraft_h264enc::Encoder::new(vcfg).map_err(|e| format!("h264 encoder: {e}"))?;
    let avc_cfg = AvcConfig::parse(&enc.avcc()).unwrap_or_else(|_| {
        let (sps, pps) = enc.sps_pps();
        AvcConfig::new(vec![sps], vec![pps], 4)
    });
    let entry_v = SampleEntry::avc(avc_cfg, w as u16, h as u16);

    let mut aac = AacEnc::new(AacCfg::cbr(sr, chans, 128_000)).map_err(|e| format!("aac encoder: {e}"))?;
    let entry_a = SampleEntry::aac(aac.audio_specific_config(), chans as u32, sr);

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(out)
        .map_err(|e| e.to_string())?;
    let mut mux = Mp4Writer::new(file, WriterOptions::new(Brand::Mp4)).map_err(|e| format!("muxer: {e}"))?;
    let mut vtc = TrackConfig::new(entry_v, fps_num);
    if enc.delay() > 0 {
        vtc.media_start = Some(fps_den as i64);
    }
    let vt = mux.add_track(vtc).map_err(|e| format!("video track: {e}"))?;
    let mut atc = TrackConfig::new(entry_a, sr);
    atc.media_start = Some(aac.priming_samples() as i64);
    let at = mux.add_track(atc).map_err(|e| format!("audio track: {e}"))?;

    let t0 = Instant::now();
    for i in 0..frames {
        let rgba = synthetic_frame(i, w, h);
        let (y, u, v) = rgba_to_yuv420(&rgba, w, h);
        let yf = YuvFrame { y: &y, u: &u, v: &v, y_stride: w as usize, uv_stride: (w / 2) as usize };
        let pkts = enc.encode(&yf, (i as i64) * (fps_den as i64)).map_err(|e| format!("encode {i}: {e}"))?;
        for p in pkts {
            mux.write_sample(vt, WriteSample { data: &p.data, duration: fps_den, composition_offset: (p.pts - p.dts) as i32, is_sync: p.keyframe })
                .map_err(|e| format!("video mux: {e}"))?;
        }
        let samples_per_frame = (sr / fps_num) as usize;
        let mut left = Vec::with_capacity(samples_per_frame);
        let mut right = Vec::with_capacity(samples_per_frame);
        let base = i as usize * samples_per_frame;
        for s in 0..samples_per_frame {
            let ph = (base + s) as f64 / sr as f64 * 440.0 * std::f64::consts::TAU;
            left.push((ph.sin() * 0.4) as f32);
            right.push((ph * 1.001).sin() as f32 * 0.4);
        }
        for au in aac.encode(&[&left, &right]) {
            mux.write_sample(at, WriteSample { data: &au, duration: 1024, composition_offset: 0, is_sync: true })
                .map_err(|e| format!("audio mux: {e}"))?;
        }
    }
    for p in enc.flush() {
        mux.write_sample(vt, WriteSample { data: &p.data, duration: fps_den, composition_offset: (p.pts - p.dts) as i32, is_sync: p.keyframe })
            .map_err(|e| format!("video flush mux: {e}"))?;
    }
    for au in aac.flush() {
        mux.write_sample(at, WriteSample { data: &au, duration: 1024, composition_offset: 0, is_sync: true })
            .map_err(|e| format!("audio flush mux: {e}"))?;
    }
    mux.finish_faststart().map_err(|e| format!("finish: {e}"))?;
    let enc_s = t0.elapsed().as_secs_f64();

    let mut probe = Command::new("/usr/bin/ffprobe");
    probe.args(["-v", "error", "-show_entries", "stream=codec_name,codec_type,start_time,duration,r_frame_rate,nb_frames", "-of", "json"])
        .arg(out)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let po = probe.output().map_err(|e| e.to_string())?;
    let probe_json = String::from_utf8_lossy(&po.stdout).to_string();

    let mut chk = Command::new(ffmpeg_bin());
    chk.args(["-v", "error", "-i"])
        .arg(out)
        .args(["-f", "null", "-"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let co = chk.output().map_err(|e| e.to_string())?;
    let null_ok = co.status.success();
    let null_err = String::from_utf8_lossy(&co.stderr).to_string();

    let mut rt = "n/a".to_string();
    if let Ok(fc) = FilmcraftBackend::open(out) {
        let mut psnrs = Vec::new();
        for i in [30u32, 150, 299] {
            if let Ok((_, _, y, u, v)) = fc.decode_frame_yuv_at(FrameRate::new(30, 1).tick_of(i as i64).seconds()) {
                let synth = synthetic_frame(i, w, h);
                let (sy, su, sv) = rgba_to_yuv420(&synth, w, h);
                let mut a = y;
                a.extend_from_slice(&u);
                a.extend_from_slice(&v);
                let mut b = sy;
                b.extend_from_slice(&su);
                b.extend_from_slice(&sv);
                psnrs.push(psnr(&a, &b));
            }
        }
        rt = psnrs.iter().map(|p| format!("{p:.1}")).collect::<Vec<_>>().join(", ");
    }

    Ok(format!(
        "encode {:.1}s, null-decode ok={null_ok}, round-trip YUV PSNR dB [{rt}]\nffprobe:\n{probe_json}{null_err}",
        enc_s
    ))
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpus = here.join("corpus");

    let videos: &[(&str, bool)] = &[
        ("h264_30fps.mp4", false),
        ("h264_2997.mp4", false),
        ("hevc_30fps.mp4", false),
        ("vp9_30fps.webm", false),
        ("h264_vfr.mp4", true),
    ];
    let audios: &[&str] = &["aac_48k.m4a", "opus_48k.opus"];

    println!("=== AniMathIO 2.0 media spike ===");

    println!("\n--- DECODE + SEEK ---");
    for (name, vfr) in videos {
        let p = corpus.join(name);
        if !p.exists() {
            println!("{name}: MISSING (run scripts/gen_corpus.sh)");
            continue;
        }
        let r = test_video(&p, *vfr);
        println!(
            "{file}: codec={codec} container={container} ok={ok}\n    decode_fps={fps:.1} seq_yuv_psnr={seq:.1}dB seek_p50={p50:.2}ms p95={p95:.2}ms cold_seek={cold:.2}ms seek_match={m:.1}%\n    note: {note}",
            file = r.file,
            codec = r.codec,
            container = r.container,
            ok = r.ok,
            fps = r.decode_fps,
            seq = r.seq_match,
            p50 = r.seek_p50_ms,
            p95 = r.seek_p95_ms,
            cold = r.cold_seek_ms,
            m = r.seek_match_pct,
            note = r.note,
        );
    }

    println!("\n--- AUDIO ---");
    for name in audios {
        let p = corpus.join(name);
        if !p.exists() {
            println!("{name}: MISSING");
            continue;
        }
        let r = test_audio(&p);
        println!(
            "{file}: codec={codec} sr={sr} ch={ch} ok={ok} rms={e:.5} best_lag={lag}samples rms_at_best_lag={al:.5}\n    note: {note}",
            file = r.file,
            codec = r.codec,
            sr = r.sample_rate,
            ch = r.channels,
            ok = r.ok,
            e = r.rms_err_vs_ffmpeg,
            lag = r.best_lag_samples,
            al = r.rms_at_best_lag,
            note = r.note,
        );
    }

    println!("\n--- ENCODE ---");
    let out = here.join("out.mp4");
    match encode_mp4(&out) {
        Ok(report) => println!("{report}"),
        Err(e) => println!("encode FAILED: {e}"),
    }
    let _ = std::fs::remove_file(out);
}
