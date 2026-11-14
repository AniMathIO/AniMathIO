use std::sync::Arc;
use std::time::Instant;

use animathio_audio::{AudioPool, EffectWarnings, Mixer, Player, Speed};
use animathio_core::{AudioEffectProcessor, Project, Rounding, TICKS_PER_SECOND, Time};
use std::sync::OnceLock;

static AUDIO_EFFECTS: OnceLock<Arc<dyn AudioEffectProcessor>> = OnceLock::new();
static AUDIO_WARNINGS: OnceLock<EffectWarnings> = OnceLock::new();

/// Warnings from playback mixers whose audio effects could not run (the audio passed
/// through unprocessed). Drained by the editor, which shows them as toasts.
pub(crate) fn take_audio_warnings() -> Vec<String> {
    AUDIO_WARNINGS.get().map_or_default(EffectWarnings::take)
}

/// Register the plugin runner that every playback mixer uses for audio effects. The handle
/// shares the plugin host's registry, so it is installed once at startup.
pub(crate) fn install_audio_effects(processor: Arc<dyn AudioEffectProcessor>) {
    let _ = AUDIO_EFFECTS.set(processor);
}

/// Make a freshly prepared playback mixer lenient (a failing audio effect is skipped with a
/// warning, see `take_audio_warnings`) and attach the registered audio-effect runner, if any.
pub(crate) fn with_audio_effects(mixer: Mixer) -> Mixer {
    let mixer = mixer.lenient(AUDIO_WARNINGS.get_or_init(EffectWarnings::default).clone());
    match AUDIO_EFFECTS.get() {
        Some(processor) => mixer.with_effect_processor(Arc::clone(processor)),
        None => mixer,
    }
}

/// The single A/V master clock. The UI thread owns it and asks "which video frame
/// is due now" once per UI frame; video is never drift-corrected on its own.
pub(crate) trait Clock {
    fn now(&self) -> Time;
    /// Change the signed playback speed; takes effect immediately, also while playing.
    fn set_rate(&mut self, _speed: Speed) -> Result<(), String> {
        Ok(())
    }
    fn has_error(&self) -> bool {
        false
    }
    fn play(&mut self) -> Result<(), String>;
    fn pause(&mut self) -> Result<(), String>;
    fn seek(&mut self, time: Time) -> Result<(), String>;
    /// Replace the project snapshot seen by the mixer worker. The default is a
    /// no-op for clocks that have no mixer (the monotonic fallback).
    fn set_project(&mut self, _project: Arc<Project>) {}
    /// Replace the prepared mixer snapshot. The default is a no-op for clocks
    /// that have no mixer.
    fn set_mixer(&mut self, _mixer: Arc<Mixer>) {}
    /// Output format used to prepare replacement mixers; `None` for clocks with
    /// no audio device.
    fn mixer_format(&self) -> Option<(u32, u16)> {
        None
    }
}

/// Device-master clock. The audio crate owns the callback/worker threads; this
/// wrapper keeps that runtime detail out of the view.
pub(crate) struct AudioClock {
    player: Player,
    rate: u32,
    channels: u16,
}

impl AudioClock {
    /// Open the default output device and prepare a mixer from the decoded pool.
    pub(crate) fn open(project: &Project, pool: &AudioPool) -> Result<Self, String> {
        let (rate, channels) = Player::default_output_format().map_err(|e| e.to_string())?;
        let mixer = with_audio_effects(pool.prepare(rate, channels).map_err(|e| e.to_string())?);
        let player =
            Player::new(Arc::new(project.clone()), Arc::new(mixer)).map_err(|e| e.to_string())?;
        Ok(Self {
            player,
            rate,
            channels,
        })
    }
}

impl Clock for AudioClock {
    fn has_error(&self) -> bool {
        self.player.has_error()
    }
    fn now(&self) -> Time {
        self.player.now()
    }

    fn play(&mut self) -> Result<(), String> {
        self.player.play().map_err(|e| e.to_string())
    }

    fn pause(&mut self) -> Result<(), String> {
        self.player.pause().map_err(|e| e.to_string())
    }

    fn seek(&mut self, time: Time) -> Result<(), String> {
        self.player.seek(time).map_err(|e| e.to_string())
    }

    fn set_rate(&mut self, speed: Speed) -> Result<(), String> {
        self.player.set_speed(speed).map_err(|e| e.to_string())
    }

    fn set_project(&mut self, project: Arc<Project>) {
        self.player.set_project(project);
    }

    fn set_mixer(&mut self, mixer: Arc<Mixer>) {
        self.player.set_mixer(mixer);
    }

    fn mixer_format(&self) -> Option<(u32, u16)> {
        Some((self.rate, self.channels))
    }
}

/// Wall-clock fallback used when no audio device is available. It implements the
/// same trait so the rest of the editor is indifferent to which clock is driving.
pub(crate) struct MonotonicClock {
    speed: Speed,
    base: Time,
    playing: bool,
    started: Instant,
}

