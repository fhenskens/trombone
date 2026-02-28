//! Android backends.

pub mod aaudio;
pub mod opensl_es;

use crate::backend::AudioBackend;
use crate::core::config::StreamConfig;
use crate::core::error::Result;
use crate::core::stream::Stream;

/// Android backend strategy.
///
/// Preference is AAudio, with optional OpenSL ES fallback for old devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AndroidBackendKind {
    /// Use AAudio backend.
    AAudio,
    /// Use OpenSL ES backend.
    OpenSLES,
}

/// Simple backend chooser for Android.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AndroidBackend {
    kind: AndroidBackendKind,
}

impl AndroidBackend {
    /// Create a backend with the desired implementation.
    pub fn new(kind: AndroidBackendKind) -> Self {
        Self { kind }
    }

    /// Return selected backend type.
    pub fn kind(&self) -> AndroidBackendKind {
        self.kind
    }
}

impl AudioBackend for AndroidBackend {
    fn create_stream(&self, config: StreamConfig) -> Result<Stream> {
        match self.kind {
            AndroidBackendKind::AAudio => aaudio::create_stream(config),
            AndroidBackendKind::OpenSLES => opensl_es::create_stream(config),
        }
    }
}
