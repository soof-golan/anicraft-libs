//! Errors of the audio crate.

use std::path::PathBuf;

/// Everything that can go wrong decoding, converting or playing audio.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    /// The input is larger than the configured cap.
    #[error("audio input is {size} bytes, more than the {limit} byte limit")]
    InputTooLarge {
        /// Size of the input.
        size: u64,
        /// The cap.
        limit: u64,
    },
    /// The decoded audio would exceed the configured sample cap.
    #[error("decoded audio exceeds the limit of {limit} samples")]
    OutputTooLarge {
        /// The cap, in samples (all channels).
        limit: usize,
    },
    /// No supported audio stream was found.
    #[error("not a supported audio file: {0}")]
    Unsupported(String),
    /// The container or codec data is damaged and nothing could be decoded.
    #[error("damaged audio data: {0}")]
    Corrupt(String),
    /// The audio parameters are out of range (zero rate, zero channels, too many channels).
    #[error("invalid audio format: {0}")]
    InvalidFormat(String),
    /// Reading a file failed.
    #[error("cannot read {path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The cause.
        source: std::io::Error,
    },
    /// No audio device is available, or the device refused a configuration.
    #[error("audio device: {0}")]
    Device(String),
    /// The actor has stopped and cannot take more commands.
    #[error("the audio actor has stopped")]
    ActorStopped,
}

/// Result alias for this crate.
pub type Result<T, E = AudioError> = std::result::Result<T, E>;