impl MonotonicClock {
    pub(crate) fn new() -> Self {
        Self {
            speed: Speed::NORMAL,
            base: Time(0),
            playing: false,
            started: Instant::now(),
        }
    }
}

impl Clock for MonotonicClock {
    fn set_rate(&mut self, speed: Speed) -> Result<(), String> {
        self.base = self.now();
        self.started = Instant::now();
        self.speed = speed;
        Ok(())
    }

    fn now(&self) -> Time {
        if self.playing {
            let scaled = i128::from(ticks(self.started.elapsed())) * i128::from(self.speed.num())
                / i128::from(self.speed.den());
            Time(
                i64::try_from(i128::from(self.base.0) + scaled).unwrap_or(if scaled < 0 {
                    i64::MIN
                } else {
                    i64::MAX
                }),
            )
        } else {
            self.base
        }
    }

    fn play(&mut self) -> Result<(), String> {
        if !self.playing {
            self.started = Instant::now();
            self.playing = true;
        }
        Ok(())
    }

    fn pause(&mut self) -> Result<(), String> {
        if self.playing {
            self.base = self.now();
            self.playing = false;
        }
        Ok(())
    }

    fn seek(&mut self, time: Time) -> Result<(), String> {
        self.base = time;
        self.started = Instant::now();
        Ok(())
    }
}

/// Choose the audio clock, falling back to a monotonic clock with no device. The
/// audio clock stays master at every shuttle speed; muted speeds still count.
pub(crate) fn make_clock(project: &Project, pool: &AudioPool) -> Box<dyn Clock> {
    match AudioClock::open(project, pool) {
        Ok(clock) => Box::new(clock),
        Err(_) => Box::new(MonotonicClock::new()),
    }
}

/// The video frame index whose half-open display interval contains `now`.
/// Playback renders at exact frame boundaries so preview matches frame-stepped
/// export, then the display holds the frame until the next boundary is due.
pub(crate) fn due_frame(project: &Project, now: Time) -> Option<Time> {
    let rate = project.frame_rate();
    let frame = rate.time_to_frame(now, Rounding::Floor).ok()?;
    rate.frame_to_time(frame, Rounding::Nearest).ok()
}

fn ticks(elapsed: std::time::Duration) -> i64 {
    let total = elapsed.as_nanos() * u128::try_from(TICKS_PER_SECOND).unwrap() / 1_000_000_000_u128;
    i64::try_from(total.min(i64::MAX as u128)).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{FrameRate, Project, Rounding, TICKS_PER_SECOND, Time};

    #[test]
    fn due_frame_matches_frame_stepped_export_boundaries() {
        let project = Project::new(FrameRate::new(30, 1).unwrap());
        let frame_ticks = TICKS_PER_SECOND / 30;
        assert_eq!(due_frame(&project, Time(0)), Some(Time(0)));
        assert_eq!(due_frame(&project, Time(frame_ticks - 1)), Some(Time(0)));
        assert_eq!(
            due_frame(&project, Time(frame_ticks)),
            Some(Time(frame_ticks))
        );
        assert_eq!(
            due_frame(&project, Time(30 * frame_ticks)),
            Some(Time(30 * frame_ticks))
        );
    }

    #[test]
    fn due_frame_handles_rational_ntsc_rates() {
        let project = Project::new(FrameRate::new(30_000, 1_001).unwrap());
        let first = project
            .frame_rate()
            .frame_to_time(1, Rounding::Nearest)
            .unwrap();
        assert_eq!(due_frame(&project, Time(0)), Some(Time(0)));
        assert_eq!(due_frame(&project, first), Some(first));
        assert_eq!(due_frame(&project, Time(first.0 - 1)), Some(Time(0)));
    }

    #[test]
    fn monotonic_clock_runs_backwards_and_at_fractional_speed() {
        let mut clock = MonotonicClock::new();
        clock.seek(Time(TICKS_PER_SECOND * 10)).unwrap();
        clock.set_rate(Speed::whole(-2).unwrap()).unwrap();
        clock.play().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let back = clock.now();
        assert!(back.0 < TICKS_PER_SECOND * 10, "{back:?}");
        clock.pause().unwrap();
        clock.set_rate(Speed::HALF).unwrap();
        clock.seek(Time(0)).unwrap();
        clock.play().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let forward = clock.now().0;
        assert!(forward > 0 && forward < TICKS_PER_SECOND / 20, "{forward}");
    }

    #[test]
    fn monotonic_clock_seek_and_advance() {
        let mut clock = MonotonicClock::new();
        assert_eq!(clock.now(), Time(0));
        clock.seek(Time(TICKS_PER_SECOND)).unwrap();
        assert_eq!(clock.now(), Time(TICKS_PER_SECOND));
        clock.play().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert!(clock.now().0 >= TICKS_PER_SECOND);
        clock.pause().unwrap();
        let paused = clock.now();
        std::thread::sleep(std::time::Duration::from_millis(5));
        assert_eq!(clock.now(), paused);
    }
}
