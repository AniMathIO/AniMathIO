//! Speed/Duration inspector and exact parsing of frame-based ramp keys.
use super::*;
use animathio_core::{ClipSpeed, FrameRate, SpeedPoint};

pub(super) fn percent(value: &str) -> Result<ClipSpeed, String> {
    let value = value.trim().trim_end_matches('%');
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if fraction.len() > 4 || !fraction.bytes().all(|c| c.is_ascii_digit()) {
        return Err("Use at most four decimal places for percent".into());
    }
    let scale = 10_u32.pow(u32::try_from(fraction.len()).unwrap_or(4));
    let n = whole
        .parse::<u32>()
        .ok()
        .and_then(|v| v.checked_mul(scale))
        .and_then(|v| {
            v.checked_add(if fraction.is_empty() {
                0
            } else {
                fraction.parse::<u32>().ok()?
            })
        })
        .ok_or("Enter a valid speed percent")?;
    ClipSpeed::new(n, scale * 100).map_err(|e| e.0)
}
pub(super) fn ramps(text: &str, rate: FrameRate) -> Result<Vec<SpeedPoint>, String> {
    if text.trim().is_empty() {
        return Ok(vec![]);
    }
    if text.split(',').count() > 128 {
        return Err("At most 128 speed keys".into());
    }
    text.split(',')
        .map(|point| {
            let (frame, speed) = point
                .trim()
                .split_once(':')
                .ok_or("Use frame:percent, frame:percent")?;
            let frame = frame
                .trim()
                .parse::<i64>()
                .map_err(|_| "Ramp frames must be nonnegative integers")?;
            if frame < 0 {
                return Err("Ramp frames must be nonnegative".into());
            }
            Ok(SpeedPoint {
                time: rate
                    .frame_to_time(frame, Rounding::Nearest)
                    .map_err(|e| e.0)?,
                speed: percent(speed)?,
            })
        })
        .collect()
}
impl Editor {
    pub(super) fn speed_rows(&self, id: ClipId, cx: &mut Context<Self>) -> Div {
        let Some(clip) = self.find_clip(id) else {
            return div();
        };
        let frames = self
            .project
            .frame_rate()
            .time_to_frame(
                Time(clip.range.end.0 - clip.range.start.0),
                Rounding::Nearest,
            )
            .unwrap_or(0);
        div()
            .flex()
            .flex_col()
            .gap_2()
            .px_3()
            .py_2()
            .child(format!(
                "{:.2}%{} · {frames} frames",
                clip.time_map.speed.percent(),
                if clip.time_map.reverse {
                    " reverse"
                } else {
                    ""
                }
            ))
            .child(if clip.time_map.ramps.is_empty() {
                "Constant speed"
            } else {
                "Linear speed ramp"
            })
            .child(
                Button::new("edit-speed").label("Speed/Duration…").on_click(
                    cx.listener(|this, _, window, cx| this.open_speed_duration(window, cx)),
                ),
            )
            .child(
                Button::new("insert-hold")
                    .label("Insert frame hold…")
                    .on_click(cx.listener(|this, _, window, cx| this.open_frame_hold(window, cx))),
            )
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn percent_and_ramp_input_stay_exact() {
        assert_eq!(percent("6.25").unwrap(), ClipSpeed::new(1, 16).unwrap());
        assert_eq!(percent("150%").unwrap(), ClipSpeed::new(3, 2).unwrap());
        assert!(percent("NaN").is_err());
        assert!(percent("1601").is_err());
        let keys = ramps("0:100, 30:200", FrameRate::new(30, 1).unwrap()).unwrap();
        assert_eq!(keys[1].time, Time(animathio_core::TICKS_PER_SECOND));
        assert_eq!(keys[1].speed, ClipSpeed::new(2, 1).unwrap());
        assert!(ramps("-1:100", FrameRate::new(30, 1).unwrap()).is_err());
    }
}
