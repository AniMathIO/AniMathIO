//! Microphone capture, outside the project model.
//!
//! A [`CaptureSource`] lists input devices and opens one as a [`CaptureStream`] that is *polled*
//! for interleaved `f32` frames. A [`Recorder`] owns one open stream: it keeps the audio as
//! 16-bit PCM, measures a live input [`Level`], and [`Recorder::finish`] returns a [`Recording`]
//! whose [`Recording::into_wav`] bytes are an ordinary WAV asset. Nothing here touches
//! `animathio-core`'s `Project`; registering the asset and clip is the caller's job, through
//! undoable commands.
//!
//! Sources:
//! - [`CpalCapture`] opens the real input devices (cpal). It is only constructed when audio is
//!   enabled; listing devices only reads their names and opening happens on `Start`.
//! - [`NullCapture`] never opens anything and explains why (`ANIMATHIO_AUDIO=none`, headless
//!   runs).
//! - [`SyntheticCapture`] replays scripted samples deterministically, for device-free tests.
//!
//! Streams are polled on the caller's thread, so they need not be `Send` (cpal streams are not on
//! every platform); sources are `Send + Sync` so one can sit in a shared environment.

use crate::{
    Error,
    stream_error::{StreamErrorAction, classify_stream_error},
};
use animathio_core::{TICKS_PER_SECOND, Time};
use cpal::{
    FromSample, I24, SampleFormat, SizedSample, U24,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rtrb::{Consumer, Producer, RingBuffer};
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};

/// The most PCM bytes one recording keeps (16-bit): 512 MiB, about 44 minutes of 48 kHz stereo.
/// It stays under the project's 1 GiB asset limit; reaching it marks the recorder full.
pub const MAX_RECORDING_BYTES: usize = 512 * 1024 * 1024;

/// A recording keeps at most this many channels (the front pair of wider devices).
const MAX_RECORDED_CHANNELS: u16 = 2;

/// An input device a [`CaptureSource`] can open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputDevice {
    /// Stable-for-the-session identifier to pass back to [`CaptureSource::open`].
    pub id: String,
    /// Name to show.
    pub name: String,
    /// Whether this is the system default input.
    pub is_default: bool,
}

/// Sample rate and channel count of an open stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputFormat {
    /// Frames per second.
    pub rate: u32,
    /// Interleaved channels.
    pub channels: u16,
}

/// One open input stream, polled for the samples captured since the last poll.
pub trait CaptureStream {
    /// The device's format; [`CaptureStream::drain`] yields interleaved samples in it.
    fn format(&self) -> InputFormat;
    /// Append the interleaved samples captured since the last call to `out`. Samples still
    /// buffered when the device fails are delivered before the error.
    ///
    /// # Errors
    /// The device stopped (unplugged, revoked or a backend failure).
    fn drain(&mut self, out: &mut Vec<f32>) -> Result<(), Error>;
    /// Whole frames the device dropped since the stream opened (its buffer ran full while the
    /// poller was stalled, say because the interface thread was busy). Monotonic; the
    /// [`Recorder`] tracks the change between polls. Zero for sources that never drop.
    fn dropped_frames(&self) -> u64 {
        0
    }
}

/// Lists input devices and opens one.
pub trait CaptureSource: fmt::Debug + Send + Sync {
    /// Why this source will never open a device (audio is turned off), or `None` when it can.
    /// Lets a UI explain itself instead of listing no devices.
    fn disabled_reason(&self) -> Option<&str> {
        None
    }
    /// The available input devices, default first when the platform names one.
    ///
    /// # Errors
    /// Device enumeration failed or capture is disabled.
    fn devices(&self) -> Result<Vec<InputDevice>, Error>;
    /// Open `device` (an [`InputDevice::id`]) or the default input when `None`. The stream
    /// starts capturing immediately and stops when dropped.
    ///
    /// # Errors
    /// The device is missing, busy, refused access, or capture is disabled. The message is
    /// written for a person.
    fn open(&self, device: Option<&str>) -> Result<Box<dyn CaptureStream>, Error>;
}

/// A capture source that never opens a device. Used when audio is disabled (`ANIMATHIO_AUDIO=none`)
/// and by the headless test environment.
#[derive(Clone, Debug)]
pub struct NullCapture {
    reason: String,
}
impl NullCapture {
    /// A source that reports `reason` whenever something asks for a device.
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}
impl CaptureSource for NullCapture {
    fn disabled_reason(&self) -> Option<&str> {
        Some(&self.reason)
    }
    fn devices(&self) -> Result<Vec<InputDevice>, Error> {
        Ok(Vec::new())
    }
    fn open(&self, _device: Option<&str>) -> Result<Box<dyn CaptureStream>, Error> {
        Err(Error::Capture(self.reason.clone()))
    }
}

