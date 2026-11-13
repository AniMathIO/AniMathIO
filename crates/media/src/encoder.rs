//! MP4 export: H.264 video + AAC audio via the FilmCraft encoders.

use std::io::Cursor;

use animathio_core::FrameRate;
use filmcraft_aac::{Encoder as AacEncoder, EncoderConfig as AacEncoderConfig};
use filmcraft_h264enc::{
    ColorConfig, Encoder as H264Encoder, EncoderConfig as H264EncoderConfig, Packet, PacketFormat,
    Preset, Profile, RateControl, YuvFrame,
};
use filmcraft_isobmff::{
    AvcConfig, Brand, Mp4Writer, SampleEntry, TrackConfig, WriteSample, WriterOptions,
};

use crate::{MediaError, Result};

/// Encoder quality target.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Quality {
    /// Constant quality (roughly the QP of P frames, 0–51; lower is better).
    Crf(f32),
    /// Constant bitrate target in kbit/s.
    CbrKbps(u32),
}

/// Video encoder configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoConfig {
    /// Width in pixels (must be even for 4:2:0).
    pub width: u32,
    /// Height in pixels (must be even for 4:2:0).
    pub height: u32,
    /// Output frame rate (exact rational).
    pub frame_rate: FrameRate,
    /// Quality target.
    pub quality: Quality,
    /// Encoder worker threads.
    pub threads: usize,
}

impl VideoConfig {
    /// A 1280×720, 30 fps, CRF 23 configuration.
    #[must_use]
    pub fn new(width: u32, height: u32, frame_rate: FrameRate) -> Self {
        Self {
            width,
            height,
            frame_rate,
            quality: Quality::Crf(23.0),
            threads: 2,
        }
    }
}

/// AAC audio encoder configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioConfig {
    /// Sample rate in Hz.
    pub sample_rate: u32,
    /// Channel count (1–8).
    pub channels: usize,
    /// Target bitrate in bits per second.
    pub bitrate_bps: u32,
}

impl AudioConfig {
    /// 48 kHz stereo at 128 kbit/s.
    #[must_use]
    pub fn new(sample_rate: u32, channels: usize, bitrate_bps: u32) -> Self {
        Self {
            sample_rate,
            channels,
            bitrate_bps,
        }
    }
}

/// MP4 encoder configuration.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EncoderConfig {
    /// Video settings.
    pub video: VideoConfig,
    /// Audio settings; `None` produces a video-only MP4.
    pub audio: Option<AudioConfig>,
    /// Whether to move the `moov` box to the front (`faststart`).
    pub faststart: bool,
}

impl EncoderConfig {
    /// A video-only configuration.
    #[must_use]
    pub fn video_only(video: VideoConfig) -> Self {
        Self {
            video,
            audio: None,
            faststart: true,
        }
    }

    /// A video + audio configuration.
    #[must_use]
    pub fn with_audio(video: VideoConfig, audio: AudioConfig) -> Self {
        Self {
            video,
            audio: Some(audio),
            faststart: true,
        }
    }
}

/// A streaming MP4 (H.264 + optional AAC) encoder writing to an in-memory buffer.
///
/// Frames are fed in presentation order with [`encode_video_frame`](Self::encode_video_frame);
/// audio is fed as interleaved f32 PCM with [`encode_audio`](Self::encode_audio). The video
/// stream signals BT.709 colour explicitly in its VUI.
pub struct Mp4Encoder {
    width: u32,
    height: u32,
    fps_den: u32,
    audio_channels: usize,
    faststart: bool,
    h264: H264Encoder,
    aac: Option<AacEncoder>,
    mux: Mp4Writer<Cursor<Vec<u8>>>,
    video_track: usize,
    audio_track: Option<usize>,
    frame_index: u64,
    audio_buf: Vec<f32>,
    encoded_bytes: usize,
}

