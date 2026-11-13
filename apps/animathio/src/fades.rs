//! Fade in/out handles. A fade is nothing but a pair of keyframes on the clip's
//! level (`gain` for audio, opacity for everything else), so it stays visible and
//! editable in the keyframe UI and renders/mixes with no extra engine support.

use animathio_core::{Clip, ClipId, ClipKind, Command, Interpolation, Keyframe, Target, Time};
use animathio_core::{ParamValue, TransformProperty};

/// Which end of the clip a fade belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FadeEdge {
    In,
    Out,
}

/// The property a fade animates for this clip.
pub(crate) fn fade_target(clip: &Clip) -> Target {
    if matches!(clip.kind, ClipKind::Audio(_)) {
        Target::Param("gain".to_owned())
    } else {
        Target::Transform(TransformProperty::Opacity)
    }
}

/// The un-faded level, restored at the inner end of a fade.
fn base_level(clip: &Clip) -> f64 {
    match fade_target(clip) {
        Target::Param(name) => clip
            .params
            .get(&name)
            .and_then(ParamValue::as_number)
            .unwrap_or(1.0),
        _ => clip.transform.opacity,
    }
}

fn clip_len(clip: &Clip) -> i64 {
    clip.range.end.0 - clip.range.start.0
}

/// Length in ticks of the fade currently on `clip`'s edge, or `0` when none.
/// A fade is a keyframe at the very edge with level 0 and a following (or
/// preceding) keyframe marking where the full level is reached.
pub(crate) fn fade_len(clip: &Clip, edge: FadeEdge) -> i64 {
    let target = fade_target(clip);
    let Some(animation) = clip.animations.iter().find(|a| a.target == target) else {
        return 0;
    };
    let keys = animation.keys.keys();
    let is_zero = |key: &Keyframe<ParamValue>| key.value.as_number().is_some_and(|v| v <= 1e-9);
    match edge {
        FadeEdge::In => {
            let mut iter = keys.iter();
            match (iter.next(), iter.next()) {
                (Some((first, key)), Some((next, _))) if first.0 == 0 && is_zero(key) => next.0,
                _ => 0,
            }
        }
        FadeEdge::Out => {
            let mut iter = keys.iter().rev();
            let len = clip_len(clip);
            match (iter.next(), iter.next()) {
                (Some((last, key)), Some((prev, _))) if last.0 == len && is_zero(key) => {
                    len - prev.0
                }
                _ => 0,
            }
        }
    }
}

/// Sample the same interpolated level used by rendering/mixing, from the edge inward.
pub(crate) fn levels(clip: &Clip, edge: FadeEdge, length: i64) -> Vec<f32> {
    (0..=32)
        .map(|i| {
            let distance = length * i / 32;
            let local = if edge == FadeEdge::In {
                distance
            } else {
                clip_len(clip) - distance
            };
            clip.animations
                .iter()
                .find(|a| a.target == fade_target(clip))
                .and_then(|a| a.keys.sample(Time(local)))
                .and_then(|v| v.as_number())
                .unwrap_or(i as f64 / 32.) as f32
        })
        .collect()
}

/// The largest fade allowed on `edge` without running into the opposite fade.
pub(crate) fn max_fade(clip: &Clip, edge: FadeEdge) -> i64 {
    let other = match edge {
        FadeEdge::In => FadeEdge::Out,
        FadeEdge::Out => FadeEdge::In,
    };
    (clip_len(clip) - fade_len(clip, other)).max(0)
}

