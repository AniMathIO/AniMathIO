use animathio_core::{ClipId, Project, TICKS_PER_SECOND, Time, TrackId};

/// Pixel geometry shared by timeline painting and hit-testing. All coordinates are
/// relative to the timeline canvas origin (not the window origin); callers add the
/// canvas offset before painting or subtract it after a pointer event.
pub(crate) struct TimelineLayout {
    pub(crate) width: f32,
    pub(crate) ruler_height: f32,
    pub(crate) row_height: f32,
    pub(crate) duration_ticks: i64,
    pub(crate) pixels_per_tick: f64,
    pub(crate) scroll_x: f64,
}
impl TimelineLayout {
    /// Build a layout with an explicit zoom (pixels-per-tick multiplier) and a
    /// horizontal scroll offset, both clamped to sane bounds.
    pub(crate) fn zoomed(
        width: f32,
        ruler_height: f32,
        row_height: f32,
        duration_ticks: i64,
        zoom: f32,
        scroll_x: f32,
    ) -> Self {
        let width = width.max(1.0);
        let pixels_per_tick =
            f64::from(width) / duration_ticks.max(1) as f64 * f64::from(zoom.clamp(1.0, 64.0));
        let max_scroll =
            (duration_ticks.max(1) as f64 * pixels_per_tick - f64::from(width)).max(0.0);
        Self {
            width,
            ruler_height,
            row_height,
            duration_ticks,
            pixels_per_tick,
            scroll_x: f64::from(scroll_x).clamp(0.0, max_scroll),
        }
    }
}

/// Ruler tick positions (multiples of `minor`) inside the visible range only.
/// The walk is bounded by `width / 4` iterations and uses `checked_add`, so a
/// huge duration, a tiny zoom or a degenerate step can never stall the UI.
pub(crate) fn ruler_ticks(layout: &TimelineLayout, minor: i64) -> Vec<i64> {
    let minor = minor.max(1);
    let ppt = layout.pixels_per_tick;
    if !(ppt.is_finite() && ppt > 0.0) {
        return Vec::new();
    }
    let first = (layout.scroll_x / ppt).floor().max(0.0) as i64;
    let last = (((layout.scroll_x + f64::from(layout.width)) / ppt).ceil())
        .min(layout.duration_ticks as f64) as i64;
    let max_iterations = (layout.width / 4.0).ceil().max(1.0) as usize + 2;
    let mut ticks = Vec::new();
    let mut tick = first.div_euclid(minor).saturating_mul(minor).max(0);
    while ticks.len() < max_iterations && tick <= last {
        ticks.push(tick);
        let Some(next) = tick.checked_add(minor) else {
            break;
        };
        tick = next;
    }
    ticks
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
    let mut end = project.duration().0.max(TICKS_PER_SECOND);
    for track in project.tracks() {
        for clip in track.clips().values() {
            end = end.max(clip.range.end.0);
        }
    }
    let seconds = end.saturating_add(TICKS_PER_SECOND - 1) / TICKS_PER_SECOND;
    seconds.saturating_mul(TICKS_PER_SECOND)
}

/// Map a timeline x (relative to the canvas left edge) to a tick, clamped to the
/// project range. Sub-tick pointer positions land on the nearest integer tick.
pub(crate) fn x_to_time(layout: &TimelineLayout, x: f32) -> Time {
    let ticks = (f64::from(x) + layout.scroll_x) / layout.pixels_per_tick;
    Time(ticks.clamp(0.0, layout.duration_ticks as f64) as i64)
}

/// Map a tick to a timeline x (relative to the canvas left edge).
pub(crate) fn time_to_x(layout: &TimelineLayout, time: Time) -> f32 {
    (time.0 as f64 * layout.pixels_per_tick - layout.scroll_x) as f32
}

/// Horizontal distance (in pixels) a zoomed timeline can scroll.
pub(crate) fn max_scroll(layout: &TimelineLayout) -> f32 {
    (layout.duration_ticks as f64 * layout.pixels_per_tick - f64::from(layout.width)).max(0.0)
        as f32
}

/// Snap threshold converted from pixels to ticks.
fn snap_threshold_ticks(layout: &TimelineLayout) -> i64 {
    (8.0 / layout.pixels_per_tick) as i64
}

/// Snap a candidate clip start to the playhead and to every other clip's edges.
/// The closest snap within the pixel threshold wins; ties keep the candidate.
pub(crate) fn snap_start(
    layout: &TimelineLayout,
    project: &Project,
    exclude: ClipId,
    playhead: Time,
    candidate: Time,
) -> Time {
    snap_start_among(layout, project, &[exclude], playhead, candidate)
}

