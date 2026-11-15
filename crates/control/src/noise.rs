//! Persisted denoise learning from a bounded timeline selection.
use crate::ControlError;
use animathio_core::{
    AssetId, AudioChain, ClipId, ClipKind, Command, EffectId, Project, TICKS_PER_SECOND, TimeRange,
};
use animathio_transcribe::prepare::{Selection, mix_selection};
use std::collections::BTreeMap;

/// Learn sixteen 500 Hz power-density bands (the last band also covers higher frequencies).
/// The caller applies the returned batch through its history.
pub fn learn_noise_command(
    project: &Project,
    assets: &BTreeMap<AssetId, Vec<u8>>,
    clip: ClipId,
    effect: EffectId,
    range: TimeRange,
) -> Result<Command, ControlError> {
    let invalid = |s: &str| ControlError::InvalidParams(s.into());
    let track = project
        .tracks()
        .iter()
        .find(|t| t.clips().contains_key(&clip))
        .ok_or_else(|| invalid("clip missing"))?;
    let source = &track.clips()[&clip];
    if track.is_locked() || !matches!(source.kind, ClipKind::Audio(_)) {
        return Err(invalid("choose audio on an unlocked track"));
    }
    if !source
        .audio_effects
        .iter()
        .any(|e| e.id == effect && e.plugin == "org.animathio.audio-basic" && e.effect == "denoise")
    {
        return Err(invalid("denoise effect missing"));
    }
    TimeRange::new(range.start, range.end)?;
    if range.start < source.range.start
        || range.end > source.range.end
        || range.end.0 - range.start.0 > 30 * TICKS_PER_SECOND
    {
        return Err(invalid(
            "noise selection must lie inside the clip and be at most 30 seconds",
        ));
    }
    let (samples, _) = mix_selection(
        project,
        assets,
        Selection {
            clip: Some(clip),
            track: None,
            range: Some(range),
        },
    )?;
    let bands =
        noise_bands(&samples).ok_or_else(|| invalid("select at least 512 audio samples"))?;
    let mut commands: Vec<_> = bands
        .iter()
        .enumerate()
        .map(|(i, value)| Command::SetAudioEffectParam {
            chain: AudioChain::Clip(clip),
            effect,
            name: format!("noise_{i}"),
            value: Some(*value),
        })
        .collect();
    commands.push(Command::SetAudioEffectParam {
        chain: AudioChain::Clip(clip),
        effect,
        name: "noise_profile".into(),
        value: Some(1.0),
    });
    Ok(Command::Batch { commands })
}

/// Deterministic periodic sqrt-Hann periodogram at 16 kHz, averaged over complete frames.
#[allow(clippy::cast_precision_loss)]
pub fn noise_bands(samples: &[f32]) -> Option<[f64; 16]> {
    let mut bands = [0.0; 16];
    let mut counts = [0_u32; 16];
    for frame in samples.windows(512).step_by((samples.len() / 16).max(128)) {
        for k in 0..=256 {
            let (mut re, mut im) = (0.0, 0.0);
            for (n, sample) in frame.iter().enumerate() {
                let phase = std::f64::consts::TAU * n as f64 / 512.0;
                let x = f64::from(*sample) * (0.5 - 0.5 * phase.cos()).sqrt();
                let angle = phase * k as f64;
                re += x * angle.cos();
                im -= x * angle.sin();
            }
            let band = (k / 16).min(15);
            bands[band] += (re * re + im * im) / (16_000.0 * 256.0);
            counts[band] += 1;
        }
    }
    if counts[0] == 0 {
        return None;
    }
    for (b, n) in bands.iter_mut().zip(counts) {
        *b /= f64::from(n);
    }
    Some(bands)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_is_deterministic_and_silent_bands_are_zero() {
        assert!(noise_bands(&[0.0; 511]).is_none());
        assert_eq!(noise_bands(&[0.0; 1024]), Some([0.0; 16]));
        let samples: Vec<_> = (0..4096)
            .map(|i| if i % 2 == 0 { 0.1 } else { -0.1 })
            .collect();
        let bands = noise_bands(&samples).unwrap();
        assert_eq!(Some(bands), noise_bands(&samples));
        assert!(bands[15] > bands[0]);
    }
}
