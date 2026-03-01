//! Android backend modules.

pub mod aaudio;
pub mod opensl_es;

use crate::backend::AudioBackend;
use crate::core::config::StreamConfig;
use crate::core::error::Result;
use crate::core::stream::Stream;

/// Android backend choice.
///
/// AAudio is preferred, with OpenSL ES as fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidBackendKind {
    /// Auto-select backend (AAudio first, then OpenSL ES).
    Auto,
    /// AAudio backend.
    AAudio,
    /// OpenSL ES backend.
    OpenSLES,
}

/// Simple Android backend selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidBackend {
    kind: AndroidBackendKind,
}

impl AndroidBackend {
    /// Create a backend with the chosen implementation.
    pub fn new(kind: AndroidBackendKind) -> Self {
        Self { kind }
    }

    /// Get selected backend type.
    pub fn kind(&self) -> AndroidBackendKind {
        self.kind
    }
}

impl AudioBackend for AndroidBackend {
    fn create_stream(&self, config: StreamConfig) -> Result<Stream> {
        match self.kind {
            AndroidBackendKind::Auto => create_stream_auto(config),
            AndroidBackendKind::AAudio => aaudio::create_stream(config),
            AndroidBackendKind::OpenSLES => opensl_es::create_stream(config),
        }
    }
}

fn create_stream_auto(config: StreamConfig) -> Result<Stream> {
    let first_error = match aaudio::create_stream(config) {
        Ok(stream) => return Ok(stream),
        Err(error) => error,
    };
    match opensl_es::create_stream(config) {
        Ok(stream) => Ok(stream),
        Err(_) => Err(first_error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::error::AudioError;

    fn create_stream_auto_with<A, O>(
        config: StreamConfig,
        aaudio_fn: A,
        opensl_fn: O,
    ) -> Result<Stream>
    where
        A: FnOnce(StreamConfig) -> Result<Stream>,
        O: FnOnce(StreamConfig) -> Result<Stream>,
    {
        let first_error = match aaudio_fn(config) {
            Ok(stream) => return Ok(stream),
            Err(error) => error,
        };
        match opensl_fn(config) {
            Ok(stream) => Ok(stream),
            Err(_) => Err(first_error),
        }
    }

    #[test]
    fn auto_returns_aaudio_when_available() {
        let config = StreamConfig::default();
        let result = create_stream_auto_with(
            config,
            |c| Ok(Stream::new(c)),
            |_c| Err(AudioError::NotImplemented),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn auto_falls_back_to_opensl_when_aaudio_fails() {
        let config = StreamConfig::default();
        let result = create_stream_auto_with(
            config,
            |_c| Err(AudioError::BackendFailure { code: -7 }),
            |c| Ok(Stream::new(c)),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn auto_returns_aaudio_error_when_both_fail() {
        let config = StreamConfig::default();
        let result = create_stream_auto_with(
            config,
            |_c| Err(AudioError::BackendFailure { code: -11 }),
            |_c| Err(AudioError::NotImplemented),
        );
        match result {
            Ok(_) => panic!("expected error when both backends fail"),
            Err(error) => assert_eq!(error, AudioError::BackendFailure { code: -11 }),
        }
    }
}
