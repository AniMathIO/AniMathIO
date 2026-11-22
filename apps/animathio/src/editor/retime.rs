//! Speed/Duration inspector and exact parsing of frame-based ramp keys.
#![allow(deprecated)] // 2.2 token aliases still in use (UI-rework PR 2-6 remove these)

use super::*;
use crate::style::{TextRole, color, hex};
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
/// Pixel radius within which a pointer grabs a ramp handle.
const RAMP_GRAB: f32 = 7.0;
/// A clip block narrower than this (pixels) draws no speed lane, so it takes no key edits.
pub(super) const RAMP_LANE_MIN_WIDTH: f32 = 24.0;

impl Editor {
    /// Ticks in one project frame: the closest two ramp keys may be.
    fn ramp_gap(&self) -> i64 {
        self.project
            .frame_rate()
            .frame_to_time(1, Rounding::Floor)
            .map_or(1, |t| t.0.max(1))
    }

    /// The vertical band (canvas-relative pixels) of the clip in display row `row`.
    pub(super) fn ramp_lane(
        layout: &TimelineLayout,
        row: usize,
    ) -> Option<crate::speed_ramp::Lane> {
        let y0 = layout.ruler_height + row as f32 * layout.row_height + 3.0;
        crate::speed_ramp::lane(y0, y0 + layout.row_height - 6.0)
    }

    /// The selected clip (alone) with its display row, when speed handles apply to it.
    pub(super) fn ramp_target(&self) -> Option<(usize, &Clip)> {
        let selected = self.selected?;
        if self.selected_ids().len() > 1 {
            return None;
        }
        timeline::display_tracks(&self.project)
            .into_iter()
            .enumerate()
            .find_map(|(row, track)| track.clips().get(&selected).map(|clip| (row, clip)))
    }

    /// Whether the selected clip's speed lane is drawn (and so editable): the row is tall
    /// enough, the block is at least [`RAMP_LANE_MIN_WIDTH`] wide and the clip is not a hold.
    fn ramp_lane_drawn(layout: &TimelineLayout, row: usize, clip: &Clip) -> bool {
        let width = timeline::time_to_x(layout, clip.range.end)
            - timeline::time_to_x(layout, clip.range.start);
        !clip.time_map.hold
            && width >= RAMP_LANE_MIN_WIDTH
            && Self::ramp_lane(layout, row).is_some()
    }

