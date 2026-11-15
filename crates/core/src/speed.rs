//! Exact clip retiming. Rates are reduced rationals on a millionth-speed lattice;
//! ramps interpolate linearly in timeline ticks and integrate before rounding once.
use crate::{Clip, ClipId, Command, Error, Project, TICKS_PER_SECOND, Time, TimeRange, invalid};
use serde::{Deserialize, Serialize};

const SCALE: i128 = 1_000_000;
/// Maximum retimed clip/curve span (seven days), bounding all i128 products.
pub const MAX_RETIME_TICKS: i64 = TICKS_PER_SECOND * 86400 * 7;

/// Positive rational playback speed, from 1/16 through 16, at millionth precision.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Rate", into = "Rate")]
pub struct ClipSpeed(u32);
#[derive(Serialize, Deserialize)]
struct Rate {
    numerator: u32,
    denominator: u32,
}
impl ClipSpeed {
    /// Construct an exactly representable rational (denominator must divide 1,000,000).
    pub fn new(numerator: u32, denominator: u32) -> Result<Self, Error> {
        if denominator == 0 {
            return Err(invalid("speed denominator must be positive"));
        }
        let divisor = gcd(numerator, denominator);
        let (numerator, denominator) = (numerator / divisor, denominator / divisor);
        if 1_000_000 % denominator != 0 {
            return Err(invalid(
                "speed must be exactly representable at millionth precision",
            ));
        }
        let units = u64::from(numerator) * u64::from(1_000_000 / denominator);
        if !(62_500..=16_000_000).contains(&units) {
            return Err(invalid("speed must be between 1/16 and 16"));
        }
        Ok(Self(
            u32::try_from(units).map_err(|_| invalid("invalid speed"))?,
        ))
    }
    /// Rational numerator.
    #[must_use]
    pub fn numerator(self) -> u32 {
        let r: Rate = self.into();
        r.numerator
    }
    /// Rational denominator.
    #[must_use]
    pub fn denominator(self) -> u32 {
        let r: Rate = self.into();
        r.denominator
    }
    /// UI percentage; never used in source-time arithmetic.
    #[must_use]
    pub fn percent(self) -> f64 {
        f64::from(self.0) / 10_000.0
    }
}
fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}
impl std::str::FromStr for ClipSpeed {
    type Err = Error;
    /// Parse an exact rational (3/2) or decimal multiplier (1.5, up to six places).
    fn from_str(value: &str) -> Result<Self, Error> {
        let value = value.trim();
        let (numerator, denominator) = if let Some((n, d)) = value.split_once('/') {
            (
                n.parse::<u32>()
                    .map_err(|_| invalid("invalid speed numerator"))?,
                d.parse::<u32>()
                    .map_err(|_| invalid("invalid speed denominator"))?,
            )
        } else if let Some((whole, decimal)) = value.split_once('.') {
            if decimal.len() > 6 || !decimal.bytes().all(|c| c.is_ascii_digit()) {
                return Err(invalid("speed accepts up to six decimal places"));
            }
            let denominator = 10_u32.pow(u32::try_from(decimal.len()).expect("six places"));
            let numerator = whole
                .parse::<u32>()
                .ok()
                .and_then(|n| n.checked_mul(denominator))
                .and_then(|n| n.checked_add(decimal.parse::<u32>().ok()?))
                .ok_or_else(|| invalid("invalid decimal speed"))?;
            (numerator, denominator)
        } else {
            (
                value.parse::<u32>().map_err(|_| invalid("invalid speed"))?,
                1,
            )
        };
        Self::new(numerator, denominator)
    }
}
impl Default for ClipSpeed {
    fn default() -> Self {
        Self(1_000_000)
    }
}
impl TryFrom<Rate> for ClipSpeed {
    type Error = Error;
    fn try_from(r: Rate) -> Result<Self, Error> {
        Self::new(r.numerator, r.denominator)
    }
}
impl From<ClipSpeed> for Rate {
    fn from(s: ClipSpeed) -> Self {
        let (mut a, mut b) = (s.0, 1_000_000);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        Self {
            numerator: s.0 / a,
            denominator: 1_000_000 / a,
        }
    }
}
/// One speed key in clip-local timeline ticks. Keys interpolate linearly;
/// outside the keys the nearest speed holds. No keys means constant speed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeedPoint {
    /// Local timeline position.
    pub time: Time,
    /// Speed at this position.
    pub speed: ClipSpeed,
}
/// Resource-free source clock, including the original source span for reverse playback.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipTimeMap {
    /// Constant speed (used when ramps are empty).
    pub speed: ClipSpeed,
    /// Read the source interval backwards.
    pub reverse: bool,
    /// Ordered linear speed keys (at most 128).
    pub ramps: Vec<SpeedPoint>,
    /// A frame hold reads only `source_offset`. Audio is silent during holds.
    pub hold: bool,
    /// Curve phase retained by splits; callers normally leave this zero.
    pub phase: Time,
    /// Exclusive original source span, populated by retiming commands.
    pub source_length: Option<Time>,
}
impl ClipTimeMap {
    /// Validate the bounded arithmetic domain.
    pub fn validate(&self, duration: i64) -> Result<(), Error> {
        if self == &Self::default() {
            return Ok(());
        }
        if self.phase.0 < 0
            || duration <= 0
            || self.phase.0 > MAX_RETIME_TICKS - duration
            || self.ramps.len() > 128
            || self
                .ramps
                .iter()
                .any(|p| p.time.0 < 0 || p.time.0 > MAX_RETIME_TICKS)
            || self.ramps.windows(2).any(|p| p[0].time >= p[1].time)
            || self
                .source_length
                .is_some_and(|t| t.0 <= 0 || t.0 > MAX_RETIME_TICKS * 16)
            || (self.reverse && self.source_length.is_none())
        {
            return Err(invalid(
                "invalid clip time map (maximum seven days, 128 ordered keys)",
            ));
        }
        if let Some(length) = self.source_length
            && self.phase.0 + duration > self.duration_for(length)?.0
        {
            return Err(invalid("trim extends past the retimed source interval"));
        }
        Ok(())
    }
    /// Exact integrated speed, rounded down once to integer source ticks.
    ///
    /// # Panics
    /// May panic on malformed clocks; call `validate` before using a clock built
    /// directly. Project loading and commands validate clocks automatically.
    #[must_use]
    pub fn integrated(&self, time: Time) -> Time {
        let t = i128::from(time.0.clamp(0, MAX_RETIME_TICKS));
        let Some(first) = self.ramps.first() else {
            return Time(
                i64::try_from(t * i128::from(self.speed.0) / SCALE).expect("bounded integral"),
            );
        };
        let mut total = 0_i128; // twice the integral, in millionth-ticks
        let mut start = 0_i128;
        let mut rate = i128::from(first.speed.0);
        for point in &self.ramps {
            let end = i128::from(point.time.0);
            let next = i128::from(point.speed.0);
            if t < end {
                let dt = t - start;
                let width = end - start;
                let n = total * width + 2 * rate * dt * width + (next - rate) * dt * dt;
                return Time(i64::try_from(n / (2 * SCALE * width)).expect("bounded integral"));
            }
            total += (rate + next) * (end - start);
            start = end;
            rate = next;
        }
        Time(
            i64::try_from((total + 2 * rate * (t - start)) / (2 * SCALE))
                .expect("bounded integral"),
        )
    }
    /// Minimal timeline duration consuming `length` source ticks (ceil boundary).
    pub fn duration_for(&self, length: Time) -> Result<Time, Error> {
        if length.0 <= 0 || self.integrated(Time(MAX_RETIME_TICKS)) < length {
            return Err(invalid("retimed duration exceeds seven days"));
        }
        let (mut lo, mut hi) = (1, MAX_RETIME_TICKS);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.integrated(Time(mid)) < length {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Ok(Time(lo))
    }
}
impl ClipTimeMap {
    /// Map local timeline ticks to absolute source ticks using a validated clock.
    ///
    /// # Panics
    /// Panics if reverse is set without a source length (an invalid clock).
    #[must_use]
    pub fn source_time(&self, source_offset: Time, local: Time) -> Time {
        if self.hold {
            return source_offset;
        }
        if self == &Self::default() {
            return Time(source_offset.0.saturating_add(local.0));
        }
        let pos = self
            .integrated(Time(local.0.saturating_add(self.phase.0)))
            .0;
        let pos = self.source_length.map_or(pos, |s| pos.min(s.0 - 1));
        Time(source_offset.0.saturating_add(if self.reverse {
            self.source_length.expect("validated reverse span").0 - 1 - pos
        } else {
            pos
        }))
    }
}
impl Clip {
    /// Timeline-local time to absolute source time. Reverse starts one tick before
    /// the exclusive source end, ensuring frame/sample lookup stays inside the range.
    #[must_use]
    pub fn source_time(&self, local: Time) -> Time {
        self.time_map.source_time(self.source_offset, local)
    }
    /// Half-open source bounds used by source-limit guards and retiming edits.
    ///
    /// # Panics
    /// Panics if reverse is set without a source length (an invalid clock).
    #[must_use]
    pub fn source_bounds(&self) -> (Time, Time) {
        if self.time_map.hold {
            return (
                self.source_offset,
                Time(self.source_offset.0.saturating_add(1)),
            );
        }
        if self.time_map == ClipTimeMap::default() {
            return (
                self.source_offset,
                Time(
                    self.source_offset
                        .0
                        .saturating_add(self.range.end.0 - self.range.start.0),
                ),
            );
        }
        // Bounds use the exclusive integrated endpoint, rather than the clamped
        // frame lookup (which always remains inside the retained source span).
        let integral = |local: i64| {
            let pos = self
                .time_map
                .integrated(Time(local.saturating_add(self.time_map.phase.0)))
                .0;
            self.time_map
                .source_length
                .map_or(pos, |length| pos.min(length.0))
        };
        let (a, b) = (integral(0), integral(self.range.end.0 - self.range.start.0));
        let (a, b) = if self.time_map.reverse {
            let length = self
                .time_map
                .source_length
                .expect("validated reverse span")
                .0;
            (length - b, length - a)
        } else {
            (a, b)
        };
        (
            Time(self.source_offset.0.saturating_add(a)),
            Time(self.source_offset.0.saturating_add(b)),
        )
    }
}

/// Build Insert Frame Hold Segment as one batch. Split tails keep their curve phase;
/// linked companions get the same silent hold, and all unlocked trailing clips ripple.
/// A hold can be inserted at the clip start or strictly inside it.
#[allow(clippy::too_many_lines)]
pub fn freeze_frame_command(
    project: &Project,
    id: ClipId,
    at: Time,
    frames: u32,
) -> Result<Command, Error> {
    let original = project
        .tracks()
        .iter()
        .find_map(|t| t.clips().get(&id))
        .ok_or_else(|| invalid("clip missing"))?;
    if frames == 0 || at < original.range.start || at >= original.range.end {
        return Err(invalid(
            "hold requires positive frames and a playhead inside the clip",
        ));
    }
    let duration = project
        .frame_rate()
        .frame_to_time(i64::from(frames), crate::Rounding::Nearest)?
        .0;
    let mut next = project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().keys())
        .map(|id| id.0)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| invalid("clip id overflow"))?;
    let tail_group = ClipId(next);
    next = next
        .checked_add(1)
        .ok_or_else(|| invalid("clip id overflow"))?;
    let hold_group = ClipId(next);
    next = next
        .checked_add(1)
        .ok_or_else(|| invalid("clip id overflow"))?;
    let mut commands = Vec::new();
    let mut ids = Vec::new();
    let mut snapshots = Vec::new();
    for track in project.tracks() {
        for clip in track.clips().values() {
            let companion = clip.id == id
                || (original.link_group.is_some() && clip.link_group == original.link_group);
            if companion && (at < clip.range.start || at >= clip.range.end) {
                return Err(invalid("linked companions must span the hold insertion"));
            }
            if companion && track.is_locked() {
                return Err(invalid("linked track is locked"));
            }
            if !companion && (track.is_locked() || clip.range.start < at) {
                continue;
            }
            if companion {
                let tail_id = ClipId(next);
                next = next
                    .checked_add(1)
                    .ok_or_else(|| invalid("clip id overflow"))?;
                let hold_id = ClipId(next);
                next = next
                    .checked_add(1)
                    .ok_or_else(|| invalid("clip id overflow"))?;
                let mut tail = crate::command::split_right(clip, at, tail_id)?;
                tail.range = TimeRange::new(
                    Time(
                        at.0.checked_add(duration)
                            .ok_or_else(|| invalid("hold overflow"))?,
                    ),
                    Time(
                        clip.range
                            .end
                            .0
                            .checked_add(duration)
                            .ok_or_else(|| invalid("hold overflow"))?,
                    ),
                )?;
                // Retain companion pairing separately for each segment.
                tail.link_group = original.link_group.map(|_| tail_group);
                let mut hold = clip.clone();
                hold.id = hold_id;
                hold.source_offset = clip.source_time(Time(at.0 - clip.range.start.0));
                hold.time_map = ClipTimeMap {
                    hold: true,
                    ..ClipTimeMap::default()
                };
                hold.range = TimeRange::new(at, Time(at.0 + duration))?;
                hold.link_group = original.link_group.map(|_| hold_group);
                hold.animations.clone_from(&tail.animations);
                let mut pieces = vec![hold, tail];
                if at > clip.range.start {
                    let mut left = clip.clone();
                    left.range.end = at;
                    pieces.push(left);
                }
                let mut replace = vec![clip.id, tail_id, hold_id];
                replace.sort_unstable();
                commands.push(Command::RestoreClips {
                    track: track.id(),
                    ids: replace,
                    clips: pieces,
                });
            } else {
                ids.push((track.id(), clip.id));
                snapshots.push(clip.clone());
            }
        }
    }
    // Restore snapshots avoid propagating companion moves twice.
    for ((track, id), mut clip) in ids.into_iter().zip(snapshots) {
        clip.range = TimeRange::new(
            Time(
                clip.range
                    .start
                    .0
                    .checked_add(duration)
                    .ok_or_else(|| invalid("hold overflow"))?,
            ),
            Time(
                clip.range
                    .end
                    .0
                    .checked_add(duration)
                    .ok_or_else(|| invalid("hold overflow"))?,
            ),
        )?;
        commands.push(Command::RestoreClips {
            track,
            ids: vec![id],
            clips: vec![clip],
        });
    }
    Ok(Command::Batch { commands })
}

