#![forbid(unsafe_op_in_unsafe_fn)]
#![deny(missing_docs)]

//! Trombone: a Rust-first low-latency audio I/O foundation.
//!
//! Design targets:
//! - Real-time-safe callback surface (no required allocation in render path).
//! - Rust-native abstractions with backend-specific adapters.
//! - Android-first backend strategy (AAudio primary, OpenSL ES fallback).
//! - Explicit stream and error state to simplify recovery under XRuns.

pub mod backend;
pub mod core;
