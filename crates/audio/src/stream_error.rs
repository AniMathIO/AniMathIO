//! What a cpal stream error callback means for a running stream.
//!
//! cpal 0.18 reports conditions through the error callback that the stream survives (the ALSA
//! backend recovers from an xrun by itself and 0.16 never reported it). Only the others end
//! playback or a take.

/// What the stream's owner should do about an error callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamErrorAction {
    /// A glitch (buffer underrun/overrun): the stream recovered; count it.
    Glitch,
    /// Informational: the stream is still running (it was rerouted, or a scheduling
    /// promotion was refused).
    Ignore,
    /// The stream is gone or unusable.
    Fatal,
}

/// Classify a cpal error reported while a stream runs. Unknown (future) kinds are fatal.
pub(crate) fn classify_stream_error(kind: cpal::ErrorKind) -> StreamErrorAction {
    use cpal::ErrorKind;
    match kind {
        ErrorKind::Xrun => StreamErrorAction::Glitch,
        // "The stream remains active and no rebuild is required."
        ErrorKind::DeviceChanged | ErrorKind::RealtimeDenied => StreamErrorAction::Ignore,
        _ => StreamErrorAction::Fatal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpal::ErrorKind;

    #[test]
    fn xrun_is_a_counted_glitch_not_a_failure() {
        assert_eq!(
            classify_stream_error(ErrorKind::Xrun),
            StreamErrorAction::Glitch
        );
    }

    #[test]
    fn routing_and_scheduling_notices_keep_the_stream_running() {
        for kind in [ErrorKind::DeviceChanged, ErrorKind::RealtimeDenied] {
            assert_eq!(classify_stream_error(kind), StreamErrorAction::Ignore);
        }
    }

    #[test]
    fn lost_or_invalid_streams_are_fatal() {
        for kind in [
            ErrorKind::DeviceNotAvailable,
            ErrorKind::StreamInvalidated,
            ErrorKind::HostUnavailable,
            ErrorKind::DeviceBusy,
            ErrorKind::PermissionDenied,
            ErrorKind::ResourceExhausted,
            ErrorKind::UnsupportedConfig,
            ErrorKind::BackendError,
            ErrorKind::Other,
        ] {
            assert_eq!(classify_stream_error(kind), StreamErrorAction::Fatal);
        }
    }
}
