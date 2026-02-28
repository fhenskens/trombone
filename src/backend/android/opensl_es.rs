//! OpenSL ES backend.

use crate::core::config::StreamConfig;
use crate::core::error::{AudioError, Result};
use crate::core::stream::Stream;

/// Create an OpenSL ES-backed stream.
pub fn create_stream(_config: StreamConfig) -> Result<Stream> {
    Err(AudioError::NotImplemented)
}
