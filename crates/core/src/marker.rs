//! Project-level timeline markers: named, coloured points or ranges on the ruler.

use crate::{Error, Time, invalid};
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
