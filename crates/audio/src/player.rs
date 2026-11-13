use crate::{BlockGenerator, Error, Mixer, Speed};
use animathio_core::{Project, TICKS_PER_SECOND, Time, TimeRange};
use arc_swap::ArcSwap;
use cpal::{
    FromSample, SampleFormat, SizedSample,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use rtrb::{Consumer, RingBuffer};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

const BLOCK: usize = 512;
const BUFFER_FRAMES: usize = 8192;

/// Device-master playback. Own this on the control/UI thread; the mixer worker owns
/// the producer and the device callback exclusively owns the consumer. There is no
/// wall-clock fallback. `now` advances in callback-sized steps, accounting for the
/// device-reported output latency, and freezes on underflow until PCM resumes.
pub struct Player {
    device: cpal::Device,
    config: cpal::StreamConfig,
    format: SampleFormat,
    mixer: Arc<ArcSwap<Mixer>>,
    project: Arc<ArcSwap<Project>>,
    position: Time,
    speed: Speed,
    session: Option<Session>,
}
impl Player {
    /// Open the default output configuration. Preparing resources remains explicit
    /// so the caller can use exactly the same mixer for export.
    pub fn new(project: Arc<Project>, mixer: Arc<Mixer>) -> Result<Self, Error> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| Error::Playback("no default output device".into()))?;
        let supported = device.default_output_config().map_err(playback_error)?;
        if supported.sample_rate().0 != mixer.rate() || supported.channels() != mixer.channels() {
            return Err(Error::Invalid(format!(
                "prepare mixer at device rate {} Hz, {} channels",
                supported.sample_rate().0,
                supported.channels()
            )));
        }
        Ok(Self {
            device,
            config: supported.config(),
            format: supported.sample_format(),
            mixer: Arc::new(ArcSwap::from(mixer)),
            project: Arc::new(ArcSwap::from(project)),
            position: Time(0),
            speed: Speed::NORMAL,
            session: None,
        })
    }
    /// Atomically replace the prepared mixer snapshot. The mixer worker picks the
    /// new snapshot up at the next chunk boundary; nothing in the device callback
    /// reads it, so there is no glitch.
    pub fn set_mixer(&self, mixer: Arc<Mixer>) {
        self.mixer.store(mixer);
    }
    /// Atomically replace the project snapshot (e.g. after a history edit).
    pub fn set_project(&self, project: Arc<Project>) {
        self.project.store(project);
    }
    /// Discover the format needed by `AudioPool::prepare` without starting playback.
    pub fn default_output_format() -> Result<(u32, u16), Error> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| Error::Playback("no default output device".into()))?;
        let config = device.default_output_config().map_err(playback_error)?;
        Ok((config.sample_rate().0, config.channels()))
    }
    /// Start or resume from the paused position. Repeated play is a no-op.
    pub fn play(&mut self) -> Result<(), Error> {
        if self.session.is_some() {
            return Ok(());
        }
        macro_rules! start {
            ($($format:ident => $sample:ty),* $(,)?) => {
                match self.format {
                    $(SampleFormat::$format => self.start::<$sample>()?,)*
                    format => return Err(Error::Unsupported(format!("device sample format {format:?}"))),
                }
            };
        }
        start!(F32 => f32, F64 => f64, I8 => i8, I16 => i16, I32 => i32, I64 => i64,
            U8 => u8, U16 => u16, U32 => u32, U64 => u64);
        Ok(())
    }
    /// Freeze at the last latency-corrected device position. Buffered PCM is dropped;
    /// resume creates a fresh stream at that position.
    pub fn pause(&mut self) -> Result<(), Error> {
        self.position = self.now();
        if let Some(mut session) = self.session.take() {
            session.stop()?;
        }
        Ok(())
    }
    /// Flush by stopping/joining the old session and replacing its entire ring and
    /// callback clock. No stale producer or callback can write into the new session.
    pub fn seek(&mut self, time: Time) -> Result<(), Error> {
        if time.0 < 0 {
            return Err(Error::Invalid("negative seek".into()));
        }
        let playing = self.session.is_some();
        self.pause()?;
        self.position = time;
        if playing {
            self.play()?;
        }
        Ok(())
    }
    /// Change the playback speed. While playing, the session is flushed and
    /// restarted at the current latency-corrected position, exactly like a seek,
    /// so the master clock keeps describing what is audible. Forward up to 8x and
    /// reverse up to 4x are audible (varispeed, pitch follows speed); faster is
    /// muted but the clock still runs at that speed.
    pub fn set_speed(&mut self, speed: Speed) -> Result<(), Error> {
        if speed == self.speed {
            return Ok(());
        }
        let playing = self.session.is_some();
        self.pause()?;
        self.speed = speed;
        if playing {
            self.play()?;
        }
        Ok(())
    }
    /// The current playback speed.
    #[must_use]
    pub fn speed(&self) -> Speed {
        self.speed
    }
    /// Last device-consumed media time minus reported output latency. Monotonic
    /// between explicit seeks; no elapsed wall time is added or extrapolated.
    #[must_use]
    pub fn now(&self) -> Time {
        self.session
            .as_ref()
            .map_or(self.position, |s| Time(s.clock.load(Ordering::Acquire)))
    }
    /// A device/worker error is signalled without allocating or logging in callbacks.
    #[must_use]
    pub fn has_error(&self) -> bool {
        self.session
            .as_ref()
            .is_some_and(|s| s.failed.load(Ordering::Acquire))
    }
    fn start<T: SizedSample + FromSample<f32>>(&mut self) -> Result<(), Error> {
        let mixer = self.mixer.load_full();
        let project = self.project.load_full();
        let channels = usize::from(mixer.channels());
        let rate = mixer.rate();
        let (mut producer, consumer) = RingBuffer::new(BUFFER_FRAMES * channels);
        // Prefill on the control thread so the first callback already has data.
        let speed = self.speed;
        #[allow(clippy::cast_precision_loss)] // Frame positions stay far below 2^53.
        let start = crate::source::sample_bounds(
            TimeRange {
                start: self.position,
                end: self.position,
            },
            rate,
        )?
        .0 as f64;
        let mut generator = BlockGenerator::new(speed, channels, start);
        let prefill = generator.next_block(BUFFER_FRAMES, |first, count| {
            mix_span(&mixer, &project, first, count)
        })?;
        for sample in prefill {
            producer
                .push(sample)
                .map_err(|e| Error::Playback(e.to_string()))?;
        }
        let clock = Arc::new(AtomicI64::new(self.position.0));
        let failed = Arc::new(AtomicBool::new(false));
        let stopped = Arc::new(AtomicBool::new(false));
        let mut callback = Callback {
            consumer,
            clock: Arc::clone(&clock),
            origin: self.position,
            submitted: 0,
            rate,
            channels,
            speed,
        };
        let callback_error = Arc::clone(&failed);
        let stream = self
            .device
            .build_output_stream(
                &self.config,
                move |data: &mut [T], info| {
                    let timestamp = info.timestamp();
                    let latency = timestamp
                        .playback
                        .duration_since(&timestamp.callback)
                        .unwrap_or_default();
                    callback.fill(data, latency);
                },
                move |_| {
                    callback_error.store(true, Ordering::Release);
                },
                None,
            )
            .map_err(playback_error)?;
        let mixer = Arc::clone(&self.mixer);
        let project = Arc::clone(&self.project);
        let stop_worker = Arc::clone(&stopped);
        let worker_error = Arc::clone(&failed);
        let worker = thread::Builder::new()
            .name("audio-mixer".into())
            .spawn(move || {
                let result = (|| {
                    while !stop_worker.load(Ordering::Acquire) {
                        if producer.slots() < BLOCK * channels {
                            thread::sleep(Duration::from_millis(1));
                            continue;
                        }
                        // Reload the snapshots once per chunk so edits are audible
                        // at the next boundary without ever tearing a chunk in two.
                        let mixer = mixer.load_full();
                        let project = project.load_full();
                        let block = generator.next_block(BLOCK, |first, count| {
                            mix_span(&mixer, &project, first, count)
                        })?;
                        for sample in block {
                            producer
                                .push(sample)
                                .map_err(|e| Error::Playback(e.to_string()))?;
                        }
                    }
                    Ok(())
                })();
                if let Err(error) = &result {
                    eprintln!("audio mixer: {error}");
                    worker_error.store(true, Ordering::Release);
                }
                result
            })?;
        let mut session = Session {
            stream: Some(stream),
            stopped,
            failed,
            clock,
            worker: Some(worker),
        };
        if let Err(error) = session.stream.as_ref().expect("new stream").play() {
            session.stop()?;
            return Err(playback_error(error));
        }
        self.session = Some(session);
        Ok(())
    }
}
struct Session {
    stream: Option<cpal::Stream>,
    stopped: Arc<AtomicBool>,
    failed: Arc<AtomicBool>,
    clock: Arc<AtomicI64>,
    worker: Option<JoinHandle<Result<(), Error>>>,
}
impl Session {
    fn stop(&mut self) -> Result<(), Error> {
        self.stream.take();
        self.stopped.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            worker
                .join()
                .map_err(|_| Error::Playback("mixer thread panicked".into()))??;
        }
        if self.failed.load(Ordering::Acquire) {
            return Err(Error::Playback("device stream failed".into()));
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
fn playback_error(error: impl std::fmt::Display) -> Error {
    Error::Playback(error.to_string())
}
/// Mix `count` timeline frames starting at a possibly negative `first` frame;
/// frames before the timeline start are silence (reverse shuttle runs into them).
fn mix_span(mixer: &Mixer, project: &Project, first: i64, count: usize) -> Result<Vec<f32>, Error> {
    let channels = usize::from(mixer.channels());
    let skip = usize::try_from(-first.min(0))
        .map_err(|e| Error::Invalid(e.to_string()))?
        .min(count);
    let mut output = vec![0.0_f32; count * channels];
    if skip < count {
        let start = usize::try_from(first.max(0)).map_err(|e| Error::Invalid(e.to_string()))?;
        let block = mix_block(mixer, project, start, count - skip)?;
        output[skip * channels..].copy_from_slice(&block);
    }
    Ok(output)
}
fn mix_block(
    mixer: &Mixer,
    project: &Project,
    start: usize,
    frames: usize,
) -> Result<Vec<f32>, Error> {
    let tick = |frame: usize| {
        i64::try_from(frame as i128 * i128::from(TICKS_PER_SECOND) / i128::from(mixer.rate()))
            .map(Time)
            .map_err(|e| Error::Invalid(e.to_string()))
    };
    mixer.mix(
        project,
        TimeRange {
            start: tick(start)?,
            end: tick(start + frames)?,
        },
    )
}

// Only this object is touched by the real-time callback. No Vec, mutex, channel,
// decoder, resampler, logger or allocator is reachable from fill().
struct Callback {
    consumer: Consumer<f32>,
    clock: Arc<AtomicI64>,
    origin: Time,
    submitted: u64,
    rate: u32,
    channels: usize,
    speed: Speed,
}
impl Callback {
    fn fill<T: SizedSample + FromSample<f32>>(&mut self, output: &mut [T], latency: Duration) {
        let latency_frames =
            u64::try_from(latency.as_nanos() * u128::from(self.rate) / 1_000_000_000)
                .unwrap_or(u64::MAX);
        let audible = self.submitted.saturating_sub(latency_frames);
        // Signed rational speed keeps the clock exact; reverse counts downwards.
        let ticks = i128::from(self.origin.0)
            + i128::from(audible) * i128::from(TICKS_PER_SECOND) * i128::from(self.speed.num())
                / (i128::from(self.rate) * i128::from(self.speed.den()));
        let ticks = i64::try_from(ticks).unwrap_or(if ticks < 0 { i64::MIN } else { i64::MAX });
        if self.speed.is_reverse() {
            self.clock.fetch_min(ticks, Ordering::Release);
        } else {
            self.clock.fetch_max(ticks, Ordering::Release);
        }
        for frame in output.chunks_mut(self.channels) {
            if frame.len() == self.channels && self.consumer.slots() >= self.channels {
                for sample in frame {
                    *sample = T::from_sample(self.consumer.pop().unwrap_or(0.0).clamp(-1.0, 1.0));
                }
                self.submitted = self.submitted.saturating_add(1);
            } else {
                for sample in frame {
                    *sample = T::from_sample(0.0);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_block_reflects_a_swapped_project_snapshot() {
        use crate::{AudioPool, Pcm};
        use animathio_core::{Asset, AssetId, Clip, ClipId, ClipKind, Command, Track, TrackId};

        let rate = 48_000;
        let id = AssetId::of(b"tone");
        let pcm = Pcm::new(rate, 1, vec![0.5; 4_800]).unwrap();
        let mut pool = AudioPool::default();
        pool.insert(id.clone(), &pcm).unwrap();
        let mixer = pool.prepare(rate, 1).unwrap();

        let project = |gain: f64| {
            let mut project = Project::default();
            Command::AddAsset {
                id: id.clone(),
                asset: Asset {
                    name: "tone".into(),
                    media_type: "audio/pcm".into(),
                },
            }
            .apply(&mut project)
            .unwrap();
            Command::AddTrack {
                track: Track::new(TrackId(0), "audio"),
                index: 0,
            }
            .apply(&mut project)
            .unwrap();
            Command::AddClip {
                track: TrackId(0),
                clip: Clip::new(
                    ClipId(0),
                    ClipKind::Audio(id.clone()),
                    TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap(),
                ),
            }
            .apply(&mut project)
            .unwrap();
            Command::SetParam {
                id: ClipId(0),
                name: "gain".into(),
                value: Some(gain),
            }
            .apply(&mut project)
            .unwrap();
            project
        };

        let swap = ArcSwap::from(Arc::new(project(1.0)));
        let audible = mix_block(&mixer, &swap.load_full(), 0, 100).unwrap();
        assert!(audible.iter().any(|sample| sample.abs() > 0.4));
        swap.store(Arc::new(project(0.0)));
        let silent = mix_block(&mixer, &swap.load_full(), 0, 100).unwrap();
        assert!(silent.iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn fake_device_clock_is_monotonic_latency_corrected_and_freezes_on_underrun() {
        let (mut producer, consumer) = RingBuffer::new(9600);
        for _ in 0..9600 {
            producer.push(0.25).unwrap();
        }
        let clock = Arc::new(AtomicI64::new(123));
        let mut callback = Callback {
            consumer,
            clock: Arc::clone(&clock),
            origin: Time(123),
            submitted: 0,
            rate: 48_000,
            channels: 2,
            speed: Speed::NORMAL,
        };
        let mut data = [0.0_f32; 960];
        let mut previous = 123;
        for i in 0..20 {
            // A latency increase must never move the master clock backwards.
            callback.fill(
                &mut data,
                Duration::from_millis(if i == 4 { 50 } else { 10 }),
            );
            let now = clock.load(Ordering::Acquire);
            assert!(now >= previous);
            assert!(now <= 123 + i64::try_from(callback.submitted).unwrap() * 14_700);
            if i == 2 {
                assert_eq!(now, 123 + 480 * 14_700);
            }
            previous = now;
        }
        let frozen = clock.load(Ordering::Acquire);
        callback.fill(&mut data, Duration::from_millis(10));
        assert_eq!(clock.load(Ordering::Acquire), frozen);
        assert!(data.iter().all(|s| *s == 0.0));
        // Fresh sessions implement seek flushing: no old consumer or clock is reused.
        let (_, consumer) = RingBuffer::new(2);
        let clock = Arc::new(AtomicI64::new(0));
        let mut sought = Callback {
            consumer,
            clock: Arc::clone(&clock),
            origin: Time(0),
            submitted: 0,
            rate: 48_000,
            channels: 2,
            speed: Speed::NORMAL,
        };
        sought.fill(&mut data, Duration::ZERO);
        assert_eq!(clock.load(Ordering::Acquire), 0);
    }

    #[test]
    fn fake_device_clock_follows_speed_forward_slow_and_reverse() {
        let run = |speed: Speed, origin: i64| {
            let (mut producer, consumer) = RingBuffer::new(9600);
            for _ in 0..9600 {
                producer.push(0.25).unwrap();
            }
            let clock = Arc::new(AtomicI64::new(origin));
            let mut callback = Callback {
                consumer,
                clock: Arc::clone(&clock),
                origin: Time(origin),
                submitted: 0,
                rate: 48_000,
                channels: 2,
                speed,
            };
            let mut data = [0.0_f32; 960];
            let mut seen = vec![origin];
            for _ in 0..5 {
                callback.fill(&mut data, Duration::ZERO);
                seen.push(clock.load(Ordering::Acquire));
            }
            seen
        };
        let second = TICKS_PER_SECOND;
        // The clock is published at the start of each callback, so the fifth fill
        // reports four callbacks of 480 device frames: 40 ms of device time.
        let device = second / 25;
        let normal = run(Speed::NORMAL, 0);
        assert_eq!(normal[5], device);
        let double = run(Speed::whole(2).unwrap(), 0);
        assert_eq!(double[5], 2 * device);
        let half = run(Speed::HALF, 0);
        assert_eq!(half[5], device / 2);
        let reverse = run(Speed::whole(-4).unwrap(), second);
        assert_eq!(reverse[5], second - 4 * device);
        assert!(reverse.windows(2).all(|w| w[1] <= w[0]), "monotonic down");
        let muted = run(Speed::whole(16).unwrap(), 0);
        assert_eq!(muted[5], 16 * device);
    }

    #[test]
    fn mix_span_pads_frames_before_the_timeline_with_silence() {
        use crate::{AudioPool, Pcm};
        let rate = 48_000;
        let pool = AudioPool::default();
        let mixer = pool.prepare(rate, 2).unwrap();
        let _ = Pcm::new(rate, 2, vec![]).unwrap();
        let project = Project::default();
        let block = mix_span(&mixer, &project, -3, 5).unwrap();
        assert_eq!(block.len(), 10);
        assert!(block.iter().all(|s| *s == 0.0));
        assert_eq!(mix_span(&mixer, &project, -10, 4).unwrap().len(), 8);
    }
}
