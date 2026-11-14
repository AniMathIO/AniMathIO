//! Offline subject-aware reframing. Analysis is snapshot-only; application is one batch.
use crate::ControlError;
use animathio_core::{
    AssetId, ClipId, ClipKind, Command, History, Interpolation, Keyframe, ParamValue, Project,
    TICKS_PER_SECOND, Target, Time, TransformProperty,
};
use animathio_media::{FilmCraftBackend, Frame, MediaBackend};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicBool, Ordering},
};

const W: usize = 96;
const H: usize = 54;
const STEP: i64 = TICKS_PER_SECOND / 4;
const MAX_SAMPLES: usize = 20_000;
const MAX_KEYS: usize = 256;

/// Auto-reframe options shared by editor, CLI and MCP.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ReframeParams {
    /// Target aspect: `9:16`, `1:1` or `4:5`.
    pub aspect: String,
    /// Follow smoothness in 0..=1 (higher is slower).
    pub smoothness: f64,
    /// Empty selects every video clip; otherwise only these ids.
    pub clips: Vec<ClipId>,
}
impl Default for ReframeParams {
    fn default() -> Self {
        Self {
            aspect: "9:16".into(),
            smoothness: 0.5,
            clips: Vec::new(),
        }
    }
}

/// Snapshot-bound analysis result, ready for one undoable application.
pub struct ReframeDraft {
    expected: Project,
    command: Command,
    /// Number of video clips reframed.
    pub clips: usize,
    /// Number of position samples after simplification (two position keys per sample).
    pub points: usize,
}

fn invalid(message: &str) -> ControlError {
    ControlError::InvalidParams(message.into())
}
fn cancelled(cancel: &AtomicBool) -> Result<(), ControlError> {
    if cancel.load(Ordering::Relaxed) {
        Err(ControlError::Other("cancelled".into()))
    } else {
        Ok(())
    }
}
fn aspect(params: &ReframeParams) -> Result<(u32, u32), ControlError> {
    if !params.smoothness.is_finite() || !(0.0..=1.0).contains(&params.smoothness) {
        return Err(invalid("smoothness must be in 0..=1"));
    }
    match params.aspect.as_str() {
        "9:16" => Ok((1080, 1920)),
        "1:1" => Ok((1080, 1080)),
        "4:5" => Ok((1080, 1350)),
        _ => Err(invalid("aspect must be 9:16, 1:1 or 4:5")),
    }
}

