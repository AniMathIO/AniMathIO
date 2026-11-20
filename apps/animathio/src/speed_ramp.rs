//! Speed-ramp handles on the timeline: where a key is drawn, how a drag maps to a speed
//! and time, and the single `Command::SetClipSpeed` each gesture ends in.
//!
//! A ramp is the clip's ordered list of [`SpeedPoint`]s: clip-local timeline ticks and a
//! speed, interpolated linearly. Every handle gesture (move, add, remove) produces one
//! `SetClipSpeed` with the new key list, so it is one undo step, follows linked
//! companions and passes through the same source-limit guards as the Speed dialog.

use animathio_core::{Clip, ClipSpeed, Command, SpeedPoint, Time};

/// Largest number of speed keys a clip accepts.
const MAX_KEYS: usize = 128;
/// Slowest and fastest speed a key can take, in percent.
const MIN_PERCENT: f64 = 6.25;
const MAX_PERCENT: f64 = 1600.0;
/// Drags within this many percent of 100% land exactly on 100%.
const SNAP_PERCENT: f64 = 3.0;
/// Handles are only drawn when the lane has at least this many pixels of height.
const MIN_LANE_PX: f32 = 14.0;

/// The vertical band of a clip block that maps to speed (log scale, 6.25% at the
/// bottom, 1600% at the top, 100% in the middle). Pixel values share one frame of
/// reference with whoever builds the lane, so painting and hit-testing agree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Lane {
    pub(crate) top: f32,
    pub(crate) bottom: f32,
}

/// The lane inside a clip block spanning `y0..y1`, inside the edges where the fade
/// handles (top) and keyframe diamonds (bottom) sit; a pointer on a key grabs the key first. `None` for blocks too
/// short to hold it.
pub(crate) fn lane(y0: f32, y1: f32) -> Option<Lane> {
    let (top, bottom) = (y0 + 11.0, y1 - 11.0);
    (bottom - top >= MIN_LANE_PX).then_some(Lane { top, bottom })
}

impl Lane {
    /// Pixel row of a speed.
    pub(crate) fn y(self, speed: ClipSpeed) -> f32 {
        let octaves = (speed.percent() / 100.0).log2().clamp(-4.0, 4.0);
        let t = ((octaves + 4.0) / 8.0) as f32;
        self.bottom - t * (self.bottom - self.top)
    }
    /// The speed a pointer row selects: whole percent, with 100% magnetic.
    pub(crate) fn speed_at(self, y: f32) -> ClipSpeed {
        let t = f64::from(((self.bottom - y) / (self.bottom - self.top)).clamp(0.0, 1.0));
        let percent = 100.0 * (8.0 * t - 4.0).exp2();
        let percent = if (percent - 100.0).abs() <= SNAP_PERCENT {
            100.0
        } else {
            percent.round()
        };
        percent_speed(percent)
    }
}

fn percent_speed(percent: f64) -> ClipSpeed {
    let units = (percent.clamp(MIN_PERCENT, MAX_PERCENT) * 10_000.0).round() as u32;
    ClipSpeed::new(units, 1_000_000).expect("clamped to the valid speed range")
}

/// Speed of `clip` at clip-local `time`: the linearly interpolated ramp (the nearest key
/// outside the keys), or the constant speed when there are no keys.
pub(crate) fn speed_at(clip: &Clip, time: Time) -> ClipSpeed {
    let keys = &clip.time_map.ramps;
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
        return clip.time_map.speed;
    };
    if time <= first.time {
        return first.speed;
    }
    if time >= last.time {
        return last.speed;
    }
    for pair in keys.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if time < b.time {
            let t = (time.0 - a.time.0) as f64 / (b.time.0 - a.time.0) as f64;
            let percent = a.speed.percent() + (b.speed.percent() - a.speed.percent()) * t;
            return percent_speed(percent.round());
        }
    }
    last.speed
}

/// The clip-local time range key `index` may occupy: strictly between its neighbours,
/// at least `gap` ticks from each, and inside the clip.
pub(crate) fn time_bounds(clip: &Clip, index: usize, gap: i64) -> (i64, i64) {
    let keys = &clip.time_map.ramps;
    let length = clip.range.end.0 - clip.range.start.0;
    let low = index
        .checked_sub(1)
        .map_or(0, |i| keys[i].time.0.saturating_add(gap));
    let high = keys
        .get(index + 1)
        .map_or(length, |next| next.time.0.saturating_sub(gap));
    (low, high.max(low))
}

