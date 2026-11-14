//! Clip effects.
//!
//! Effects are pixel filters implemented by sandboxed plugins; this crate only owns the
//! chain mechanics. A clip is rasterized alone into a canvas-sized straight-alpha sRGB RGBA8
//! layer, each [`EffectInstance`] of its chain is handed to the context's
//! [`EffectProcessor`] in order, and the filtered layer is composited. The CPU reference
//! and the GPU preview backend share this path, so effects look the same in both.

use crate::{Error, Frame, ResourcePool};
use animathio_core::{EffectProcessor, SceneClip};

/// Apply a clip's ordered effect chain to its rasterized layer.
///
/// A clip without active effects (bypassed ones are skipped) never touches the processor. A
/// chain with an active effect needs one
/// (see [`RenderCtx::set_effect_processor`](crate::RenderCtx::set_effect_processor)); a
/// missing plugin, rejected parameter or sandbox violation fails the frame with
/// [`Error::Effect`] instead of silently skipping the filter.
pub fn apply_chain(
    frame: &mut Frame,
    clip: &SceneClip,
    pool: &mut ResourcePool,
) -> Result<(), Error> {
    frame.validate()?;
    if !clip.has_active_effects() {
        return Ok(());
    }
    let processor = pool.effects.as_deref_mut().ok_or_else(|| {
        Error::Effect("clip has effects but no effect plugins are loaded".to_owned())
    })?;
    run(processor, frame, clip)
}

fn run(
    processor: &mut dyn EffectProcessor,
    frame: &mut Frame,
    clip: &SceneClip,
) -> Result<(), Error> {
    for instance in clip.effects.iter().filter(|effect| !effect.bypass) {
        processor
            .apply(instance, clip.source_time, frame.size, &mut frame.pixels)
            .map_err(Error::Effect)?;
        frame.validate()?;
    }
    Ok(())
}
