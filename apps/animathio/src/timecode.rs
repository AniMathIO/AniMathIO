//! SMPTE-style `HH:MM:SS:FF` timecode formatting and parsing for the viewer
//! transport. Pure functions over the integer-tick time model; no GPUI.

use animathio_core::{FrameRate, Rounding, Time};

/// Frames shown per timecode second. For the rational NTSC rates this is the
/// ceiling (e.g. 30 for 30000/1001), matching non-drop-frame timecode. Drop-frame
/// NTSC numbering is deliberately not implemented.
pub fn frames_per_second(rate: FrameRate) -> u32 {
    rate.numerator().div_ceil(rate.denominator())
}

/// Format a tick position as `HH:MM:SS:FF`, clamped to non-negative time.
/// The frame field counts from zero within each second.
pub fn format_timecode(time: Time, rate: FrameRate) -> String {
    let fps = i64::from(frames_per_second(rate));
    let total_frames = rate
        .time_to_frame(time, Rounding::Floor)
        .unwrap_or(0)
        .max(0);
    let frame = total_frames % fps;
    let total_seconds = total_frames / fps;
    let second = total_seconds % 60;
    let total_minutes = total_seconds / 60;
    let minute = total_minutes % 60;
    let hour = total_minutes / 60;
    format!("{hour:02}:{minute:02}:{second:02}:{frame:02}")
}

/// Parse `HH:MM:SS:FF` into a tick position. Rejects a wrong field count,
/// out-of-range fields and a frame field at or past the rate's frame count.
pub fn parse_timecode(text: &str, rate: FrameRate) -> Option<Time> {
    let mut parts = text.split(':');
    let hours: i64 = parts.next()?.parse().ok()?;
    let minutes: i64 = parts.next()?.parse().ok()?;
    let seconds: i64 = parts.next()?.parse().ok()?;
    let frames: i64 = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    let fps = i64::from(frames_per_second(rate));
    if hours < 0
        || minutes < 0
        || seconds < 0
        || frames < 0
        || minutes >= 60
        || seconds >= 60
        || frames >= fps
    {
        return None;
    }
    let total_frames = ((hours * 60 + minutes) * 60 + seconds) * fps + frames;
    rate.frame_to_time(total_frames, Rounding::Nearest).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use animathio_core::TICKS_PER_SECOND;

    fn rate(n: u32) -> FrameRate {
        FrameRate::new(n, 1).unwrap()
    }

    #[test]
    fn formats_zero_and_sub_second() {
        assert_eq!(format_timecode(Time(0), rate(30)), "00:00:00:00");
        assert_eq!(
            format_timecode(Time(TICKS_PER_SECOND / 30), rate(30)),
            "00:00:00:01"
        );
    }

    #[test]
    fn formats_whole_seconds_and_hours() {
        assert_eq!(
            format_timecode(Time(TICKS_PER_SECOND), rate(30)),
            "00:00:01:00"
        );
        let hour = TICKS_PER_SECOND * 3600;
        assert_eq!(format_timecode(Time(hour), rate(30)), "01:00:00:00");
    }

    #[test]
    fn round_trips_integer_rates() {
        for fps in [24, 25, 30, 50, 60] {
            let rate = rate(fps);
            for frame in [0, 1, (fps - 1) as i64, fps as i64, fps as i64 * 59 + 12] {
                let time = rate
                    .frame_to_time(frame, Rounding::Nearest)
                    .expect("valid frame");
                let text = format_timecode(time, rate);
                let parsed = parse_timecode(&text, rate).expect("round trip");
                assert_eq!(time, parsed, "fps={fps} frame={frame} text={text}");
            }
        }
    }

    #[test]
    fn rejects_malformed_timecode() {
        let rate = rate(30);
        assert_eq!(parse_timecode("", rate), None);
        assert_eq!(parse_timecode("00:00:00", rate), None);
        assert_eq!(parse_timecode("00:00:00:00:00", rate), None);
        assert_eq!(parse_timecode("00:00:00:xx", rate), None);
        assert_eq!(parse_timecode("00:00:99:00", rate), None);
        assert_eq!(parse_timecode("00:99:00:00", rate), None);
        assert_eq!(parse_timecode("00:00:00:30", rate), None);
        assert_eq!(parse_timecode("-1:00:00:00", rate), None);
    }

    #[test]
    fn ntsc_rate_uses_ceil_frame_count() {
        let rate = FrameRate::new(30_000, 1_001).unwrap();
        assert_eq!(frames_per_second(rate), 30);
        // One wall-clock second is 29.97 frames, so the frame field reads 29
        // (non-drop-frame numbering): the display second boundary lags the
        // wall clock by the expected NTSC drift.
        let text = format_timecode(Time(TICKS_PER_SECOND), rate);
        assert_eq!(text, "00:00:00:29");
    }
}
