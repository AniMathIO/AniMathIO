//! Project-level timeline markers: named, coloured points or ranges on the ruler.

use crate::{Command, Error, Project, Time, TimeRange, TrackId, invalid};
use serde::{Deserialize, Serialize};

/// Stable marker identity, allocated by the command caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MarkerId(pub u64);

/// Longest accepted marker name, in characters.
pub const MAX_MARKER_NAME: usize = 200;
/// Longest accepted marker note, in characters.
pub const MAX_MARKER_NOTE: usize = 4000;
/// Most markers a project may hold.
pub const MAX_MARKERS: usize = 100_000;

/// Small fixed marker palette, so projects stay portable across themes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerColor {
    /// Red.
    Red,
    /// Orange.
    Orange,
    /// Yellow.
    Yellow,
    /// Green.
    Green,
    /// Cyan.
    Cyan,
    /// Blue (the default).
    #[default]
    Blue,
    /// Purple.
    Purple,
    /// Pink.
    Pink,
}
impl MarkerColor {
    /// Every palette entry, in display order.
    pub const ALL: [Self; 8] = [
        Self::Red,
        Self::Orange,
        Self::Yellow,
        Self::Green,
        Self::Cyan,
        Self::Blue,
        Self::Purple,
        Self::Pink,
    ];
    /// Lowercase identifier used by the CLI, control protocol and MCP.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Red => "red",
            Self::Orange => "orange",
            Self::Yellow => "yellow",
            Self::Green => "green",
            Self::Cyan => "cyan",
            Self::Blue => "blue",
            Self::Purple => "purple",
            Self::Pink => "pink",
        }
    }
    /// Parse a lowercase palette name.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.name().eq_ignore_ascii_case(name.trim()))
    }
    /// Opaque sRGB colour of the palette entry.
    #[must_use]
    pub const fn rgb(self) -> [u8; 3] {
        match self {
            Self::Red => [0xE5, 0x48, 0x4D],
            Self::Orange => [0xF7, 0x8B, 0x2D],
            Self::Yellow => [0xF5, 0xC5, 0x18],
            Self::Green => [0x3F, 0xB9, 0x50],
            Self::Cyan => [0x2B, 0xC4, 0xD6],
            Self::Blue => [0x4A, 0x90, 0xE2],
            Self::Purple => [0x9B, 0x6B, 0xE0],
            Self::Pink => [0xE0, 0x5C, 0xA8],
        }
    }
}

/// A named position (or span) on the timeline ruler. Markers never affect rendering.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    /// Stable identity, unique across the project.
    pub id: MarkerId,
    /// Position in timeline ticks (start of the span for a range marker).
    pub time: Time,
    /// Length of a range marker; `None` for a point marker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration: Option<Time>,
    /// Display name; may be empty.
    #[serde(default)]
    pub name: String,
    /// Palette colour.
    #[serde(default)]
    pub color: MarkerColor,
    /// Free-form note; may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}
impl Marker {
    /// A point marker with an empty name and note.
    #[must_use]
    pub fn new(id: MarkerId, time: Time) -> Self {
        Self {
            id,
            time,
            duration: None,
            name: String::new(),
            color: MarkerColor::default(),
            note: String::new(),
        }
    }
    /// Exclusive end of the span, or the point itself.
    #[must_use]
    pub fn end(&self) -> Time {
        Time(self.time.0.saturating_add(self.duration.map_or(0, |d| d.0)))
    }
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.time.0 < 0 {
            return Err(invalid("negative marker time"));
        }
        if self
            .duration
            .is_some_and(|d| d.0 <= 0 || self.time.0.checked_add(d.0).is_none())
        {
            return Err(invalid("invalid marker duration"));
        }
        if self.name.chars().count() > MAX_MARKER_NAME
            || self.note.chars().count() > MAX_MARKER_NOTE
        {
            return Err(invalid("marker name or note too long"));
        }
        Ok(())
    }
}

/// Whether markers follow a ripple edit (`RemoveRange`, `RemoveRangeOnTracks`, `RippleDelete`).
///
/// A marker at or after the end of the removed range moves left by the range length; a
/// marker inside the range collapses onto the cut point (it is kept, never deleted); a range
/// marker spanning the cut shrinks by the removed length and one whose end falls inside the
/// range is clipped at the cut. The marker updates are part of the same command, so one undo
/// restores clips and markers exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkerRipple {
    /// Markers ride along when the ripple leaves no content after the cut on a track it did
    /// not shift, i.e. when it effectively affects every track (the default).
    #[default]
    Auto,
    /// Always move markers.
    Ride,
    /// Never move markers.
    Stay,
}

impl MarkerRipple {
    /// Whether markers should move for a ripple that starts at `from` and slides the tracks in
    /// `affected` (`None` = every track). `Auto` rides when no track outside `affected` has
    /// content ending after `from`.
    #[must_use]
    pub fn rides(self, project: &Project, from: Time, affected: Option<&[TrackId]>) -> bool {
        match self {
            Self::Ride => true,
            Self::Stay => false,
            Self::Auto => affected.is_none_or(|ids| {
                project
                    .tracks()
                    .iter()
                    .filter(|t| !ids.contains(&t.id()))
                    .all(|t| t.clips().values().all(|c| c.range.end.0 <= from.0))
            }),
        }
    }
}

/// `UpdateMarker` commands moving every marker through ripple-removed `gaps` (sorted,
/// non-overlapping). A time `x` maps to `x` minus the part of each gap before it, so times
/// inside a gap collapse onto its start. Range markers map both ends; one that collapses
/// becomes a point marker. Empty when no marker changes.
#[must_use]
pub fn ripple_marker_commands(project: &Project, gaps: &[TimeRange]) -> Vec<Command> {
    let map = |x: i64| {
        x - gaps
            .iter()
            .map(|g| (x - g.start.0).clamp(0, g.end.0 - g.start.0))
            .sum::<i64>()
    };
    let mut commands = Vec::new();
    for marker in project.markers() {
        let time = map(marker.time.0);
        let end = map(marker.end().0);
        let duration = marker
            .duration
            .and_then(|_| (end > time).then_some(Time(end - time)));
        if time != marker.time.0 || duration != marker.duration {
            commands.push(Command::UpdateMarker {
                id: marker.id,
                marker: Marker {
                    time: Time(time),
                    duration,
                    ..marker.clone()
                },
            });
        }
    }
    commands
}

/// `UpdateMarker` commands for a ripple that opened `len` ticks of space at `at`: markers at
/// or after `at` move later by `len`, and a range marker spanning `at` grows by `len`.
#[must_use]
pub fn ripple_marker_insert_commands(project: &Project, at: Time, len: i64) -> Vec<Command> {
    let map = |x: i64| if x >= at.0 { x.saturating_add(len) } else { x };
    let mut commands = Vec::new();
    for marker in project.markers() {
        let time = map(marker.time.0);
        let end = map(marker.end().0);
        let duration = marker.duration.map(|_| Time(end - time));
        if time != marker.time.0 || duration != marker.duration {
            commands.push(Command::UpdateMarker {
                id: marker.id,
                marker: Marker {
                    time: Time(time),
                    duration,
                    ..marker.clone()
                },
            });
        }
    }
    commands
}
