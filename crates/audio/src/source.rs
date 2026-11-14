#![allow(clippy::cast_possible_truncation)] // Frame counts bounded by a segment or request.
use crate::{Error, layout::ChannelLayout};
use animathio_core::{TICKS_PER_SECOND, Time, TimeRange};
use std::{
    fs::File,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_OPUS, Decoder, DecoderOptions},
    errors::Error as DecodeError,
    formats::{FormatOptions, FormatReader, Packet},
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
    units::TimeBase,
};

/// Highest sample rate accepted from untrusted media, in Hz.
pub const MAX_SAMPLE_RATE: u32 = 768_000;
/// Highest channel count accepted from untrusted media.
pub const MAX_CHANNELS: u16 = 32;
/// Hard cap on the total number of interleaved `f32` samples held in one buffer.
pub const MAX_INTERLEAVED_SAMPLES: usize = 1 << 31;

/// Resource limits applied while scanning untrusted media at open.
#[derive(Clone, Copy, Debug)]
pub struct DecodeLimits {
    /// Maximum decoded media duration.
    pub max_duration: Duration,
    /// Hard cap on the total number of interleaved `f32` samples.
    pub max_samples: usize,
}
impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_duration: Duration::from_hours(4),
            max_samples: MAX_INTERLEAVED_SAMPLES,
        }
    }
}

/// Interleaved, finite PCM with an integer sample rate. Frames start at media time zero.
#[derive(Clone, Debug)]
pub struct Pcm {
    rate: u32,
    channels: u16,
    samples: Vec<f32>,
}
impl Pcm {
    /// Validate and own interleaved audio. Empty sources are allowed.
    pub fn new(rate: u32, channels: u16, samples: Vec<f32>) -> Result<Self, Error> {
        if rate == 0
            || rate > MAX_SAMPLE_RATE
            || channels == 0
            || channels > MAX_CHANNELS
            || samples.len() > MAX_INTERLEAVED_SAMPLES
            || !samples.len().is_multiple_of(usize::from(channels))
            || samples.iter().any(|s| !s.is_finite())
        {
            return Err(Error::Invalid("PCM rate/channels/samples".into()));
        }
        Ok(Self {
            rate,
            channels,
            samples,
        })
    }
    /// Native frames per second.
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.rate
    }
    /// Interleaved channel count.
    #[must_use]
    pub fn channels(&self) -> u16 {
        self.channels
    }
    /// Interleaved samples.
    #[must_use]
    pub fn samples(&self) -> &[f32] {
        &self.samples
    }
    /// Number of complete frames.
    #[must_use]
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels)
    }
}

/// Shape of a decoded stream: native rate, channel layout and exact length.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StreamInfo {
    /// Native frames per second.
    pub rate: u32,
    /// Interleaved channel count.
    pub channels: u16,
    /// Exact number of native frames (after gapless trimming).
    pub frames: u64,
    /// Speaker of each interleaved channel.
    pub layout: ChannelLayout,
}

/// A seekable, stateful view of one asset's native PCM. Readers are cheap, hold at most one
/// decoder, and never buffer the whole asset (except resident in-memory sources).
pub trait PcmReader: Send {
    /// Rate, channels and length of the stream.
    fn info(&self) -> &StreamInfo;
    /// Append the frames `[start, start + frames)` (interleaved) to `out`, clipped to the end
    /// of the stream, and return how many frames were appended. The result for a given
    /// `(start, frames)` never depends on earlier reads.
    fn read_at(&mut self, start: u64, frames: usize, out: &mut Vec<f32>) -> Result<usize, Error>;
}