/// Peak and RMS of the samples one poll delivered, linear (1.0 is full scale).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Level {
    /// Largest absolute sample.
    pub peak: f32,
    /// Root mean square over all channels.
    pub rms: f32,
}
impl Level {
    /// Peak in dB relative to full scale (`f32::NEG_INFINITY` for silence).
    #[must_use]
    pub fn peak_db(self) -> f32 {
        20.0 * self.peak.log10()
    }
    /// Whether the peak reached full scale (the recording is probably distorted).
    #[must_use]
    pub fn clipped(self) -> bool {
        self.peak >= 0.999
    }
    fn of(samples: &[f32]) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        let mut peak = 0.0_f32;
        let mut square = 0.0_f64;
        for &sample in samples {
            let sample = if sample.is_finite() { sample } else { 0.0 };
            peak = peak.max(sample.abs());
            square += f64::from(sample) * f64::from(sample);
        }
        #[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
        // A poll holds far fewer than 2^52 samples and the result is at most 1.
        let rms = (square / samples.len() as f64).sqrt() as f32;
        Self {
            peak: peak.min(1.0),
            rms: rms.min(1.0),
        }
    }
}

/// An in-progress recording: one open stream, the 16-bit PCM captured so far and the live level.
/// Dropping it (or [`Recorder::cancel`]) discards the audio and closes the device.
pub struct Recorder {
    stream: Box<dyn CaptureStream>,
    format: InputFormat,
    source_channels: usize,
    pcm: Vec<i16>,
    scratch: Vec<f32>,
    /// Samples of a torn frame from the last poll (a poll can land between the device
    /// callback's frames), carried into the next one so the channels stay in order.
    tail: Vec<f32>,
    level: Level,
    max_samples: usize,
    full: bool,
    /// Frames the stream dropped so far, as seen on the last poll.
    seen_dropped: u64,
    /// Frames the stream dropped in total.
    dropped_frames: u64,
}
impl fmt::Debug for Recorder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Recorder")
            .field("format", &self.format)
            .field("frames", &self.frames())
            .field("full", &self.full)
            .finish_non_exhaustive()
    }
}
impl Recorder {
    /// Open `device` (or the default input) on `source` and start capturing.
    ///
    /// # Errors
    /// The source could not open the device; see [`CaptureSource::open`].
    pub fn start(source: &dyn CaptureSource, device: Option<&str>) -> Result<Self, Error> {
        Self::from_stream(source.open(device)?, MAX_RECORDING_BYTES)
    }

    /// Like [`Recorder::start`] with a custom size cap (bytes of 16-bit PCM), for tests.
    ///
    /// # Errors
    /// See [`Recorder::start`].
    pub fn start_with_limit(
        source: &dyn CaptureSource,
        device: Option<&str>,
        max_bytes: usize,
    ) -> Result<Self, Error> {
        Self::from_stream(source.open(device)?, max_bytes)
    }

    fn from_stream(stream: Box<dyn CaptureStream>, max_bytes: usize) -> Result<Self, Error> {
        let input = stream.format();
        if input.rate == 0 || input.channels == 0 {
            return Err(Error::Capture(
                "the input device reported an empty format".into(),
            ));
        }
        let format = InputFormat {
            rate: input.rate,
            channels: input.channels.min(MAX_RECORDED_CHANNELS),
        };
        Ok(Self {
            stream,
            format,
            source_channels: usize::from(input.channels),
            pcm: Vec::new(),
            scratch: Vec::new(),
            tail: Vec::new(),
            level: Level::default(),
            max_samples: max_bytes / 2,
            full: false,
            seen_dropped: 0,
            dropped_frames: 0,
        })
    }

    /// Move everything captured since the last poll into the recording and return its level.
    /// Call this regularly (every 30 to 100 ms); the device buffers about four seconds.
    ///
    /// # Errors
    /// The device failed. Audio captured so far is kept, so [`Recorder::finish`] still works.
    pub fn poll(&mut self) -> Result<Level, Error> {
        self.scratch.clear();
        let drained = self.stream.drain(&mut self.scratch);
        let dropped = self.stream.dropped_frames();
        self.dropped_frames += dropped.saturating_sub(self.seen_dropped);
        self.seen_dropped = dropped;
        // A poll can land between the device callback's frames, so `drain` may end with a
        // torn frame. Keep the leftovers and finish the frame from the next poll's samples
        // instead of dropping the odd sample and swapping the channels for the rest of the
        // take.
        self.tail.append(&mut self.scratch);
        let channels = usize::from(self.format.channels);
        let whole = self.tail.len() - self.tail.len() % self.source_channels;
        let mut metered = Vec::with_capacity(whole / self.source_channels * channels);
        let mut consumed = 0;
        for frame in self.tail[..whole].chunks_exact(self.source_channels) {
            if self.pcm.len() + channels > self.max_samples {
                self.full = true;
                break;
            }
            for &sample in &frame[..channels] {
                metered.push(sample);
                self.pcm.push(to_pcm16(sample));
            }
            consumed += self.source_channels;
        }
        if self.full {
            // The cap was reached: whatever is still buffered is dropped from here on.
            self.tail.clear();
        } else {
            self.tail.drain(..consumed);
        }
        self.level = Level::of(&metered);
        drained.map(|()| self.level)
    }

    /// Frames the input dropped because its buffer ran full while the poller was stalled
    /// (the interface was busy for longer than the stream buffers). The recording is
    /// shorter than the take was; the caller should tell the person.
    #[must_use]
    pub fn dropped_frames(&self) -> u64 {
        self.dropped_frames
    }

