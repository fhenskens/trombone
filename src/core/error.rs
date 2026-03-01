//! Errors for stream setup and runtime.

/// Result type used by this library.
pub type Result<T> = core::result::Result<T, AudioError>;

/// Errors from backend and stream operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// The backend does not support this config.
    UnsupportedConfig,
    /// The requested state change is not valid right now.
    InvalidStateTransition,
    /// The backend returned an error code.
    BackendFailure {
        /// Native backend error value.
        code: i32,
    },
    /// No render callback has been set on the stream.
    RenderCallbackNotSet,
    /// No capture callback has been set on the stream.
    CaptureCallbackNotSet,
    /// This operation is not implemented yet.
    NotImplemented,
}