impl Mp4Encoder {
    /// Create an encoder.
    #[allow(clippy::too_many_lines)]
    pub fn new(config: EncoderConfig) -> Result<Self> {
        let video = config.video;
        let audio_config = config.audio;
        crate::limits::frame_bytes(video.width, video.height)?;
        if video.threads > 3
            || matches!(video.quality, Quality::Crf(v) if !v.is_finite() || !(0.0..=51.0).contains(&v))
        {
            return Err(MediaError::Invalid("invalid encoder threads or CRF".into()));
        }
        if let Some(a) = audio_config {
            crate::limits::audio(a.sample_rate, a.channels, 0)?;
        }
        if video.width == 0 || video.height == 0 {
            return Err(MediaError::Invalid(
                "video dimensions must be positive".into(),
            ));
        }
        if !video.width.is_multiple_of(2) || !video.height.is_multiple_of(2) {
            return Err(MediaError::Invalid(
                "video dimensions must be even for 4:2:0".into(),
            ));
        }
        let fps_num = video.frame_rate.numerator();
        let fps_den = video.frame_rate.denominator();

        let mut h264_cfg = H264EncoderConfig::new(video.width, video.height, fps_num, fps_den);
        h264_cfg.profile = Profile::High;
        h264_cfg.preset = Preset::Balanced;
        h264_cfg.rate = match video.quality {
            Quality::Crf(crf) => RateControl::Crf(crf),
            Quality::CbrKbps(kbps) => RateControl::Cbr { kbps },
        };
        h264_cfg.keyint = fps_num.saturating_mul(2).max(1);
        h264_cfg.bframes = 2;
        h264_cfg.format = PacketFormat::LengthPrefixed;
        h264_cfg.threads = video.threads.max(1);
        // Explicit BT.709, limited-range signalling (also the default, written here on purpose).
        h264_cfg.color = ColorConfig {
            primaries: 1,
            transfer: 1,
            matrix: 1,
            full_range: false,
        };
        let h264 = H264Encoder::new(h264_cfg).map_err(|e| MediaError::Decode(e.to_string()))?;

        let avc_cfg = AvcConfig::parse(&h264.avcc()).unwrap_or_else(|_| {
            let (sps, pps) = h264.sps_pps();
            AvcConfig::new(vec![sps], vec![pps], 4)
        });
        let video_entry = SampleEntry::avc(avc_cfg, video.width as u16, video.height as u16);

        let audio_channels = audio_config.map_or(0, |a| a.channels);
        let mut aac = None;
        let mut audio_entry = None;
        if let Some(audio) = audio_config {
            if audio.sample_rate == 0 || audio.channels == 0 || audio.channels > 8 {
                return Err(MediaError::Invalid("invalid audio configuration".into()));
            }
            let aac_encoder = AacEncoder::new(AacEncoderConfig::cbr(
                audio.sample_rate,
                audio.channels,
                audio.bitrate_bps,
            ))
            .map_err(|e| MediaError::Decode(e.to_string()))?;
            audio_entry = Some(SampleEntry::aac(
                aac_encoder.audio_specific_config(),
                audio.channels as u32,
                audio.sample_rate,
            ));
            aac = Some(aac_encoder);
        }

        let cursor = Cursor::new(Vec::new());
        let mut mux = Mp4Writer::new(cursor, WriterOptions::new(Brand::Mp4))
            .map_err(|e| MediaError::Decode(e.to_string()))?;

        let mut video_track_cfg = TrackConfig::new(video_entry, fps_num);
        if h264.delay() > 0 {
            video_track_cfg.media_start = Some(i64::from(fps_den));
        }
        let video_track = mux
            .add_track(video_track_cfg)
            .map_err(|e| MediaError::Decode(e.to_string()))?;

        let mut audio_track = None;
        if let (Some(entry), Some(aac_encoder)) = (audio_entry, aac.as_ref()) {
            let sample_rate = audio_config.map_or(0, |a| a.sample_rate);
            let mut audio_track_cfg = TrackConfig::new(entry, sample_rate);
            audio_track_cfg.media_start = Some(i64::from(aac_encoder.priming_samples()));
            audio_track = Some(
                mux.add_track(audio_track_cfg)
                    .map_err(|e| MediaError::Decode(e.to_string()))?,
            );
        }

        Ok(Self {
            width: video.width,
            height: video.height,
            fps_den,
            audio_channels,
            faststart: config.faststart,
            h264,
            aac,
            mux,
            video_track,
            audio_track,
            frame_index: 0,
            audio_buf: Vec::new(),
            encoded_bytes: 0,
        })
    }