// Analysis runs at 96x54, regardless of decoded source resolution. FilmCraft currently
// decodes a full frame first; no full-resolution analysis or retained frame sequence.
#[allow(clippy::cast_possible_truncation)]
fn thumbnail(frame: &Frame) -> Vec<f64> {
    let mut out = Vec::with_capacity(W * H);
    for y in 0..H {
        for x in 0..W {
            let sx = (x as u64 * u64::from(frame.width) / W as u64) as usize;
            let sy = (y as u64 * u64::from(frame.height) / H as u64) as usize;
            let i = (sy * frame.width as usize + sx) * 4;
            let p = &frame.pixels[i..i + 3];
            out.push(
                (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]))
                    / 255.,
            );
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
struct Point {
    time: Time,
    x: f64,
    y: f64,
    cut: bool,
}

struct Tracker {
    previous: Vec<f64>,
    centre: (f64, f64),
    difference: f64,
    last_time: Time,
}
impl Tracker {
    fn new() -> Self {
        Self {
            previous: Vec::new(),
            centre: (0.5, 0.5),
            difference: 0.0,
            last_time: Time(0),
        }
    }

    #[allow(clippy::cast_precision_loss)]
    fn sample(&mut self, luma: Vec<f64>, time: Time, crop: (f64, f64), smoothness: f64) -> Point {
        let first = self.previous.is_empty();
        let diff = if first {
            0.0
        } else {
            luma.iter()
                .zip(&self.previous)
                .map(|(a, b)| (a - b).abs())
                .sum::<f64>()
                / (W * H) as f64
        };
        let cut = !first && diff > 0.18 && diff > self.difference * 3.0;
        let mean = luma.iter().sum::<f64>() / (W * H) as f64;
        let (mut total, mut sx, mut sy) = (0., 0., 0.);
        for y in 1..H - 1 {
            for x in 1..W - 1 {
                let i = y * W + x;
                let edge = (luma[i] - luma[i - 1]).abs() + (luma[i] - luma[i - W]).abs();
                let contrast = (luma[i] - mean).abs();
                let motion = if first || cut {
                    0.
                } else {
                    (luma[i] - self.previous[i]).abs()
                };
                // Current-frame contrast/edges disambiguate the two motion lobes.
                let weight = contrast * contrast + edge * 0.4 + motion * (contrast + edge) * 2.;
                total += weight;
                sx += weight * (x as f64 + 0.5) / W as f64;
                sy += weight * (y as f64 + 0.5) / H as f64;
            }
        }
        let target = if total > 1e-6 {
            (sx / total, sy / total)
        } else {
            self.centre
        };
        // The final sample can be less than one regular step after its predecessor.
        // Scale both damping and the rate limit by actual integer-tick spacing.
        let elapsed = (time.0 - self.last_time.0).max(0) as f64 / STEP as f64;
        let follow = |current: f64, target: f64, size: f64| {
            let delta = target - current;
            let dead = size * 0.06;
            let movement = (delta.abs() - dead).max(0.) * delta.signum();
            let max_step = 0.25 * (1.0 - 0.8 * smoothness) * elapsed;
            let response = 1.0 - (0.85 * smoothness).powf(elapsed);
            (current + (movement * response).clamp(-max_step, max_step))
                .clamp(size / 2., 1. - size / 2.)
        };
        self.centre = if first || cut {
            (
                target.0.clamp(crop.0 / 2., 1. - crop.0 / 2.),
                target.1.clamp(crop.1 / 2., 1. - crop.1 / 2.),
            )
        } else {
            (
                follow(self.centre.0, target.0, crop.0),
                follow(self.centre.1, target.1, crop.1),
            )
        };
        self.previous = luma;
        self.last_time = time;
        self.difference = if cut {
            0.0
        } else {
            self.difference * 0.8 + diff * 0.2
        };
        Point {
            time,
            x: self.centre.0,
            y: self.centre.1,
            cut,
        }
    }
}

// Time-aware RDP, preserving both sides of every hard cut. Increase tolerance to
// enforce the key budget, never dropping a cut or silently truncating the clip.
#[allow(clippy::cast_precision_loss)]
fn simplify(points: &[Point]) -> Result<Vec<Point>, ControlError> {
    let mut anchors = BTreeSet::from([0, points.len() - 1]);
    for (i, p) in points.iter().enumerate().skip(1) {
        if p.cut {
            anchors.extend([i - 1, i]);
        }
    }
    if anchors.len() > MAX_KEYS {
        return Err(invalid(
            "too many scene changes; split the clip before reframing",
        ));
    }
    let mut tolerance = 0.002;
    loop {
        let mut keep = anchors.clone();
        let ids: Vec<_> = anchors.iter().copied().collect();
        let mut stack: Vec<_> = ids.windows(2).map(|w| (w[0], w[1])).collect();
        while let Some((a, b)) = stack.pop() {
            let mut worst = None;
            let mut error = tolerance;
            for i in a + 1..b {
                let t = (points[i].time.0 - points[a].time.0) as f64
                    / (points[b].time.0 - points[a].time.0) as f64;
                let x = points[a].x * (1. - t) + points[b].x * t;
                let y = points[a].y * (1. - t) + points[b].y * t;
                let e = (points[i].x - x).hypot(points[i].y - y);
                if e > error {
                    error = e;
                    worst = Some(i);
                }
            }
            if let Some(i) = worst {
                keep.insert(i);
                stack.extend([(a, i), (i, b)]);
            }
        }
        if keep.len() <= MAX_KEYS {
            return Ok(keep.into_iter().map(|i| points[i]).collect());
        }
        tolerance *= 2.;
    }
}

// Keep each node below core's command-batch size cap while retaining one root
// batch and one undo entry, even when replacing dense pre-existing animation.
fn bounded_batch(mut commands: Vec<Command>) -> Command {
    const CHUNK: usize = 512;
    while commands.len() > CHUNK {
        commands = commands
            .chunks(CHUNK)
            .map(|commands| Command::Batch {
                commands: commands.to_vec(),
            })
            .collect();
    }
    Command::Batch { commands }
}

/// Analyse at four samples per second without modifying the project. Progress is
/// 0..=100; cancellation is checked around each decode. Run on a worker in GUIs.
#[allow(clippy::too_many_lines, clippy::cast_precision_loss)]
pub fn analyse_reframe(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    params: &ReframeParams,
    cancel: &AtomicBool,
    progress: &dyn Fn(u32),
) -> Result<ReframeDraft, ControlError> {
    let (width, height) = aspect(params)?;
    let selected: BTreeSet<_> = params.clips.iter().copied().collect();
    let clips: Vec<_> = project
        .tracks()
        .iter()
        .flat_map(|t| t.clips().values().map(move |c| (t, c)))
        .filter(|(_, c)| {
            matches!(c.kind, ClipKind::Video(_))
                && (selected.is_empty() || selected.contains(&c.id))
        })
        .collect();
    if clips.is_empty() || (!selected.is_empty() && clips.len() != selected.len()) {
        return Err(invalid("select existing video clips"));
    }
    if clips.iter().any(|(t, _)| t.is_locked()) {
        return Err(invalid("cannot reframe locked tracks"));
    }
    let total = clips
        .iter()
        .try_fold(0usize, |sum, (_, c)| {
            let n = (c.range.end.0 - c.range.start.0 - 1) / STEP + 2;
            usize::try_from(n)
                .ok()
                .and_then(|n| sum.checked_add(n))
                .filter(|n| *n <= MAX_SAMPLES)
        })
        .ok_or_else(|| invalid("analysis exceeds 20000 samples; select fewer or shorter clips"))?;
    let mut commands = vec![Command::SetSettings {
        width,
        height,
        frame_rate: project.frame_rate(),
        background: project.background(),
        duration: project.duration(),
    }];
    let mut completed = 0usize;
    let mut count = 0;
    progress(0);
    for (_, clip) in &clips {
        cancelled(cancel)?;
        if clip.transform.rotation != 0.
            || clip
                .animations
                .iter()
                .any(|a| a.target == Target::Transform(TransformProperty::Rotation))
        {
            return Err(invalid("auto reframe requires unrotated video clips"));
        }
        let ClipKind::Video(asset) = &clip.kind else {
            return Err(invalid("not a video clip"));
        };
        let bytes = assets
            .get(asset)
            .ok_or_else(|| invalid("video asset is missing"))?;
        if AssetId::of(bytes) != *asset {
            return Err(invalid("video asset hash mismatch"));
        }
        let decoder = FilmCraftBackend::new()
            .open_video_bytes("media", bytes.clone().into())
            .map_err(|e| ControlError::Other(e.to_string()))?;
        let info = decoder.info();
        let source_aspect = f64::from(info.width) / f64::from(info.height);
        let canvas_aspect = f64::from(width) / f64::from(height);
        let scale_x = (source_aspect / canvas_aspect).max(1.);
        let scale_y = (canvas_aspect / source_aspect).max(1.);
        let crop = (1. / scale_x, 1. / scale_y);
        let duration = clip.range.end.0 - clip.range.start.0;
        let mut times: Vec<_> = (0..duration)
            .step_by(usize::try_from(STEP).map_err(|_| invalid("unsupported sample step"))?)
            .collect();
        if times.last() != Some(&(duration - 1)) {
            times.push(duration - 1);
        }
        let mut tracker = Tracker::new();
        let mut points = Vec::new();
        for t in times {
            cancelled(cancel)?;
            let source = clip
                .source_offset
                .0
                .checked_add(t)
                .ok_or_else(|| invalid("source time overflow"))?;
            let frame = decoder
                .frame_at(Time(source))
                .map_err(|e| ControlError::Other(e.to_string()))?;
            cancelled(cancel)?;
            let point = tracker.sample(thumbnail(&frame), Time(t), crop, params.smoothness);
            if point.cut
                && let Some(previous) = points.last().copied()
            {
                let Point { x, y, .. } = previous;
                if t > 0 && points.last().is_none_or(|p: &Point| p.time.0 < t - 1) {
                    points.push(Point {
                        time: Time(t - 1),
                        x,
                        y,
                        cut: false,
                    });
                }
            }
            points.push(point);
            completed += 1;
            progress(u32::try_from(completed * 100 / total).unwrap_or(100));
        }
        let points = simplify(&points)?;
        count += points.len();
        for animation in &clip.animations {
            if matches!(
                animation.target,
                Target::Transform(
                    TransformProperty::X
                        | TransformProperty::Y
                        | TransformProperty::ScaleX
                        | TransformProperty::ScaleY
                )
            ) {
                for time in animation.keys.keys().keys() {
                    commands.push(Command::RemoveKeyframe {
                        id: clip.id,
                        target: animation.target.clone(),
                        time: *time,
                    });
                }
            }
        }
        let mut transform = clip.transform;
        transform.scale_x = scale_x;
        transform.scale_y = scale_y;
        transform.x = 0.5 + (0.5 - points[0].x) * scale_x;
        transform.y = 0.5 + (0.5 - points[0].y) * scale_y;
        commands.push(Command::SetTransform {
            id: clip.id,
            transform,
        });
        // Explicit constant scale curves keep the generated fill scale editable in
        // the same keyframe UI as the tracked position curves.
        for (property, value) in [
            (TransformProperty::ScaleX, scale_x),
            (TransformProperty::ScaleY, scale_y),
        ] {
            commands.push(Command::AddKeyframe {
                id: clip.id,
                target: Target::Transform(property),
                time: Time(0),
                keyframe: Keyframe {
                    value: ParamValue::Number(value),
                    interpolation: Interpolation::Linear,
                },
            });
        }
        for (i, p) in points.iter().enumerate() {
            let interpolation = if points.get(i + 1).is_some_and(|n| n.cut) {
                Interpolation::Hold
            } else {
                Interpolation::Linear
            };
            for (property, value) in [
                (TransformProperty::X, 0.5 + (0.5 - p.x) * scale_x),
                (TransformProperty::Y, 0.5 + (0.5 - p.y) * scale_y),
            ] {
                commands.push(Command::AddKeyframe {
                    id: clip.id,
                    target: Target::Transform(property),
                    time: p.time,
                    keyframe: Keyframe {
                        value: ParamValue::Number(value),
                        interpolation,
                    },
                });
            }
        }
    }
    cancelled(cancel)?;
    progress(100);
    Ok(ReframeDraft {
        expected: project.clone(),
        command: bounded_batch(commands),
        clips: clips.len(),
        points: count,
    })
}

/// Reject stale drafts before returning the single undoable batch.
pub fn reframe_command(project: &Project, draft: &ReframeDraft) -> Result<Command, ControlError> {
    if project != &draft.expected {
        return Err(invalid("project changed during analysis; reframe again"));
    }
    Ok(draft.command.clone())
}
/// Analyse and apply synchronously for headless clients. Returns clip count.
pub fn auto_reframe(
    project: &mut Project,
    history: &mut History,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    params: &ReframeParams,
) -> Result<usize, ControlError> {
    let draft = analyse_reframe(project, assets, params, &AtomicBool::new(false), &|_| {})?;
    history.execute(project, &reframe_command(project, &draft)?)?;
    Ok(draft.clips)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn square(x: usize) -> Vec<f64> {
        let mut frame = vec![0.; W * H];
        for y in 20..34 {
            for xx in x..x + 10 {
                frame[y * W + xx] = 1.;
            }
        }
        frame
    }
    #[test]
    fn follows_moving_square_and_static_frame_is_still() {
        let mut tracker = Tracker::new();
        let mut path = Vec::new();
        for i in 0..25 {
            let point = tracker.sample(
                square(10 + i * 2),
                Time(i64::try_from(i).unwrap() * STEP),
                (0.32, 1.),
                0.3,
            );
            let subject = f64::from(u32::try_from(15 + i * 2).unwrap()) / 96.;
            assert!((point.x - subject).abs() + 5. / 96. <= 0.16);
            path.push(point);
        }
        assert!(path.last().unwrap().x - path[0].x > 0.3);
        let mut tracker = Tracker::new();
        let a = tracker.sample(square(60), Time(0), (0.32, 1.), 0.8);
        for i in 1..20 {
            let b = tracker.sample(square(60), Time(i * STEP), (0.32, 1.), 0.8);
            assert!((a.x - b.x).abs() < 1e-12);
        }
    }
    #[test]
    fn follow_rate_respects_short_final_sample_spacing() {
        let mut tracker = Tracker::new();
        let a = tracker.sample(square(5), Time(0), (0.32, 1.), 0.0);
        let b = tracker.sample(square(70), Time(STEP / 1000), (0.32, 1.), 0.0);
        assert!(!b.cut);
        assert!((b.x - a.x).abs() <= 0.00025 + 1e-12);
    }

    #[test]
    fn simplification_enforces_budget_and_keeps_endpoints() {
        let points: Vec<_> = (0..2000)
            .map(|i| Point {
                time: Time(i * STEP),
                x: if i % 2 == 0 { 0.2 } else { 0.8 },
                y: 0.5,
                cut: false,
            })
            .collect();
        let path = simplify(&points).unwrap();
        assert!(path.len() <= MAX_KEYS);
        assert_eq!(path.first().unwrap().time, points.first().unwrap().time);
        assert_eq!(path.last().unwrap().time, points.last().unwrap().time);
    }

    #[test]
    fn scene_change_jumps_and_simplification_preserves_cuts() {
        let mut tracker = Tracker::new();
        tracker.sample(square(5), Time(0), (0.32, 1.), 1.);
        let mut frame = vec![0.7; W * H];
        for y in 15..40 {
            for x in 65..85 {
                frame[y * W + x] = 0.;
            }
        }
        let b = tracker.sample(frame, Time(STEP), (0.32, 1.), 1.);
        assert!(b.cut);
        assert!(b.x > 0.6);
        let points = vec![
            Point {
                time: Time(0),
                x: 0.2,
                y: 0.5,
                cut: false,
            },
            Point {
                time: Time(STEP - 1),
                x: 0.2,
                y: 0.5,
                cut: false,
            },
            b,
        ];
        assert_eq!(simplify(&points).unwrap().len(), 3);
    }
}
