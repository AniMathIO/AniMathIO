//! Streaming adapter from a media-backend audio decoder to the mixer's `AudioSource`, so the
//! exporter never holds a whole decoded track in memory.

use animathio_audio::{
    AudioSource, ChannelLayout, Error, MAX_INTERLEAVED_SAMPLES, Pcm, PcmReader, StreamInfo,
};
use animathio_core::{TICKS_PER_SECOND, Time};
use animathio_media::AudioDecoder;
use std::sync::Arc;

/// Frames requested from the decoder per call; keeps each request well under its limits.
const CHUNK_FRAMES: usize = 1 << 15;

/// An [`AudioDecoder`] of known length served as a seekable source.
pub(crate) struct DecoderSource {
    decoder: Arc<dyn AudioDecoder>,
    info: StreamInfo,
}
impl DecoderSource {
    pub(crate) fn new(
        decoder: Box<dyn AudioDecoder>,
        rate: u32,
        channels: u16,
        frames: u64,
    ) -> Self {
        Self {
            decoder: decoder.into(),
            info: StreamInfo {
                rate,
                channels,
                frames,
                layout: ChannelLayout::default_for(channels),
            },
        }
    }
}
impl AudioSource for DecoderSource {
    fn decode_all(&self) -> Result<Pcm, Error> {
        let mut reader = self.open_reader()?;
        let mut samples = Vec::new();
        let mut at = 0;
        while at < self.info.frames {
            let got = reader.read_at(at, CHUNK_FRAMES, &mut samples)?;
            if got == 0 {
                break;
            }
            at += got as u64;
            if samples.len() > MAX_INTERLEAVED_SAMPLES {
                return Err(Error::Invalid("decoded audio too large for memory".into()));
            }
        }
        Pcm::new(self.info.rate, self.info.channels, samples)
    }
    fn open_reader(&self) -> Result<Box<dyn PcmReader>, Error> {
        Ok(Box::new(DecoderReader {
            decoder: Arc::clone(&self.decoder),
            info: self.info.clone(),
        }))
    }
}

struct DecoderReader {
    decoder: Arc<dyn AudioDecoder>,
    info: StreamInfo,
}
impl PcmReader for DecoderReader {
    fn info(&self) -> &StreamInfo {
        &self.info
    }
    fn read_at(&mut self, start: u64, frames: usize, out: &mut Vec<f32>) -> Result<usize, Error> {
        let end = start.saturating_add(frames as u64).min(self.info.frames);
        let rate = i128::from(self.info.rate);
        let channels = usize::from(self.info.channels);
        let mut at = start;
        while at < end {
            let len = usize::try_from(end - at)
                .unwrap_or(usize::MAX)
                .min(CHUNK_FRAMES);
            // The smallest tick at or after the frame, so the decoder's floor lands on it.
            let ticks = (i128::from(at) * i128::from(TICKS_PER_SECOND) + rate - 1) / rate;
            let samples = self
                .decoder
                .samples(
                    Time(i64::try_from(ticks).map_err(|e| Error::Invalid(e.to_string()))?),
                    len,
                )
                .map_err(|e| Error::Decode(e.to_string()))?;
            let usable = (samples.len() / channels).min(len);
            out.extend_from_slice(&samples[..usable * channels]);
            if usable < len {
                // A short decode is zero filled so the stream keeps its announced length.
                out.resize(out.len() + (len - usable) * channels, 0.0);
            }
            at += len as u64;
        }
        Ok(usize::try_from(end.saturating_sub(start)).unwrap_or(usize::MAX))
    }
}