fn with_keys(clip: &Clip, ramps: Vec<SpeedPoint>) -> Command {
    // Removing the last key leaves a constant clip at that key's speed.
    let speed = match (clip.time_map.ramps.as_slice(), ramps.is_empty()) {
        ([only], true) => only.speed,
        _ => clip.time_map.speed,
    };
    Command::SetClipSpeed {
        id: clip.id,
        speed,
        reverse: clip.time_map.reverse,
        ramps,
        ripple: false,
    }
}

/// Move key `index` to clip-local `time` and `speed`. `None` when nothing changes.
pub(crate) fn move_key(clip: &Clip, index: usize, time: Time, speed: ClipSpeed) -> Option<Command> {
    let mut ramps = clip.time_map.ramps.clone();
    let key = ramps.get_mut(index)?;
    if key.time == time && key.speed == speed {
        return None;
    }
    *key = SpeedPoint { time, speed };
    Some(with_keys(clip, ramps))
}

/// Insert a key at clip-local `time` at the speed the clip already has there, so the
/// curve does not change until the new handle is dragged. `None` when a key is already
/// within `gap` ticks, the time is outside the clip or the clip is at the key limit.
pub(crate) fn add_key(clip: &Clip, time: Time, gap: i64) -> Option<Command> {
    let length = clip.range.end.0 - clip.range.start.0;
    let keys = &clip.time_map.ramps;
    if time.0 < 0
        || time.0 > length
        || keys.len() >= MAX_KEYS
        || keys.iter().any(|k| (k.time.0 - time.0).abs() < gap)
    {
        return None;
    }
    let mut ramps = keys.clone();
    let at = ramps.partition_point(|k| k.time < time);
    ramps.insert(
        at,
        SpeedPoint {
            time,
            speed: speed_at(clip, time),
        },
    );
    Some(with_keys(clip, ramps))
}

