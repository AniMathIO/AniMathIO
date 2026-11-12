//! Track-level transitions on the edit point between two adjacent clips.
//!
//! A transition is stored on the track that owns both clips and names the outgoing
//! (`from`) and incoming (`to`) clip. Clips never overlap in the model: the transition
//! window is centered on the cut, and evaluation extends both clips into each other
//! (the outgoing clip plays past its end, the incoming one starts early). Edits that
//! would break adjacency (move, trim, split, remove) fail validation until the
//! transition is removed, so undo/redo stays exact.
use crate::{Clip, ClipId, Error, Time, TimeRange, Track, invalid};
use serde::{Deserialize, Serialize};

/// Stable transition identity, allocated by the command caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TransitionId(pub u64);

/// Direction a wipe edge travels across the canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WipeDirection {
    /// The incoming clip is revealed from the left edge.
    #[default]
    LeftToRight,
    /// The incoming clip is revealed from the right edge.
    RightToLeft,
    /// The incoming clip is revealed from the top edge.
    TopToBottom,
    /// The incoming clip is revealed from the bottom edge.
    BottomToTop,
}

/// Visual (and for a dissolve, audio) blend between the outgoing and incoming clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionKind {
    /// Linear opacity blend; audio crossfades with equal-power curves.
    CrossDissolve,
    /// Fade the outgoing clip to black, then the incoming clip from black.
    DipToBlack,
    /// Fade the outgoing clip to white, then the incoming clip from white.
    DipToWhite,
    /// Hard-edged reveal of the incoming clip.
    Wipe {
        /// Edge travel direction.
        direction: WipeDirection,
    },
}

/// A transition between two adjacent clips on one track.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transition {
    /// Stable identity, unique across the project.
    pub id: TransitionId,
    /// Blend kind.
    pub kind: TransitionKind,
    /// Total window length in ticks, centered on the cut.
    pub duration: Time,
    /// Outgoing clip; its end is the edit point.
    pub from: ClipId,
    /// Incoming clip; its start is the edit point.
    pub to: ClipId,
}
impl Transition {
    /// Ticks the window extends before the cut (into the outgoing clip).
    #[must_use]
    pub const fn before(&self) -> i64 {
        self.duration.0 / 2
    }
    /// Ticks the window extends after the cut (into the incoming clip).
    #[must_use]
    pub const fn after(&self) -> i64 {
        self.duration.0 - self.duration.0 / 2
    }
    /// Timeline window `[cut - before, cut + after)` for an edit point.
    #[must_use]
    pub fn window(&self, cut: Time) -> TimeRange {
        TimeRange {
            start: Time(cut.0 - self.before()),
            end: Time(cut.0 + self.after()),
        }
    }
    /// Linear progress in `[0, 1)` at `time` inside `window`.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // Progress is a blend weight, not a position.
    pub fn progress(&self, window: TimeRange, time: Time) -> f64 {
        ((time.0 - window.start.0) as f64 / self.duration.0 as f64).clamp(0.0, 1.0)
    }
}

/// Which side of a transition a clip plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionRole {
    /// The clip before the cut.
    Outgoing,
    /// The clip after the cut.
    Incoming,
}

/// Evaluated transition state attached to a scene clip.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SceneTransition {
    /// Transition identity; both participating clips carry the same id.
    pub id: TransitionId,
    /// Blend kind.
    pub kind: TransitionKind,
    /// Linear progress in `[0, 1)`.
    pub progress: f64,
    /// Side played by this clip.
    pub role: TransitionRole,
}

/// Equal-power crossfade gains `(outgoing, incoming)` at linear progress `p`.
#[must_use]
pub fn equal_power(progress: f64) -> (f64, f64) {
    let angle = progress.clamp(0.0, 1.0) * std::f64::consts::FRAC_PI_2;
    (angle.cos(), angle.sin())
}

/// Where a clip plays, including transition extensions.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipSpan<'a> {
    /// Timeline range including transition handles.
    pub range: TimeRange,
    /// Transition at the clip's start, if any.
    pub incoming: Option<&'a Transition>,
    /// Transition at the clip's end, if any.
    pub outgoing: Option<&'a Transition>,
}
impl ClipSpan<'_> {
    /// Transition state at `time` for this clip, if a transition window covers it.
    #[must_use]
    pub fn transition_at(&self, clip: &Clip, time: Time) -> Option<SceneTransition> {
        let check = |transition: &Transition, cut: Time, role| {
            let window = transition.window(cut);
            (time >= window.start && time < window.end).then(|| SceneTransition {
                id: transition.id,
                kind: transition.kind,
                progress: transition.progress(window, time),
                role,
            })
        };
        self.incoming
            .and_then(|t| check(t, clip.range.start, TransitionRole::Incoming))
            .or_else(|| {
                self.outgoing
                    .and_then(|t| check(t, clip.range.end, TransitionRole::Outgoing))
            })
    }
}

impl Track {
    /// Transitions on this track's edit points.
    #[must_use]
    pub fn transitions(&self) -> &[Transition] {
        &self.transitions
    }
    /// The playing span of a clip on this track, extended by its transitions.
    #[must_use]
    pub fn clip_span<'a>(&'a self, clip: &Clip) -> ClipSpan<'a> {
        let incoming = self.transitions.iter().find(|t| t.to == clip.id);
        let outgoing = self.transitions.iter().find(|t| t.from == clip.id);
        let start = incoming.map_or(clip.range.start, |t| Time(clip.range.start.0 - t.before()));
        let end = outgoing.map_or(clip.range.end, |t| Time(clip.range.end.0 + t.after()));
        ClipSpan {
            range: TimeRange { start, end },
            incoming,
            outgoing,
        }
    }
    pub(crate) fn validate_transitions(&self) -> Result<(), Error> {
        let mut ids = std::collections::BTreeSet::new();
        for transition in &self.transitions {
            if !ids.insert(transition.id) {
                return Err(invalid("duplicate transition id"));
            }
            if transition.duration.0 <= 0 {
                return Err(invalid("transition duration must be positive"));
            }
            let (Some(from), Some(to)) = (
                self.clips.get(&transition.from),
                self.clips.get(&transition.to),
            ) else {
                return Err(invalid("transition clips must be on its track"));
            };
            if from.range.end != to.range.start {
                return Err(invalid("transition clips must be adjacent"));
            }
            if self
                .transitions
                .iter()
                .filter(|t| t.from == transition.from || t.to == transition.to)
                .count()
                != 1
            {
                return Err(invalid("edit point already has a transition"));
            }
        }
        // Each clip must cover the halves of the transitions on both of its edges.
        for clip in self.clips.values() {
            let span = self.clip_span(clip);
            let used = span.incoming.map_or(0, Transition::after)
                + span.outgoing.map_or(0, Transition::before);
            if used > clip.range.end.0 - clip.range.start.0 {
                return Err(invalid("clip too short for its transitions"));
            }
        }
        Ok(())
    }
}
