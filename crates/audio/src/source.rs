use crate::Error;
use animathio_core::{TICKS_PER_SECOND, Time, TimeRange};
use std::{fs::File, path::Path};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{CODEC_TYPE_OPUS, DecoderOptions},
    errors::Error as DecodeError,
    formats::FormatOptions,
    io::{MediaSourceStream, MediaSourceStreamOptions},
    meta::MetadataOptions,
    probe::Hint,
};

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
            || channels == 0
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
    /// Probe actual bytes (the filename is only a hint), then decode the default audio track.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref();
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|s| s.to_str()) {
            hint.with_extension(extension);
        }
        let stream = MediaSourceStream::new(
            Box::new(File::open(path)?),
            MediaSourceStreamOptions::default(),
        );
        let mut format = symphonia::default::get_probe()
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
            .default_track()
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
            if spec.is_some_and(|s| s != *audio.spec()) {
                return Err(Error::Unsupported("midstream rate/channel change".into()));
            }
            spec = Some(*audio.spec());
            let mut buffer = SampleBuffer::<f32>::new(audio.capacity() as u64, *audio.spec());
            buffer.copy_interleaved_ref(audio);
            samples.extend_from_slice(buffer.samples());
        }
        let spec = spec.ok_or_else(|| Error::Invalid("no decoded audio frames".into()))?;
        let channels =
            u16::try_from(spec.channels.count()).map_err(|e| Error::Invalid(e.to_string()))?;
        Ok(Self(Pcm::new(spec.rate, channels, samples)?))
    }
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