/// An asset decoder. Reads use the native sample lattice: every frame whose start
/// lies in the half-open media-time range is returned, zero padded beyond EOF.
/// Implementations may cache or stream; neither belongs in `Project`.
pub trait AudioSource: Send + Sync {
    /// Decode all PCM into memory. Prefer [`AudioSource::open_reader`] for long media.
    fn decode_all(&self) -> Result<Pcm, Error>;
    /// Open a seekable reader. The default decodes everything once and serves it from memory;
    /// streaming sources override it.
    fn open_reader(&self) -> Result<Box<dyn PcmReader>, Error> {
        Ok(Box::new(ResidentReader::new(self.decode_all()?)))
    }
    /// Decode native PCM over a media-time interval.
    fn decode(&self, range: TimeRange) -> Result<Pcm, Error> {
        let mut reader = self.open_reader()?;
        let (rate, channels) = (reader.info().rate, reader.info().channels);
        let (start, end) = sample_bounds(range, rate)?;
        let frames = end - start;
        let len = frames
            .checked_mul(usize::from(channels))
            .ok_or_else(|| Error::Invalid("PCM range too large".into()))?;
        let mut out = Vec::with_capacity(len);
        reader.read_at(start as u64, frames, &mut out)?;
        out.resize(len, 0.0);
        Pcm::new(rate, channels, out)
    }
}
impl AudioSource for Pcm {
    fn decode_all(&self) -> Result<Pcm, Error> {
        Ok(self.clone())
    }
    fn open_reader(&self) -> Result<Box<dyn PcmReader>, Error> {
        Ok(Box::new(ResidentReader::new(self.clone())))
    }
    fn decode(&self, range: TimeRange) -> Result<Pcm, Error> {
        let (start, end) = sample_bounds(range, self.rate)?;
        let channels = usize::from(self.channels);
        let len = (end - start)
            .checked_mul(channels)
            .ok_or_else(|| Error::Invalid("PCM range too large".into()))?;
        let mut out = vec![0.0; len];
        let available = self.frames().min(end).saturating_sub(start);
        if available != 0 {
            out[..available * channels]
                .copy_from_slice(&self.samples[start * channels..(start + available) * channels]);
        }
        Pcm::new(self.rate, self.channels, out)
    }
}

/// Reader over PCM that already lives in memory.
pub struct ResidentReader {
    pcm: Pcm,
    info: StreamInfo,
}
impl ResidentReader {
    /// Serve `pcm` with the default layout for its channel count.
    #[must_use]
    pub fn new(pcm: Pcm) -> Self {
        let info = StreamInfo {
            rate: pcm.rate,
            channels: pcm.channels,
            frames: pcm.frames() as u64,
            layout: ChannelLayout::default_for(pcm.channels),
        };
        Self { pcm, info }
    }
}
impl PcmReader for ResidentReader {
    fn info(&self) -> &StreamInfo {
        &self.info
    }
    fn read_at(&mut self, start: u64, frames: usize, out: &mut Vec<f32>) -> Result<usize, Error> {
        let total = self.pcm.frames();
        let start = usize::try_from(start).unwrap_or(usize::MAX).min(total);
        let count = frames.min(total - start);
        let channels = usize::from(self.pcm.channels);
        out.extend_from_slice(&self.pcm.samples[start * channels..(start + count) * channels]);
        Ok(count)
    }
}

/// Where the encoded bytes of a [`SymphoniaSource`] live.
#[derive(Clone, Debug)]
enum Origin {
    File(PathBuf),
    Bytes(Arc<[u8]>),
}

/// Native frames per decode segment. Decoding is canonical per segment: it starts at a packet
/// at least [`preroll_frames`] before the segment and runs front to back, so the samples of a
/// frame never depend on which frames were read before it.
pub const SEGMENT_FRAMES: u64 = 1 << 21;

/// Packet to restart from for one segment: its ordinal among the track's packets, the frames
/// decoded before it and the frames it decoded to in the front-to-back pass. A fresh decoder
/// may output fewer frames for that first packet than the continuous one did (lapped codecs
/// such as Vorbis output nothing until they have a second block), so the difference realigns
/// the restarted stream.
type Checkpoint = (u64, u64, u64);

