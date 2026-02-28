//! Stream lifecycle and state.

use crate::core::callback::RenderCallback;
use crate::core::config::StreamConfig;
use crate::core::error::{AudioError, Result};

/// Current stream state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamState {
    /// Created but not started.
    Stopped,
    /// Actively processing callbacks.
    Running,
    /// Stream encountered a recoverable runtime fault.
    XRun,
}

/// Backend-agnostic stream handle.
///
/// Backends are expected to wrap their native handle and map it to this shape.
pub struct Stream {
    config: StreamConfig,
    state: StreamState,
}

impl Stream {
    /// Construct a new stream in stopped state.
    pub fn new(config: StreamConfig) -> Self {
        Self {
            config,
            state: StreamState::Stopped,
        }
    }

    /// Return the stream configuration.
    pub fn config(&self) -> StreamConfig {
        self.config
    }

    /// Return current state.
    pub fn state(&self) -> StreamState {
        self.state
    }

    /// Start the stream.
    pub fn start(&mut self) -> Result<()> {
        match self.state {
            StreamState::Stopped => {
                self.state = StreamState::Running;
                Ok(())
            }
            StreamState::Running | StreamState::XRun => Err(AudioError::InvalidStateTransition),
        }
    }

    /// Stop the stream.
    pub fn stop(&mut self) -> Result<()> {
        match self.state {
            StreamState::Running | StreamState::XRun => {
                self.state = StreamState::Stopped;
                Ok(())
            }
            StreamState::Stopped => Err(AudioError::InvalidStateTransition),
        }
    }

    /// Attach the callback.
    ///
    /// Current scaffold accepts the callback to lock in trait shape.
    pub fn set_render_callback<C>(&mut self, _callback: C) -> Result<()>
    where
        C: RenderCallback,
    {
        if self.state != StreamState::Stopped {
            return Err(AudioError::InvalidStateTransition);
        }
        Ok(())
    }
}
