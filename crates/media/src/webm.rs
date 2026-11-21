//! WebM export: VP9 video + Opus audio in a Matroska/WebM container.
//!
//! Everything here is pure Rust and in process: `effectcraft-vp9enc` (VP9 profile 0, 8-bit 4:2:0),
//! `effectcraft-opusenc` (Opus, 48 kHz, 20 ms packets) and `filmcraft-matroska` (muxer). The
//! encoder mirrors [`Mp4Encoder`](crate::Mp4Encoder): the same [`EncoderConfig`], the same
//! RGBA8 to BT.709 4:2:0 conversion and the same call sequence.

use std::collections::VecDeque;
use std::io::Cursor;

use effectcraft_opusenc::OpusEncoder;
use effectcraft_vp9enc::{EncoderConfig as Vp9Config, Vp9Encoder};
use filmcraft_matroska::{Colour, MkvWriter, MuxOptions, TrackKind, TrackSpec};

use crate::encoder::{EncoderConfig, Quality, rgba_to_yuv420};
use crate::{MediaError, Result};

/// Samples per channel in one Opus packet (20 ms at 48 kHz).
const OPUS_FRAME: usize = OpusEncoder::FRAME_SIZE;
/// Opus always runs at 48 kHz inside WebM.
const OPUS_RATE: u32 = 48_000;
/// `SeekPreRoll` for Opus, 80 ms (RFC 7845 section 4.6).
const OPUS_SEEK_PRE_ROLL_NS: u64 = 80_000_000;

/// One encoded packet waiting for its place in the file.
struct Packet {
    pts_ns: i64,
    key: bool,
    data: Vec<u8>,
}

/// Map the shared H.264-style CRF scale (0 best .. 51 worst) onto the VP9 encoder's 1..=99
/// quality scale. CRF 23 (the default) lands on quality 72 (`base_q_idx` 84).
fn vp9_quality(crf: f32) -> u8 {
    (100.0 - f64::from(crf) * 1.2).round().clamp(1.0, 99.0) as u8
}

/// A streaming WebM (VP9 + optional Opus) encoder writing to an in-memory buffer.
///
/// Frames are fed in presentation order with [`encode_video_frame`](Self::encode_video_frame);
/// audio is fed as interleaved f32 PCM with [`encode_audio`](Self::encode_audio). Video and
/// audio packets are interleaved by presentation time so clusters are strictly ascending.
pub struct WebmEncoder {
    width: u32,
    height: u32,
    frame_rate: (u32, u32),
    audio_channels: usize,
    vp9: Vp9Encoder,
    opus: Option<OpusEncoder>,
    mux: MkvWriter<Cursor<Vec<u8>>>,
    video_track: usize,
    audio_track: Option<usize>,
    /// Opus decoder delay in nanoseconds; the first packet is presented that long before zero.
    opus_delay_ns: i64,
    /// Opus pre-skip in frames: the encoder lags its input by this much, so `finish` feeds that
    /// much silence to flush the end of the audio.
    opus_pre_skip: usize,
    frame_index: u64,
    opus_packets: u64,
    audio_buf: Vec<f32>,
    video_queue: VecDeque<Packet>,
    audio_queue: VecDeque<Packet>,
    encoded_bytes: usize,
}

