//! Zerovisor management API (Task 12.1)
//! Provides lightweight, architecture-agnostic functions that higher-level
//! tooling can call to query runtime information without requiring complex
//! protocols.
//! All comments are in English per project guidelines.

#![allow(dead_code)]

use zerovisor_hal::PhysicalAddress;
use crate::monitor;
use crate::security;

/// Return the physical address of the 4-KiB metrics page exposed by the
/// monitoring subsystem. External agents can map this page read-only to
/// obtain real-time performance statistics at zero overhead.
pub fn metrics_phys_addr() -> PhysicalAddress {
    monitor::metrics_mmio_ptr() as PhysicalAddress
}

/// Return pointer to security events ring buffer for diagnostics.
pub fn security_events_ptr() -> *const [core::option::Option<security::SecurityEvent>; 1024] {
    security::events() as *const _
} 