//! PluginManager – dynamic hypervisor feature extension framework (Task 2 of extensibility)
//! Provides registration, initialization, and VMEXIT dispatch for plugins implementing
//! `HypervisorPlugin` trait as defined in design.md.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use spin::Mutex;

use zerovisor_hal::virtualization::{VmExitReason, VmExitAction};

pub trait HypervisorPlugin: Send + Sync {
    /// Called once during hypervisor boot.
    fn initialize(&self) -> Result<(), ()>;
    /// Optional fast-path VMEXIT handler. Return Some(action) to override.
    fn handle_vmexit(&self, exit: &VmExitReason) -> Option<VmExitAction> { let _ = exit; None }
    /// Called on shutdown to cleanup resources.
    fn cleanup(&self) {}
}

pub struct PluginManager {
    plugins: Mutex<Vec<&'static dyn HypervisorPlugin>>,
}

impl PluginManager {
    pub const fn new() -> Self { Self { plugins: Mutex::new(Vec::new()) } }

    pub fn register(&self, p: &'static dyn HypervisorPlugin) -> Result<(), ()> {
        p.initialize()?;
        self.plugins.lock().push(p);
        Ok(())
    }

    pub fn handle_vmexit(&self, exit: &VmExitReason) -> Option<VmExitAction> {
        for p in self.plugins.lock().iter() {
            if let Some(a) = p.handle_vmexit(exit) { return Some(a); }
        }
        None
    }

    pub fn cleanup(&self) { for p in self.plugins.lock().iter() { p.cleanup(); } }
}

static PLUGINS: PluginManager = PluginManager::new();

pub fn global() -> &'static PluginManager { &PLUGINS } 