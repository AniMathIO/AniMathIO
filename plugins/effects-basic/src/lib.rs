//! First-party `effects-basic` plugin: eight deterministic pixel filters.
//!
//! # Light choice
//!
//! Color filters (`grayscale`, `sepia`, `saturate`, `brightness`, `contrast`) run in
//! **linear light**: each pixel's sRGB channels are decoded to linear, the filter is
//! applied there and the result is encoded back to sRGB. `invert` works on the
//! gamma-encoded channels (like CSS `invert()`), which makes `invert∘invert` exactly
//! the identity in 8-bit storage. `blur` operates on linear-light, premultiplied
//! pixels so transparent regions do not bleed into their neighbors. `opacity`
//! multiplies straight alpha.
//!
//! The sRGB decode is a 256-entry table and the encode is a threshold search, both
//! derived from the reference formulas, so results match the formulas without paying
//! for `powf` on every channel.

wit_bindgen::generate!({
    path: "../../crates/plugin-api/wit",
    world: "effect-plugin",
});

use animathio::plugin::types::{
    Diagnostic, NumberRange, ParamEntry, ParamKind, ParamSpec, ParamValue, Size,
};
use exports::animathio::plugin::frame_effects::{EffectDescriptor, Guest as FrameEffects};

struct BasicEffects;

impl Guest for BasicEffects {
    fn describe() -> Manifest {
        Manifest {
            name: "Basic Effects".to_owned(),
            version: "1.0.0".to_owned(),
            description: "Color and blur filters".to_owned(),
            kinds: vec!["effect".into()],
            metadata: vec![],
            params: Vec::new(),
        }
    }
}

/// How a filter validates and names its single parameter.
#[derive(Clone, Copy)]
struct Spec {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    param: &'static str,
    label: &'static str,
    max: f64,
    step: f64,
    default: f64,
    /// `true` when the parameter is limited to `[0, 1]`; otherwise only `>= 0`.
    unit: bool,
}

const SPECS: [Spec; 8] = [
    Spec {
        id: "grayscale",
        name: "Grayscale",
        description: "Desaturate toward luminance",
        param: "amount",
        label: "Amount",
        max: 1.0,
        step: 0.01,
        default: 1.0,
        unit: true,
    },
    Spec {
        id: "sepia",
        name: "Sepia",
        description: "Warm sepia tint",
        param: "amount",
        label: "Amount",
        max: 1.0,
        step: 0.01,
        default: 1.0,
        unit: true,
    },
    Spec {
        id: "invert",
        name: "Invert",
        description: "Invert colors",
        param: "amount",
        label: "Amount",
        max: 1.0,
        step: 0.01,
        default: 1.0,
        unit: true,
    },
    Spec {
        id: "saturate",
        name: "Saturate",
        description: "Saturation multiplier (1 is unchanged)",
        param: "amount",
        label: "Amount",
        max: 4.0,
        step: 0.01,
        default: 1.0,
        unit: false,
    },
    Spec {
        id: "brightness",
        name: "Brightness",
        description: "Linear-light brightness multiplier (1 is unchanged)",
        param: "amount",
        label: "Amount",
        max: 4.0,
        step: 0.01,
        default: 1.0,
        unit: false,
    },
    Spec {
        id: "contrast",
        name: "Contrast",
        description: "Contrast slope around mid-gray (1 is unchanged)",
        param: "amount",
        label: "Amount",
        max: 4.0,
        step: 0.01,
        default: 1.0,
        unit: false,
    },
    Spec {
        id: "blur",
        name: "Blur",
        description: "Box blur; radius is a fraction of the canvas height",
        param: "radius",
        label: "Radius",
        max: 0.25,
        step: 0.001,
        default: 0.0,
        unit: false,
    },
    Spec {
        id: "opacity",
        name: "Opacity",
        description: "Multiply straight alpha",
        param: "amount",
        label: "Amount",
        max: 1.0,
        step: 0.01,
        default: 1.0,
        unit: true,
    },
];

impl FrameEffects for BasicEffects {
    fn list_effects() -> Vec<EffectDescriptor> {
        SPECS
            .iter()
            .map(|spec| EffectDescriptor {
                id: spec.id.to_owned(),
                name: spec.name.to_owned(),
                description: spec.description.to_owned(),
                params: vec![ParamSpec {
                    id: spec.param.to_owned(),
                    label: spec.label.to_owned(),
                    kind: ParamKind::Number(NumberRange {
                        min: 0.0,
                        max: spec.max,
                        default: spec.default,
                        step: spec.step,
                    }),
                }],
            })
            .collect()
    }