pub(crate) fn speed_commands(
    project: &Project,
    id: ClipId,
    speed: ClipSpeed,
    reverse: bool,
    ramps: &[SpeedPoint],
    ripple: bool,
) -> Result<Vec<Command>, Error> {
    let original = project
        .tracks()
        .iter()
        .find_map(|t| t.clips().get(&id))
        .ok_or_else(|| invalid("clip missing"))?;
    let map = ClipTimeMap {
        speed,
        reverse,
        ramps: ramps.to_vec(),
        ..ClipTimeMap::default()
    };
    // Validate before doing any integration of untrusted ramp times.
    let mut checked = map.clone();
    checked.reverse = false;
    checked.validate(1)?;
    let (start, end) = original.source_bounds();
    let duration = map.duration_for(Time(end.0 - start.0))?;
    let delta = duration.0 - (original.range.end.0 - original.range.start.0);
    let mut commands = Vec::new();
    for track in project.tracks() {
        for clip in track.clips().values() {
            let companion = clip.id == id
                || (original.link_group.is_some() && clip.link_group == original.link_group);
            if !companion && (!ripple || track.is_locked() || clip.range.start < original.range.end)
            {
                continue;
            }
            if track.is_locked() {
                return Err(invalid("linked track is locked"));
            }
            let mut updated = clip.clone();
            if companion {
                if clip.time_map.hold {
                    return Err(invalid("trim frame holds to change their duration"));
                }
                let (start, end) = clip.source_bounds();
                let length = Time(end.0 - start.0);
                let duration = map.duration_for(length)?;
                updated.time_map = ClipTimeMap {
                    source_length: Some(length),
                    ..map.clone()
                };
                updated.source_offset = start;
                updated.range.end = Time(
                    clip.range
                        .start
                        .0
                        .checked_add(duration.0)
                        .ok_or_else(|| invalid("speed duration overflow"))?,
                );
                if ripple && duration.0 - (clip.range.end.0 - clip.range.start.0) != delta {
                    return Err(invalid("ripple speed requires aligned companion durations"));
                }
            } else {
                updated.range = TimeRange::new(
                    Time(
                        clip.range
                            .start
                            .0
                            .checked_add(delta)
                            .ok_or_else(|| invalid("ripple overflow"))?,
                    ),
                    Time(
                        clip.range
                            .end
                            .0
                            .checked_add(delta)
                            .ok_or_else(|| invalid("ripple overflow"))?,
                    ),
                )?;
            }
            commands.push(Command::RestoreClips {
                track: track.id(),
                ids: vec![clip.id],
                clips: vec![updated],
            });
        }
    }
    Ok(commands)
}