/// Streaming Symphonia adapter. WAV/PCM, MP3, FLAC, AAC-LC in ADTS/M4A, ALAC, Ogg Vorbis and
/// Opus (Ogg, Matroska/WebM, MP4; decoded by `filmcraft-opus`) are supported. Opening scans the
/// asset once (decoding in bounded memory) to learn its exact length, to enforce
/// [`DecodeLimits`] and to record a restart packet for every [`SEGMENT_FRAMES`]-frame segment;
/// afterwards memory is independent of the length. Reading a segment reopens the container,
/// skips to its restart packet without decoding and decodes from there, so output is a pure
/// function of the file: bit-identical to a front-to-back decode for codecs without long
/// inter-frame state (PCM, FLAC, MP3, AAC, Vorbis with its pre-roll) and converged after the
/// pre-roll for Opus. Cloning is cheap and shares the encoded bytes.
#[derive(Clone, Debug)]
pub struct SymphoniaSource {
    origin: Origin,
    extension: Option<String>,
    info: StreamInfo,
    segment_frames: u64,
    checkpoints: Arc<[Checkpoint]>,
}
impl SymphoniaSource {
    /// Probe actual bytes (the filename is only a hint), then scan the default audio
    /// track using the default resource limits.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::open_with_limits(path, DecodeLimits::default())
    }
    /// Probe actual bytes (the filename is only a hint), then scan the default audio
    /// track, enforcing `limits` on decoded duration and interleaved sample count.
    pub fn open_with_limits(path: impl AsRef<Path>, limits: DecodeLimits) -> Result<Self, Error> {
        let path = path.as_ref();
        let extension = path.extension().and_then(|s| s.to_str()).map(str::to_owned);
        Self::scan(
            Origin::File(path.to_owned()),
            extension,
            limits,
            SEGMENT_FRAMES,
        )
    }
    /// Use already-read asset bytes. The content is probed directly, so the
    /// caller does not need to supply a filename or extension.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, Error> {
        Self::from_bytes_with_limits(bytes, DecodeLimits::default())
    }
    /// Use in-memory bytes, enforcing `limits` on decoded duration and
    /// interleaved sample count.
    pub fn from_bytes_with_limits(bytes: Vec<u8>, limits: DecodeLimits) -> Result<Self, Error> {
        Self::scan(Origin::Bytes(bytes.into()), None, limits, SEGMENT_FRAMES)
    }
    /// [`SymphoniaSource::from_bytes_with_limits`] with a custom decode-segment length, for
    /// tests that need many segments in a short file. Segments shorter than the pre-roll
    /// (a quarter second) all restart from the beginning.
    pub fn from_bytes_with_segments(
        bytes: Vec<u8>,
        limits: DecodeLimits,
        segment_frames: u64,
    ) -> Result<Self, Error> {
        Self::scan(
            Origin::Bytes(bytes.into()),
            None,
            limits,
            segment_frames.max(1),
        )
    }
    /// Rate, channels and exact length discovered at open.
    #[must_use]
    pub fn info(&self) -> &StreamInfo {
        &self.info
    }
    fn scan(
        origin: Origin,
        extension: Option<String>,
        limits: DecodeLimits,
        segment_frames: u64,
    ) -> Result<Self, Error> {
        let mut stream = Stream::open(&origin, extension.as_deref(), None)?;
        let mut checkpoints = Vec::new();
        let frames = stream.drain(
            Some(limits),
            Some((segment_frames, &mut checkpoints)),
            &mut |_| {},
        )?;
        let (rate, channels, layout) = stream
            .spec
            .take()
            .ok_or_else(|| Error::Invalid("no decoded audio frames".into()))?;
        Ok(Self {
            origin,
            extension,
            info: StreamInfo {
                rate,
                channels,
                frames,
                layout,
            },
            segment_frames,
            checkpoints: checkpoints.into(),
        })
    }
    fn spec_tuple(&self) -> (u32, u16, ChannelLayout) {
        (self.info.rate, self.info.channels, self.info.layout.clone())
    }
}
impl AudioSource for SymphoniaSource {
    fn decode_all(&self) -> Result<Pcm, Error> {
        let samples_wanted = self
            .info
            .frames
            .checked_mul(u64::from(self.info.channels))
            .and_then(|n| usize::try_from(n).ok())
            .filter(|n| *n <= MAX_INTERLEAVED_SAMPLES)
            .ok_or_else(|| Error::Invalid("decoded audio too large for memory".into()))?;
        let mut stream = Stream::open(
            &self.origin,
            self.extension.as_deref(),
            Some(self.spec_tuple()),
        )?;
        let mut samples = Vec::with_capacity(samples_wanted);
        stream.drain(None, None, &mut |chunk| samples.extend_from_slice(chunk))?;
        Pcm::new(self.info.rate, self.info.channels, samples)
    }
    fn open_reader(&self) -> Result<Box<dyn PcmReader>, Error> {
        Ok(Box::new(SymphoniaReader {
            source: self.clone(),
            live: None,
        }))
    }
}