    /// Encode one RGBA8 frame in presentation order.
    pub fn encode_video_frame(&mut self, rgba: &[u8]) -> Result<()> {
        let _timer = animathio_core::timing::Timer::new("encode-video");

        let expected = crate::limits::frame_bytes(self.width, self.height)?;
        if self.frame_index >= crate::limits::MAX_FRAMES {
            return Err(crate::limits::invalid());
        }
        if rgba.len() != expected {
            return Err(MediaError::Invalid(format!(
                "frame buffer is {len} bytes, expected {expected}",
                len = rgba.len()
            )));
        }
        let (y, u, v) = rgba_to_yuv420(rgba, self.width, self.height);
        let yuv = YuvFrame {
            y: &y,
            u: &u,
            v: &v,
            y_stride: self.width as usize,
            uv_stride: (self.width / 2) as usize,
        };
        let frame_index = i64::try_from(self.frame_index)
            .map_err(|_| MediaError::Invalid("frame count overflow".into()))?;
        let pts = frame_index
            .checked_mul(i64::from(self.fps_den))
            .ok_or_else(crate::limits::invalid)?;
        let packets = self
            .h264
            .encode(&yuv, pts)
            .map_err(|e| MediaError::Decode(e.to_string()))?;
        for packet in packets {
            self.write_video_packet(&packet)?;
        }
        self.frame_index += 1;
        Ok(())
    }

    /// Feed interleaved f32 PCM samples (a whole number of frames).
    pub fn encode_audio(&mut self, interleaved: &[f32]) -> Result<()> {
        let _timer = animathio_core::timing::Timer::new("encode-audio");

        if self.audio_track.is_none() {
            return Err(MediaError::NoStream("audio"));
        }
        let channels = self.audio_channels;
        if !interleaved.len().is_multiple_of(channels) {
            return Err(MediaError::Invalid(
                "audio sample count is not a whole number of frames".into(),
            ));
        }
        if interleaved.len() > crate::limits::MAX_AUDIO_SAMPLES
            || interleaved.iter().any(|s| !s.is_finite())
        {
            return Err(crate::limits::invalid());
        }
        let frame_len = channels * filmcraft_aac::FRAME_LEN;
        let mut remaining = interleaved;
        if !self.audio_buf.is_empty() {
            let needed = (frame_len - self.audio_buf.len()).min(remaining.len());
            self.audio_buf.extend_from_slice(&remaining[..needed]);
            remaining = &remaining[needed..];
            if self.audio_buf.len() == frame_len {
                let chunk = std::mem::take(&mut self.audio_buf);
                self.encode_audio_frame(&chunk, channels)?;
            }
        }
        let mut chunks = remaining.chunks_exact(frame_len);
        for chunk in &mut chunks {
            self.encode_audio_frame(chunk, channels)?;
        }
        self.audio_buf.extend_from_slice(chunks.remainder());
        Ok(())
    }

