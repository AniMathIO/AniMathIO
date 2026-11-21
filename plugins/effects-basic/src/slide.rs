//! The `slide` effect: slide a clip in or out of its own bounds.
//!
//! The effect sees the clip's rasterized layer, so it works for any source (media,
//! text, shapes, math). The *bounds* are the tight box around every pixel with
//! nonzero alpha; the content moves by up to its own width (left/right) or height
//! (top/bottom) and, when `clip` is on, is masked to those bounds, so it emerges from
//! (or retreats behind) its own edge instead of sweeping across the canvas.
//!
//! With `characters` on, the ink is split into glyph units and each unit gets its own
//! progress, staggered in reading order. A unit is a run of columns that contain ink
//! inside one text line, which keeps the layout the renderer already produced
//! (wrapping, alignment, kerning, ligatures, captions): glyphs that touch or overlap
//! horizontally (ligatures, tight kerning, italic overhang) travel together, and the
//! dot of an `i` stays with its stem. Content without gaps (an image, a video frame)
//! is a single unit and slides as a whole.
//!
//! Everything is a pure function of the pixels and parameters. `progress = 1` returns
//! the input untouched, and `progress = 0` leaves nothing visible inside the bounds.

use super::number;
use super::animathio::plugin::types::ParamEntry;

/// Upper bound on glyph units; past it the content slides as one unit.
const MAX_UNITS: usize = 4096;

/// An inclusive pixel rectangle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rect {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Rect {
    fn width(self) -> usize {
        self.x1 - self.x0 + 1
    }

    fn height(self) -> usize {
        self.y1 - self.y0 + 1
    }
}

pub fn apply(
    pixels: &[u8],
    width: usize,
    height: usize,
    params: &[ParamEntry],
) -> Result<Vec<u8>, String> {
    let progress = unit_param(params, "progress", 1.0)?;
    let direction = number(params, "direction").unwrap_or(1.0);
    if !direction.is_finite() || !(0.5..=4.5).contains(&direction) {
        return Err("effect direction must be 1 (left), 2 (right), 3 (top) or 4 (bottom)".into());
    }
    let direction = direction.round() as u8;
    let characters = unit_param(params, "characters", 0.0)? > 0.5;
    let clip = unit_param(params, "clip", 1.0)? > 0.5;

    if progress >= 1.0 {
        return Ok(pixels.to_vec());
    }
    let Some(bounds) = ink_bounds(pixels, width, height) else {
        return Ok(pixels.to_vec());
    };
    let vertical = direction >= 3;
    let sign = if direction % 2 == 1 { -1.0_f32 } else { 1.0 };
    let extent = if vertical {
        bounds.height()
    } else {
        bounds.width()
    } as f32;

    let units = if characters {
        glyph_units(pixels, width, bounds)
    } else {
        vec![bounds]
    };
    let mask = if clip {
        bounds
    } else {
        Rect {
            x0: 0,
            y0: 0,
            x1: width - 1,
            y1: height - 1,
        }
    };

    let mut out = vec![0_u8; pixels.len()];
    let count = units.len();
    // The last unit starts after `span` of the progress and every unit takes the rest, so
    // one unit uses the whole range and many units overlap in the second half.
    let span = 0.5 * (1.0 - 1.0 / count as f32);
    let each = 1.0 - span;
    for (index, unit) in units.iter().enumerate() {
        let start = if count > 1 {
            span * index as f32 / (count - 1) as f32
        } else {
            0.0
        };
        let local = ((progress as f32 - start) / each).clamp(0.0, 1.0);
        let offset = sign * (1.0 - local) * extent;
        blit(pixels, &mut out, width, *unit, offset, vertical, mask);
    }
    Ok(out)
}

fn unit_param(params: &[ParamEntry], id: &str, default: f64) -> Result<f64, String> {
    let value = number(params, id).unwrap_or(default);
    if value.is_finite() && (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(format!("effect {id} must be in [0, 1]"))
    }
}

fn alpha(pixels: &[u8], width: usize, x: usize, y: usize) -> u8 {
    pixels[(y * width + x) * 4 + 3]
}

/// Tight box around every pixel with nonzero alpha.
fn ink_bounds(pixels: &[u8], width: usize, height: usize) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    for y in 0..height {
        let row = &pixels[y * width * 4..(y + 1) * width * 4];
        let first = row.chunks_exact(4).position(|p| p[3] > 0);
        let Some(first) = first else { continue };
        let last = row.chunks_exact(4).rposition(|p| p[3] > 0).unwrap_or(first);
        bounds = Some(match bounds {
            None => Rect {
                x0: first,
                y0: y,
                x1: last,
                y1: y,
            },
            Some(b) => Rect {
                x0: b.x0.min(first),
                y0: b.y0,
                x1: b.x1.max(last),
                y1: y,
            },
        });
    }
    bounds
}

