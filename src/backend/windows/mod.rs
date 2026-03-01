//! Windows backend modules.

pub mod wasapi;

use crate::backend::AudioBackend;
use crate::core::config::StreamConfig;
use crate::core::error::Result;
use crate::core::stream::Stream;

/// Windows backend choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowsBackendKind {
    /// WASAPI backend.
    Wasapi,
}

/// Simple Windows backend selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsBackend {
    kind: WindowsBackendKind,
}

impl WindowsBackend {
    /// Create a backend with the chosen implementation.
    pub fn new(kind: WindowsBackendKind) -> Self {
        Self { kind }
    }

    /// Get selected backend type.
    pub fn kind(&self) -> WindowsBackendKind {
        self.kind
    }
}

impl AudioBackend for WindowsBackend {
    fn create_stream(&self, config: StreamConfig) -> Result<Stream> {
        match self.kind {
            WindowsBackendKind::Wasapi => wasapi::create_stream(config),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::StreamConfig;

    #[test]
    fn backend_kind_roundtrips() {
        let backend = WindowsBackend::new(WindowsBackendKind::Wasapi);
        assert_eq!(backend.kind(), WindowsBackendKind::Wasapi);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn backend_returns_not_implemented_on_non_windows() {
        use crate::core::error::AudioError;

        let backend = WindowsBackend::new(WindowsBackendKind::Wasapi);
        let result = backend.create_stream(StreamConfig::default());
        match result {
            Ok(_) => panic!("expected not implemented on non-windows"),
            Err(err) => assert_eq!(err, AudioError::NotImplemented),
        }
    }
}