    /// The level of the last poll.
    #[must_use]
    pub fn level(&self) -> Level {
        self.level
    }

    /// The recorded format (at most two channels).
    #[must_use]
    pub fn format(&self) -> InputFormat {
        self.format
    }

    /// Frames recorded so far.
    #[must_use]
    pub fn frames(&self) -> u64 {
        (self.pcm.len() / usize::from(self.format.channels)) as u64
    }

    /// Recorded length so far.
    #[must_use]
    pub fn duration(&self) -> Time {
        frames_to_time(self.frames(), self.format.rate)
    }

    /// Whether the size cap was reached; later audio is dropped, so the caller should stop.
    #[must_use]
    pub fn is_full(&self) -> bool {
        self.full
    }

    /// Stop capturing, take the audio still buffered by the device, and close it.
    ///
    /// # Errors
    /// Nothing was recorded. A device that failed after capturing audio still yields a
    /// recording of what it delivered.
    pub fn finish(mut self) -> Result<Recording, Error> {
        let _ = self.poll();
        if self.pcm.is_empty() {
            return Err(Error::Capture(
                "nothing was recorded; check that the microphone is not muted".into(),
            ));
        }
        let Self {
            format,
            pcm,
            dropped_frames,
            ..
        } = self;
        Ok(Recording {
            rate: format.rate,
            channels: format.channels,
            frames: (pcm.len() / usize::from(format.channels)) as u64,
            dropped_frames,
            pcm,
        })
    }

    /// Discard the recording and close the device.
    pub fn cancel(self) {}
}

/// A finished recording: 16-bit PCM ready to become a WAV asset.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recording {
    /// Frames per second.
    pub rate: u32,
    /// Interleaved channels (one or two).
    pub channels: u16,
    /// Number of frames.
    pub frames: u64,
    /// Frames the input dropped during the take (see [`Recorder::dropped_frames`]).
    pub dropped_frames: u64,
    pcm: Vec<i16>,
}
impl Recording {
    /// Length of the recording.
    #[must_use]
    pub fn duration(&self) -> Time {
        frames_to_time(self.frames, self.rate)
    }
    /// The recording as a 16-bit PCM WAV file.
    #[must_use]
    pub fn into_wav(self) -> Vec<u8> {
        encode_wav(self.rate, self.channels, &self.pcm)
    }
}

/// Encode interleaved 16-bit PCM as a canonical 44-byte-header WAV file.
///
/// # Panics
/// The data exceeds the 4 GiB a WAV chunk can describe (recordings are capped far below it).
#[must_use]
pub fn encode_wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
    let data_len = u32::try_from(samples.len() * 2).expect("WAV data fits a RIFF chunk");
    let block_align = channels * 2;
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * u32::from(block_align)).to_le_bytes());
    wav.extend_from_slice(&block_align.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

fn to_pcm16(sample: f32) -> i16 {
    if !sample.is_finite() {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation)] // Clamped to the i16 range first.
    let value = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
    value
}

fn frames_to_time(frames: u64, rate: u32) -> Time {
    let ticks = u128::from(frames) * TICKS_PER_SECOND as u128 / u128::from(rate.max(1));
    Time(i64::try_from(ticks).unwrap_or(i64::MAX))
}

// ---------------------------------------------------------------------------------------------
// Synthetic source (tests).
// ---------------------------------------------------------------------------------------------

/// One scripted input device of a [`SyntheticCapture`].
#[derive(Clone, Debug)]
pub struct SyntheticDevice {
    /// Device name (also its id).
    pub name: String,
    /// Format the stream reports.
    pub format: InputFormat,
    /// The interleaved samples the device "hears", in order; silence follows the script.
    pub samples: Vec<f32>,
    /// Frames delivered per poll.
    pub chunk_frames: usize,
    /// Opening fails with this message (a permission or busy-device error).
    pub open_error: Option<String>,
    /// The stream fails with this message after this many polls (an unplugged device).
    pub fail_after_polls: Option<(usize, String)>,
    /// Whole frames the stream reports as dropped (a full buffer while the poller stalled).
    pub dropped_frames: u64,
    /// Deliver at most this many raw samples per poll instead of `chunk_frames` frames, so a
    /// frame can tear across polls as on a real device whose poll lands mid-callback.
    pub chunk_samples: Option<usize>,
}
impl SyntheticDevice {
    /// A device that hears `seconds` of a 440 Hz sine at `amplitude`, `chunk_frames` per poll.
    #[must_use]
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )] // Test signal: small, positive durations and sample counts.
    pub fn tone(
        name: &str,
        rate: u32,
        channels: u16,
        seconds: f32,
        amplitude: f32,
        chunk_frames: usize,
    ) -> Self {
        let frames = (seconds * rate as f32) as usize;
        let mut samples = Vec::with_capacity(frames * usize::from(channels));
        for n in 0..frames {
            let phase = 2.0 * std::f32::consts::PI * 440.0 * n as f32 / rate as f32;
            let value = amplitude * phase.sin();
            samples.extend(std::iter::repeat_n(value, usize::from(channels)));
        }
        Self::scripted(name, InputFormat { rate, channels }, samples, chunk_frames)
    }
    /// A device that hears exactly `samples` (interleaved).
    #[must_use]
    pub fn scripted(
        name: &str,
        format: InputFormat,
        samples: Vec<f32>,
        chunk_frames: usize,
    ) -> Self {
        Self {
            name: name.into(),
            format,
            samples,
            chunk_frames: chunk_frames.max(1),
            open_error: None,
            fail_after_polls: None,
            dropped_frames: 0,
            chunk_samples: None,
        }
    }
    /// The same device, but its stream reports `frames` dropped frames (a full buffer).
    #[must_use]
    pub fn dropping(mut self, frames: u64) -> Self {
        self.dropped_frames = frames;
        self
    }
    /// The same device, but each poll delivers at most `samples` raw samples instead of
    /// `chunk_frames` whole frames, so a frame tears across polls (as when a poll lands
    /// between a real device callback's frames).
    #[must_use]
    pub fn torn(mut self, samples: usize) -> Self {
        self.chunk_samples = Some(samples.max(1));
        self
    }
    /// The same device, but opening it fails with `message`.
    #[must_use]
    pub fn failing_open(mut self, message: &str) -> Self {
        self.open_error = Some(message.into());
        self
    }
    /// The same device, but its stream fails with `message` from the poll after `polls` polls.
    #[must_use]
    pub fn failing_after(mut self, polls: usize, message: &str) -> Self {
        self.fail_after_polls = Some((polls, message.into()));
        self
    }
}

