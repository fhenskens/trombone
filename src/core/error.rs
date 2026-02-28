//! Error domain for stream setup and runtime.

/// Library-wide result type.
pub type Result<T> = core::result::Result<T, AudioError>;

/// Error type for backend and stream operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// Requested configuration is unsupported by the backend.
    UnsupportedConfig,
    /// Stream state transition was invalid for current state.
    InvalidStateTransition,
    /// Underlying backend returned an error code.
    BackendFailure {
        /// Backend-native error value.
        code: i32,
    },
    /// Operation is not yet implemented in current backend.
    NotImplemented,
}