/// Pre-roll, in native frames, decoded and discarded at the start of a segment so codecs with
/// inter-frame state (MDCT overlap, MP3 bit reservoir, Opus energy prediction) have settled.
fn preroll_frames(rate: u32) -> u64 {
    u64::from(rate / 4).max(2048)
}

/// Records, while scanning, the packet to restart from for every segment.
struct CheckpointBuilder<'a> {
    segment: u64,
    preroll: u64,
    next: u64,
    last: Checkpoint,
    out: &'a mut Vec<Checkpoint>,
}
impl CheckpointBuilder<'_> {
    fn target(&self, segment: u64) -> u64 {
        segment
            .saturating_mul(self.segment)
            .saturating_sub(self.preroll)
    }
    /// Called with every packet's `(ordinal, frames before it, frames it decoded to)` in order.
    fn observe(&mut self, packet: Checkpoint) {
        while self.target(self.next) < packet.1 {
            self.out.push(self.last);
            self.next += 1;
        }
        // Among packets that decoded no frames (codec priming), restart from the first.
        if packet.1 > self.last.1 || packet.0 == 0 {
            self.last = packet;
        }
    }
    fn finish(&mut self, total_frames: u64) {
        while self.next.saturating_mul(self.segment) < total_frames {
            self.out.push(self.last);
            self.next += 1;
        }
    }
}

/// A decoder positioned inside one segment.
struct Live {
    stream: Stream,
    segment: u64,
    /// Frame index of `pending[0]`, or of the next packet when `pending` is empty.
    position: u64,
    /// Decoded interleaved frames not yet handed out.
    pending: Vec<f32>,
}

struct SymphoniaReader {
    source: SymphoniaSource,
    live: Option<Live>,
}
impl SymphoniaReader {
    fn restart(&mut self, segment: u64) -> Result<(), Error> {
        let source = &self.source;
        let (ordinal, frames_before, frames_decoded) = usize::try_from(segment)
            .ok()
            .and_then(|i| source.checkpoints.get(i))
            .copied()
            .ok_or_else(|| Error::Decode("segment beyond the end of the stream".into()))?;
        let mut stream = Stream::open(
            &source.origin,
            source.extension.as_deref(),
            Some(source.spec_tuple()),
        )?;
        stream.skip(ordinal)?;
        stream.position = frames_before;
        if ordinal > 0 {
            stream.opus_raw = frames_before + stream.ts_shift;
        }
        // Decode the restart packet itself: whatever it yields starts where the continuous
        // decode's output of that packet would have ended, minus what it yields.
        let preroll_end = segment * source.segment_frames;
        let channels = usize::from(source.info.channels);
        let mut pending = Vec::new();
        if let Some(decoded) = stream.next(preroll_end)? {
            pending.extend_from_slice(&stream.scratch[..decoded.frames * channels]);
        }
        let yielded = (pending.len() / channels) as u64;
        let end_of_packet = frames_before + frames_decoded;
        let position = end_of_packet
            .checked_sub(yielded)
            .ok_or_else(|| Error::Decode("restart packet decoded to too many frames".into()))?;
        stream.position = end_of_packet;
        self.live = Some(Live {
            stream,
            segment,
            position,
            pending,
        });
        Ok(())
    }
    /// Append frames `[a, b)`, which lie in `segment`; returns how many were available.
    fn read_piece(
        &mut self,
        segment: u64,
        a: u64,
        b: u64,
        out: &mut Vec<f32>,
    ) -> Result<u64, Error> {
        if self
            .live
            .as_ref()
            .is_none_or(|l| l.segment != segment || l.position > a)
        {
            self.restart(segment)?;
        }
        let channels = usize::from(self.source.info.channels);
        let preroll_end = segment * self.source.segment_frames;
        let live = self.live.as_mut().expect("reader was just positioned");
        let mut produced = 0;
        while live.position < b {
            if live.pending.is_empty() {
                match live.stream.next(preroll_end)? {
                    Some(decoded) => live
                        .pending
                        .extend_from_slice(&live.stream.scratch[..decoded.frames * channels]),
                    None => break,
                }
                continue;
            }
            let have = (live.pending.len() / channels) as u64;
            if live.position < a {
                let drop = have.min(a - live.position);
                live.pending.drain(..drop as usize * channels);
                live.position += drop;
            } else {
                let take = have.min(b - live.position);
                out.extend_from_slice(&live.pending[..take as usize * channels]);
                live.pending.drain(..take as usize * channels);
                live.position += take;
                produced += take;
            }
        }
        Ok(produced)
    }
}
impl PcmReader for SymphoniaReader {
    fn info(&self) -> &StreamInfo {
        &self.source.info
    }
    fn read_at(&mut self, start: u64, frames: usize, out: &mut Vec<f32>) -> Result<usize, Error> {
        let end = start
            .saturating_add(frames as u64)
            .min(self.source.info.frames);
        let mut at = start;
        let mut total = 0;
        while at < end {
            let segment = at / self.source.segment_frames;
            let piece_end = end.min((segment + 1) * self.source.segment_frames);
            let got = self.read_piece(segment, at, piece_end, out)?;
            total += got;
            if got < piece_end - at {
                break; // the stream ended early
            }
            at = piece_end;
        }
        Ok(total as usize)
    }
}