/// A scripted capture source: deterministic, no device. The first device is the default.
#[derive(Clone, Debug, Default)]
pub struct SyntheticCapture {
    devices: Vec<SyntheticDevice>,
    live: Arc<AtomicUsize>,
    opened: Arc<AtomicUsize>,
}
impl SyntheticCapture {
    /// A source with these devices.
    #[must_use]
    pub fn new(devices: Vec<SyntheticDevice>) -> Self {
        Self {
            devices,
            live: Arc::default(),
            opened: Arc::default(),
        }
    }
    /// Streams opened and not yet dropped.
    #[must_use]
    pub fn live_streams(&self) -> usize {
        self.live.load(Ordering::Acquire)
    }
    /// Streams opened so far, dropped or not.
    #[must_use]
    pub fn opened_streams(&self) -> usize {
        self.opened.load(Ordering::Acquire)
    }
}
impl CaptureSource for SyntheticCapture {
    fn devices(&self) -> Result<Vec<InputDevice>, Error> {
        Ok(self
            .devices
            .iter()
            .enumerate()
            .map(|(index, device)| InputDevice {
                id: device.name.clone(),
                name: device.name.clone(),
                is_default: index == 0,
            })
            .collect())
    }
    fn open(&self, device: Option<&str>) -> Result<Box<dyn CaptureStream>, Error> {
        let found = match device {
            Some(id) => self.devices.iter().find(|d| d.name == id),
            None => self.devices.first(),
        }
        .ok_or_else(|| Error::Capture("no microphone was found".into()))?;
        if let Some(message) = &found.open_error {
            return Err(Error::Capture(message.clone()));
        }
        self.live.fetch_add(1, Ordering::AcqRel);
        self.opened.fetch_add(1, Ordering::AcqRel);
        Ok(Box::new(SyntheticStream {
            device: found.clone(),
            position: 0,
            polls: 0,
            live: Arc::clone(&self.live),
        }))
    }
}
struct SyntheticStream {
    device: SyntheticDevice,
    position: usize,
    polls: usize,
    live: Arc<AtomicUsize>,
}
impl Drop for SyntheticStream {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::AcqRel);
    }
}
impl CaptureStream for SyntheticStream {
    fn format(&self) -> InputFormat {
        self.device.format
    }
    fn drain(&mut self, out: &mut Vec<f32>) -> Result<(), Error> {
        self.polls += 1;
        if let Some((after, message)) = &self.device.fail_after_polls
            && self.polls > *after
        {
            return Err(Error::Capture(message.clone()));
        }
        let channels = usize::from(self.device.format.channels);
        let per_poll = self
            .device
            .chunk_samples
            .unwrap_or(self.device.chunk_frames * channels);
        let end = (self.position + per_poll).min(self.device.samples.len());
        out.extend_from_slice(&self.device.samples[self.position..end]);
        self.position = end;
        Ok(())
    }
    fn dropped_frames(&self) -> u64 {
        self.device.dropped_frames
    }
}

// ---------------------------------------------------------------------------------------------
// Real devices (cpal).
// ---------------------------------------------------------------------------------------------

/// The system's input devices through cpal.
#[derive(Clone, Copy, Debug, Default)]
pub struct CpalCapture;

/// Seconds of audio the callback can buffer before it drops samples.
const RING_SECONDS: usize = 4;