    fn apply_frame(
        effect: String,
        mut pixels: Vec<u8>,
        size: Size,
        params: Vec<ParamEntry>,
        _time_ticks: i64,
    ) -> Result<Vec<u8>, Diagnostic> {
        (|| -> Result<Vec<u8>, String> {
            let width = size.width as usize;
            let height = size.height as usize;
            if width == 0 || height == 0 || pixels.len() != width * height * 4 {
                return Err("frame size does not match the pixel buffer".to_owned());
            }
            let spec = SPECS
                .iter()
                .find(|spec| spec.id == effect)
                .ok_or_else(|| format!("unknown effect {effect:?}"))?;
            let value = number(&params, spec.param).unwrap_or(spec.default);
            if !value.is_finite() || value < 0.0 || (spec.unit && value > 1.0) {
                return Err(if spec.unit {
                    format!("effect {} must be in [0, 1]", spec.param)
                } else {
                    format!("effect {} must be nonnegative", spec.param)
                });
            }
            match spec.id {
                "grayscale" => color(&mut pixels, |r, g, b| {
                    let l = luminance(r, g, b);
                    (lerp(r, l, value), lerp(g, l, value), lerp(b, l, value))
                }),
                "sepia" => color(&mut pixels, |r, g, b| {
                    let sr = 0.393 * r + 0.769 * g + 0.189 * b;
                    let sg = 0.349 * r + 0.686 * g + 0.168 * b;
                    let sb = 0.272 * r + 0.534 * g + 0.131 * b;
                    (lerp(r, sr, value), lerp(g, sg, value), lerp(b, sb, value))
                }),
                "saturate" => color(&mut pixels, |r, g, b| {
                    let l = luminance(r, g, b);
                    (
                        l + (r - l) * value,
                        l + (g - l) * value,
                        l + (b - l) * value,
                    )
                }),
                "brightness" => color(&mut pixels, |r, g, b| (r * value, g * value, b * value)),
                "contrast" => color(&mut pixels, |r, g, b| {
                    (
                        (r - 0.5) * value + 0.5,
                        (g - 0.5) * value + 0.5,
                        (b - 0.5) * value + 0.5,
                    )
                }),
                "invert" => invert(&mut pixels, value),
                "opacity" => opacity(&mut pixels, value),
                "blur" => blur(&mut pixels, width, height, value),
                _ => unreachable!("SPECS and the match cover the same ids"),
            }
            Ok(pixels)
        })()
        .map_err(|message| Diagnostic {
            code: "plugin-error".into(),
            message,
            metadata: vec![],
        })
    }
}

export!(BasicEffects);

fn number(params: &[ParamEntry], id: &str) -> Option<f64> {
    params
        .iter()
        .find(|entry| entry.id == id)
        .and_then(|entry| match entry.value {
            ParamValue::Number(n) => Some(n),
            _ => None,
        })
}

fn luminance(r: f64, g: f64, b: f64) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn lerp(from: f64, to: f64, amount: f64) -> f64 {
    from + (to - from) * amount
}