    /// The speed key under a canvas-relative point, on the selected clip only. Only keys
    /// inside the clip block have handles, and the pointer must be over that block.
    pub(super) fn hit_ramp_handle(
        &self,
        layout: &TimelineLayout,
        x: f32,
        y: f32,
    ) -> Option<(ClipId, usize)> {
        let (row, clip) = self.ramp_target()?;
        if !Self::ramp_lane_drawn(layout, row, clip) {
            return None;
        }
        let lane = Self::ramp_lane(layout, row)?;
        let (block_x0, block_x1) = (
            timeline::time_to_x(layout, clip.range.start),
            timeline::time_to_x(layout, clip.range.end),
        );
        if x < block_x0 || x > block_x1 {
            return None;
        }
        crate::speed_ramp::vertices(clip)
            .into_iter()
            .filter_map(|vertex| {
                let index = vertex.index?;
                let kx = timeline::time_to_x(layout, Time(clip.range.start.0 + vertex.time.0));
                let (dx, dy) = ((x - kx).abs(), (y - lane.y(vertex.speed)).abs());
                (dx <= RAMP_GRAB && dy <= RAMP_GRAB).then_some((index, dx + dy))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(index, _)| (clip.id, index))
    }

    /// Mouse press on the timeline: grab a speed key to drag it, remove it with Alt, or
    /// Alt-click the selected clip to add a key there. Returns whether it consumed the press.
    pub(super) fn ramp_press(
        &mut self,
        layout: &TimelineLayout,
        x: f32,
        y: f32,
        alt: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if let Some((id, index)) = self.hit_ramp_handle(layout, x, y) {
            if alt {
                let command = self
                    .find_clip(id)
                    .and_then(|clip| crate::speed_ramp::remove_key(clip, index));
                if let Some(command) = command {
                    self.try_execute(&command, cx);
                }
                return true;
            }
            let Some((row, key)) = self.ramp_target().and_then(|(row, clip)| {
                clip.time_map.ramps.get(index).map(|k| {
                    let local = Time(crate::speed_ramp::local_time(clip, k));
                    (row, (local, k.speed))
                })
            }) else {
                return false;
            };
            let Some(lane) = Self::ramp_lane(layout, row) else {
                return false;
            };
            self.selected_field = None;
            self.selected_keyframe = None;
            self.gesture = Some(TimelineGesture::Ramp(RampDrag {
                clip_id: id,
                index,
                time: key.0,
                speed: key.1,
                lane,
            }));
            cx.notify();
            return true;
        }
        if !alt {
            return false;
        }
        // Alt-click on the selected clip adds a key at the pointer's time, but only where
        // the speed lane is drawn (a row too short or a clip block too narrow for it shows
        // no keys).
        if self
            .ramp_target()
            .is_none_or(|(row, clip)| !Self::ramp_lane_drawn(layout, row, clip))
        {
            return false;
        }
        let Some(hit) = timeline::hit_test(layout, &self.project, x, y) else {
            return false;
        };
        if self
            .ramp_target()
            .is_none_or(|(_, clip)| clip.id != hit.clip)
        {
            return false;
        }
        let gap = self.ramp_gap();
        let command = self.find_clip(hit.clip).and_then(|clip| {
            let local = timeline::x_to_time(layout, x).0 - clip.range.start.0;
            crate::speed_ramp::add_key(clip, Time(local), gap)
        });
        if let Some(command) = command {
            self.try_execute(&command, cx);
        }
        true
    }

    /// Pointer motion while dragging a speed key: time between its neighbours, speed from
    /// the pointer row.
    pub(super) fn ramp_drag_to(
        &mut self,
        layout: &TimelineLayout,
        x: f32,
        y: f32,
        cx: &mut Context<Self>,
    ) {
        let gap = self.ramp_gap();
        let Some(TimelineGesture::Ramp(drag)) = &self.gesture else {
            return;
        };
        let (id, index, lane) = (drag.clip_id, drag.index, drag.lane);
        let Some(clip) = self.find_clip(id) else {
            return;
        };
        let (low, high) = crate::speed_ramp::time_bounds(clip, index, gap);
        let time = Time((timeline::x_to_time(layout, x).0 - clip.range.start.0).clamp(low, high));
        let speed = lane.speed_at(y);
        if let Some(TimelineGesture::Ramp(drag)) = &mut self.gesture {
            drag.time = time;
            drag.speed = speed;
        }
        cx.notify();
    }

    /// Release: one `SetClipSpeed` for the whole drag, or nothing when the key is back
    /// where it started.
    pub(super) fn ramp_release(&mut self, drag: &RampDrag, cx: &mut Context<Self>) {
        let command = self
            .find_clip(drag.clip_id)
            .and_then(|clip| crate::speed_ramp::move_key(clip, drag.index, drag.time, drag.speed));
        if let Some(command) = command {
            self.try_execute(&command, cx);
        }
        cx.notify();
    }

    /// Curve vertices to paint for the selected clip, with an in-flight drag applied.
    /// `start` is where the clip is painted (it may be mid-gesture).
    pub(super) fn ramp_paints(&self, clip: &Clip, start: Time) -> Vec<RampKeyPaint> {
        crate::speed_ramp::vertices(clip)
            .into_iter()
            .map(|vertex| match &self.gesture {
                Some(TimelineGesture::Ramp(drag))
                    if drag.clip_id == clip.id && vertex.index == Some(drag.index) =>
                {
                    RampKeyPaint {
                        time: Time(start.0 + drag.time.0),
                        speed: drag.speed,
                        active: true,
                        handle: true,
                    }
                }
                _ => RampKeyPaint {
                    time: Time(start.0 + vertex.time.0),
                    speed: vertex.speed,
                    active: false,
                    handle: vertex.index.is_some(),
                },
            })
            .collect()
    }

    /// Insert a speed key at the playhead, at the speed the clip already has there.
    pub(super) fn add_speed_key_at_playhead(&mut self, id: ClipId, cx: &mut Context<Self>) {
        let gap = self.ramp_gap();
        let command = self.find_clip(id).and_then(|clip| {
            crate::speed_ramp::add_key(clip, Time(self.position.0 - clip.range.start.0), gap)
        });
        if let Some(command) = command {
            self.try_execute(&command, cx);
        } else {
            self.error =
                Some("Move the playhead inside the clip, away from an existing speed key".into());
            cx.notify();
        }
    }

    pub(super) fn set_maintain_pitch(
        &mut self,
        id: ClipId,
        maintain_pitch: bool,
        cx: &mut Context<Self>,
    ) {
        self.try_execute(&Command::SetClipMaintainPitch { id, maintain_pitch }, cx);
    }
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
        let editor = cx.entity();
        let summary = |value: String| {
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_role(crate::style::Role::Label)
                .text_color(hex(color::TEXT))
                .tabular()
                .child(value)
        };
        if clip.time_map.hold {
            return div()
                .flex()
                .flex_col()
                .child(Self::param_row_flush(
                    "Speed",
                    summary(format!("Hold, {frames} frames")),
                ))
                .child(
                    div()
                        .px_2()
                        .text_role(crate::style::Role::Caption)
                        .text_color(hex(color::TEXT_MUTED))
                        .child("Trim edges to change duration; audio is silent."),
                );
        }
        div()
            .flex()
            .flex_col()
            .child(Self::param_row_flush(
                "Speed",
                summary(format!(
                    "{}{}, {frames} frames",
                    speed_summary(&clip.time_map),
                    if clip.time_map.reverse {
                        ", reverse"
                    } else {
                        ""
                    }
                )),
            ))
            .child(
                div()
                    .px_2()
                    .text_role(crate::style::Role::Caption)
                    .text_color(hex(color::TEXT_MUTED))
                    .child(if clip.time_map.ramps.is_empty() {
                        "Constant speed"
                    } else {
                        "Linear speed ramp"
                    }),
            )
            .child(
                // The two actions side by side at `Size::Small`, never full width.
                div()
                    .px_2()
                    .pt_1()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("edit-speed")
                            .outline()
                            .small()
                            .label("Speed/Duration…")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_speed_duration(window, cx);
                            })),
                    )
                    .child(
                        Button::new("add-speed-key")
                            .outline()
                            .small()
                            .icon(icons::icon(IconName::Plus).small())
                            .label("Add speed key")
                            .tooltip("Add a speed key at the playhead")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.add_speed_key_at_playhead(id, cx);
                            })),
                    )
                    .child(
                        Button::new("insert-hold")
                            .outline()
                            .small()
                            .label("Insert frame hold…")
                            .on_click(
                                cx.listener(|this, _, window, cx| this.open_frame_hold(window, cx)),
                            ),
                    ),
            )
            .child(div().px_2().child(crate::style::switch_row(
                "maintain-pitch",
                "Maintain pitch",
                None,
                clip.maintain_pitch,
                move |checked, _, cx| {
                    editor.update(cx, |this, cx| this.set_maintain_pitch(id, *checked, cx));
                },
            )))
    }
}
/// A percentage without trailing zeros: `100%`, `62.5%`.
fn percent_text(percent: f64) -> String {
    let text = format!("{percent:.2}");
    format!("{}%", text.trim_end_matches('0').trim_end_matches('.'))
}

/// The speed line of the inspector: one value for a constant speed, first to last key for a ramp.
fn speed_summary(map: &animathio_core::ClipTimeMap) -> String {
    match (map.ramps.first(), map.ramps.last()) {
        (Some(first), Some(last)) if map.ramps.len() > 1 => format!(
            "{} to {}",
            percent_text(first.speed.percent()),
            percent_text(last.speed.percent())
        ),
        (Some(only), _) => percent_text(only.speed.percent()),
        _ => percent_text(map.speed.percent()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[core::prelude::v1::test]
    fn ramp_summary_shows_first_to_last_speed() {
        let rate = FrameRate::new(30, 1).unwrap();
        let mut map = animathio_core::ClipTimeMap::default();
        assert_eq!(speed_summary(&map), "100%");
        map.ramps = ramps("0:100, 30:200", rate).unwrap();
        assert_eq!(speed_summary(&map), "100% to 200%");
        map.ramps = ramps("0:62.5", rate).unwrap();
        assert_eq!(speed_summary(&map), "62.5%");
    }
    #[core::prelude::v1::test]
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
