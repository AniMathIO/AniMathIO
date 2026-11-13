use crate::Error;
use animathio_core::{TICKS_PER_SECOND, Time, TimeRange};
use std::{fs::File, path::Path, time::Duration};
use symphonia::core::{
    audio::{AudioBufferRef, SampleBuffer},
    codecs::{CODEC_TYPE_OPUS, DecoderOptions},
    errors::Error as DecodeError,
    formats::FormatOptions,
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
};

/// Highest sample rate accepted from untrusted media, in Hz.
pub const MAX_SAMPLE_RATE: u32 = 768_000;
/// Highest channel count accepted from untrusted media.
pub const MAX_CHANNELS: u16 = 32;
/// Hard cap on the total number of interleaved `f32` samples held in one buffer.
pub const MAX_INTERLEAVED_SAMPLES: usize = 1 << 31;

/// Resource limits applied while eagerly decoding untrusted media.
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

/// An asset decoder. Reads use the native sample lattice: every frame whose start
/// lies in the half-open media-time range is returned, zero padded beyond EOF.
/// Implementations may cache or stream; neither belongs in `Project`.
pub trait AudioSource: Send + Sync {
    /// Decode all PCM for preparing an immutable mixer snapshot.
    fn decode_all(&self) -> Result<Pcm, Error>;
    /// Decode native PCM over a media-time interval.
    fn decode(&self, range: TimeRange) -> Result<Pcm, Error> {
        let pcm = self.decode_all()?;
        let (start, end) = sample_bounds(range, pcm.rate)?;
        let channels = usize::from(pcm.channels);
        let len = (end - start)
            .checked_mul(channels)
            .ok_or_else(|| Error::Invalid("PCM range too large".into()))?;
        let mut out = vec![0.0; len];
        let available = pcm.frames().min(end).saturating_sub(start);
        if available != 0 {
            out[..available * channels]
                .copy_from_slice(&pcm.samples[start * channels..(start + available) * channels]);
        }
        Pcm::new(pcm.rate, pcm.channels, out)
    }
}
impl AudioSource for Pcm {
    fn decode_all(&self) -> Result<Pcm, Error> {
        Ok(self.clone())
    }
}

/// Symphonia adapter, eagerly decoded once. WAV/PCM, MP3, FLAC, AAC-LC in
/// ADTS/M4A, ALAC and Ogg Vorbis are enabled. Symphonia 0.5 has no Opus decoder;
/// Opus and other unavailable codecs return `Error::Unsupported`.
#[derive(Clone, Debug)]
pub struct SymphoniaSource(Pcm);
impl SymphoniaSource {
    /// Probe actual bytes (the filename is only a hint), then decode the default audio
    /// track using the default resource limits.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        Self::open_with_limits(path, DecodeLimits::default())
    }
    /// Probe actual bytes (the filename is only a hint), then decode the default audio
    /// track, enforcing `limits` on decoded duration and interleaved sample count.
    pub fn open_with_limits(path: impl AsRef<Path>, limits: DecodeLimits) -> Result<Self, Error> {
        let path = path.as_ref();
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|s| s.to_str()) {
            hint.with_extension(extension);
        }
        let stream = MediaSourceStream::new(
            Box::new(File::open(path)?),
            MediaSourceStreamOptions::default(),
        );
        Self::decode(stream, limits, &hint)
    }
    /// Decode already-read asset bytes. The content is probed directly, so the
    /// caller does not need to supply a filename or extension.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, Error> {
        Self::from_bytes_with_limits(bytes, DecodeLimits::default())
    }
    /// Decode in-memory bytes, enforcing `limits` on decoded duration and
    /// interleaved sample count.
    pub fn from_bytes_with_limits(bytes: Vec<u8>, limits: DecodeLimits) -> Result<Self, Error> {
        let stream = MediaSourceStream::new(
            Box::new(std::io::Cursor::new(bytes)),
            MediaSourceStreamOptions::default(),
        );
        Self::decode(stream, limits, &Hint::new())
    }
    /// Borrow the decoded PCM without cloning it.
    #[must_use]
    pub fn pcm(&self) -> &Pcm {
        &self.0
    }
    fn decode(stream: MediaSourceStream, limits: DecodeLimits, hint: &Hint) -> Result<Self, Error> {
        let mut format = symphonia::default::get_probe()
            .format(
                hint,
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
        if track.codec_params.codec == CODEC_TYPE_OPUS {
            return Err(Error::Unsupported(
                "Opus decoding is unavailable in Symphonia 0.5".into(),
            ));
        }
        let id = track.id;
        let mut decoder = symphonia::default::get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map_err(decode_error)?;
        let mut samples = Vec::new();
        let mut spec = None;
        let mut decoded_frames = 0usize;
        loop {
            let packet = match format.next_packet() {
                Ok(packet) => packet,
                Err(DecodeError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    break;
                }
                Err(e) => return Err(decode_error(e)),
            };
            if packet.track_id() != id {
                continue;
            }
            let audio = decoder.decode(&packet).map_err(decode_error)?;
            let audio_spec = *audio.spec();
            if spec.is_some_and(|s| s != audio_spec) {
                return Err(Error::Unsupported("midstream rate/channel change".into()));
            }
            spec = Some(audio_spec);
            record_decoded(&mut samples, &mut decoded_frames, audio, limits)?;
        }
        let spec = spec.ok_or_else(|| Error::Invalid("no decoded audio frames".into()))?;
        let channels =
            u16::try_from(spec.channels.count()).map_err(|e| Error::Invalid(e.to_string()))?;
        Ok(Self(Pcm::new(spec.rate, channels, samples)?))
    }
}