/// [`snap_start`] for a dragged group: none of the group's own edges attract.
pub(crate) fn snap_start_among(
    layout: &TimelineLayout,
    project: &Project,
    exclude: &[ClipId],
    playhead: Time,
    candidate: Time,
) -> Time {
    snap_target_among(layout, project, exclude, playhead, candidate).unwrap_or(candidate)
}

/// The engaged snap target, including an exact hit on an edge.
pub(crate) fn snap_target(
    layout: &TimelineLayout,
    project: &Project,
    exclude: Option<ClipId>,
    playhead: Time,
    candidate: Time,
) -> Option<Time> {
    snap_target_among(layout, project, exclude.as_slice(), playhead, candidate)
}

/// [`snap_target`] ignoring every clip in `exclude`.
pub(crate) fn snap_target_among(
    layout: &TimelineLayout,
    project: &Project,
    exclude: &[ClipId],
    playhead: Time,
    candidate: Time,
) -> Option<Time> {
    let threshold = snap_threshold_ticks(layout);
    let mut best = None;
    let mut best_distance = i64::MAX;
    let mut consider = |edge: Time| {
        let distance = edge.0.abs_diff(candidate.0);
        if distance <= threshold as u64 && distance < best_distance as u64 {
            best = Some(edge);
            best_distance = i64::try_from(distance).expect("bounded by snap threshold");
        }
    };
    consider(playhead);
    for track in project.tracks() {
        for clip in track.clips().values() {
            if !exclude.contains(&clip.id) {
                consider(clip.range.start);
                consider(clip.range.end);
            }
        }
    }
    best
}

/// Position a newly dropped clip with the same snapping rules as a timeline edit.
pub(crate) fn drop_start(
    layout: &TimelineLayout,
    project: &Project,
    x: f32,
    playhead: Time,
    snap: bool,
) -> Time {
    let candidate = x_to_time(layout, x);
    if snap {
        snap_target(layout, project, None, playhead, candidate).unwrap_or(candidate)
    } else {
        candidate
    }
}

/// Resolve OS and internal drops using the same scroll, zoom, ruler and snap geometry.
pub(crate) fn drop_placement(
    layout: &TimelineLayout,
    project: &Project,
    x: f32,
    y: f32,
    playhead: Time,
    snap: bool,
) -> Option<(TrackId, Time)> {
    if y < layout.ruler_height || x < 0.0 || x > layout.width {
        return None;
    }
    let row = ((y - layout.ruler_height) / layout.row_height.max(1.0)) as usize;
    let track = project.tracks().get(row)?;
    Some((track.id(), drop_start(layout, project, x, playhead, snap)))
}

/// A trim handle at a clip's left or right edge.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrimEdge {
    /// Left (start) edge.
    Left,
    /// Right (end) edge.
    Right,
}

