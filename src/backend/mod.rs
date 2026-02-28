//! Backend abstractions and platform-specific implementations.

pub mod android;

use crate::core::config::StreamConfig;
use crate::core::error::Result;
use crate::core::stream::Stream;

/// Backend contract for stream creation.
pub trait AudioBackend {
    /// Create a stream for the given configuration.
    fn create_stream(&self, config: StreamConfig) -> Result<Stream>;
}
