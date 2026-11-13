//! The pre-plugin builtin effect implementations (scalar `f64`, `powf` per channel), kept
//! verbatim as the reference the `effects-basic` plugin must reproduce.
#![allow(clippy::all, clippy::pedantic)]

pub fn apply(effect: &str, amount: f64, pixels: &mut [u8], width: usize, height: usize) {
    match effect {
        "invert" => {
            for chunk in pixels.chunks_exact_mut(4) {
                for c in &mut chunk[..3] {
                    let v = f64::from(*c);
                    *c = (v + (255.0 - 2.0 * v) * amount).round() as u8;
                }
            }
        }
        "opacity" => {
            for chunk in pixels.chunks_exact_mut(4) {
                chunk[3] = (f64::from(chunk[3]) * amount).round() as u8;
            }
        }
        "blur" => blur(pixels, width, height, amount),
        _ => color(effect, amount, pixels),
    }
}

fn color(effect: &str, amount: f64, pixels: &mut [u8]) {
    for chunk in pixels.chunks_exact_mut(4) {
        let r = srgb_to_linear(chunk[0]);
        let g = srgb_to_linear(chunk[1]);
        let b = srgb_to_linear(chunk[2]);
        let (r, g, b) = match effect {
            "grayscale" => {
                let l = luminance(r, g, b);
                (lerp(r, l, amount), lerp(g, l, amount), lerp(b, l, amount))
            }
            "sepia" => {
                let (sr, sg, sb) = (
                    0.393 * r + 0.769 * g + 0.189 * b,
                    0.349 * r + 0.686 * g + 0.168 * b,
                    0.272 * r + 0.534 * g + 0.131 * b,
                );
                (
                    lerp(r, sr, amount),
                    lerp(g, sg, amount),
                    lerp(b, sb, amount),
                )
            }
            "saturate" => {
                let l = luminance(r, g, b);
                (
                    l + (r - l) * amount,
                    l + (g - l) * amount,
                    l + (b - l) * amount,
                )
            }
            "brightness" => (r * amount, g * amount, b * amount),
            "contrast" => (
                (r - 0.5) * amount + 0.5,
                (g - 0.5) * amount + 0.5,
                (b - 0.5) * amount + 0.5,
            ),
            other => panic!("no reference for {other}"),
        };
        chunk[0] = linear_to_srgb(r);
        chunk[1] = linear_to_srgb(g);
        chunk[2] = linear_to_srgb(b);
    }
}

fn blur(pixels: &mut [u8], width: usize, height: usize, radius_fraction: f64) {
    let radius_px = (radius_fraction * height as f64).round();
    if !radius_px.is_finite() || radius_px <= 0.0 {
        return;
    }
    let radius = (radius_px as usize).min(width.max(height));
    let mut planes = vec![vec![0.0_f64; width * height]; 4];
    for (index, chunk) in pixels.chunks_exact(4).enumerate() {
        let alpha = f64::from(chunk[3]) / 255.0;
        planes[0][index] = srgb_to_linear(chunk[0]) * alpha;
        planes[1][index] = srgb_to_linear(chunk[1]) * alpha;
        planes[2][index] = srgb_to_linear(chunk[2]) * alpha;
        planes[3][index] = alpha;
    }
    let mut scratch = vec![0.0_f64; width * height];
    for plane in &mut planes {
        for y in 0..height {
            box_blur_1d(
                width,
                radius,
                |x| plane[y * width + x],
                |x, v| scratch[y * width + x] = v,
            );
        }
        for x in 0..width {
            box_blur_1d(
                height,
                radius,
                |y| scratch[y * width + x],
                |y, v| plane[y * width + x] = v,
            );
        }
    }
    for (index, chunk) in pixels.chunks_exact_mut(4).enumerate() {
        let alpha = planes[3][index];
        let (r, g, b) = if alpha > 1e-6 {
            (
                planes[0][index] / alpha,
                planes[1][index] / alpha,
                planes[2][index] / alpha,
            )
        } else {
            (0.0, 0.0, 0.0)
        };
        chunk[0] = linear_to_srgb(r);
        chunk[1] = linear_to_srgb(g);
        chunk[2] = linear_to_srgb(b);
        chunk[3] = (alpha * 255.0).round() as u8;
    }
}

fn luminance(r: f64, g: f64, b: f64) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn lerp(from: f64, to: f64, amount: f64) -> f64 {
    from + (to - from) * amount
}

fn srgb_to_linear(value: u8) -> f64 {
    let c = f64::from(value) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f64) -> u8 {
    let c = value.clamp(0.0, 1.0);
    let c = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

fn box_blur_1d(
    n: usize,
    radius: usize,
    get: impl Fn(usize) -> f64,
    mut set: impl FnMut(usize, f64),
) {
    let radius = radius.min(n);
    let window = (radius * 2 + 1) as f64;
    let radius_f = radius as f64;
    let mut sum = get(0) * radius_f;
    for k in 0..=radius {
        sum += get(k.min(n - 1));
    }
    set(0, sum / window);
    for i in 1..n {
        let add = (i + radius).min(n - 1);
        let remove = i.saturating_sub(radius + 1);
        sum += get(add);
        sum -= get(remove);
        set(i, sum / window);
    }
}