impl WebmEncoder {
    /// Create an encoder. Dimensions must be even, audio (when present) must be 48 kHz mono or
    /// stereo, because that is what the Opus encoder supports.
    pub fn new(config: EncoderConfig) -> Result<Self> {
        let video = config.video;
        if video.width == 0 || video.height == 0 {
            return Err(MediaError::Invalid(
                "video dimensions must be positive".into(),
            ));
        }
        crate::limits::frame_bytes(video.width, video.height)?;
        if !video.width.is_multiple_of(2) || !video.height.is_multiple_of(2) {
            return Err(MediaError::Invalid(
                "video dimensions must be even for 4:2:0".into(),
            ));
        }
        let (vp9, video_spec) = video_stream(&video)?;
        let (fps_num, fps_den) = (video.frame_rate.numerator(), video.frame_rate.denominator());
        let mut tracks = vec![video_spec];

        let audio_channels = config.audio.map_or(0, |a| a.channels);
        let mut opus = None;
        let mut opus_delay_ns = 0;
        let mut opus_pre_skip = 0;
        if let Some(audio) = config.audio {
            crate::limits::audio(audio.sample_rate, audio.channels, 0)?;
            if audio.sample_rate != OPUS_RATE {
                return Err(MediaError::Invalid(format!(
                    "WebM audio is Opus at 48 kHz; {} Hz is not supported",
                    audio.sample_rate
                )));
            }
            let channels = u8::try_from(audio.channels)
                .ok()
                .filter(|c| (1..=2).contains(c))
                .ok_or_else(|| {
                    MediaError::Invalid(format!(
                        "WebM (Opus) audio supports mono or stereo, not {} channels",
                        audio.channels
                    ))
                })?;
            let encoder = OpusEncoder::new(channels, audio.bitrate_bps);
            opus_delay_ns = i64::from(encoder.pre_skip()) * 1_000_000_000 / i64::from(OPUS_RATE);
            opus_pre_skip = usize::from(encoder.pre_skip());
            let mut spec = TrackSpec::new(TrackKind::Audio, "A_OPUS");
            spec.codec_private = encoder.opus_head();
            spec.audio = Some((f64::from(OPUS_RATE), u64::from(channels), None));
            spec.default_duration_ns = Some(20_000_000);
            spec.codec_delay_ns = opus_delay_ns.unsigned_abs();
            spec.seek_pre_roll_ns = OPUS_SEEK_PRE_ROLL_NS;
            tracks.push(spec);
            opus = Some(encoder);
        }

        let options = MuxOptions {
            doc_type: "webm".into(),
            writing_app: "AniMathIO".into(),
            ..MuxOptions::default()
        };
        let mux = MkvWriter::new(Cursor::new(Vec::new()), tracks, options)
            .map_err(|e| MediaError::Format(format!("WebM muxer: {e}")))?;
        let audio_track = opus.is_some().then_some(1);
        Ok(Self {
            width: video.width,
            height: video.height,
            frame_rate: (fps_num, fps_den),
            audio_channels,
            vp9,
            opus,
            mux,
            video_track: 0,
            audio_track,
            opus_delay_ns,
            opus_pre_skip,
            frame_index: 0,
            opus_packets: 0,
            audio_buf: Vec::new(),
            video_queue: VecDeque::new(),
            audio_queue: VecDeque::new(),
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
        let convert = animathio_core::timing::Timer::new("encode-rgba-to-yuv");
        let (y, u, v) = rgba_to_yuv420(rgba, self.width, self.height);
        drop(convert);
        let _vp9 = animathio_core::timing::Timer::new("encode-vp9");
        let frame = self.vp9.encode_frame(&y, &u, &v, false);
        let pts_ns = frame_ns(self.frame_index, self.frame_rate.0, self.frame_rate.1)?;
        self.account(frame.data.len())?;
        self.video_queue.push_back(Packet {
            pts_ns,
            key: frame.key,
            data: frame.data,
        });
        self.frame_index += 1;
        self.drain(false)
    }

    /// Feed interleaved f32 PCM samples (a whole number of frames).
    pub fn encode_audio(&mut self, interleaved: &[f32]) -> Result<()> {
        let _timer = animathio_core::timing::Timer::new("encode-audio");
        if self.opus.is_none() {
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
        let packet_len = channels * OPUS_FRAME;
        let mut remaining = interleaved;
        if !self.audio_buf.is_empty() {
            let needed = (packet_len - self.audio_buf.len()).min(remaining.len());
            self.audio_buf.extend_from_slice(&remaining[..needed]);
            remaining = &remaining[needed..];
            if self.audio_buf.len() == packet_len {
                let chunk = std::mem::take(&mut self.audio_buf);
                self.encode_opus_packet(&chunk)?;
            }
        }
        let mut chunks = remaining.chunks_exact(packet_len);
        for chunk in &mut chunks {
            self.encode_opus_packet(chunk)?;
        }
        self.audio_buf.extend_from_slice(chunks.remainder());
        self.drain(false)
    }

    /// Flush the encoders and finish the file, returning the WebM bytes. The encoder lags its
    /// input by the pre-skip, so that much silence is fed after the last sample (otherwise the
    /// final ~2.5 ms of audio never leaves the encoder), and the last partial Opus packet is
    /// padded with silence, so the audio can run up to 20 ms past the last video frame.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        let _timer = animathio_core::timing::Timer::new("encode-flush-mux");
        if self.opus.is_some() && (self.opus_packets > 0 || !self.audio_buf.is_empty()) {
            let packet_len = self.audio_channels * OPUS_FRAME;
            let mut tail = std::mem::take(&mut self.audio_buf);
            tail.resize(tail.len() + self.opus_pre_skip * self.audio_channels, 0.0);
            tail.resize(tail.len().div_ceil(packet_len) * packet_len, 0.0);
            for chunk in tail.chunks_exact(packet_len) {
                self.encode_opus_packet(chunk)?;
            }
        }
        self.drain(true)?;
        let cursor = self
            .mux
            .finish()
            .map_err(|e| MediaError::Format(format!("WebM muxer: {e}")))?;
        Ok(cursor.into_inner())
    }

    fn encode_opus_packet(&mut self, chunk: &[f32]) -> Result<()> {
        let samples: Vec<f32> = chunk.iter().map(|s| s.clamp(-1.0, 1.0)).collect();
        let encoder = self.opus.as_mut().ok_or(MediaError::NoStream("audio"))?;
        let data = encoder.encode_float(&samples);
        // The first packet starts `opus_delay_ns` before time zero; the muxer adds the codec
        // delay back, so the first block lands on timestamp 0.
        let index = i64::try_from(self.opus_packets)
            .map_err(|_| MediaError::Invalid("audio packet count overflow".into()))?;
        let pts_ns = index
            .checked_mul(20_000_000)
            .ok_or_else(crate::limits::invalid)?
            - self.opus_delay_ns;
        self.opus_packets += 1;
        self.account(data.len())?;
        self.audio_queue.push_back(Packet {
            pts_ns,
            key: true,
            data,
        });
        Ok(())
    }

    fn account(&mut self, bytes: usize) -> Result<()> {
        self.encoded_bytes = self
            .encoded_bytes
            .checked_add(bytes)
            .filter(|n| *n <= crate::limits::MAX_INPUT_BYTES)
            .ok_or_else(crate::limits::invalid)?;
        Ok(())
    }

    /// Write queued packets in presentation order. While both streams are live a packet is only
    /// written once the other stream has a later one waiting; `all` writes everything left.
    fn drain(&mut self, all: bool) -> Result<()> {
        loop {
            let video_first = match (self.video_queue.front(), self.audio_queue.front()) {
                (Some(v), Some(a)) => v.pts_ns <= a.pts_ns,
                (Some(_), None) if self.audio_track.is_none() || all => true,
                (None, Some(_)) if all => false,
                _ => return Ok(()),
            };
            let (track, queue) = if video_first {
                (self.video_track, &mut self.video_queue)
            } else {
                (self.audio_track.unwrap_or(0), &mut self.audio_queue)
            };
            let Some(packet) = queue.pop_front() else {
                return Ok(());
            };
            self.mux
                .write_frame(track, packet.pts_ns, packet.key, &packet.data, None)
                .map_err(|e| MediaError::Format(format!("WebM muxer: {e}")))?;
        }
    }
}

/// The VP9 encoder and the matching WebM video track for `video`.
fn video_stream(video: &crate::encoder::VideoConfig) -> Result<(Vp9Encoder, TrackSpec)> {
    let (quality, target_kbps) = match video.quality {
        Quality::Crf(v) if !v.is_finite() || !(0.0..=51.0).contains(&v) => {
            return Err(MediaError::Invalid("CRF must be within 0..=51".into()));
        }
        Quality::Crf(v) => (vp9_quality(v), None),
        Quality::CbrKbps(0) => {
            return Err(MediaError::Invalid("bitrate must be positive".into()));
        }
        Quality::CbrKbps(kbps) => (72, Some(kbps)),
    };
    let (fps_num, fps_den) = (video.frame_rate.numerator(), video.frame_rate.denominator());
    let fps = f64::from(fps_num) / f64::from(fps_den.max(1));
    let vp9 = Vp9Encoder::new(Vp9Config {
        width: video.width,
        height: video.height,
        quality,
        full_range: false,
        // A key frame every two seconds, like the MP4 path.
        keyframe_interval: fps_num.div_ceil(fps_den.max(1)).saturating_mul(2).max(1),
        target_kbps,
        frame_rate: fps,
        loop_filter: true,
    });

    let mut video_spec = TrackSpec::new(TrackKind::Video, "V_VP9");
    video_spec.video_size = Some((video.width, video.height));
    video_spec.default_duration_ns = Some(frame_ns(1, fps_num, fps_den)?.unsigned_abs());
    // Explicit BT.709, limited range, 8-bit 4:2:0 (H.273 code points).
    video_spec.colour = Some(Colour {
        matrix_coefficients: Some(1),
        transfer_characteristics: Some(1),
        primaries: Some(1),
        range: Some(1),
        bits_per_channel: Some(8),
        chroma_subsampling_horz: Some(1),
        chroma_subsampling_vert: Some(1),
        ..Colour::default()
    });
    Ok((vp9, video_spec))
}

/// Presentation time of frame `index` in nanoseconds, exact for rational frame rates.
fn frame_ns(index: u64, num: u32, den: u32) -> Result<i64> {
    let ns = u128::from(index) * u128::from(den) * 1_000_000_000 / u128::from(num.max(1));
    i64::try_from(ns).map_err(|_| crate::limits::invalid())
}
