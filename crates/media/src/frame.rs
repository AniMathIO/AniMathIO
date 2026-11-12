//! Decoded RGBA frames.

use std::sync::Arc;

use animathio_core::Time;

use crate::color::ColorInfo;

/// A decoded video frame in straight-alpha RGBA8.
#[derive(Clone, Debug)]
pub struct Frame {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes, RGBA order.
    pub pixels: Arc<[u8]>,
    /// Media time this frame was requested for.
    pub pts: Time,
    /// Source colour metadata (after our FFmpeg-compatible default is applied).
    pub color: ColorInfo,
}

impl Frame {
    /// Approximate in-memory footprint in bytes (used by the pool's LRU budget).
    #[must_use]
    pub fn byte_size(&self) -> usize {
        self.pixels.len()
    }
}