impl CaptureSource for CpalCapture {
    fn devices(&self) -> Result<Vec<InputDevice>, Error> {
        let host = cpal::default_host();
        let default_id = host
            .default_input_device()
            .and_then(|device| device_id(&device));
        let devices = host
            .input_devices()
            .map_err(|error| describe_error(&error.to_string(), ErrorKind::Other))?;
        let mut listed: Vec<InputDevice> = Vec::new();
        for device in devices {
            // The stable cpal id is the picker's key (the label is the ALSA hint description
            // on Linux and repeats across hints); a device without one cannot be reopened.
            let Some(id) = device_id(&device) else {
                continue;
            };
            if listed.iter().any(|known| known.id == id) {
                continue;
            }
            let name = device_name(&device).unwrap_or_else(|| id.clone());
            let is_default =
                default_id.as_deref() == Some(&id) && !listed.iter().any(|known| known.is_default);
            listed.push(InputDevice {
                id,
                name,
                is_default,
            });
        }
        // Default first, then the system's order.
        listed.sort_by_key(|device| !device.is_default);
        Ok(listed)
    }

    fn open(&self, device: Option<&str>) -> Result<Box<dyn CaptureStream>, Error> {
        let host = cpal::default_host();
        let device = match device {
            Some(id) => find_input_device(&host, id)?.ok_or_else(|| {
                Error::Capture(format!(
                    "the microphone \"{id}\" is no longer available; pick another one"
                ))
            })?,
            None => host
                .default_input_device()
                .ok_or_else(|| Error::Capture("no microphone was found".into()))?,
        };
        let supported = device
            .default_input_config()
            .map_err(|error| capture_error(&error, ErrorKind::Config))?;
        let format = InputFormat {
            rate: supported.sample_rate(),
            channels: supported.channels(),
        };
        let config = supported.config();
        macro_rules! open {
            ($($variant:ident => $sample:ty),* $(,)?) => {
                match supported.sample_format() {
                    $(SampleFormat::$variant => open_stream::<$sample>(&device, &config, format)?,)*
                    other => {
                        return Err(Error::Capture(format!(
                            "the microphone uses an unsupported sample format ({other:?})"
                        )))
                    }
                }
            };
        }
        let stream = open!(
            F32 => f32, F64 => f64, I8 => i8, I16 => i16, I32 => i32, I64 => i64,
            U8 => u8, U16 => u16, U32 => u32, U64 => u64, I24 => I24, U24 => U24,
        );
        Ok(Box::new(stream))
    }
}

/// The human-readable name of an input device, when the backend reports one.
fn device_name(device: &cpal::Device) -> Option<String> {
    device
        .description()
        .ok()
        .map(|description| description.name().to_owned())
}

/// The stable id of an input device (cpal's `DeviceId`, host-prefixed), when it has one.
fn device_id(device: &cpal::Device) -> Option<String> {
    device.id().ok().map(|id| id.to_string())
}

/// The input device with this picker id. Ids are cpal `DeviceId` strings; an id that is not one
/// (a choice saved by an older build stored the label) falls back to the first device whose
/// label matches, so it keeps working where the name is unambiguous.
fn find_input_device(host: &cpal::Host, id: &str) -> Result<Option<cpal::Device>, Error> {
    if let Some(device) = id
        .parse::<cpal::DeviceId>()
        .ok()
        .and_then(|parsed| host.device_by_id(&parsed))
    {
        return Ok(Some(device));
    }
    let devices = host
        .input_devices()
        .map_err(|error| describe_error(&error.to_string(), ErrorKind::Other))?;
    Ok(devices
        .into_iter()
        .find(|candidate| device_name(candidate).as_deref() == Some(id)))
}

/// A message for a person from a cpal error, keyed on its kind rather than its text.
fn capture_error(error: &cpal::Error, fallback: ErrorKind) -> Error {
    match error.kind() {
        cpal::ErrorKind::PermissionDenied => denied_error(),
        cpal::ErrorKind::DeviceNotAvailable | cpal::ErrorKind::DeviceBusy => {
            kind_error(&error.to_string(), ErrorKind::Unavailable)
        }
        cpal::ErrorKind::UnsupportedConfig => kind_error(&error.to_string(), ErrorKind::Config),
        _ => kind_error(&error.to_string(), fallback),
    }
}

struct CpalStream {
    _stream: cpal::Stream,
    consumer: Consumer<f32>,
    failed: Arc<AtomicBool>,
    dropped: Arc<AtomicUsize>,
    format: InputFormat,
}
impl CaptureStream for CpalStream {
    fn format(&self) -> InputFormat {
        self.format
    }
    fn drain(&mut self, out: &mut Vec<f32>) -> Result<(), Error> {
        while let Ok(sample) = self.consumer.pop() {
            out.push(sample);
        }
        if self.failed.load(Ordering::Acquire) {
            return Err(Error::Capture(
                "the microphone stopped; it may have been unplugged".into(),
            ));
        }
        Ok(())
    }
    fn dropped_frames(&self) -> u64 {
        self.dropped.load(Ordering::Acquire) as u64
    }
}