    /// Flush encoders and finish the MP4, returning the file bytes.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        let _timer = animathio_core::timing::Timer::new("encode-flush-mux");
        if self.audio_track.is_some() {
            let channels = self.audio_channels;
            let frame_len = channels * filmcraft_aac::FRAME_LEN;
            if !self.audio_buf.is_empty() {
                self.audio_buf.resize(frame_len, 0.0);
                let chunk = std::mem::take(&mut self.audio_buf);
                self.encode_audio_frame(&chunk, channels)?;
            }
            if let Some(aac) = self.aac.as_mut() {
                for au in aac.flush() {
                    self.write_audio_access_unit(&au)?;
                }
            }
        }
        let packets = self.h264.flush();
        for packet in &packets {
            self.write_video_packet(packet)?;
        }
        let cursor = if self.faststart {
            self.mux
                .finish_faststart()
                .map_err(|e| MediaError::Decode(e.to_string()))?
        } else {
            self.mux
                .finish()
                .map_err(|e| MediaError::Decode(e.to_string()))?
        };
        Ok(cursor.into_inner())
    }

    fn encode_audio_frame(&mut self, chunk: &[f32], channels: usize) -> Result<()> {
        let planar: Vec<Vec<f32>> = (0..channels)
            .map(|c| chunk.iter().skip(c).step_by(channels).copied().collect())
            .collect();
        let slices: Vec<&[f32]> = planar.iter().map(Vec::as_slice).collect();
        let access_units = self
            .aac
            .as_mut()
            .ok_or(MediaError::NoStream("audio"))?
            .encode(&slices);
        for au in access_units {
            self.write_audio_access_unit(&au)?;
        }
        Ok(())
    }

    fn account_packet(&mut self, bytes: usize) -> Result<()> {
        self.encoded_bytes = self
            .encoded_bytes
            .checked_add(bytes)
            .filter(|n| *n <= crate::limits::MAX_INPUT_BYTES)
            .ok_or_else(crate::limits::invalid)?;
        Ok(())
    }

    fn write_video_packet(&mut self, packet: &Packet) -> Result<()> {
        self.account_packet(packet.data.len())?;
        self.mux
            .write_sample(
                self.video_track,
                WriteSample {
                    data: &packet.data,
                    duration: self.fps_den,
                    composition_offset: (packet.pts - packet.dts) as i32,
                    is_sync: packet.keyframe,
                },
            )
            .map_err(|e| MediaError::Decode(e.to_string()))
    }

    fn write_audio_access_unit(&mut self, au: &[u8]) -> Result<()> {
        self.account_packet(au.len())?;
        let Some(track) = self.audio_track else {
            return Ok(());
        };
        self.mux
            .write_sample(
                track,
                WriteSample {
                    data: au,
                    duration: filmcraft_aac::FRAME_LEN as u32,
                    composition_offset: 0,
                    is_sync: true,
                },
            )
            .map_err(|e| MediaError::Decode(e.to_string()))
    }
}

/// Convert straight-alpha RGBA8 to planar 4:2:0 YUV with BT.709 limited-range coefficients and
/// 2×2 chroma averaging.
fn rgba_to_yuv420(rgba: &[u8], width: u32, height: u32) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let (w, h) = (width as usize, height as usize);
    let mut y = vec![0u8; w * h];
    let cw = w / 2;
    let ch = h / 2;
    let mut u = vec![0u8; cw * ch];
    let mut v = vec![0u8; cw * ch];

    for row in 0..h {
        for col in 0..w {
            let (rf, gf, bf) = rgb_at(rgba, w, row, col);
            let yv = 0.2126 * rf + 0.7152 * gf + 0.0722 * bf;
            y[row * w + col] = (16.0 + 219.0 * yv).round().clamp(0.0, 255.0) as u8;
        }
    }

    for row in (0..ch).map(|r| r * 2) {
        for col in (0..cw).map(|c| c * 2) {
            let (mut rf, mut gf, mut bf) = (0.0, 0.0, 0.0);
            for dy in 0..2 {
                for dx in 0..2 {
                    let (r, g, b) = rgb_at(rgba, w, row + dy, col + dx);
                    rf += r;
                    gf += g;
                    bf += b;
                }
            }
            rf /= 4.0;
            gf /= 4.0;
            bf /= 4.0;
            let yv = 0.2126 * rf + 0.7152 * gf + 0.0722 * bf;
            let cb = (bf - yv) / 1.8556;
            let cr = (rf - yv) / 1.5748;
            let idx = (row / 2) * cw + col / 2;
            u[idx] = (128.0 + 224.0 * cb).round().clamp(0.0, 255.0) as u8;
            v[idx] = (128.0 + 224.0 * cr).round().clamp(0.0, 255.0) as u8;
        }
    }

    (y, u, v)
}

fn rgb_at(rgba: &[u8], w: usize, row: usize, col: usize) -> (f64, f64, f64) {
    let p = &rgba[(row * w + col) * 4..(row * w + col) * 4 + 3];
    (
        f64::from(p[0]) / 255.0,
        f64::from(p[1]) / 255.0,
        f64::from(p[2]) / 255.0,
    )
}
