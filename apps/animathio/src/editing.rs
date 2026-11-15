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
    RateStretch,
    Slip,
    Slide,
    Rolling,
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
            ripple_delete_command(
                Some(ClipId(1)),
                Some(range),
                animathio_core::MarkerRipple::default()
            ),
            Some(Command::RippleDelete {
                clip: ClipId(1),
                markers: animathio_core::MarkerRipple::default()
            })
        );
        assert_eq!(
            ripple_delete_command(None, Some(range), animathio_core::MarkerRipple::default()),
            Some(Command::RemoveRange {
                range,
                ripple: true,
                markers: animathio_core::MarkerRipple::default(),
            })
        );
        assert_eq!(
            ripple_delete_command(None, None, animathio_core::MarkerRipple::default()),
            None
        );
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
                    markers: animathio_core::MarkerRipple::default(),
                },
            )
            .unwrap();
        assert_eq!(project.tracks()[0].clips()[&ClipId(2)].range.start, Time(0));
    }
}

/// Plan an atomic edit and clamp the pointer delta against core source, lock,
/// overlap and retiming guards, including linked companions.
pub(crate) fn tool_command(
    project: &Project,
    id: ClipId,
    tool: Tool,
    edge: TrimEdge,
    delta: i64,
) -> Option<(Command, i64)> {
    tool_command_guarded(project, id, tool, edge, delta, |_| true)
}

