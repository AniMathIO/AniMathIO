use animathio_core::{ClipId, Project, TICKS_PER_SECOND, Time, TrackId};

/// Pixel geometry shared by timeline painting and hit-testing. All coordinates are
/// relative to the timeline canvas origin (not the window origin); callers add the
/// canvas offset before painting or subtract it after a pointer event.
pub(crate) struct TimelineLayout {
    pub(crate) width: f32,
    pub(crate) ruler_height: f32,
    pub(crate) row_height: f32,
    pub(crate) duration_ticks: i64,
}

/// A clip under the pointer, in back-to-front track order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ClipHit {
    pub(crate) track: TrackId,
    pub(crate) clip: ClipId,
}

/// Project playback length: the furthest clip end, rounded up to whole seconds
/// and at least one second so an empty timeline still has a usable ruler.
pub(crate) fn project_duration(project: &Project) -> i64 {
    let mut end = TICKS_PER_SECOND;
    for track in project.tracks() {
        for clip in track.clips().values() {
            end = end.max(clip.range.end.0);
        }
    }
    let seconds = end.saturating_add(TICKS_PER_SECOND - 1) / TICKS_PER_SECOND;
    seconds.saturating_mul(TICKS_PER_SECOND)
}

/// Map a timeline x (relative to the canvas left edge) to a tick, clamped to the
/// visible range. Sub-tick pointer positions land on the nearest integer tick.
pub(crate) fn x_to_time(layout: &TimelineLayout, x: f32) -> Time {
    let fraction = (f64::from(x) / f64::from(layout.width.max(1.0))).clamp(0.0, 1.0);
    Time((fraction * layout.duration_ticks as f64) as i64)
}

/// Map a tick to a timeline x (relative to the canvas left edge).
pub(crate) fn time_to_x(layout: &TimelineLayout, time: Time) -> f32 {
    let fraction = time.0 as f64 / layout.duration_ticks.max(1) as f64;
    (fraction * f64::from(layout.width)) as f32
}

/// Return the clip under a canvas-relative point, or `None` for the ruler and
/// empty track space.
pub(crate) fn hit_test(
    layout: &TimelineLayout,
    project: &Project,
    x: f32,
    y: f32,
) -> Option<ClipHit> {
    if y < layout.ruler_height {
        return None;
    }
    let row = ((y - layout.ruler_height) / layout.row_height.max(1.0)) as usize;
    let track = project.tracks().get(row)?;
    for clip in track.clips().values() {
        let start_x = time_to_x(layout, clip.range.start);
        let end_x = time_to_x(layout, clip.range.end);
        if x >= start_x && x < end_x {
            return Some(ClipHit {
                track: track.id(),
                clip: clip.id,
            });
        }
    }
    None
}

/// Map a drag pointer x to a new clip start, preserving the grab offset captured
/// when the drag began. The result never goes negative.
pub(crate) fn drag_start(layout: &TimelineLayout, grab_offset: i64, x: f32) -> Time {
    Time(x_to_time(layout, x).0.saturating_sub(grab_offset).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{
        Clip, ClipId, Command, Project, TICKS_PER_SECOND, Time, TimeRange, Track, TrackId,
        builtin::Builtin,
    };

    fn sample_project() -> Project {
        let mut project = Project::default();
        let range = TimeRange::new(Time(0), Time(TICKS_PER_SECOND * 2)).unwrap();
        Command::AddTrack {
            track: Track::new(TrackId(1), "A"),
            index: 0,
        }
        .apply(&mut project)
        .unwrap();
        Command::AddClip {
            track: TrackId(1),
            clip: Clip::new(
                ClipId(10),
                animathio_core::ClipKind::Builtin(Builtin::Shape {
                    shape: animathio_core::builtin::Shape::Rect,
                    fill: None,
                    stroke: None,
                }),
                range,
            ),
        }
        .apply(&mut project)
        .unwrap();
        project
    }

    fn layout() -> TimelineLayout {
        TimelineLayout {
            width: 1000.0,
            ruler_height: 24.0,
            row_height: 40.0,
            duration_ticks: project_duration(&sample_project()),
        }
    }

    #[test]
    fn duration_rounds_up_to_whole_seconds_with_a_floor() {
        assert_eq!(project_duration(&Project::default()), TICKS_PER_SECOND);
        assert_eq!(project_duration(&sample_project()), TICKS_PER_SECOND * 2);
    }

    #[test]
    fn time_and_x_round_trip_and_clamp() {
        let layout = layout();
        for time in [Time(0), Time(TICKS_PER_SECOND), Time(TICKS_PER_SECOND * 2)] {
            let x = time_to_x(&layout, time);
            assert_eq!(x_to_time(&layout, x), time);
        }
        assert_eq!(x_to_time(&layout, -5.0), Time(0));
        assert_eq!(x_to_time(&layout, 10_000.0), Time(layout.duration_ticks));
    }

    #[test]
    fn hit_test_finds_clips_in_rows_and_ignores_ruler_and_gaps() {
        let project = sample_project();
        let layout = layout();
        // Inside the first (only) row and inside the clip [0, 2s).
        let hit = hit_test(&layout, &project, 100.0, 40.0).unwrap();
        assert_eq!(hit.clip, ClipId(10));
        assert_eq!(hit.track, TrackId(1));
        // Ruler and empty track space.
        assert!(hit_test(&layout, &project, 100.0, 10.0).is_none());
        assert!(hit_test(&layout, &project, 100.0, 80.0).is_none());
    }

    #[test]
    fn drag_start_preserves_grab_offset_and_clamps_to_zero() {
        let layout = layout();
        let grab = 100;
        let start = drag_start(&layout, grab, 300.0);
        assert_eq!(start.0, x_to_time(&layout, 300.0).0 - grab);
        assert_eq!(drag_start(&layout, grab, 0.0), Time(0));
    }
}