/// An opened container plus decoder, advancing packet by packet.
struct Stream {
    format: Box<dyn FormatReader>,
    decoder: FrameDecoder,
    track_id: u32,
    /// Opus pre-skip: frames to drop from the head of the stream.
    ts_shift: u64,
    /// Raw (untrimmed) frames the Opus decoder produced before the next packet.
    opus_raw: u64,
    /// The track's time base, to convert packet durations to frames.
    time_base: Option<TimeBase>,
    /// Rate, channels and layout, fixed by the first decoded packet.
    spec: Option<(u32, u16, ChannelLayout)>,
    /// Interleaved frames of the last decoded packet.
    scratch: Vec<f32>,
    /// Packets of the track consumed so far.
    ordinal: u64,
    /// Frames decoded (after trimming) before the next packet.
    position: u64,
}
struct Decoded {
    /// Frames in `scratch`.
    frames: usize,
}
enum FrameDecoder {
    Symphonia(Box<dyn Decoder>),
    Opus(filmcraft_opus::Decoder),
}
impl Stream {
    fn open(
        origin: &Origin,
        extension: Option<&str>,
        spec: Option<(u32, u16, ChannelLayout)>,
    ) -> Result<Self, Error> {
        let mut hint = Hint::new();
        if let Some(extension) = extension {
            hint.with_extension(extension);
        }
        let source: Box<dyn symphonia::core::io::MediaSource> = match origin {
            Origin::File(path) => Box::new(File::open(path)?),
            Origin::Bytes(bytes) => Box::new(std::io::Cursor::new(Arc::clone(bytes))),
        };
        let stream = MediaSourceStream::new(source, MediaSourceStreamOptions::default());
        let format = symphonia::default::get_probe()
            .format(
                &hint,
                stream,
                &FormatOptions {
                    enable_gapless: true,
                    ..FormatOptions::default()
                },
                &MetadataOptions::default(),
            )
            .map_err(decode_error)?
            .format;
        let track = format
            .tracks()
            .iter()
            .find(|track| track.codec_params.sample_rate.is_some())
            .or_else(|| format.default_track())
            .ok_or_else(|| Error::Unsupported("no audio track".into()))?;
        let track_id = track.id;
        let time_base = track.codec_params.time_base;
        let mut ts_shift = 0;
        let mut spec = spec;
        let decoder = if track.codec_params.codec == CODEC_TYPE_OPUS {
            let head = track
                .codec_params
                .extra_data
                .as_deref()
                .ok_or_else(|| Error::Invalid("Opus track without OpusHead".into()))?;
            // MP4 stores the header as a `dOps` box (big-endian fields); Symphonia hands it over
            // behind an `OpusHead` magic without converting, so MP4 needs the box parser.
            let mp4 = sniff_mp4(origin);
            let decoder = if mp4 && head.len() > 8 {
                let head = filmcraft_opus::OpusHead::from_dops(&head[8..]).map_err(opus_error)?;
                filmcraft_opus::Decoder::from_head(head, 48_000)
            } else {
                filmcraft_opus::Decoder::new(head)
            }
            .map_err(opus_error)?;
            let channels = u16::try_from(decoder.channels())
                .ok()
                .filter(|c| (1..=MAX_CHANNELS).contains(c))
                .ok_or_else(|| Error::Unsupported("Opus channel count is out of range".into()))?;
            // Symphonia's Ogg demuxer trims the end padding of Opus but not its pre-skip, and
            // the other containers trim neither, so the pre-skip is always dropped here.
            ts_shift = decoder.pre_skip() as u64;
            let layout = if decoder.head().mapping_family == 1 {
                ChannelLayout::vorbis(channels)
            } else {
                ChannelLayout::default_for(channels)
            };
            spec.get_or_insert((decoder.sample_rate(), channels, layout));
            FrameDecoder::Opus(decoder)
        } else {
            FrameDecoder::Symphonia(
                symphonia::default::get_codecs()
                    .make(&track.codec_params, &DecoderOptions::default())
                    .map_err(decode_error)?,
            )
        };
        Ok(Self {
            format,
            decoder,
            track_id,
            ts_shift,
            opus_raw: 0,
            time_base,
            spec,
            scratch: Vec::new(),
            ordinal: 0,
            position: 0,
        })
    }