/// sRGB transfer function, the reference definition both tables are built from.
fn srgb_to_linear_exact(value: u8) -> f64 {
    let c = f64::from(value) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb_exact(value: f64) -> u8 {
    let c = value.clamp(0.0, 1.0);
    let c = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

fn decode_table() -> [f64; 256] {
    let mut table = [0.0; 256];
    for (value, slot) in table.iter_mut().enumerate() {
        *slot = srgb_to_linear_exact(value as u8);
    }
    table
}

/// Linear -> sRGB byte without `powf`: `thresholds[k]` is the smallest linear value
/// that encodes to a code above `k`, and `coarse` seeds the search per 1/4096 bucket.
struct Encoder {
    thresholds: [f64; 255],
    coarse: Vec<u8>,
}

const BUCKETS: f64 = 4096.0;

impl Encoder {
    fn new() -> Self {
        let mut thresholds = [0.0; 255];
        for (k, slot) in thresholds.iter_mut().enumerate() {
            let c = (k as f64 + 0.5) / 255.0;
            *slot = if c <= 0.040_45 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            };
        }
        let coarse = (0..=BUCKETS as usize)
            .map(|bucket| linear_to_srgb_exact(bucket as f64 / BUCKETS))
            .collect();
        Self { thresholds, coarse }
    }

    #[inline]
    fn encode(&self, value: f64) -> u8 {
        let v = value.clamp(0.0, 1.0);
        let mut code = self.coarse[(v * BUCKETS) as usize];
        while code < 255 && v >= self.thresholds[usize::from(code)] {
            code += 1;
        }
        code
    }
}

fn color(pixels: &mut [u8], filter: impl Fn(f64, f64, f64) -> (f64, f64, f64)) {
    let decode = decode_table();
    let encoder = Encoder::new();
    for chunk in pixels.chunks_exact_mut(4) {
        let (r, g, b) = filter(
            decode[usize::from(chunk[0])],
            decode[usize::from(chunk[1])],
            decode[usize::from(chunk[2])],
        );
        chunk[0] = encoder.encode(r);
        chunk[1] = encoder.encode(g);
        chunk[2] = encoder.encode(b);
    }
}

fn invert(pixels: &mut [u8], amount: f64) {
    let mut table = [0u8; 256];
    for (value, slot) in table.iter_mut().enumerate() {
        let v = value as f64;
        *slot = (v + (255.0 - 2.0 * v) * amount).round() as u8;
    }
    for chunk in pixels.chunks_exact_mut(4) {
        chunk[0] = table[usize::from(chunk[0])];
        chunk[1] = table[usize::from(chunk[1])];
        chunk[2] = table[usize::from(chunk[2])];
    }
}

fn opacity(pixels: &mut [u8], amount: f64) {
    let mut table = [0u8; 256];
    for (value, slot) in table.iter_mut().enumerate() {
        *slot = (value as f64 * amount).round() as u8;
    }
    for chunk in pixels.chunks_exact_mut(4) {
        chunk[3] = table[usize::from(chunk[3])];
    }
}

/// Separable box blur over linear-light premultiplied planes with edge replication.
///
/// Planes are `f32` (the window sums accumulate in `f64`), which keeps a 4K frame well
/// inside the sandbox memory budget.
fn blur(pixels: &mut [u8], width: usize, height: usize, radius_fraction: f64) {
    let radius_px = (radius_fraction * height as f64).round();
    if !radius_px.is_finite() || radius_px <= 0.0 {
        return;
    }
    let radius = (radius_px as usize).min(width.max(height));
    let count = width * height;
    let decode = decode_table();

    let mut planes: [Vec<f32>; 4] = std::array::from_fn(|_| vec![0.0; count]);
    for (index, chunk) in pixels.chunks_exact(4).enumerate() {
        let alpha = f64::from(chunk[3]) / 255.0;
        planes[0][index] = (decode[usize::from(chunk[0])] * alpha) as f32;
        planes[1][index] = (decode[usize::from(chunk[1])] * alpha) as f32;
        planes[2][index] = (decode[usize::from(chunk[2])] * alpha) as f32;
        planes[3][index] = alpha as f32;
    }

    let mut scratch = vec![0.0_f32; count];
    let mut row = vec![0.0_f32; width];
    let mut sums = vec![0.0_f64; width];
    for plane in &mut planes {
        for line in plane.chunks_exact_mut(width) {
            blur_row(line, &mut row, radius);
            line.copy_from_slice(&row);
        }
        blur_columns(plane, &mut scratch, &mut sums, width, height, radius);
        std::mem::swap(plane, &mut scratch);
    }

    let encoder = Encoder::new();
    for (index, chunk) in pixels.chunks_exact_mut(4).enumerate() {
        let alpha = f64::from(planes[3][index]);
        let (r, g, b) = if alpha > 1e-6 {
            (
                f64::from(planes[0][index]) / alpha,
                f64::from(planes[1][index]) / alpha,
                f64::from(planes[2][index]) / alpha,
            )
        } else {
            (0.0, 0.0, 0.0)
        };
        chunk[0] = encoder.encode(r);
        chunk[1] = encoder.encode(g);
        chunk[2] = encoder.encode(b);
        chunk[3] = (alpha * 255.0).round() as u8;
    }
}

/// Sliding-window box blur of one line with edge replication, `O(n)` for any radius.
fn blur_row(src: &[f32], dst: &mut [f32], radius: usize) {
    let n = src.len();
    let radius = radius.min(n);
    let window = (radius * 2 + 1) as f64;
    let mut sum = f64::from(src[0]) * radius as f64;
    for k in 0..=radius {
        sum += f64::from(src[k.min(n - 1)]);
    }
    dst[0] = (sum / window) as f32;
    for i in 1..n {
        let add = (i + radius).min(n - 1);
        let remove = i.saturating_sub(radius + 1);
        sum += f64::from(src[add]);
        sum -= f64::from(src[remove]);
        dst[i] = (sum / window) as f32;
    }
}

/// The same window sweep down every column at once, walking rows so memory access
/// stays sequential.
fn blur_columns(
    src: &[f32],
    dst: &mut [f32],
    sums: &mut [f64],
    width: usize,
    height: usize,
    radius: usize,
) {
    let n = height;
    let radius = radius.min(n);
    let window = (radius * 2 + 1) as f64;
    let at = |y: usize| &src[y * width..(y + 1) * width];
    for (x, sum) in sums.iter_mut().enumerate() {
        *sum = f64::from(at(0)[x]) * radius as f64;
    }
    for k in 0..=radius {
        for (sum, value) in sums.iter_mut().zip(at(k.min(n - 1))) {
            *sum += f64::from(*value);
        }
    }
    for (out, sum) in dst[..width].iter_mut().zip(sums.iter()) {
        *out = (*sum / window) as f32;
    }
    for i in 1..n {
        let add = at((i + radius).min(n - 1));
        let remove = at(i.saturating_sub(radius + 1));
        let out = &mut dst[i * width..(i + 1) * width];
        for x in 0..width {
            sums[x] += f64::from(add[x]);
            sums[x] -= f64::from(remove[x]);
            out[x] = (sums[x] / window) as f32;
        }
    }
}
