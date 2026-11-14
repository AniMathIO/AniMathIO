//! First-party `animations` plugin: entrance, exit and emphasis presets that the
//! host turns into ordinary keyframes (API 1.1 `animation-preset-plugin` world).
//!
//! Every preset is a pure function of the clip's rest values, the window length and
//! the options. Easings the core keyframes interpolate natively (linear and
//! ease-in-out) produce two keys; the others (ease-in, ease-out, back, bounce,
//! elastic) are sampled into linear segments, at most one per frame and never more
//! than [`MAX_SEGMENTS`].

wit_bindgen::generate!({
    path: "../../crates/plugin-api/wit",
    world: "animation-preset-plugin",
});

use std::f64::consts::TAU;

use animathio::plugin::types::{
    BoolParam, Diagnostic, EnumParam, NumberRange, ParamEntry, ParamKind, ParamSpec, ParamValue,
};
use exports::animathio::plugin::animation_presets::{
    ClipInfo, Curve, CurveInterpolation, CurveKey, Guest as Presets, PresetDescriptor, PresetRole,
};

/// Flicks per second.
const TICKS_PER_SECOND: f64 = 705_600_000.0;
/// Default entrance/exit window: half a second.
const HALF_SECOND: i64 = 352_800_000;
/// Emphasis presets default to the whole clip; the host clamps the window to it.
const WHOLE_CLIP: i64 = 705_600_000 * 3600;
/// Most linear segments one sampled easing curve may use.
const MAX_SEGMENTS: i64 = 48;
/// Most oscillations an emphasis preset emits (bounds the key count).
const MAX_CYCLES: i64 = 60;
/// Clip kind id of the first-party text plugin.
const TEXT: &str = "plugin:org.animathio.text";

struct Animations;

impl Guest for Animations {
    fn describe() -> Manifest {
        Manifest {
            name: "Animations".to_owned(),
            version: "1.0.0".to_owned(),
            description: "Entrance, exit and emphasis presets".to_owned(),
            kinds: vec!["animation-preset".into()],
            metadata: vec![],
            params: Vec::new(),
        }
    }
}

// --- option schema helpers ------------------------------------------------------

fn choice(id: &str, label: &str, options: &[&str], default: &str) -> ParamSpec {
    ParamSpec {
        id: id.into(),
        label: label.into(),
        kind: ParamKind::EnumChoice(EnumParam {
            options: options.iter().map(|o| (*o).to_owned()).collect(),
            default: default.into(),
        }),
    }
}

fn number(id: &str, label: &str, min: f64, max: f64, default: f64, step: f64) -> ParamSpec {
    ParamSpec {
        id: id.into(),
        label: label.into(),
        kind: ParamKind::Number(NumberRange {
            min,
            max,
            default,
            step,
        }),
    }
}

fn toggle(id: &str, label: &str, default: bool) -> ParamSpec {
    ParamSpec {
        id: id.into(),
        label: label.into(),
        kind: ParamKind::Bool(BoolParam { default }),
    }
}

const EASINGS: [&str; 7] = [
    "linear",
    "ease-in",
    "ease-out",
    "ease-in-out",
    "back",
    "bounce",
    "elastic",
];

fn easing(default: &str) -> ParamSpec {
    choice("easing", "Easing", &EASINGS, default)
}

fn direction(default: &str) -> ParamSpec {
    choice(
        "direction",
        "Direction",
        &["left", "right", "top", "bottom"],
        default,
    )
}

fn period() -> ParamSpec {
    number("period", "Period (s)", 0.1, 10.0, 1.0, 0.1)
}

fn preset(
    id: &str,
    name: &str,
    description: &str,
    role: PresetRole,
    applies_to: &[&str],
    params: Vec<ParamSpec>,
) -> PresetDescriptor {
    PresetDescriptor {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        role,
        applies_to: applies_to.iter().map(|k| (*k).to_owned()).collect(),
        default_duration_ticks: if role == PresetRole::Emphasis {
            WHOLE_CLIP
        } else {
            HALF_SECOND
        },
        params,
    }
}