/// Return a trim handle under a canvas-relative point, preferring the left edge
/// on very narrow clips. The handle is roughly six pixels wide.
pub(crate) fn hit_trim_edge(
    layout: &TimelineLayout,
    project: &Project,
    x: f32,
    y: f32,
) -> Option<(ClipHit, TrimEdge)> {
    if y < layout.ruler_height {
        return None;
    }
    let row = ((y - layout.ruler_height) / layout.row_height.max(1.0)) as usize;
    let track = project.tracks().get(row)?;
    for clip in track.clips().values() {
        let start_x = time_to_x(layout, clip.range.start);
        let end_x = time_to_x(layout, clip.range.end);
        let margin = 6.0_f32.min((end_x - start_x).max(0.0) / 3.0);
        let hit = ClipHit {
            track: track.id(),
            clip: clip.id,
        };
        if (x - start_x).abs() <= margin {
            return Some((hit, TrimEdge::Left));
        }
        if (x - end_x).abs() <= margin {
            return Some((hit, TrimEdge::Right));
        }
    }
    None
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
        TimelineLayout::zoomed(
            1000.0,
            24.0,
            40.0,
            project_duration(&sample_project()),
            1.0,
            0.0,
        )
    }

    fn snap_tick(layout: &TimelineLayout) -> i64 {
        (8.0 / layout.pixels_per_tick) as i64
    }

    #[test]
    fn os_drop_placement_respects_rows_scroll_zoom_and_ruler() {
        let mut project = Project::new(animathio_core::FrameRate::new(30, 1).unwrap());
        for (index, id) in [TrackId(1), TrackId(2)].into_iter().enumerate() {
            animathio_core::Command::AddTrack {
                track: animathio_core::Track::new(id, "Track"),
                index,
            }
            .apply(&mut project)
            .unwrap();
        }
        let layout = TimelineLayout::zoomed(100., 28., 44., 1000, 2., 50.);
        assert_eq!(
            drop_placement(&layout, &project, 50., 73., Time(0), false),
            Some((TrackId(2), Time(500)))
        );
        assert_eq!(
            drop_placement(&layout, &project, 50., 27., Time(0), false),
            None
        );
        assert_eq!(
            drop_placement(&layout, &project, 50., 117., Time(0), false),
            None
        );
        assert_eq!(
            drop_placement(&layout, &project, 50., 73., Time(510), true),
            Some((TrackId(2), Time(510)))
        );
    }

    #[test]
    fn ruler_walks_only_the_visible_range_even_for_absurd_durations() {
        // Fully zoomed-out over a near-i64::MAX project: still bounded.
        let huge = TimelineLayout::zoomed(1200.0, 24.0, 40.0, i64::MAX / 2, 1.0, 0.0);
        let ticks = ruler_ticks(&huge, 1);
        assert!(ticks.len() <= 1200 / 4 + 2, "{}", ticks.len());
        // Degenerate steps and overflow-prone starts terminate too.
        assert!(ruler_ticks(&huge, 0).len() <= 1200 / 4 + 2);
        assert!(ruler_ticks(&huge, i64::MAX).len() <= 2);
        // A zoomed-in view starts at the scroll position, not at tick zero.
        let zoomed =
            TimelineLayout::zoomed(1000.0, 24.0, 40.0, TICKS_PER_SECOND * 600, 64.0, 5000.0);
        let step = TICKS_PER_SECOND;
        let ticks = ruler_ticks(&zoomed, step);
        assert!(ticks.first().copied().unwrap() > 0);
        assert!(ticks.windows(2).all(|pair| pair[1] - pair[0] == step));
        let first_x = time_to_x(&zoomed, Time(ticks[0]));
        assert!(first_x <= 1.0 && first_x > -(step as f64 * zoomed.pixels_per_tick) as f32 - 1.0);
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

    #[test]
    fn snap_start_prefers_the_nearest_playhead_or_other_clip_edge() {
        let mut project = sample_project();
        // Extend the single clip to make room and add a neighbour with a known edge.
        Command::TrimClip {
            id: ClipId(10),
            range: TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap(),
            source_offset: Time(0),
        }
        .apply(&mut project)
        .unwrap();
        Command::AddClip {
            track: TrackId(1),
            clip: Clip::new(
                ClipId(11),
                animathio_core::ClipKind::Builtin(Builtin::Shape {
                    shape: animathio_core::builtin::Shape::Rect,
                    fill: None,
                    stroke: None,
                }),
                TimeRange::new(Time(TICKS_PER_SECOND), Time(TICKS_PER_SECOND * 2)).unwrap(),
            ),
        }
        .apply(&mut project)
        .unwrap();
        let layout = TimelineLayout::zoomed(1000.0, 24.0, 40.0, TICKS_PER_SECOND * 3, 1.0, 0.0);
        let playhead = Time(TICKS_PER_SECOND * 3 / 2);
        // Close to the shared 1s edge, but nearer to it than the playhead.
        let near_edge = Time(TICKS_PER_SECOND - snap_tick(&layout) / 2);
        assert_eq!(
            snap_start(&layout, &project, ClipId(10), playhead, near_edge),
            Time(TICKS_PER_SECOND)
        );
        // Close to the playhead snaps there instead.
        let near_head = Time(playhead.0 - snap_tick(&layout) / 2);
        assert_eq!(
            snap_start(&layout, &project, ClipId(10), playhead, near_head),
            playhead
        );
        // Far from every edge keeps the candidate.
        let far = Time(TICKS_PER_SECOND / 2);
        assert_eq!(
            snap_start(&layout, &project, ClipId(10), playhead, far),
            far
        );
    }

    #[test]
    fn drop_start_snaps_to_playhead_and_clip_edges_or_keeps_candidate() {
        let mut project = sample_project();
        Command::TrimClip {
            id: ClipId(10),
            range: TimeRange::new(Time(0), Time(TICKS_PER_SECOND)).unwrap(),
            source_offset: Time(0),
        }
        .apply(&mut project)
        .unwrap();
        let layout = TimelineLayout::zoomed(1000.0, 24.0, 40.0, TICKS_PER_SECOND * 3, 1.0, 0.0);
        let playhead = Time(TICKS_PER_SECOND / 2);
        // A drop near the clip's end edge (1s) snaps to it.
        let edge_x = time_to_x(&layout, Time(TICKS_PER_SECOND - snap_tick(&layout) / 2));
        assert_eq!(
            drop_start(&layout, &project, edge_x, playhead, true),
            Time(TICKS_PER_SECOND)
        );
        // A drop near the playhead snaps there.
        let head_x = time_to_x(&layout, Time(playhead.0 - snap_tick(&layout) / 2));
        assert_eq!(
            drop_start(&layout, &project, head_x, playhead, true),
            playhead
        );
        // With snapping off the raw tick under the pointer is used.
        let raw_x = 250.0;
        assert_eq!(
            drop_start(&layout, &project, raw_x, playhead, false),
            x_to_time(&layout, raw_x)
        );
    }
}
