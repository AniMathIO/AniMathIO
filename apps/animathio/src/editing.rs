//! Pure view-state and command planning for the editor's editing loop.
use crate::timeline::TrimEdge;
use animathio_core::{Clip, ClipId, Command, Project, Time, TimeRange};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Tool {
    #[default]
    Select,
    Blade,
    Ripple,
    Hand,
}

/// J/K/L shuttle state: the whole-number rate (negative is reverse) plus a flag
/// for the half-speed K+J/K+L slow jog. J/L doubles up to 16x; audio follows up to
/// 8x forward and 4x reverse (see `animathio_audio::Speed::is_audible`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Shuttle(pub(crate) i32, pub(crate) bool);
impl Shuttle {
    pub(crate) fn press(&mut self, direction: i32) -> i32 {
        self.0 = if self.0.signum() == direction && !self.1 {
            (self.0 * 2).clamp(-16, 16)
        } else {
            direction
        };
        self.1 = false;
        self.0
    }
    /// K+J / K+L: half-speed jog in `direction`.
    pub(crate) fn jog(&mut self, direction: i32) {
        self.0 = direction;
        self.1 = true;
    }
    /// The playback speed this state asks the clock for.
    pub(crate) fn speed(self) -> animathio_audio::Speed {
        if self.1 {
            animathio_audio::Speed::new(self.0.signum(), 2)
        } else {
            animathio_audio::Speed::whole(if self.0 == 0 { 1 } else { self.0 })
        }
        .unwrap_or_default()
    }
    /// Short transport label such as `2x`, `-4x (muted)` or `0.5x`.
    pub(crate) fn label(self) -> String {
        let speed = self.speed();
        let value = if self.1 {
            format!("{}0.5x", if self.0 < 0 { "-" } else { "" })
        } else {
            format!("{}x", self.0)
        };
        if speed.is_audible() {
            value
        } else {
            format!("{value} (muted)")
        }
    }
}

/// Out is exclusive, just like every core `TimeRange`. Partial marks use project bounds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct InOut {
    pub(crate) start: Option<Time>,
    pub(crate) end: Option<Time>,
}
impl InOut {
    pub(crate) fn set_in(&mut self, time: Time) {
        self.start = Some(time);
        if self.end.is_some_and(|end| end <= time) {
            self.end = None;
        }
    }
    pub(crate) fn set_out(&mut self, time: Time) {
        self.end = Some(time);
        if self.start.is_some_and(|start| start >= time) {
            self.start = None;
        }
    }
    pub(crate) fn marked(self) -> bool {
        self.start.is_some() || self.end.is_some()
    }
    pub(crate) fn range(self, duration: i64) -> Option<TimeRange> {
        let duration = duration.max(0);
        TimeRange::new(
            Time(self.start.unwrap_or(Time(0)).0.clamp(0, duration)),
            Time(self.end.unwrap_or(Time(duration)).0.clamp(0, duration)),
        )
        .ok()
        .filter(|range| range.start < range.end)
    }
}

/// How far past the end of the project playback may run when it starts there.
const OVERRUN: i64 = 3_600 * animathio_core::TICKS_PER_SECOND;

/// Where forward playback starts: from the playhead wherever the user parked it (also past
/// the content end), except exactly at the end, where it rewinds to the start like a player
/// that has finished.
pub(crate) fn play_start(position: Time, duration: Time) -> Time {
    if position == duration || position < Time(0) {
        Time(0)
    } else {
        position
    }
}

/// The time playback stops at when it begins at `start`. A playhead parked past the end of the
/// content keeps playing from there (black frames, running timecode) instead of stopping at once.
pub(crate) fn play_limit(start: Time, duration: Time) -> Time {
    if start > duration {
        Time(start.0.saturating_add(OVERRUN))
    } else {
        duration
    }
}

/// Return a bounded position and whether playback has reached the end. Handles reverse
/// shuttle and arbitrarily large overshoots without losing the loop remainder.
pub(crate) fn playback_position(
    time: Time,
    range: TimeRange,
    rate: i32,
    looping: bool,
) -> (Time, bool) {
    let crossed = if rate < 0 {
        time < range.start
    } else {
        time >= range.end
    };
    if looping && (time < range.start || time >= range.end) {
        let length = range.end.0 - range.start.0;
        return (
            Time(
                range.start.0
                    + i64::try_from(
                        (i128::from(time.0) - i128::from(range.start.0))
                            .rem_euclid(i128::from(length)),
                    )
                    .expect("remainder fits range"),
            ),
            false,
        );
    }
    (Time(time.0.clamp(range.start.0, range.end.0)), crossed)
}