fn record_decoded(
    samples: &mut Vec<f32>,
    decoded_frames: &mut usize,
    audio: AudioBufferRef<'_>,
    limits: DecodeLimits,
) -> Result<(), Error> {
    let spec = *audio.spec();
    let channels = spec.channels.count();
    if spec.rate == 0 || spec.rate > MAX_SAMPLE_RATE {
        return Err(Error::Unsupported(format!(
            "sample rate {} Hz is out of range",
            spec.rate
        )));
    }
    if channels == 0 || channels > usize::from(MAX_CHANNELS) {
        return Err(Error::Unsupported(format!(
            "channel count {channels} is out of range"
        )));
    }
    let capacity = audio.capacity();
    let packet_samples = capacity
        .checked_mul(channels)
        .ok_or_else(|| Error::Invalid("decoder frame count overflow".into()))?;
    if packet_samples > limits.max_samples {
        return Err(Error::Invalid(
            "decoder produced an oversized packet".into(),
        ));
    }
    let mut buffer = SampleBuffer::<f32>::new(capacity as u64, spec);
    buffer.copy_interleaved_ref(audio);
    let incoming = buffer.samples();
    if !incoming.len().is_multiple_of(channels) {
        return Err(Error::Invalid("decoder produced partial frames".into()));
    }
    let incoming_frames = incoming.len() / channels;
    let total_frames = decoded_frames
        .checked_add(incoming_frames)
        .ok_or_else(|| Error::Invalid("decoded frame count overflow".into()))?;
    let max_frames = duration_to_frames(limits.max_duration, spec.rate)?;
    if total_frames > max_frames {
        return Err(Error::Invalid(format!(
            "decoded audio exceeds the {} second duration limit",
            limits.max_duration.as_secs()
        )));
    }
    let total_samples = samples
        .len()
        .checked_add(incoming.len())
        .ok_or_else(|| Error::Invalid("decoded sample count overflow".into()))?;
    if total_samples > limits.max_samples {
        return Err(Error::Invalid(format!(
            "decoded audio exceeds the {} sample limit",
            limits.max_samples
        )));
    }
    samples.extend_from_slice(incoming);
    *decoded_frames = total_frames;
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
impl AudioSource for SymphoniaSource {
    fn decode_all(&self) -> Result<Pcm, Error> {
        self.0.decode_all()
    }
}
fn decode_error(error: DecodeError) -> Error {
    match error {
        DecodeError::Unsupported(message) => Error::Unsupported(message.into()),
        other => Error::Decode(other.to_string()),
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