pub(crate) fn tool_command_guarded(
    project: &Project,
    id: ClipId,
    tool: Tool,
    edge: TrimEdge,
    delta: i64,
    mut guard: impl FnMut(&Command) -> bool,
) -> Option<(Command, i64)> {
    let track = project
        .tracks()
        .iter()
        .find(|t| t.clips().contains_key(&id))?;
    let clip = track.clips().get(&id)?;
    let plan = |d: i64| -> Option<Command> {
        let trim = |c: &Clip, start: i64, end: i64, offset: i64| -> Option<Command> {
            Some(Command::TrimClip {
                id: c.id,
                range: TimeRange::new(Time(start), Time(end)).ok()?,
                source_offset: Time(offset),
            })
        };
        let mut commands = Vec::new();
        match tool {
            Tool::RateStretch => {
                if clip.time_map.hold {
                    return None;
                }
                let duration = (clip.range.end.0 - clip.range.start.0)
                    .checked_add(if edge == TrimEdge::Left { -d } else { d })?;
                if duration <= 0 {
                    return None;
                }
                let (a, b) = clip.source_bounds();
                let units = (i128::from(b.0 - a.0) * 1_000_000 + i128::from(duration) / 2)
                    / i128::from(duration);
                let speed =
                    animathio_core::ClipSpeed::new(u32::try_from(units).ok()?, 1_000_000).ok()?;
                if edge == TrimEdge::Left {
                    commands.push(Command::MoveClip {
                        id,
                        track: track.id(),
                        start: Time(
                            clip.range.end.0.checked_sub(
                                animathio_core::ClipTimeMap {
                                    speed,
                                    ..Default::default()
                                }
                                .duration_for(Time(b.0 - a.0))
                                .ok()?
                                .0,
                            )?,
                        ),
                    });
                }
                commands.push(Command::SetClipSpeed {
                    id,
                    speed,
                    reverse: clip.time_map.reverse,
                    ramps: vec![],
                    ripple: false,
                });
            }
            Tool::Slip => commands.push(trim(
                clip,
                clip.range.start.0,
                clip.range.end.0,
                clip.source_offset.0.checked_add(d)?,
            )?),
            Tool::Rolling | Tool::Slide => {
                let left = track
                    .clips()
                    .values()
                    .find(|c| c.id != id && c.range.end == clip.range.start);
                let right = track
                    .clips()
                    .values()
                    .find(|c| c.id != id && c.range.start == clip.range.end);
                if tool == Tool::Slide || edge == TrimEdge::Left {
                    let left = left?;
                    commands.push(trim(
                        left,
                        left.range.start.0,
                        left.range.end.0.checked_add(d)?,
                        left.source_offset.0,
                    )?);
                }
                if tool == Tool::Slide || edge == TrimEdge::Right {
                    let right = right?;
                    commands.push(trim(
                        right,
                        right.range.start.0.checked_add(d)?,
                        right.range.end.0,
                        right.source_offset.0.checked_add(d)?,
                    )?);
                }
                if tool == Tool::Slide {
                    commands.push(Command::MoveClip {
                        id,
                        track: track.id(),
                        start: Time(clip.range.start.0.checked_add(d)?),
                    });
                } else {
                    let start = clip.range.start.0 + if edge == TrimEdge::Left { d } else { 0 };
                    let end = clip.range.end.0 + if edge == TrimEdge::Right { d } else { 0 };
                    commands.push(trim(
                        clip,
                        start,
                        end,
                        clip.source_offset.0 + if edge == TrimEdge::Left { d } else { 0 },
                    )?);
                }
            }
            _ => return None,
        }
        Some(Command::Batch { commands })
    };
    let mut valid = |d| {
        let cmd = plan(d)?;
        if !guard(&cmd) {
            return None;
        }
        cmd.apply(&mut project.clone()).ok()?;
        Some(cmd)
    };
    if let Some(cmd) = valid(delta) {
        return Some((cmd, delta));
    }
    let mut low = 0_i64;
    let mut high = delta.saturating_abs();
    while low < high {
        let mid = low + (high - low) / 2 + 1;
        if valid(mid * delta.signum()).is_some() {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    let d = low * delta.signum();
    valid(d).map(|cmd| (cmd, d))
}

#[cfg(test)]
pub(crate) mod tool_tests {
    use super::*;
    use animathio_core::{ClipKind, Track, TrackId};

    pub(crate) fn fixture() -> Project {
        let mut p = Project::default();
        for row in 0..2 {
            let track = TrackId(row + 1);
            Command::AddTrack {
                track: Track::new(track, "Linked media"),
                index: usize::try_from(row).unwrap(),
            }
            .apply(&mut p)
            .unwrap();
            for i in 0..3 {
                let mut c = Clip::new(
                    ClipId(row * 3 + i + 1),
                    ClipKind::Plugin {
                        plugin_id: "test.clip".into(),
                        params: serde_json::json!({}),
                    },
                    TimeRange::new(
                        Time(i64::try_from(i).unwrap() * 1000),
                        Time(i64::try_from(i + 1).unwrap() * 1000),
                    )
                    .unwrap(),
                );
                c.source_offset = Time(1000);
                c.link_group = Some(ClipId(i + 1));
                Command::AddClip { track, clip: c }.apply(&mut p).unwrap();
            }
        }
        p
    }

    #[test]
    fn tools_clamp_and_restore_linked_companions_exactly() {
        for tool in [Tool::RateStretch, Tool::Slip, Tool::Slide, Tool::Rolling] {
            for delta in [-100_000, -100, 100, 100_000] {
                let original = fixture();
                let mut p = original.clone();
                let (cmd, actual) =
                    tool_command(&p, ClipId(2), tool, TrimEdge::Right, delta).unwrap();
                assert!(actual.abs() <= delta.abs());
                assert!(matches!(cmd, Command::Batch { .. }));
                let undo = cmd.apply(&mut p).unwrap();
                let a = p.tracks()[0].clips().get(&ClipId(2)).unwrap();
                let b = p.tracks()[1].clips().get(&ClipId(5)).unwrap();
                assert_eq!(a.range, b.range);
                assert_eq!(a.source_bounds(), b.source_bounds());
                if tool == Tool::Slip {
                    assert_eq!(a.range, original.tracks()[0].clips()[&ClipId(2)].range);
                }
                if tool == Tool::RateStretch {
                    assert_eq!(
                        a.source_bounds(),
                        original.tracks()[0].clips()[&ClipId(2)].source_bounds()
                    );
                }
                if matches!(tool, Tool::Slide | Tool::Rolling) {
                    for track in p.tracks() {
                        assert_eq!(
                            track.clips().values().map(|c| c.range.end).max(),
                            Some(Time(3000))
                        );
                    }
                }
                undo.apply(&mut p).unwrap();
                assert_eq!(p, original, "{tool:?} {delta}");
            }
        }
    }

    #[test]
    fn locked_companions_and_missing_neighbours_reject_edits() {
        let mut p = fixture();
        Command::SetTrackLocked {
            id: TrackId(2),
            locked: true,
        }
        .apply(&mut p)
        .unwrap();
        for tool in [Tool::RateStretch, Tool::Slip, Tool::Slide, Tool::Rolling] {
            assert!(tool_command(&p, ClipId(2), tool, TrimEdge::Right, -100).is_none());
        }
        let p = fixture();
        assert!(tool_command(&p, ClipId(1), Tool::Slide, TrimEdge::Right, 100).is_none());
        assert!(tool_command(&p, ClipId(3), Tool::Rolling, TrimEdge::Right, 100).is_none());
    }

    #[test]
    fn stretch_limits_and_left_anchor() {
        let mut p = fixture();
        for id in [ClipId(1), ClipId(3)] {
            Command::RemoveClip { id }.apply(&mut p).unwrap();
        }
        for (edge, delta) in [
            (TrimEdge::Right, 100_000),
            (TrimEdge::Right, -100_000),
            (TrimEdge::Left, 500),
        ] {
            let (cmd, _) = tool_command(&p, ClipId(2), Tool::RateStretch, edge, delta).unwrap();
            let mut result = p.clone();
            cmd.apply(&mut result).unwrap();
            let c = &result.tracks()[0].clips()[&ClipId(2)];
            assert!((6.25..=1600.).contains(&c.time_map.speed.percent()));
            assert_eq!(
                c.source_bounds(),
                p.tracks()[0].clips()[&ClipId(2)].source_bounds()
            );
            if edge == TrimEdge::Left {
                assert_eq!(c.range.end, Time(2000));
            }
        }
    }

    #[test]
    fn probed_media_limits_clamp_linked_slip() {
        let mut p = fixture();
        let asset = animathio_core::AssetId::of(b"media");
        Command::AddAsset {
            id: asset.clone(),
            asset: animathio_core::Asset {
                name: "media.wav".into(),
                media_type: "audio/wav".into(),
            },
        }
        .apply(&mut p)
        .unwrap();
        let tracks = p.tracks().to_vec();
        for track in tracks {
            let clips = track
                .clips()
                .values()
                .cloned()
                .map(|mut c| {
                    c.kind = ClipKind::Audio(asset.clone());
                    c
                })
                .collect::<Vec<_>>();
            Command::RestoreClips {
                track: track.id(),
                ids: clips.iter().map(|c| c.id).collect(),
                clips,
            }
            .apply(&mut p)
            .unwrap();
        }
        let mut limits = animathio_control::SourceLimits::new();
        limits.set(asset, Time(2100));
        for delta in [5000, -5000] {
            let (cmd, d) =
                tool_command_guarded(&p, ClipId(2), Tool::Slip, TrimEdge::Right, delta, |cmd| {
                    limits
                        .guard(&p, &std::collections::BTreeMap::new(), cmd)
                        .is_ok()
                })
                .unwrap();
            assert_eq!(d, if delta > 0 { 100 } else { -1000 });
            let mut result = p.clone();
            let inverse = cmd.apply(&mut result).unwrap();
            inverse.apply(&mut result).unwrap();
            assert_eq!(result, p);
        }
    }

    #[test]
    fn source_guard_clamps_slip_and_neighbour_edits() {
        for tool in [Tool::Slip, Tool::Slide, Tool::Rolling] {
            let p = fixture();
            let (_, d) = tool_command_guarded(&p, ClipId(2), tool, TrimEdge::Right, 5000, |cmd| {
                let mut candidate = p.clone();
                cmd.apply(&mut candidate).is_ok()
                    && candidate
                        .tracks()
                        .iter()
                        .flat_map(|t| t.clips().values())
                        .all(|c| c.source_bounds().1 <= Time(2100))
            })
            .unwrap();
            assert_eq!(d, 100);
        }
    }
}
