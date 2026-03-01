#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]

//! Trombone is a Rust library for low-latency audio I/O.
//!
//! Main goals:
//! - Real-time-safe callback API (no required allocation in render code).
//! - Rust-first public types.
//! - Backends for major platforms over time.
//! - Clear stream state and error handling.

pub mod backend;
pub mod core;