fn catalog() -> Vec<PresetDescriptor> {
    use PresetRole::{Emphasis, Entrance, Exit};
    let mut presets = Vec::new();
    for (role, verb, suffix) in [(Entrance, "in", "In"), (Exit, "out", "Out")] {
        let into = if role == Entrance { "into" } else { "out of" };
        presets.extend([
            preset(
                &format!("fade-{verb}"),
                &format!("Fade {suffix}"),
                &format!(
                    "Fade opacity {}",
                    if role == Entrance {
                        "up from 0"
                    } else {
                        "down to 0"
                    }
                ),
                role,
                &[],
                vec![easing("ease-in-out")],
            ),
            preset(
                &format!("slide-{verb}"),
                &format!("Slide {suffix}"),
                &format!("Slide {into} view from one side"),
                role,
                &[],
                vec![
                    direction(if role == Entrance { "left" } else { "right" }),
                    choice("distance", "Distance", &["offscreen", "short"], "offscreen"),
                    toggle("fade", "Fade", false),
                    easing(if role == Entrance {
                        "ease-out"
                    } else {
                        "ease-in"
                    }),
                ],
            ),
            preset(
                &format!("zoom-{verb}"),
                &format!("Zoom {suffix}"),
                "Scale from (or to) a fraction of the size, fading",
                role,
                &[],
                vec![
                    number("scale", "Start scale", 0.0, 0.95, 0.0, 0.05),
                    toggle("fade", "Fade", true),
                    easing(if role == Entrance {
                        "ease-out"
                    } else {
                        "ease-in"
                    }),
                ],
            ),
            preset(
                &format!("pop-{verb}"),
                &format!("Pop {suffix}"),
                "Scale with an overshoot, like a pop",
                role,
                &[],
                vec![number("intensity", "Overshoot", 0.0, 4.0, 1.7, 0.1)],
            ),
            preset(
                &format!("spin-{verb}"),
                &format!("Spin {suffix}"),
                "Rotate while scaling",
                role,
                &[],
                vec![
                    number("turns", "Turns", 0.25, 8.0, 1.0, 0.25),
                    choice(
                        "rotation",
                        "Rotation",
                        &["clockwise", "counterclockwise"],
                        "clockwise",
                    ),
                    easing("ease-out"),
                ],
            ),
            preset(
                &format!("bounce-{verb}"),
                &format!("Bounce {suffix}"),
                &format!("Drop {into} view with a bounce"),
                role,
                &[],
                vec![choice("from", "From", &["top", "bottom"], "top")],
            ),
            preset(
                &format!("wipe-{verb}"),
                &format!("Wipe {suffix}"),
                "Unfold from one edge (a stretch reveal)",
                role,
                &[],
                vec![direction("left"), easing("ease-in-out")],
            ),
            preset(
                &format!("typewriter-{verb}"),
                &format!("Typewriter {suffix}"),
                &format!(
                    "{} the text one character at a time",
                    if role == Entrance { "Write" } else { "Erase" }
                ),
                role,
                &[TEXT],
                vec![choice(
                    "easing",
                    "Easing",
                    &["linear", "ease-in-out"],
                    "linear",
                )],
            ),
        ]);
    }
    presets.extend([
        preset(
            "breathe",
            "Breathe",
            "Gently pulse the size",
            Emphasis,
            &[],
            vec![
                number("intensity", "Intensity", 0.0, 1.0, 0.06, 0.01),
                period(),
            ],
        ),
        preset(
            "shake",
            "Shake",
            "Shake side to side",
            Emphasis,
            &[],
            vec![
                number("intensity", "Distance", 0.0, 0.25, 0.02, 0.005),
                number("period", "Period (s)", 0.05, 5.0, 0.2, 0.05),
            ],
        ),
        preset(
            "spin",
            "Spin",
            "Rotate full turns",
            Emphasis,
            &[],
            vec![
                number("turns", "Turns", 0.25, 32.0, 1.0, 0.25),
                choice(
                    "rotation",
                    "Rotation",
                    &["clockwise", "counterclockwise"],
                    "clockwise",
                ),
                easing("ease-in-out"),
            ],
        ),
        preset(
            "hop",
            "Hop",
            "Jump up and land repeatedly",
            Emphasis,
            &[],
            vec![
                number("intensity", "Height", 0.0, 0.5, 0.05, 0.01),
                period(),
            ],
        ),
        preset(
            "flash",
            "Flash",
            "Blink the opacity",
            Emphasis,
            &[],
            vec![number("intensity", "Depth", 0.0, 1.0, 1.0, 0.05), period()],
        ),
    ]);
    presets
}

// --- evaluation -----------------------------------------------------------------

fn invalid(message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        code: "invalid-input".into(),
        message: message.into(),
        metadata: vec![],
    }
}

struct Options<'a>(&'a [ParamEntry]);

