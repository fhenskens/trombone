//! Linux backend modules.

pub mod alsa;
pub mod pipewire;

use crate::backend::AudioBackend;
use crate::core::config::StreamConfig;
use crate::core::error::Result;
use crate::core::stream::Stream;

/// Linux backend choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxBackendKind {
    /// Auto-select backend (PipeWire first, then ALSA).
    Auto,
    /// ALSA backend (planned).
    Alsa,
    /// PipeWire backend (planned).
    PipeWire,
}

/// Linux backend selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinuxBackend {
    kind: LinuxBackendKind,
}

impl LinuxBackend {
    /// Create a backend with the chosen implementation.
    pub fn new(kind: LinuxBackendKind) -> Self {
        Self { kind }
    }

    /// Get selected backend type.
    pub fn kind(&self) -> LinuxBackendKind {
        self.kind
    }
}

impl AudioBackend for LinuxBackend {
    fn create_stream(&self, config: StreamConfig) -> Result<Stream> {
        match self.kind {
            LinuxBackendKind::Auto => create_stream_auto(config),
            LinuxBackendKind::PipeWire => create_stream_pipewire(config),
            LinuxBackendKind::Alsa => create_stream_alsa(config),
        }
    }
}

fn create_stream_auto(config: StreamConfig) -> Result<Stream> {
    let first_error = match create_stream_pipewire(config) {
        Ok(stream) => return Ok(stream),
        Err(error) => error,
    };
    match create_stream_alsa(config) {
        Ok(stream) => Ok(stream),
        Err(_) => Err(first_error),
    }
}

fn create_stream_pipewire(config: StreamConfig) -> Result<Stream> {
    pipewire::create_stream(config)
}

fn create_stream_alsa(config: StreamConfig) -> Result<Stream> {
    alsa::create_stream(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::StreamConfig;
    use crate::core::error::AudioError;

    fn create_stream_auto_with<P, A>(
        config: StreamConfig,
        pipewire_fn: P,
        alsa_fn: A,
    ) -> Result<Stream>
    where
        P: FnOnce(StreamConfig) -> Result<Stream>,
        A: FnOnce(StreamConfig) -> Result<Stream>,
    {
        let first_error = match pipewire_fn(config) {
            Ok(stream) => return Ok(stream),
            Err(error) => error,
        };
        match alsa_fn(config) {
            Ok(stream) => Ok(stream),
            Err(_) => Err(first_error),
        }
    }

    #[test]
    fn backend_kind_roundtrips() {
        let auto = LinuxBackend::new(LinuxBackendKind::Auto);
        assert_eq!(auto.kind(), LinuxBackendKind::Auto);

        let alsa = LinuxBackend::new(LinuxBackendKind::Alsa);
        assert_eq!(alsa.kind(), LinuxBackendKind::Alsa);

        let pipewire = LinuxBackend::new(LinuxBackendKind::PipeWire);
        assert_eq!(pipewire.kind(), LinuxBackendKind::PipeWire);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn backend_auto_can_create_output_stream_on_linux() {
        let backend = LinuxBackend::new(LinuxBackendKind::Auto);
        let result = backend.create_stream(StreamConfig::default());
        assert!(result.is_ok());
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn backend_returns_not_implemented_on_non_linux() {
        let backend = LinuxBackend::new(LinuxBackendKind::Auto);
        let result = backend.create_stream(StreamConfig::default());
        match result {
            Ok(_) => panic!("expected not implemented on non-linux platforms"),
            Err(err) => assert_eq!(err, AudioError::NotImplemented),
        }
    }

    #[test]
    fn auto_returns_pipewire_when_available() {
        let config = StreamConfig::default();
        let result = create_stream_auto_with(
            config,
            |c| Ok(Stream::new(c)),
            |_c| Err(AudioError::NotImplemented),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn auto_falls_back_to_alsa_when_pipewire_fails() {
        let config = StreamConfig::default();
        let result = create_stream_auto_with(
            config,
            |_c| Err(AudioError::BackendFailure { code: -77 }),
            |c| Ok(Stream::new(c)),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn auto_returns_pipewire_error_when_both_fail() {
        let config = StreamConfig::default();
        let result = create_stream_auto_with(
            config,
            |_c| Err(AudioError::BackendFailure { code: -88 }),
            |_c| Err(AudioError::NotImplemented),
        );
        match result {
            Ok(_) => panic!("expected error when both linux backends fail"),
            Err(err) => assert_eq!(err, AudioError::BackendFailure { code: -88 }),
        }
    }
}