/// Remove key `index`.
pub(crate) fn remove_key(clip: &Clip, index: usize) -> Option<Command> {
    let mut ramps = clip.time_map.ramps.clone();
    if index >= ramps.len() {
        return None;
    }
    ramps.remove(index);
    Some(with_keys(clip, ramps))
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{AssetId, ClipId, ClipKind, Project, TICKS_PER_SECOND, TimeRange, Track};
    use animathio_core::{Command, TrackId};

    fn speed(percent: u32) -> ClipSpeed {
        ClipSpeed::new(percent, 100).unwrap()
    }
    fn project_with_ramp(keys: &[(i64, u32)]) -> (Project, ClipId) {
        let mut project = Project::default();
        let asset = AssetId::of(b"clip");
        for command in [
            Command::AddAsset {
                id: asset.clone(),
                asset: animathio_core::Asset {
                    name: "a.wav".into(),
                    media_type: "audio/wav".into(),
                },
            },
            Command::AddTrack {
                track: Track::new(TrackId(1), "t"),
                index: 0,
            },
            Command::AddClip {
                track: TrackId(1),
                clip: animathio_core::Clip::new(
                    ClipId(1),
                    ClipKind::Audio(asset),
                    TimeRange::new(Time(0), Time(4 * TICKS_PER_SECOND)).unwrap(),
                ),
            },
            Command::SetClipSpeed {
                id: ClipId(1),
                speed: ClipSpeed::default(),
                reverse: false,
                ramps: keys
                    .iter()
                    .map(|(time, percent)| SpeedPoint {
                        time: Time(*time),
                        speed: speed(*percent),
                    })
                    .collect(),
                ripple: false,
            },
        ] {
            command.apply(&mut project).unwrap();
        }
        (project, ClipId(1))
    }
    fn clip(project: &Project, id: ClipId) -> Clip {
        project.tracks()[0].clips()[&id].clone()
    }

    #[test]
    fn lane_maps_speed_on_a_log_scale_with_a_magnetic_100_percent() {
        let lane = lane(0.0, 100.0).unwrap();
        assert_eq!((lane.top, lane.bottom), (11.0, 89.0));
        let middle = f32::midpoint(lane.top, lane.bottom);
        assert!((lane.y(speed(100)) - middle).abs() < 1e-3);
        assert!((lane.y(speed(1600)) - lane.top).abs() < 1e-3);
        assert!(lane.y(speed(25)) > lane.y(speed(50)), "slower is lower");
        assert_eq!(lane.speed_at(middle - 0.1), speed(100));
        assert_eq!(lane.speed_at(lane.top - 50.0), speed(1600));
        assert!((lane.speed_at(lane.bottom + 50.0).percent() - 6.25).abs() < 1e-9);
        // Round trips through a pointer row, to whole percent.
        for percent in [25, 50, 200, 400, 800] {
            assert_eq!(lane.speed_at(lane.y(speed(percent))), speed(percent));
        }
        assert!(super::lane(0.0, 30.0).is_none());
    }

    #[test]
    fn interpolates_the_curve_and_holds_outside_the_keys() {
        let (project, id) = project_with_ramp(&[(1000, 50), (3000, 150)]);
        let clip = clip(&project, id);
        assert_eq!(speed_at(&clip, Time(0)), speed(50));
        assert_eq!(speed_at(&clip, Time(2000)), speed(100));
        assert_eq!(speed_at(&clip, Time(9000)), speed(150));
    }

    #[test]
    fn key_time_is_kept_between_neighbours_and_inside_the_clip() {
        let (project, id) = project_with_ramp(&[(1000, 50), (3000, 150), (5000, 200)]);
        let clip = clip(&project, id);
        assert_eq!(time_bounds(&clip, 0, 10), (0, 2990));
        assert_eq!(time_bounds(&clip, 1, 10), (1010, 4990));
        let length = clip.range.end.0 - clip.range.start.0;
        assert_eq!(time_bounds(&clip, 2, 10), (3010, length));
    }

    #[test]
    fn each_gesture_is_one_undoable_set_clip_speed() {
        let (mut project, id) = project_with_ramp(&[(1000, 50), (3000, 150)]);
        let before = project.clone();
        let original = clip(&project, id);
        // Move.
        let command = move_key(&original, 1, Time(2500), speed(300)).unwrap();
        assert!(matches!(command, Command::SetClipSpeed { .. }));
        let inverse = command.apply(&mut project).unwrap();
        let moved = clip(&project, id);
        assert_eq!(
            moved.time_map.ramps[1],
            SpeedPoint {
                time: Time(2500),
                speed: speed(300)
            }
        );
        inverse.apply(&mut project).unwrap();
        assert_eq!(project, before);
        assert!(move_key(&original, 1, Time(3000), speed(150)).is_none());
        assert!(move_key(&original, 9, Time(3000), speed(150)).is_none());
        // Add keeps the curve identical at the new key.
        let command = add_key(&original, Time(2000), 10).unwrap();
        command.apply(&mut project).unwrap();
        let added = clip(&project, id);
        assert_eq!(added.time_map.ramps.len(), 3);
        assert_eq!(added.time_map.ramps[1].time, Time(2000));
        assert_eq!(added.time_map.ramps[1].speed, speed(100));
        assert!(
            add_key(&added, Time(2005), 10).is_none(),
            "too close to a key"
        );
        assert!(add_key(&added, Time(-1), 10).is_none());
        // Remove.
        let command = remove_key(&added, 1).unwrap();
        command.apply(&mut project).unwrap();
        assert_eq!(clip(&project, id).time_map.ramps, original.time_map.ramps);
        assert!(remove_key(&original, 5).is_none());
    }

    #[test]
    fn removing_the_last_key_leaves_a_constant_clip_at_its_speed() {
        let (mut project, id) = project_with_ramp(&[(1000, 200)]);
        let command = remove_key(&clip(&project, id), 0).unwrap();
        command.apply(&mut project).unwrap();
        let constant = clip(&project, id);
        assert_eq!(constant.time_map.ramps.len(), 0);
        assert_eq!(constant.time_map.speed, speed(200));
    }

    #[test]
    fn adding_to_a_constant_clip_creates_a_flat_curve() {
        let (mut project, id) = project_with_ramp(&[]);
        let command = add_key(&clip(&project, id), Time(1500), 10).unwrap();
        command.apply(&mut project).unwrap();
        let ramped = clip(&project, id);
        assert_eq!(
            ramped.time_map.ramps,
            [SpeedPoint {
                time: Time(1500),
                speed: speed(100)
            }]
        );
    }
}
