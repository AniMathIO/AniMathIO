//! Pixel helpers for edit-point transitions. Inputs are straight-alpha RGBA8 frames of
//! equal size; blends happen in premultiplied space so transparent edges stay clean.
use crate::Frame;

/// Premultiplied linear interpolation `a·(1−t) + b·t`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Rounded to 0..=255.
pub(crate) fn mix(a: &Frame, b: &Frame, t: f64) -> Frame {
    let t = t.clamp(0.0, 1.0);
    let mut pixels = vec![0_u8; a.pixels.len()];
    for ((out, pa), pb) in pixels
        .chunks_exact_mut(4)
        .zip(a.pixels.chunks_exact(4))
        .zip(b.pixels.chunks_exact(4))
    {
        let (aa, ab) = (f64::from(pa[3]) / 255.0, f64::from(pb[3]) / 255.0);
        let alpha = aa * (1.0 - t) + ab * t;
        if alpha <= 0.0 {
            continue;
        }
        for c in 0..3 {
            let premultiplied = f64::from(pa[c]) * aa * (1.0 - t) + f64::from(pb[c]) * ab * t;
            out[c] = (premultiplied / alpha).round().clamp(0.0, 255.0) as u8;
        }
        out[3] = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
    }
    Frame {
        size: a.size,
        pixels,
    }
}

/// Opaque solid frame.
pub(crate) fn solid(size: crate::Size, rgb: u8) -> Frame {
    Frame {
        size,
        pixels: [rgb, rgb, rgb, 255].repeat(size.0 as usize * size.1 as usize),
    }
}

/// Multiply alpha by a clip opacity in `[0, 1]`.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Rounded to 0..=255.
pub(crate) fn fade(frame: &mut Frame, opacity: f64) {
    if opacity >= 1.0 {
        return;
    }
    for alpha in frame.pixels.iter_mut().skip(3).step_by(4) {
        *alpha = (f64::from(*alpha) * opacity).round().clamp(0.0, 255.0) as u8;
    }
}