/// Runs of consecutive `true` values as inclusive `(first, last)` pairs.
fn runs(flags: impl Iterator<Item = bool>) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = None;
    let mut len = 0;
    for (index, flag) in flags.enumerate() {
        len = index + 1;
        match (flag, start) {
            (true, None) => start = Some(index),
            (false, Some(first)) => {
                out.push((first, index - 1));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(first) = start {
        out.push((first, len - 1));
    }
    out
}

/// Glyph units in reading order: lines top to bottom, columns of ink left to right.
fn glyph_units(pixels: &[u8], width: usize, bounds: Rect) -> Vec<Rect> {
    let row_ink = (bounds.y0..=bounds.y1)
        .map(|y| (bounds.x0..=bounds.x1).any(|x| alpha(pixels, width, x, y) > 0));
    let mut units = Vec::new();
    for (first, last) in runs(row_ink) {
        let (y0, y1) = (bounds.y0 + first, bounds.y0 + last);
        let column_ink = (bounds.x0..=bounds.x1)
            .map(|x| (y0..=y1).any(|y| alpha(pixels, width, x, y) > 0));
        for (a, b) in runs(column_ink) {
            units.push(Rect {
                x0: bounds.x0 + a,
                y0,
                x1: bounds.x0 + b,
                y1,
            });
            if units.len() > MAX_UNITS {
                return vec![bounds];
            }
        }
    }
    units
}

/// Premultiplied RGBA (0..=255 scale) of one pixel of `unit`; transparent outside it.
fn sample(pixels: &[u8], width: usize, unit: Rect, x: i64, y: i64) -> [f32; 4] {
    if x < unit.x0 as i64 || x > unit.x1 as i64 || y < unit.y0 as i64 || y > unit.y1 as i64 {
        return [0.0; 4];
    }
    let at = (y as usize * width + x as usize) * 4;
    let a = f32::from(pixels[at + 3]);
    let k = a / 255.0;
    [
        f32::from(pixels[at]) * k,
        f32::from(pixels[at + 1]) * k,
        f32::from(pixels[at + 2]) * k,
        a,
    ]
}

/// Draw `unit` of `src` into `dst` shifted by `offset` pixels along one axis (linear
/// sub-pixel resampling along it), restricted to `mask` and composited with "over".
fn blit(
    src: &[u8],
    dst: &mut [u8],
    width: usize,
    unit: Rect,
    offset: f32,
    vertical: bool,
    mask: Rect,
) {
    let whole = offset.abs() < f32::EPSILON;
    // `along` runs over the shifted axis, `across` over the other one.
    let (along_lo, along_hi, across_lo, across_hi) = if vertical {
        (unit.y0, unit.y1, unit.x0, unit.x1)
    } else {
        (unit.x0, unit.x1, unit.y0, unit.y1)
    };
    let (mask_lo, mask_hi) = if vertical {
        (mask.y0, mask.y1)
    } else {
        (mask.x0, mask.x1)
    };
    let (across_mask_lo, across_mask_hi) = if vertical {
        (mask.x0, mask.x1)
    } else {
        (mask.y0, mask.y1)
    };
    let low = ((along_lo as f32 + offset).floor() as i64 - 1).max(mask_lo as i64);
    let high = ((along_hi as f32 + offset).ceil() as i64 + 1).min(mask_hi as i64);
    for across in across_lo.max(across_mask_lo)..=across_hi.min(across_mask_hi) {
        for along in low..=high {
            let (x, y) = if vertical {
                (across, along as usize)
            } else {
                (along as usize, across)
            };
            let at = (y * width + x) * 4;
            if whole {
                let (sx, sy) = (x as i64, y as i64);
                if sx < unit.x0 as i64
                    || sx > unit.x1 as i64
                    || sy < unit.y0 as i64
                    || sy > unit.y1 as i64
                {
                    continue;
                }
                let pixel = &src[at..at + 4];
                if pixel[3] == 0 {
                    continue;
                }
                over(&mut dst[at..at + 4], pixel_premult(pixel));
                continue;
            }
            let source = along as f32 - offset;
            let floor = source.floor();
            let f = source - floor;
            let floor = floor as i64;
            let at_source = |along_source: i64| {
                if vertical {
                    sample(src, width, unit, across as i64, along_source)
                } else {
                    sample(src, width, unit, along_source, across as i64)
                }
            };
            let a = at_source(floor);
            let b = at_source(floor + 1);
            let mixed = [
                a[0] * (1.0 - f) + b[0] * f,
                a[1] * (1.0 - f) + b[1] * f,
                a[2] * (1.0 - f) + b[2] * f,
                a[3] * (1.0 - f) + b[3] * f,
            ];
            if mixed[3] <= 0.0 {
                continue;
            }
            over(&mut dst[at..at + 4], mixed);
        }
    }
}

fn pixel_premult(pixel: &[u8]) -> [f32; 4] {
    let a = f32::from(pixel[3]);
    let k = a / 255.0;
    [
        f32::from(pixel[0]) * k,
        f32::from(pixel[1]) * k,
        f32::from(pixel[2]) * k,
        a,
    ]
}

/// Composite a premultiplied source over a straight-alpha destination pixel.
fn over(dst: &mut [u8], src: [f32; 4]) {
    if dst[3] == 0 && src[3] >= 254.5 {
        // Opaque onto nothing: skip the divide so unmoved pixels stay bit exact.
        dst[0] = src[0].round() as u8;
        dst[1] = src[1].round() as u8;
        dst[2] = src[2].round() as u8;
        dst[3] = 255;
        return;
    }
    let sa = src[3] / 255.0;
    let da = f32::from(dst[3]) / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        return;
    }
    for channel in 0..3 {
        let premult = src[channel] + f32::from(dst[channel]) * da * (1.0 - sa);
        dst[channel] = (premult / out_a).round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
}