    /// Consume `ordinal` packets of the track without decoding them.
    fn skip(&mut self, ordinal: u64) -> Result<(), Error> {
        while self.ordinal < ordinal {
            let packet = self.format.next_packet().map_err(decode_error)?;
            if packet.track_id() == self.track_id {
                self.ordinal += 1;
            }
        }
        Ok(())
    }

    /// Decode the next packet of the track into `scratch`; `Ok(None)` at the end. A packet that
    /// fails to decode before frame `tolerate_before` (the discarded pre-roll of a segment) is
    /// replaced by silence of its duration, so the position stays right.
    fn next(&mut self, tolerate_before: u64) -> Result<Option<Decoded>, Error> {
        loop {
            let packet = match self.format.next_packet() {
                Ok(packet) => packet,
                Err(DecodeError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    return Ok(None);
                }
                Err(e) => return Err(decode_error(e)),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            self.ordinal += 1;
            match self.decode_packet(&packet) {
                Ok(decoded) => {
                    self.position += decoded.frames as u64;
                    return Ok(Some(decoded));
                }
                Err((error, soft)) => {
                    let channels = self.spec.as_ref().map(|s| usize::from(s.1));
                    match channels {
                        Some(channels) if soft && self.position < tolerate_before => {
                            let frames = self.packet_frames(packet.dur);
                            self.scratch.clear();
                            self.scratch.resize(frames * channels, 0.0);
                            self.position += frames as u64;
                            return Ok(Some(Decoded { frames }));
                        }
                        _ => return Err(error),
                    }
                }
            }
        }
    }

    /// A packet duration in time-base units as native frames.
    fn packet_frames(&self, dur: u64) -> usize {
        let rate = self.spec.as_ref().map_or(1, |s| u128::from(s.0));
        let frames = match self.time_base {
            Some(tb) if tb.denom != 0 => {
                u128::from(dur) * u128::from(tb.numer) * rate / u128::from(tb.denom)
            }
            _ => u128::from(dur),
        };
        usize::try_from(frames).unwrap_or(0)
    }

    /// `Err((error, soft))`; `soft` errors are recoverable corruption inside one packet.
    fn decode_packet(&mut self, packet: &Packet) -> Result<Decoded, (Error, bool)> {
        self.scratch.clear();
        let (rate, channels, layout, skip, drop_end) = match &mut self.decoder {
            FrameDecoder::Symphonia(decoder) => {
                let audio = decoder.decode(packet).map_err(|e| {
                    let soft = matches!(e, DecodeError::DecodeError(_));
                    (decode_error(e), soft)
                })?;
                let spec = *audio.spec();
                let channels = u16::try_from(spec.channels.count())
                    .map_err(|e| (Error::Invalid(e.to_string()), false))?;
                let mut buffer = SampleBuffer::<f32>::new(audio.capacity() as u64, spec);
                buffer.copy_interleaved_ref(audio);
                self.scratch.extend_from_slice(buffer.samples());
                (
                    spec.rate,
                    channels,
                    ChannelLayout::from_symphonia(spec.channels),
                    0,
                    0,
                )
            }
            FrameDecoder::Opus(decoder) => {
                let planar = decoder
                    .decode(Some(&packet.data))
                    .map_err(|e| (opus_error(e), true))?;
                let frames = planar.first().map_or(0, Vec::len);
                for frame in 0..frames {
                    for channel in &planar {
                        self.scratch.push(channel[frame]);
                    }
                }
                let channels = u16::try_from(planar.len())
                    .map_err(|e| (Error::Invalid(e.to_string()), false))?;
                // Frames still owed to the pre-skip; a demuxer that already marked some of them
                // in `trim_start` must not be trimmed twice.
                let lead = self.ts_shift.saturating_sub(self.opus_raw);
                self.opus_raw += frames as u64;
                let skip = usize::try_from(lead)
                    .unwrap_or(usize::MAX)
                    .max(packet.trim_start as usize);
                let spec = self.spec.clone().unwrap_or_else(|| {
                    (
                        decoder.sample_rate(),
                        channels,
                        ChannelLayout::default_for(channels),
                    )
                });
                (spec.0, channels, spec.2, skip, packet.trim_end as usize)
            }
        };
        if rate == 0 || rate > MAX_SAMPLE_RATE {
            return Err((
                Error::Unsupported(format!("sample rate {rate} Hz is out of range")),
                false,
            ));
        }
        if channels == 0 || channels > MAX_CHANNELS {
            return Err((
                Error::Unsupported(format!("channel count {channels} is out of range")),
                false,
            ));
        }
        match &self.spec {
            Some((r, c, _)) if *r != rate || *c != channels => {
                return Err((
                    Error::Unsupported("midstream rate/channel change".into()),
                    false,
                ));
            }
            Some(_) => {}
            None => self.spec = Some((rate, channels, layout)),
        }
        let width = usize::from(channels);
        if !self.scratch.len().is_multiple_of(width) {
            return Err((
                Error::Invalid("decoder produced partial frames".into()),
                false,
            ));
        }
        let mut frames = self.scratch.len() / width;
        let skip = skip.min(frames);
        if skip > 0 {
            self.scratch.drain(..skip * width);
            frames -= skip;
        }
        let drop_end = drop_end.min(frames);
        frames -= drop_end;
        self.scratch.truncate(frames * width);
        Ok(Decoded { frames })
    }

    /// Decode every remaining packet sequentially, applying `limits` when given and recording
    /// segment restart packets into `checkpoints`. Returns the total frame count.
    fn drain(
        &mut self,
        limits: Option<DecodeLimits>,
        checkpoints: Option<(u64, &mut Vec<Checkpoint>)>,
        sink: &mut dyn FnMut(&[f32]),
    ) -> Result<u64, Error> {
        let mut pending_checkpoints = checkpoints;
        let mut builder: Option<CheckpointBuilder<'_>> = None;
        let mut total = 0_u64;
        loop {
            let (ordinal, before) = (self.ordinal, self.position);
            let Some(decoded) = self.next(0)? else { break };
            let packet = (ordinal, before, decoded.frames as u64);
            let (rate, channels) = self.spec.as_ref().map_or((48_000, 1), |s| (s.0, s.1));
            if let Some(limits) = limits {
                check_limits(
                    &mut total,
                    decoded.frames,
                    rate,
                    usize::from(channels),
                    limits,
                )?;
            } else {
                total += decoded.frames as u64;
            }
            if builder.is_none()
                && let Some((segment, out)) = pending_checkpoints.take()
            {
                builder = Some(CheckpointBuilder {
                    segment,
                    preroll: preroll_frames(rate),
                    next: 0,
                    last: packet,
                    out,
                });
            }
            if let Some(builder) = builder.as_mut() {
                builder.observe(packet);
            }
            sink(&self.scratch);
        }
        if let Some(builder) = builder.as_mut() {
            builder.finish(total);
        }
        Ok(total)
    }
}

fn check_limits(
    total: &mut u64,
    incoming_frames: usize,
    rate: u32,
    channels: usize,
    limits: DecodeLimits,
) -> Result<(), Error> {
    let packet_samples = incoming_frames
        .checked_mul(channels)
        .ok_or_else(|| Error::Invalid("decoder frame count overflow".into()))?;
    if packet_samples > limits.max_samples {
        return Err(Error::Invalid(
            "decoder produced an oversized packet".into(),
        ));
    }
    let new_total = total
        .checked_add(incoming_frames as u64)
        .ok_or_else(|| Error::Invalid("decoded frame count overflow".into()))?;
    let max_frames = duration_to_frames(limits.max_duration, rate)?;
    if new_total > max_frames as u64 {
        return Err(Error::Invalid(format!(
            "decoded audio exceeds the {} second duration limit",
            limits.max_duration.as_secs()
        )));
    }
    let total_samples = usize::try_from(new_total)
        .ok()
        .and_then(|n| n.checked_mul(channels))
        .ok_or_else(|| Error::Invalid("decoded sample count overflow".into()))?;
    if total_samples > limits.max_samples {
        return Err(Error::Invalid(format!(
            "decoded audio exceeds the {} sample limit",
            limits.max_samples
        )));
    }
    *total = new_total;
    Ok(())
}

fn duration_to_frames(duration: Duration, rate: u32) -> Result<usize, Error> {
    let rate = u128::from(rate);
    let whole = u128::from(duration.as_secs())
        .checked_mul(rate)
        .ok_or_else(|| Error::Invalid("duration limit overflow".into()))?;
    let fraction = u128::from(duration.subsec_nanos()) * rate / 1_000_000_000;
    usize::try_from(whole + fraction).map_err(|e| Error::Invalid(e.to_string()))
}
fn decode_error(error: DecodeError) -> Error {
    match error {
        DecodeError::Unsupported(message) => Error::Unsupported(message.into()),
        other => Error::Decode(other.to_string()),
    }
}
/// Whether the container is ISO-BMFF (MP4/M4A), judged from the `ftyp` box.
fn sniff_mp4(origin: &Origin) -> bool {
    use std::io::Read;
    let mut head = [0_u8; 8];
    let ok = match origin {
        Origin::File(path) => File::open(path)
            .and_then(|mut file| file.read_exact(&mut head))
            .is_ok(),
        Origin::Bytes(bytes) if bytes.len() >= 8 => {
            head.copy_from_slice(&bytes[..8]);
            true
        }
        Origin::Bytes(_) => false,
    };
    ok && &head[4..8] == b"ftyp"
}
fn opus_error(error: filmcraft_opus::Error) -> Error {
    match error {
        filmcraft_opus::Error::Unsupported(_) => Error::Unsupported(format!("Opus: {error}")),
        other => Error::Decode(format!("Opus: {other}")),
    }
}

pub(crate) fn sample_bounds(range: TimeRange, rate: u32) -> Result<(usize, usize), Error> {
    if rate == 0 || range.start.0 < 0 || range.end < range.start {
        return Err(Error::Invalid("sample rate or time range".into()));
    }
    Ok((
        sample_ceil(range.start, rate)?,
        sample_ceil(range.end, rate)?,
    ))
}
fn sample_ceil(time: Time, rate: u32) -> Result<usize, Error> {
    let n = i128::from(time.0) * i128::from(rate);
    usize::try_from((n + i128::from(TICKS_PER_SECOND) - 1) / i128::from(TICKS_PER_SECOND))
        .map_err(|e| Error::Invalid(e.to_string()))
}