/// Build the single undoable command that sets `edge`'s fade to `ticks`
/// (`0` removes it). Returns `None` when nothing would change.
pub(crate) fn fade_command(clip: &Clip, edge: FadeEdge, ticks: i64) -> Option<Command> {
    let ticks = ticks.clamp(0, max_fade(clip, edge));
    let old = fade_len(clip, edge);
    if ticks == old {
        return None;
    }
    let len = clip_len(clip);
    let target = fade_target(clip);
    let local = |distance: i64| match edge {
        FadeEdge::In => Time(distance),
        FadeEdge::Out => Time(len - distance),
    };
    let id: ClipId = clip.id;
    let key = |distance: i64, value: f64| Command::AddKeyframe {
        id,
        target: target.clone(),
        time: local(distance),
        keyframe: Keyframe {
            value: value.into(),
            interpolation: Interpolation::Linear,
        },
    };
    let remove = |distance: i64| Command::RemoveKeyframe {
        id,
        target: target.clone(),
        time: local(distance),
    };
    let base = base_level(clip);
    let mut commands = Vec::new();
    // Core only animates parameters that exist, so materialise `gain` first.
    if let Target::Param(name) = &target
        && !clip.params.contains_key(name)
        && ticks > 0
    {
        commands.push(Command::SetParam {
            id,
            name: name.clone(),
            value: Some(base),
        });
    }
    if old > 0 {
        // Drop the previous ramp; the edge key is rewritten (or removed) below.
        commands.push(remove(old));
    }
    if ticks == 0 {
        commands.push(remove(0));
    } else {
        // The edge key is replaced in place, so a plain add suffices.
        commands.push(key(0, 0.0));
        commands.push(key(ticks, base));
    }
    Some(Command::Batch { commands })
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{
        AssetId, History, Project, TICKS_PER_SECOND, TimeRange, Track, TrackId, evaluate,
    };

    fn project_with(kind: ClipKind) -> (Project, Clip) {
        let mut project = Project::default();
        Command::AddTrack {
            track: Track::new(TrackId(1), "A"),
            index: 0,
        }
        .apply(&mut project)
        .unwrap();
        if let ClipKind::Audio(id) | ClipKind::Video(id) | ClipKind::Image(id) = &kind {
            Command::AddAsset {
                id: id.clone(),
                asset: animathio_core::Asset {
                    name: "a.wav".into(),
                    media_type: "audio/wav".into(),
                },
            }
            .apply(&mut project)
            .unwrap();
        }
        let clip = Clip::new(
            ClipId(1),
            kind,
            TimeRange::new(Time(0), Time(4 * TICKS_PER_SECOND)).unwrap(),
        );
        Command::AddClip {
            track: TrackId(1),
            clip: clip.clone(),
        }
        .apply(&mut project)
        .unwrap();
        (project, clip)
    }

    fn current(project: &Project) -> Clip {
        project.tracks()[0].clips()[&ClipId(1)].clone()
    }

    #[test]
    fn drawn_fade_uses_keyframe_interpolation() {
        for (interpolation, expected) in [
            (Interpolation::Linear, 0.25),
            (Interpolation::EaseInOut, 0.15625),
            (Interpolation::Hold, 0.),
        ] {
            let (mut project, _) = project_with(ClipKind::Audio(AssetId::of(b"a")));
            fade_command(&current(&project), FadeEdge::In, TICKS_PER_SECOND)
                .unwrap()
                .apply(&mut project)
                .unwrap();
            Command::AddKeyframe {
                id: ClipId(1),
                target: Target::Param("gain".into()),
                time: Time(0),
                keyframe: Keyframe {
                    value: ParamValue::Number(0.),
                    interpolation,
                },
            }
            .apply(&mut project)
            .unwrap();
            let sampled = levels(&current(&project), FadeEdge::In, TICKS_PER_SECOND);
            assert!((sampled[8] - expected).abs() < 1e-6);
        }
    }

    #[test]
    fn audio_fades_are_gain_keyframes_in_one_undo_step() {
        let (mut project, _) = project_with(ClipKind::Audio(AssetId::of(b"a")));
        let mut history = History::default();
        let second = TICKS_PER_SECOND;

        let command = fade_command(&current(&project), FadeEdge::In, second).unwrap();
        history.execute(&mut project, &command).unwrap();
        let clip = current(&project);
        assert_eq!(fade_target(&clip), Target::Param("gain".into()));
        assert_eq!(fade_len(&clip, FadeEdge::In), second);
        assert_eq!(fade_len(&clip, FadeEdge::Out), 0);
        let keys = clip.animations[0].keys.keys();
        assert_eq!(keys[&Time(0)].value.as_number(), Some(0.0));
        assert_eq!(keys[&Time(second)].value.as_number(), Some(1.0));

        // Resizing replaces the old ramp instead of stacking keyframes.
        let command = fade_command(&current(&project), FadeEdge::In, 2 * second).unwrap();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(current(&project).animations[0].keys.keys().len(), 2);
        assert_eq!(fade_len(&current(&project), FadeEdge::In), 2 * second);

        let command = fade_command(&current(&project), FadeEdge::Out, second).unwrap();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(fade_len(&current(&project), FadeEdge::Out), second);
        assert_eq!(current(&project).animations[0].keys.keys().len(), 4);

        // The fade-out cannot grow into the fade-in.
        assert_eq!(max_fade(&current(&project), FadeEdge::Out), 2 * second);
        let command = fade_command(&current(&project), FadeEdge::Out, 3 * second).unwrap();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(fade_len(&current(&project), FadeEdge::Out), 2 * second);

        // One undo reverts one handle drag.
        assert!(history.undo(&mut project).unwrap());
        assert_eq!(fade_len(&current(&project), FadeEdge::Out), second);

        // Zero removes a fade and drops the empty animation.
        let command = fade_command(&current(&project), FadeEdge::Out, 0).unwrap();
        history.execute(&mut project, &command).unwrap();
        let command = fade_command(&current(&project), FadeEdge::In, 0).unwrap();
        history.execute(&mut project, &command).unwrap();
        assert_eq!(current(&project).animations.len(), 0);
        assert!(fade_command(&current(&project), FadeEdge::In, 0).is_none());
    }

    #[test]
    fn visual_fades_animate_opacity_and_evaluate_to_the_ramp() {
        let (mut project, _) = project_with(ClipKind::Error("stand-in".into()));
        let command = fade_command(&current(&project), FadeEdge::In, TICKS_PER_SECOND).unwrap();
        command.apply(&mut project).unwrap();
        assert_eq!(
            fade_target(&current(&project)),
            Target::Transform(TransformProperty::Opacity)
        );
        let opacity = |time: i64| evaluate(&project, Time(time)).clips[0].transform.opacity;
        assert!(opacity(0).abs() < 1e-9);
        assert!((opacity(TICKS_PER_SECOND / 2) - 0.5).abs() < 1e-6);
        assert!((opacity(2 * TICKS_PER_SECOND) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn locked_tracks_reject_fades() {
        let (mut project, _) = project_with(ClipKind::Audio(AssetId::of(b"a")));
        Command::SetTrackLocked {
            id: TrackId(1),
            locked: true,
        }
        .apply(&mut project)
        .unwrap();
        let command = fade_command(&current(&project), FadeEdge::In, 1).unwrap();
        assert!(command.apply(&mut project).is_err());
    }
}
