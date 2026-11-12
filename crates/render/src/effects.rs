//! Deterministic CPU implementation of the builtin clip effects.
//!
//! # Light choice
//!
//! Color filters (`Grayscale`, `Sepia`, `Saturate`, `Brightness`, `Contrast`)
//! run in **linear light**: each pixel's sRGB channels are decoded to linear,
//! the filter is applied there, and the result is encoded back to sRGB. This
//! keeps brightness/contrast/saturation physically consistent with how the rest
//! of the pipeline treats color, and is independent of the host SIMD feature
//! set. `Invert` is the exception: it works on gamma-encoded sRGB channels (like
//! CSS `invert()`), which makes `invert∘invert` exactly the identity even in
//! 8-bit storage — the linear round-trip would crush near-black/near-white
//! values. `Blur` operates on linear-light, premultiplied pixels so transparent
//! regions do not bleed into neighbors.
//!
//! Everything is scalar and single-threaded on purpose: frames are small in the
//! prototype and determinism beats throughput here.

use crate::{Error, Frame, check_size};
use animathio_core::{EffectInstance, EffectKind};

/// An effect that transforms a straight-alpha sRGB RGBA8 frame in place.
pub trait EffectFilter {
    /// Apply the effect to every pixel of the frame.
    fn apply(&self, frame: &mut Frame) -> Result<(), Error>;
}

/// Apply an ordered effect chain to a frame, leaving alpha untouched except for
/// the explicit `Opacity` effect.
pub fn apply_chain(frame: &mut Frame, effects: &[EffectInstance]) -> Result<(), Error> {
    frame.validate()?;
    for instance in effects {
        filter(instance)?.apply(frame)?;
    }
    Ok(())
}

fn filter(instance: &EffectInstance) -> Result<Box<dyn EffectFilter>, Error> {
    match instance.kind {
        EffectKind::Grayscale => Ok(Box::new(ColorFilter {
            op: ColorOp::Grayscale,
            amount: unit_param(instance)?,
        })),
        EffectKind::Sepia => Ok(Box::new(ColorFilter {
            op: ColorOp::Sepia,
            amount: unit_param(instance)?,
        })),
        EffectKind::Invert => Ok(Box::new(ColorFilter {
            op: ColorOp::Invert,
            amount: unit_param(instance)?,
        })),
        EffectKind::Saturate => Ok(Box::new(ColorFilter {
            op: ColorOp::Saturate,
            amount: nonneg_param(instance, "amount")?,
        })),
        EffectKind::Brightness => Ok(Box::new(ColorFilter {
            op: ColorOp::Brightness,
            amount: nonneg_param(instance, "amount")?,
        })),
        EffectKind::Contrast => Ok(Box::new(ColorFilter {
            op: ColorOp::Contrast,
            amount: nonneg_param(instance, "amount")?,
        })),
        EffectKind::Blur => Ok(Box::new(Blur {
            radius: nonneg_param(instance, "radius")?,
        })),
        EffectKind::Opacity => Ok(Box::new(Opacity {
            amount: unit_param(instance)?,
        })),
    }
}

fn param(instance: &EffectInstance, name: &str) -> Result<f64, Error> {
    instance
        .params
        .get(name)
        .copied()
        .ok_or_else(|| Error::Effect(format!("effect is missing parameter {name:?}")))
}

fn unit_param(instance: &EffectInstance) -> Result<f64, Error> {
    let value = param(instance, "amount")?;
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(Error::Effect("effect amount must be in [0, 1]".to_owned()));
    }
    Ok(value)
}

fn nonneg_param(instance: &EffectInstance, name: &str) -> Result<f64, Error> {
    let value = param(instance, name)?;
    if !value.is_finite() || value < 0.0 {
        return Err(Error::Effect(format!("effect {name} must be nonnegative")));
    }
    Ok(value)
}

#[derive(Clone, Copy)]
enum ColorOp {
    Grayscale,
    Sepia,
    Invert,
    Saturate,
    Brightness,
    Contrast,
}

