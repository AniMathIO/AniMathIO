//! Visual design tokens for the editor (see `docs/design.md`).
//!
//! UI code takes colours, radii and spacing from here (or from the gpui-component
//! `Theme`, which is derived from these tokens) instead of hard-coding values.
#![allow(dead_code)]

use gpui::{Hsla, rgb};

/// Colour tokens as `0xRRGGBB`.
pub mod color {
    /// Window background, visible as the gutter between panels.
    pub const BG: u32 = 0x0B0D12;
    /// Panel surface.
    pub const PANEL: u32 = 0x11141B;
    /// Raised surface: cards, inputs, popovers, track lanes.
    pub const RAISED: u32 = 0x171B24;
    /// Hovered surface.
    pub const HOVER: u32 = 0x1E2330;
    /// Hairline borders and dividers.
    pub const BORDER: u32 = 0x262C3A;
    pub const TEXT: u32 = 0xE6E9F0;
    pub const TEXT_SECONDARY: u32 = 0x9AA3B5;
    pub const TEXT_MUTED: u32 = 0x5D6578;

    /// Solid accent for selection outlines, active tabs and focus.
    pub const ACCENT: u32 = 0x8B7BFF;
    /// Primary-action gradient start and end.
    pub const ACCENT_FROM: u32 = 0x7C5CFF;
    pub const ACCENT_TO: u32 = 0x22D3EE;
    pub const PLAYHEAD: u32 = 0xFF4D6D;
    pub const SUCCESS: u32 = 0x22C55E;
    pub const WARNING: u32 = 0xF59E0B;
    pub const DANGER: u32 = 0xEF4444;

    /// Clip colours by kind.
    pub const CLIP_VIDEO: u32 = 0x3B82F6;
    pub const CLIP_IMAGE: u32 = 0x14B8A6;
    pub const CLIP_AUDIO: u32 = 0x22C55E;
    pub const CLIP_TEXT: u32 = 0xA855F7;
    pub const CLIP_SHAPE: u32 = 0xF59E0B;
    pub const CLIP_LATEX: u32 = 0xEC4899;
    pub const CLIP_GRAPH: u32 = 0x06B6D4;
    pub const CLIP_PLUGIN: u32 = 0x64748B;
    pub const CLIP_ERROR: u32 = 0xEF4444;
}

/// Corner radii in pixels.
pub mod radius {
    pub const PANEL: f32 = 10.0;
    pub const CARD: f32 = 6.0;
    pub const CLIP: f32 = 6.0;
    pub const CONTROL: f32 = 6.0;
    pub const PILL: f32 = 999.0;
}

/// Spacing in pixels (4px grid).
pub mod space {
    pub const GUTTER: f32 = 6.0;
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

/// Animation durations in milliseconds.
pub mod motion {
    pub const FAST_MS: u64 = 120;
    pub const BASE_MS: u64 = 180;
}

/// An opaque colour from a `0xRRGGBB` token.
pub fn hex(token: u32) -> Hsla {
    rgb(token).into()
}

/// A colour from a `0xRRGGBB` token with the given alpha (0–1).
pub fn hexa(token: u32, alpha: f32) -> Hsla {
    let mut color: Hsla = rgb(token).into();
    color.a = alpha.clamp(0.0, 1.0);
    color
}
