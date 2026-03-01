//! Backend traits and platform modules.

pub mod android;

use crate::core::config::StreamConfig;
use crate::core::error::Result;
use crate::core::stream::Stream;

/// Trait for creating streams.
pub trait AudioBackend {
    /// Create a stream for the given config.
    fn create_stream(&self, config: StreamConfig) -> Result<Stream>;
}