fn open_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    format: InputFormat,
) -> Result<CpalStream, Error>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let capacity = (format.rate as usize * usize::from(format.channels) * RING_SECONDS).max(1);
    let (mut producer, consumer) = RingBuffer::new(capacity);
    let failed = Arc::new(AtomicBool::new(false));
    let dropped = Arc::new(AtomicUsize::new(0));
    let callback_error = Arc::clone(&failed);
    let callback_dropped = Arc::clone(&dropped);
    let channels = usize::from(format.channels.max(1));
    let stream = device
        .build_input_stream(
            *config,
            move |data: &[T], _| {
                // Real-time callback: no allocation, locking or logging. Push whole frames
                // only — a partial frame at the ring's end would drop an odd sample and
                // swap the channels of every later frame. A full ring drops whole frames
                // and counts them.
                push_whole_frames(&mut producer, data, channels, &callback_dropped);
            },
            move |error| match classify_stream_error(error.kind()) {
                // A backend-recovered xrun (or a rerouting notice): the take goes on. The gap's
                // size is unknown, so it is not counted as dropped frames.
                StreamErrorAction::Glitch | StreamErrorAction::Ignore => {}
                StreamErrorAction::Fatal => callback_error.store(true, Ordering::Release),
            },
            None,
        )
        .map_err(|error| capture_error(&error, ErrorKind::Other))?;
    stream
        .play()
        .map_err(|error| capture_error(&error, ErrorKind::Other))?;
    Ok(CpalStream {
        _stream: stream,
        consumer,
        failed,
        dropped,
        format,
    })
}

/// Push `data`'s whole frames onto `producer`, dropping (and counting in `dropped`) what does
/// not fit, so a torn frame can never reach the ring. Called from the real-time callback.
fn push_whole_frames<T>(
    producer: &mut Producer<f32>,
    data: &[T],
    channels: usize,
    dropped: &AtomicUsize,
) where
    T: SizedSample,
    f32: FromSample<T>,
{
    let frames = (producer.slots() / channels).min(data.len() / channels);
    let keep = frames * channels;
    if keep < data.len() {
        dropped.fetch_add((data.len() - keep) / channels, Ordering::Relaxed);
    }
    for &sample in &data[..keep] {
        let _ = producer.push(sample.to_sample::<f32>());
    }
}

#[derive(Clone, Copy)]
enum ErrorKind {
    Unavailable,
    Config,
    Other,
}

/// The message for an operating-system refusal to use the microphone.
fn denied_error() -> Error {
    Error::Capture(
        "Microphone access was denied. Allow AniMathIO to use the microphone in your system's \
         privacy settings, then try again."
            .into(),
    )
}

/// A message for a person from a backend error known only by its text (a listing failure, a
/// scripted test device): permission wording gets the access message, then `kind` decides.
fn describe_error(detail: &str, kind: ErrorKind) -> Error {
    let lower = detail.to_ascii_lowercase();
    let denied = ["permission", "denied", "not allowed", "privacy", "authoriz"]
        .iter()
        .any(|word| lower.contains(word));
    if denied {
        return denied_error();
    }
    kind_error(detail, kind)
}