pub(crate) fn blade_command(clip: &Clip, at: Time, right_id: ClipId) -> Option<Command> {
    (clip.range.start < at && at < clip.range.end).then_some(Command::SplitClip {
        id: clip.id,
        right_id,
        at,
    })
}

/// Ripple trim affects only this track. The batch keeps both trimming and later moves
/// atomic, including extensions; `RemoveRange` is intentionally reserved for all-track cuts.
pub(crate) fn trim_command(
    project: &Project,
    id: ClipId,
    edge: TrimEdge,
    at: Time,
    ripple: bool,
) -> Option<Command> {
    let track = project
        .tracks()
        .iter()
        .find(|track| track.clips().contains_key(&id))?;
    let clip = track.clips().get(&id)?;
    if at
        == match edge {
            TrimEdge::Left => clip.range.start,
            TrimEdge::Right => clip.range.end,
        }
    {
        return None;
    }
    let (start, end, offset) = match edge {
        TrimEdge::Left => (
            at,
            clip.range.end,
            Time(
                clip.source_offset
                    .0
                    .checked_add(at.0 - clip.range.start.0)?,
            ),
        ),
        TrimEdge::Right => (clip.range.start, at, clip.source_offset),
    };
    if offset.0 < 0 || start >= end {
        return None;
    }
    let range = TimeRange::new(start, end).ok()?;
    let trim = Command::TrimClip {
        id,
        range,
        source_offset: offset,
    };
    if !ripple {
        return Some(trim);
    }
    let delta = (end.0 - start.0) - (clip.range.end.0 - clip.range.start.0);
    let mut commands = vec![trim];
    if edge == TrimEdge::Left {
        commands.push(Command::MoveClip {
            id,
            track: track.id(),
            start: clip.range.start,
        });
    }
    for later in track
        .clips()
        .values()
        .filter(|later| later.id != id && later.range.start >= clip.range.end)
    {
        commands.push(Command::MoveClip {
            id: later.id,
            track: track.id(),
            start: Time(later.range.start.0.checked_add(delta)?),
        });
    }
    Some(Command::Batch { commands })
}

/// Prefer a selected clip; otherwise ripple an explicitly marked all-track range.
pub(crate) fn ripple_delete_command(
    clip: Option<ClipId>,
    range: Option<TimeRange>,
    markers: animathio_core::MarkerRipple,
) -> Option<Command> {
    clip.map(|clip| Command::RippleDelete { clip, markers })
        .or_else(|| {
            range.map(|range| Command::RemoveRange {
                range,
                ripple: true,
                markers,
            })
        })
}