impl Options<'_> {
    fn number(&self, id: &str, default: f64) -> f64 {
        self.0
            .iter()
            .find_map(|entry| match (&entry.value, entry.id == id) {
                (ParamValue::Number(n), true) if n.is_finite() => Some(*n),
                _ => None,
            })
            .unwrap_or(default)
    }
    fn choice<'s>(&'s self, id: &str, default: &'s str) -> &'s str {
        self.0
            .iter()
            .find_map(|entry| match (&entry.value, entry.id == id) {
                (ParamValue::EnumChoice(s), true) => Some(s.as_str()),
                _ => None,
            })
            .unwrap_or(default)
    }
    fn flag(&self, id: &str, default: bool) -> bool {
        self.0
            .iter()
            .find_map(|entry| match (&entry.value, entry.id == id) {
                (ParamValue::Bool(b), true) => Some(*b),
                _ => None,
            })
            .unwrap_or(default)
    }
}

struct Rest<'a>(&'a [(String, f64)]);

impl Rest<'_> {
    fn get(&self, target: &str, default: f64) -> f64 {
        self.0
            .iter()
            .find(|(key, _)| key == target)
            .map_or(default, |(_, value)| *value)
    }
}

/// Normalised easing curves on `t` in `[0, 1]`.
fn ease(name: &str, t: f64, overshoot: f64) -> f64 {
    match name {
        "ease-in" => t * t,
        "ease-out" => 1.0 - (1.0 - t) * (1.0 - t),
        "ease-in-out" => t * t * (3.0 - 2.0 * t),
        "back" => {
            let s = overshoot;
            let u = t - 1.0;
            1.0 + u * u * ((s + 1.0) * u + s)
        }
        "bounce" => bounce_out(t),
        "elastic" => {
            if t <= 0.0 || t >= 1.0 {
                t.clamp(0.0, 1.0)
            } else {
                1.0 + 2f64.powf(-10.0 * t) * ((t * 10.0 - 0.75) * TAU / 3.0).sin()
            }
        }
        _ => t,
    }
}

fn bounce_out(t: f64) -> f64 {
    const N: f64 = 7.5625;
    const D: f64 = 2.75;
    if t < 1.0 / D {
        N * t * t
    } else if t < 2.0 / D {
        let t = t - 1.5 / D;
        N * t * t + 0.75
    } else if t < 2.5 / D {
        let t = t - 2.25 / D;
        N * t * t + 0.9375
    } else {
        let t = t - 2.625 / D;
        N * t * t + 0.984_375
    }
}

/// Shared sampling context for one window.
struct Window {
    length: i64,
    frame: i64,
}

impl Window {
    /// `from → to` over `[start, end]` (fractions of the window) with `easing`.
    fn tween(
        &self,
        target: &str,
        from: f64,
        to: f64,
        easing: &str,
        overshoot: f64,
        span: (f64, f64),
    ) -> Curve {
        let start = (span.0 * self.length as f64).round() as i64;
        let end = ((span.1 * self.length as f64).round() as i64)
            .max(start + 1)
            .min(self.length);
        let start = start.min(end - 1).max(0);
        let keys = match easing {
            "linear" | "ease-in-out" => {
                let interpolation = if easing == "linear" {
                    CurveInterpolation::Linear
                } else {
                    CurveInterpolation::EaseInOut
                };
                vec![
                    key(start, from, interpolation),
                    key(end, to, CurveInterpolation::Linear),
                ]
            }
            _ => {
                let segments = ((end - start) / self.frame)
                    .clamp(2, MAX_SEGMENTS)
                    .min(end - start);
                (0..=segments)
                    .map(|i| {
                        let t = i as f64 / segments as f64;
                        let time = start + ((end - start) as f64 * t).round() as i64;
                        key(
                            time,
                            from + (to - from) * ease(easing, t, overshoot),
                            CurveInterpolation::Linear,
                        )
                    })
                    .collect()
            }
        };
        Curve {
            target: target.into(),
            keys,
        }
    }

    /// Exit presets run their entrance counterpart backwards: same keys, mirrored
    /// in time, so `out` ends where `in` starts.
    fn reverse(&self, mut curve: Curve) -> Curve {
        let mut keys: Vec<CurveKey> = curve
            .keys
            .iter()
            .rev()
            .map(|k| CurveKey {
                time_ticks: self.length - k.time_ticks,
                value: k.value,
                interpolation: CurveInterpolation::Linear,
            })
            .collect();
        // Interpolation belongs to the segment after a key: shift it to stay on the
        // same (mirrored) segment.
        for i in 0..keys.len().saturating_sub(1) {
            keys[i].interpolation = curve.keys[curve.keys.len() - 2 - i].interpolation;
        }
        curve.keys = keys;
        curve
    }