struct ColorFilter {
    op: ColorOp,
    amount: f64,
}

impl EffectFilter for ColorFilter {
    fn apply(&self, frame: &mut Frame) -> Result<(), Error> {
        if matches!(self.op, ColorOp::Invert) {
            for chunk in frame.pixels.as_chunks_mut::<4>().0 {
                chunk[0] = invert_channel(chunk[0], self.amount);
                chunk[1] = invert_channel(chunk[1], self.amount);
                chunk[2] = invert_channel(chunk[2], self.amount);
            }
            return Ok(());
        }
        for chunk in frame.pixels.as_chunks_mut::<4>().0 {
            let r = srgb_to_linear(chunk[0]);
            let g = srgb_to_linear(chunk[1]);
            let b = srgb_to_linear(chunk[2]);
            let (r, g, b) = match self.op {
                ColorOp::Grayscale => {
                    let l = luminance(r, g, b);
                    (
                        lerp(r, l, self.amount),
                        lerp(g, l, self.amount),
                        lerp(b, l, self.amount),
                    )
                }
                ColorOp::Sepia => {
                    let (sr, sg, sb) = sepia(r, g, b);
                    (
                        lerp(r, sr, self.amount),
                        lerp(g, sg, self.amount),
                        lerp(b, sb, self.amount),
                    )
                }
                ColorOp::Saturate => {
                    let l = luminance(r, g, b);
                    (
                        l + (r - l) * self.amount,
                        l + (g - l) * self.amount,
                        l + (b - l) * self.amount,
                    )
                }
                ColorOp::Brightness => (r * self.amount, g * self.amount, b * self.amount),
                ColorOp::Contrast => (
                    (r - 0.5) * self.amount + 0.5,
                    (g - 0.5) * self.amount + 0.5,
                    (b - 0.5) * self.amount + 0.5,
                ),
                ColorOp::Invert => unreachable!("handled above"),
            };
            chunk[0] = linear_to_srgb(r);
            chunk[1] = linear_to_srgb(g);
            chunk[2] = linear_to_srgb(b);
        }
        Ok(())
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Result is in [0, 255].
fn invert_channel(value: u8, amount: f64) -> u8 {
    let value = f64::from(value);
    (value + (255.0 - 2.0 * value) * amount).round() as u8
}

struct Opacity {
    amount: f64,
}

impl EffectFilter for Opacity {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // amount in [0, 1], alpha in [0, 255].
    fn apply(&self, frame: &mut Frame) -> Result<(), Error> {
        for chunk in frame.pixels.as_chunks_mut::<4>().0 {
            chunk[3] = (f64::from(chunk[3]) * self.amount).round() as u8;
        }
        Ok(())
    }
}

struct Blur {
    /// Radius as a fraction of the canvas height, matching the core time model's
    /// resolution-independent convention (no absolute pixels).
    radius: f64,
}

impl EffectFilter for Blur {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Radius/alpha stay in range.
    fn apply(&self, frame: &mut Frame) -> Result<(), Error> {
        let (width_u16, height_u16) = check_size(frame.size)?;
        let width = usize::from(width_u16);
        let height = usize::from(height_u16);
        let radius_px = (self.radius * f64::from(height_u16)).round();
        if !radius_px.is_finite() || radius_px <= 0.0 {
            return Ok(());
        }
        let radius = (radius_px as usize).min(width.max(height));

        // Linear-light premultiplied planes, so transparent pixels do not bleed.
        let mut planes = vec![vec![0.0_f64; width * height]; 4];
        for (index, chunk) in frame.pixels.as_chunks::<4>().0.iter().enumerate() {
            let alpha = f64::from(chunk[3]) / 255.0;
            planes[0][index] = srgb_to_linear(chunk[0]) * alpha;
            planes[1][index] = srgb_to_linear(chunk[1]) * alpha;
            planes[2][index] = srgb_to_linear(chunk[2]) * alpha;
            planes[3][index] = alpha;
        }

        let mut scratch = vec![0.0_f64; width * height];
        for plane in &mut planes {
            blur_horizontal(plane, &mut scratch, width, height, radius);
            blur_vertical(&scratch, plane, width, height, radius);
        }

        for (index, chunk) in frame.pixels.as_chunks_mut::<4>().0.iter_mut().enumerate() {
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
        Ok(())
    }
}

fn luminance(r: f64, g: f64, b: f64) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn lerp(from: f64, to: f64, amount: f64) -> f64 {
    from + (to - from) * amount
}

fn sepia(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    (
        0.393 * r + 0.769 * g + 0.189 * b,
        0.349 * r + 0.686 * g + 0.168 * b,
        0.272 * r + 0.534 * g + 0.131 * b,
    )
}

fn srgb_to_linear(value: u8) -> f64 {
    let c = f64::from(value) / 255.0;
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Input is clamped to [0, 1] first.
fn linear_to_srgb(value: f64) -> u8 {
    let c = value.clamp(0.0, 1.0);
    let c = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

fn blur_horizontal(src: &[f64], dst: &mut [f64], width: usize, height: usize, radius: usize) {
    for y in 0..height {
        box_blur_1d(
            width,
            radius,
            |x| src[y * width + x],
            |x, value| {
                dst[y * width + x] = value;
            },
        );
    }
}

fn blur_vertical(src: &[f64], dst: &mut [f64], width: usize, height: usize, radius: usize) {
    for x in 0..width {
        box_blur_1d(
            height,
            radius,
            |y| src[y * width + x],
            |y, value| dst[y * width + x] = value,
        );
    }
}

/// Sliding-window box blur with edge replication, `O(n)` regardless of radius.
#[allow(clippy::cast_precision_loss)] // Radius is capped to the frame dimension.
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

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::{EffectId, EffectKind};
    use proptest::prelude::*;

    fn instance(kind: EffectKind, params: &[(&str, f64)]) -> EffectInstance {
        let mut effect = EffectInstance::new(EffectId(1), kind);
        for (name, value) in params {
            effect.params.insert((*name).to_owned(), *value);
        }
        effect
    }

    fn apply(frame: &mut Frame, effect: &EffectInstance) {
        apply_chain(frame, std::slice::from_ref(effect)).unwrap();
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn invert_twice_is_identity(
            pixels in prop::collection::vec(any::<u8>(), 4 * 32 * 32),
        ) {
            let original = Frame { size: (32, 32), pixels: pixels.clone() };
            let invert = instance(EffectKind::Invert, &[("amount", 1.0)]);
            let mut actual = original.clone();
            apply(&mut actual, &invert);
            apply(&mut actual, &invert);
            for (index, channel) in actual.pixels.iter().enumerate() {
                prop_assert!(
                    channel.abs_diff(original.pixels[index]) <= 2,
                    "channel {index}: {channel} vs {}",
                    original.pixels[index]
                );
            }
        }

        #[test]
        fn saturate_one_is_identity(
            pixels in prop::collection::vec(any::<u8>(), 4 * 16 * 16),
        ) {
            let original = Frame { size: (16, 16), pixels: pixels.clone() };
            let saturate = instance(EffectKind::Saturate, &[("amount", 1.0)]);
            let mut actual = original.clone();
            apply(&mut actual, &saturate);
            for (index, channel) in actual.pixels.iter().enumerate() {
                prop_assert!(
                    channel.abs_diff(original.pixels[index]) <= 2,
                    "channel {index}: {channel} vs {}",
                    original.pixels[index]
                );
            }
        }
    }

    #[test]
    fn opacity_effect_scales_only_alpha() {
        let mut frame = Frame {
            size: (1, 1),
            pixels: vec![200, 100, 50, 200],
        };
        apply(
            &mut frame,
            &instance(EffectKind::Opacity, &[("amount", 0.5)]),
        );
        assert_eq!(frame.pixels, vec![200, 100, 50, 100]);
    }
}