/// Replace existing reveal animation with a deterministic two-key Write on preset.
pub(crate) fn write_on_command(clip: &Clip, seconds: f64) -> Option<Command> {
    if !seconds.is_finite() || seconds <= 0.0 {
        return None;
    }
    let ticks = (seconds * animathio_core::TICKS_PER_SECOND as f64).round() as i64;
    if ticks <= 0 || ticks > clip.range.end.0 - clip.range.start.0 {
        return None;
    }
    let target = animathio_core::Target::Param("reveal".into());
    let mut commands = Vec::new();
    if let Some(animation) = clip.animations.iter().find(|a| a.target == target) {
        commands.extend(
            animation
                .keys
                .keys()
                .keys()
                .map(|time| Command::RemoveKeyframe {
                    id: clip.id,
                    target: target.clone(),
                    time: *time,
                }),
        );
    }
    for (time, value) in [(Time(0), 0.0), (Time(ticks), 1.0)] {
        commands.push(Command::AddKeyframe {
            id: clip.id,
            target: target.clone(),
            time,
            keyframe: animathio_core::Keyframe {
                value: value.into(),
                interpolation: animathio_core::Interpolation::Linear,
            },
        });
    }
    Some(Command::Batch { commands })
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{ClipKind, FrameRate, History, Track, TrackId};

    #[test]
    fn play_from_the_end_restarts_at_the_beginning() {
        let duration = Time(1000);
        assert_eq!(play_start(Time(1000), duration), Time(0));
        assert_eq!(play_start(Time(999), duration), Time(999));
        assert_eq!(play_start(Time(0), duration), Time(0));
    }
    #[test]
    fn play_from_a_scrubbed_playhead_past_the_end_continues_from_it() {
        let duration = Time(1000);
        // The maintainer scrubbed to 0:06 of a shorter project: play from 0:06, not 0:00.
        assert_eq!(play_start(Time(5000), duration), Time(5000));
        assert!(play_limit(Time(5000), duration) > Time(5000));
        assert_eq!(play_limit(Time(999), duration), duration);
        assert_eq!(play_limit(Time(1000), duration), duration);
        let range = TimeRange::new(Time(0), play_limit(Time(5000), duration)).unwrap();
        let (now, stopped) = playback_position(Time(5100), range, 1, false);
        assert_eq!((now, stopped), (Time(5100), false));
    }
    #[test]
    fn ripple_delete_uses_clip_or_explicit_range() {
        let range = TimeRange::new(Time(2), Time(4)).unwrap();
        assert_eq!(
            ripple_delete_command(Some(ClipId(1)), Some(range), Default::default()),
            Some(Command::RippleDelete {
                clip: ClipId(1),
                markers: Default::default()
            })
        );
        assert_eq!(
            ripple_delete_command(None, Some(range), Default::default()),
            Some(Command::RemoveRange {
                range,
                ripple: true,
                markers: Default::default(),
            })
        );
        assert_eq!(ripple_delete_command(None, None, Default::default()), None);
    }
    #[test]
    fn write_on_replaces_keys_and_rejects_invalid_durations() {
        let clip = Clip::new(
            ClipId(1),
            ClipKind::Plugin {
                plugin_id: "org.animathio.latex".into(),
                params: serde_json::json!({}),
            },
            TimeRange::new(Time(0), Time(animathio_core::TICKS_PER_SECOND * 2)).unwrap(),
        );
        for seconds in [0., -1., f64::NAN, 3.] {
            assert!(write_on_command(&clip, seconds).is_none());
        }
        let mut project = Project::new(FrameRate::new(30, 1).unwrap());
        Command::AddTrack {
            track: Track::new(TrackId(1), "Math"),
            index: 0,
        }
        .apply(&mut project)
        .unwrap();
        Command::AddClip {
            track: TrackId(1),
            clip,
        }
        .apply(&mut project)
        .unwrap();
        let old = write_on_command(&project.tracks()[0].clips()[&ClipId(1)], 0.5).unwrap();
        old.apply(&mut project).unwrap();
        let original = project.clone();
        let command = write_on_command(&project.tracks()[0].clips()[&ClipId(1)], 1.).unwrap();
        let mut history = History::default();
        history.execute(&mut project, &command).unwrap();
        let animation = &project.tracks()[0].clips()[&ClipId(1)].animations[0];
        assert_eq!(animation.keys.keys().len(), 2);
        assert_eq!(
            animation.keys.keys()[&Time(animathio_core::TICKS_PER_SECOND)]
                .value
                .as_number(),
            Some(1.)
        );
        assert!(
            !animation
                .keys
                .keys()
                .contains_key(&Time(animathio_core::TICKS_PER_SECOND / 2))
        );
        history.undo(&mut project).unwrap();
        assert_eq!(project, original);
    }

    #[test]
    fn shuttle_repeats_caps_stops_and_changes_direction() {
        let mut shuttle = Shuttle::default();
        for expected in [1, 2, 4, 8, 16, 16] {
            assert_eq!(shuttle.press(1), expected);
        }
        assert_eq!(shuttle.press(-1), -1);
        assert_eq!(shuttle.press(-1), -2);
        shuttle.0 = 0;
        assert_eq!(shuttle.press(-1), -1);
    }
    #[test]
    fn shuttle_jog_is_half_speed_and_labels_muted_rates() {
        let mut shuttle = Shuttle::default();
        shuttle.jog(-1);
        assert_eq!(shuttle.speed(), animathio_audio::Speed::HALF_REVERSE);
        assert_eq!(shuttle.label(), "-0.5x");
        // The next plain press leaves jog mode instead of doubling.
        assert_eq!(shuttle.press(-1), -1);
        assert_eq!(shuttle.speed().as_f64(), -1.0);
        let mut shuttle = Shuttle::default();
        let labels: Vec<_> = (0..5)
            .map(|_| {
                shuttle.press(1);
                shuttle.label()
            })
            .collect();
        assert_eq!(
            labels,
            ["1x", "2x", "4x", "8x", "16x (muted)"].map(String::from)
        );
        shuttle.0 = -4;
        assert_eq!(shuttle.label(), "-4x");
        shuttle.0 = -8;
        assert_eq!(shuttle.label(), "-8x (muted)");
    }
    #[test]
    fn marks_partial_reversed_and_out_of_bounds() {
        let mut marks = InOut::default();
        marks.set_in(Time(3));
        assert_eq!(
            marks.range(10).unwrap(),
            TimeRange::new(Time(3), Time(10)).unwrap()
        );
        marks.set_out(Time(8));
        assert_eq!(marks.range(6).unwrap().end, Time(6));
        marks.set_in(Time(9));
        assert_eq!(marks.end, None);
        marks.set_out(Time(2));
        assert_eq!(marks.start, None);
        marks.set_out(Time(0));
        assert_eq!(marks.range(10), None);
    }
    #[test]
    fn loop_keeps_overshoot_in_both_directions() {
        let range = TimeRange::new(Time(10), Time(20)).unwrap();
        assert_eq!(
            playback_position(Time(43), range, 4, true),
            (Time(13), false)
        );
        assert_eq!(
            playback_position(Time(8), range, -2, true),
            (Time(18), false)
        );
        assert_eq!(
            playback_position(Time(21), range, 1, false),
            (Time(20), true)
        );
    }
    #[test]
    fn blade_and_ripple_are_single_undo_steps_on_the_source_track() {
        let mut project = Project::new(FrameRate::new(30, 1).unwrap());
        Command::AddTrack {
            track: Track::new(TrackId(1), "Video"),
            index: 0,
        }
        .apply(&mut project)
        .unwrap();
        for (id, start, end) in [(1, 0, 10), (2, 10, 20)] {
            Command::AddClip {
                track: TrackId(1),
                clip: Clip::new(
                    ClipId(id),
                    ClipKind::Plugin {
                        plugin_id: "test".into(),
                        params: serde_json::json!({}),
                    },
                    TimeRange::new(Time(start), Time(end)).unwrap(),
                ),
            }
            .apply(&mut project)
            .unwrap();
        }
        Command::AddTrack {
            track: Track::new(TrackId(2), "Other"),
            index: 1,
        }
        .apply(&mut project)
        .unwrap();
        Command::AddClip {
            track: TrackId(2),
            clip: Clip::new(
                ClipId(9),
                ClipKind::Plugin {
                    plugin_id: "other".into(),
                    params: serde_json::json!({}),
                },
                TimeRange::new(Time(10), Time(20)).unwrap(),
            ),
        }
        .apply(&mut project)
        .unwrap();
        let original = project.clone();
        let clip = &project.tracks()[0].clips()[&ClipId(1)];
        assert!(blade_command(clip, Time(0), ClipId(3)).is_none());
        let blade = blade_command(clip, Time(5), ClipId(3)).unwrap();
        let mut history = History::default();
        history.execute(&mut project, &blade).unwrap();
        history.undo(&mut project).unwrap();
        assert_eq!(project, original);
        let trim = trim_command(&project, ClipId(1), TrimEdge::Right, Time(6), true).unwrap();
        history.execute(&mut project, &trim).unwrap();
        assert_eq!(project.tracks()[0].clips()[&ClipId(2)].range.start, Time(6));
        history.undo(&mut project).unwrap();
        assert_eq!(project, original);
        for (edge, at, later_start) in [
            (TrimEdge::Right, Time(12), Time(12)),
            (TrimEdge::Left, Time(3), Time(7)),
        ] {
            let trim = trim_command(&project, ClipId(1), edge, at, true).unwrap();
            history.execute(&mut project, &trim).unwrap();
            assert_eq!(
                project.tracks()[0].clips()[&ClipId(2)].range.start,
                later_start
            );
            assert_eq!(project.tracks()[1], original.tracks()[1]);
            history.undo(&mut project).unwrap();
            assert_eq!(project, original);
        }
        history
            .execute(
                &mut project,
                &Command::RippleDelete {
                    clip: ClipId(1),
                    markers: Default::default(),
                },
            )
            .unwrap();
        assert_eq!(project.tracks()[0].clips()[&ClipId(2)].range.start, Time(0));
    }
}