    /// Alternate between `rest` and `peak` every half period, easing in and out.
    fn oscillate(&self, target: &str, values: &[f64], period_ticks: i64) -> Curve {
        let steps = (self.length / (period_ticks / values.len() as i64).max(1))
            .clamp(values.len() as i64, MAX_CYCLES * values.len() as i64)
            .min(self.length);
        let keys = (0..=steps)
            .map(|i| {
                let time = (self.length as f64 * i as f64 / steps as f64).round() as i64;
                let value = if i == steps {
                    values[0]
                } else {
                    values[(i as usize) % values.len()]
                };
                key(time, value, CurveInterpolation::EaseInOut)
            })
            .collect();
        Curve {
            target: target.into(),
            keys,
        }
    }
}

fn key(time_ticks: i64, value: f64, interpolation: CurveInterpolation) -> CurveKey {
    CurveKey {
        time_ticks,
        value,
        interpolation,
    }
}

#[allow(clippy::too_many_lines)] // One arm per preset keeps the catalog readable.
fn generate(
    preset: &str,
    clip: &ClipInfo,
    window: i64,
    params: &[ParamEntry],
) -> Result<Vec<Curve>, Diagnostic> {
    let descriptor = catalog()
        .into_iter()
        .find(|d| d.id == preset)
        .ok_or_else(|| invalid(format!("unknown preset {preset:?}")))?;
    if !descriptor.applies_to.is_empty() && !descriptor.applies_to.contains(&clip.kind) {
        return Err(invalid(format!(
            "preset {preset} does not apply to {} clips",
            clip.kind
        )));
    }
    if window <= 0 || clip.frame_ticks <= 0 {
        return Err(invalid("window and frame length must be positive"));
    }
    let o = Options(params);
    let rest = Rest(&clip.rest);
    let w = Window {
        length: window,
        frame: clip.frame_ticks,
    };
    let exit = descriptor.role == PresetRole::Exit;
    let base = preset.trim_end_matches("-in").trim_end_matches("-out");
    let opacity = rest.get("opacity", 1.0);
    let (x, y) = (rest.get("x", 0.5), rest.get("y", 0.5));
    let (sx, sy) = (rest.get("scale-x", 1.0), rest.get("scale-y", 1.0));
    let rotation = rest.get("rotation", 0.0);
    let full = (0.0, 1.0);

    // Entrance curves; exits mirror them in time.
    let entrance = match base {
        "fade" => vec![w.tween(
            "opacity",
            0.0,
            opacity,
            o.choice("easing", "ease-in-out"),
            0.0,
            full,
        )],
        "slide" => {
            let side = o.choice("direction", if exit { "right" } else { "left" });
            let short = o.choice("distance", "offscreen") == "short";
            let easing = o.choice("easing", if exit { "ease-in" } else { "ease-out" });
            // Entrances come from `side`; exits leave toward `side`.
            let (target, from, to) = match side {
                "left" => ("x", if short { x - 0.15 } else { -sx.abs() / 2.0 }, x),
                "right" => (
                    "x",
                    if short {
                        x + 0.15
                    } else {
                        1.0 + sx.abs() / 2.0
                    },
                    x,
                ),
                "top" => ("y", if short { y - 0.15 } else { -sy.abs() / 2.0 }, y),
                _ => (
                    "y",
                    if short {
                        y + 0.15
                    } else {
                        1.0 + sy.abs() / 2.0
                    },
                    y,
                ),
            };
            let mut curves = vec![w.tween(target, from, to, mirrored(easing, exit), 0.0, full)];
            if o.flag("fade", false) {
                curves.push(w.tween("opacity", 0.0, opacity, mirrored(easing, exit), 0.0, full));
            }
            curves
        }
        "zoom" => {
            let factor = o.number("scale", 0.0);
            let easing = mirrored(
                o.choice("easing", if exit { "ease-in" } else { "ease-out" }),
                exit,
            );
            let mut curves = vec![
                w.tween("scale-x", sx * factor, sx, easing, 0.0, full),
                w.tween("scale-y", sy * factor, sy, easing, 0.0, full),
            ];
            if o.flag("fade", true) {
                curves.push(w.tween("opacity", 0.0, opacity, easing, 0.0, full));
            }
            curves
        }
        "pop" => {
            let overshoot = o.number("intensity", 1.7);
            vec![
                w.tween("scale-x", 0.0, sx, "back", overshoot, full),
                w.tween("scale-y", 0.0, sy, "back", overshoot, full),
                w.tween("opacity", 0.0, opacity, "linear", 0.0, (0.0, 0.3)),
            ]
        }
        "spin" => {
            let turns = o.number("turns", 1.0);
            let sign = if o.choice("rotation", "clockwise") == "clockwise" {
                1.0
            } else {
                -1.0
            };
            let easing = mirrored(o.choice("easing", "ease-out"), exit);
            // An entrance arrives turning in the chosen direction; an exit leaves so.
            let start = if exit {
                rotation + sign * turns * TAU
            } else {
                rotation - sign * turns * TAU
            };
            vec![
                w.tween("rotation", start, rotation, easing, 0.0, full),
                w.tween("scale-x", 0.0, sx, easing, 0.0, full),
                w.tween("scale-y", 0.0, sy, easing, 0.0, full),
            ]
        }
        "bounce" => {
            let from = if o.choice("from", "top") == "top" {
                -sy.abs() / 2.0
            } else {
                1.0 + sy.abs() / 2.0
            };
            vec![w.tween("y", from, y, "bounce", 0.0, full)]
        }
        "wipe" => {
            let side = o.choice("direction", "left");
            let easing = mirrored(o.choice("easing", "ease-in-out"), exit);
            // The edge opposite the motion stays put while the clip unfolds.
            let (scale_target, size, pos_target, pos, sign) = match side {
                "left" => ("scale-x", sx, "x", x, -1.0),
                "right" => ("scale-x", sx, "x", x, 1.0),
                "top" => ("scale-y", sy, "y", y, -1.0),
                _ => ("scale-y", sy, "y", y, 1.0),
            };
            let anchor = pos + sign * size / 2.0;
            vec![
                w.tween(scale_target, 0.0, size, easing, 0.0, full),
                w.tween(pos_target, anchor, pos, easing, 0.0, full),
            ]
        }
        "typewriter" => {
            let reveal = rest.get("param:reveal", 1.0);
            vec![w.tween(
                "param:reveal",
                0.0,
                reveal,
                o.choice("easing", "linear"),
                0.0,
                full,
            )]
        }
        _ => Vec::new(),
    };
    if descriptor.role != PresetRole::Emphasis {
        return Ok(if exit {
            entrance.into_iter().map(|curve| w.reverse(curve)).collect()
        } else {
            entrance
        });
    }

    let period = ((o.number("period", 1.0) * TICKS_PER_SECOND) as i64).max(2);
    let curves = match preset {
        "breathe" => {
            let k = 1.0 + o.number("intensity", 0.06);
            vec![
                w.oscillate("scale-x", &[sx, sx * k], period),
                w.oscillate("scale-y", &[sy, sy * k], period),
            ]
        }
        "shake" => {
            let a = o.number("intensity", 0.02);
            let period = ((o.number("period", 0.2) * TICKS_PER_SECOND) as i64).max(4);
            vec![w.oscillate("x", &[x, x + a, x, x - a], period)]
        }
        "spin" => {
            let turns = o.number("turns", 1.0);
            let sign = if o.choice("rotation", "clockwise") == "clockwise" {
                1.0
            } else {
                -1.0
            };
            vec![w.tween(
                "rotation",
                rotation,
                rotation + sign * turns * TAU,
                o.choice("easing", "ease-in-out"),
                0.0,
                full,
            )]
        }
        "hop" => {
            let h = o.number("intensity", 0.05);
            vec![w.oscillate("y", &[y, y - h], period)]
        }
        "flash" => {
            let depth = o.number("intensity", 1.0);
            vec![w.oscillate("opacity", &[opacity, opacity * (1.0 - depth)], period)]
        }
        _ => return Err(invalid(format!("unknown preset {preset:?}"))),
    };
    Ok(curves)
}

/// A reversed curve plays its easing backwards; swap in/out so an exit with
/// "ease-in" still starts slowly.
fn mirrored(easing: &str, exit: bool) -> &str {
    match (easing, exit) {
        ("ease-in", true) => "ease-out",
        ("ease-out", true) => "ease-in",
        (other, _) => other,
    }
}

impl Presets for Animations {
    fn list_presets() -> Vec<PresetDescriptor> {
        catalog()
    }

    fn generate(
        preset: String,
        clip: ClipInfo,
        window_ticks: i64,
        params: Vec<ParamEntry>,
    ) -> Result<Vec<Curve>, Diagnostic> {
        generate(&preset, &clip, window_ticks, &params)
    }
}

export!(Animations);