/// The message for a classified failure; `detail` is only quoted where it helps.
fn kind_error(detail: &str, kind: ErrorKind) -> Error {
    Error::Capture(match kind {
        ErrorKind::Unavailable => "The microphone is not available. It may be unplugged or in \
             use by another application."
            .to_owned(),
        ErrorKind::Config => {
            format!("The microphone does not offer a format AniMathIO can record ({detail}).")
        }
        ErrorKind::Other => format!("The microphone could not be opened: {detail}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SymphoniaSource;

    fn source(device: SyntheticDevice) -> SyntheticCapture {
        SyntheticCapture::new(vec![device])
    }

    fn record(source: &SyntheticCapture, polls: usize) -> Recording {
        let mut recorder = Recorder::start(source, None).unwrap();
        for _ in 0..polls {
            recorder.poll().unwrap();
        }
        recorder.finish().unwrap()
    }

    #[test]
    fn synthetic_capture_round_trips_through_a_wav_the_importer_decodes() {
        let capture = source(SyntheticDevice::tone("Mic", 48_000, 1, 1.0, 0.5, 4800));
        let recording = record(&capture, 10);
        assert_eq!((recording.rate, recording.channels), (48_000, 1));
        assert_eq!(recording.frames, 48_000);
        assert_eq!(recording.duration(), Time(TICKS_PER_SECOND));
        let wav = recording.into_wav();
        let decoded = SymphoniaSource::from_bytes(wav).unwrap();
        let info = decoded.info();
        assert_eq!((info.rate, info.channels, info.frames), (48_000, 1, 48_000));
        assert_eq!(capture.live_streams(), 0, "finish closes the device");
    }

    #[test]
    fn wav_header_describes_the_samples() {
        let wav = encode_wav(44_100, 2, &[1, -1, 2, -2]);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()), 36 + 8);
        assert_eq!(&wav[8..16], b"WAVEfmt ");
        assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()), 2);
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 44_100);
        assert_eq!(
            u32::from_le_bytes(wav[28..32].try_into().unwrap()),
            44_100 * 4
        );
        assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()), 8);
        assert_eq!(wav.len(), 44 + 8);
    }

    #[test]
    fn level_tracks_the_latest_poll_and_flags_clipping() {
        let loud = SyntheticDevice::scripted(
            "Mic",
            InputFormat {
                rate: 8000,
                channels: 1,
            },
            [vec![0.25; 4], vec![0.0; 4], vec![1.5, -1.5, 0.0, 0.0]].concat(),
            4,
        );
        let capture = source(loud);
        let mut recorder = Recorder::start(&capture, None).unwrap();
        let first = recorder.poll().unwrap();
        assert!((first.peak - 0.25).abs() < 1e-6 && (first.rms - 0.25).abs() < 1e-6);
        assert!(!first.clipped());
        assert!((first.peak_db() - 20.0 * 0.25_f32.log10()).abs() < 1e-4);
        let silent = recorder.poll().unwrap();
        assert_eq!(silent, Level::default());
        assert_eq!(silent.peak_db(), f32::NEG_INFINITY);
        let clipped = recorder.poll().unwrap();
        assert!(clipped.clipped());
        assert_eq!(
            clipped.peak, 1.0,
            "out-of-range samples are clamped for the meter"
        );
        assert_eq!(recorder.frames(), 12);
    }

    #[test]
    fn wide_devices_keep_the_front_pair_and_non_finite_samples_become_silence() {
        let frames = [0.5, -0.5, 0.9, 0.9, f32::NAN, 0.25, 0.9, 0.9];
        let capture = source(SyntheticDevice::scripted(
            "Interface",
            InputFormat {
                rate: 8000,
                channels: 4,
            },
            frames.to_vec(),
            8,
        ));
        let mut recorder = Recorder::start(&capture, None).unwrap();
        assert_eq!(
            recorder.format(),
            InputFormat {
                rate: 8000,
                channels: 2
            }
        );
        recorder.poll().unwrap();
        let recording = recorder.finish().unwrap();
        assert_eq!(recording.frames, 2);
        assert_eq!(recording.pcm, vec![16384, -16384, 0, 8192]);
    }

    #[test]
    fn cancel_closes_the_device_and_keeps_nothing() {
        let capture = source(SyntheticDevice::tone("Mic", 8000, 1, 1.0, 0.5, 800));
        let mut recorder = Recorder::start(&capture, None).unwrap();
        recorder.poll().unwrap();
        assert_eq!(capture.live_streams(), 1);
        recorder.cancel();
        assert_eq!(capture.live_streams(), 0);
        assert_eq!(capture.opened_streams(), 1);
    }

    #[test]
    fn size_cap_marks_the_recorder_full_and_drops_the_excess() {
        let capture = source(SyntheticDevice::tone("Mic", 8000, 1, 1.0, 0.5, 8000));
        let mut recorder = Recorder::start_with_limit(&capture, None, 2 * 1000).unwrap();
        recorder.poll().unwrap();
        assert!(recorder.is_full());
        assert_eq!(recorder.frames(), 1000);
        assert_eq!(recorder.finish().unwrap().frames, 1000);
    }

    #[test]
    fn an_empty_recording_is_an_error_not_an_empty_asset() {
        let capture = source(SyntheticDevice::scripted(
            "Mic",
            InputFormat {
                rate: 8000,
                channels: 1,
            },
            Vec::new(),
            100,
        ));
        let mut recorder = Recorder::start(&capture, None).unwrap();
        recorder.poll().unwrap();
        let error = recorder.finish().unwrap_err();
        assert!(
            error.to_string().contains("nothing was recorded"),
            "{error}"
        );
    }

    #[test]
    fn a_device_that_fails_mid_recording_keeps_what_it_captured() {
        let capture = source(
            SyntheticDevice::tone("Mic", 8000, 1, 1.0, 0.5, 100)
                .failing_after(2, "the microphone was unplugged"),
        );
        let mut recorder = Recorder::start(&capture, None).unwrap();
        recorder.poll().unwrap();
        recorder.poll().unwrap();
        let error = recorder.poll().unwrap_err();
        assert!(error.to_string().contains("unplugged"), "{error}");
        assert_eq!(recorder.finish().unwrap().frames, 200);
    }

    #[test]
    fn a_poll_landing_mid_frame_keeps_the_channels_in_order_and_drops_nothing() {
        // Four stereo frames; each poll delivers three raw samples, so every frame except
        // the first tears across a poll boundary, as when a poll lands mid-callback.
        let frames = [0.1, -0.1, 0.2, -0.2, 0.3, -0.3, 0.4, -0.4];
        let capture = source(
            SyntheticDevice::scripted(
                "Mic",
                InputFormat {
                    rate: 8000,
                    channels: 2,
                },
                frames.to_vec(),
                1,
            )
            .torn(3),
        );
        let mut recorder = Recorder::start(&capture, None).unwrap();
        while recorder.frames() < 4 {
            recorder.poll().unwrap();
        }
        let expected: Vec<i16> = frames.iter().map(|&sample| to_pcm16(sample)).collect();
        let recording = recorder.finish().unwrap();
        assert_eq!(recording.frames, 4);
        assert_eq!(recording.pcm, expected, "left and right stay in order");
    }

    #[test]
    fn a_full_ring_drops_whole_frames_and_counts_them() {
        let (mut producer, mut consumer) = RingBuffer::new(7);
        let dropped = AtomicUsize::new(0);
        // Three stereo frames fit the seven-sample ring whole, leaving one odd slot free.
        push_whole_frames(
            &mut producer,
            &[0.1, -0.1, 0.2, -0.2, 0.3, -0.3],
            2,
            &dropped,
        );
        assert_eq!(dropped.load(Ordering::Acquire), 0, "three frames fit whole");
        // One free slot: a whole second callback cannot fit, so both its frames are dropped.
        push_whole_frames(&mut producer, &[0.4, -0.4, 0.5, -0.5], 2, &dropped);
        assert_eq!(dropped.load(Ordering::Acquire), 2);
        let mut buffered = Vec::new();
        while let Ok(sample) = consumer.pop() {
            buffered.push(sample);
        }
        assert_eq!(buffered, [0.1, -0.1, 0.2, -0.2, 0.3, -0.3]);
        // A ring with room again takes whole frames unchanged.
        push_whole_frames(&mut producer, &[0.6, -0.6, 0.7, -0.7], 2, &dropped);
        assert_eq!(dropped.load(Ordering::Acquire), 2);
        let mut buffered = Vec::new();
        while let Ok(sample) = consumer.pop() {
            buffered.push(sample);
        }
        assert_eq!(buffered, [0.6, -0.6, 0.7, -0.7]);
    }

    #[test]
    fn dropped_frames_are_counted_once_and_reported() {
        let capture = source(SyntheticDevice::tone("Mic", 8000, 1, 1.0, 0.5, 800).dropping(5));
        let mut recorder = Recorder::start(&capture, None).unwrap();
        recorder.poll().unwrap();
        assert_eq!(recorder.dropped_frames(), 5);
        recorder.poll().unwrap();
        assert_eq!(recorder.dropped_frames(), 5, "the counter is monotonic");
    }

    #[test]
    fn devices_are_listed_selectable_and_open_errors_are_clear() {
        let capture = SyntheticCapture::new(vec![
            SyntheticDevice::tone("Built-in", 48_000, 1, 0.1, 0.5, 480),
            SyntheticDevice::tone("USB mic", 44_100, 2, 0.1, 0.5, 441),
            SyntheticDevice::tone("Blocked", 48_000, 1, 0.1, 0.5, 480)
                .failing_open("Microphone access was denied."),
        ]);
        let devices = capture.devices().unwrap();
        assert_eq!(
            devices.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
            ["Built-in", "USB mic", "Blocked"]
        );
        assert!(devices[0].is_default && !devices[1].is_default);
        let usb = Recorder::start(&capture, Some("USB mic")).unwrap();
        assert_eq!(
            usb.format(),
            InputFormat {
                rate: 44_100,
                channels: 2
            }
        );
        let blocked = Recorder::start(&capture, Some("Blocked")).unwrap_err();
        assert!(blocked.to_string().contains("denied"), "{blocked}");
        let missing = Recorder::start(&capture, Some("Gone")).unwrap_err();
        assert!(missing.to_string().contains("no microphone"), "{missing}");
        assert_eq!(capture.live_streams(), 1, "only the opened USB mic is live");
    }

    #[test]
    fn the_null_source_never_opens_and_says_why() {
        let null = NullCapture::new("audio is turned off (ANIMATHIO_AUDIO=none)");
        assert!(null.disabled_reason().unwrap().contains("turned off"));
        assert_eq!(null.devices().unwrap(), []);
        let error = Recorder::start(&null, None).unwrap_err();
        assert!(
            error.to_string().contains("ANIMATHIO_AUDIO=none"),
            "{error}"
        );
    }

    #[test]
    fn backend_errors_are_explained_for_a_person() {
        let denied = describe_error("Permission denied (os error 13)", ErrorKind::Other);
        assert!(denied.to_string().contains("privacy settings"), "{denied}");
        let busy = describe_error("device not available", ErrorKind::Unavailable);
        assert!(busy.to_string().contains("in use by another"), "{busy}");
        let format = describe_error("no 16-bit config", ErrorKind::Config);
        assert!(format.to_string().contains("no 16-bit config"), "{format}");
    }

    #[test]
    fn cpal_error_kinds_pick_the_message_without_reading_the_text() {
        let message = |kind| capture_error(&cpal::Error::new(kind), ErrorKind::Other).to_string();
        // A permission refusal is recognised by its kind even when the text says nothing.
        let denied = cpal::Error::with_message(cpal::ErrorKind::PermissionDenied, "EPERM");
        assert!(
            capture_error(&denied, ErrorKind::Other)
                .to_string()
                .contains("privacy settings")
        );
        for kind in [
            cpal::ErrorKind::DeviceBusy,
            cpal::ErrorKind::DeviceNotAvailable,
        ] {
            assert!(message(kind).contains("in use by another"), "{kind:?}");
        }
        assert!(message(cpal::ErrorKind::UnsupportedConfig).contains("format"));
        // Wording that merely resembles a refusal does not turn a busy device into one.
        let busy = cpal::Error::with_message(cpal::ErrorKind::DeviceBusy, "access denied by peer");
        assert!(
            capture_error(&busy, ErrorKind::Other)
                .to_string()
                .contains("in use by another")
        );
    }
}
