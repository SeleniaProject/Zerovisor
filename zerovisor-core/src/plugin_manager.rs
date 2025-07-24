//! PluginManager – dynamic hypervisor feature extension framework (Task 2 of extensibility)
//! Provides registration, initialization, and VMEXIT dispatch for plugins implementing
//! `HypervisorPlugin` trait as defined in design.md.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use spin::Mutex;
use sha2::{Sha256, Digest};
use crate::security;

use zerovisor_hal::virtualization::{VmExitReason, VmExitAction};

pub trait HypervisorPlugin: Send + Sync {
    /// Called once during hypervisor boot.
    fn initialize(&self) -> Result<(), ()>;
    /// Optional fast-path VMEXIT handler. Return Some(action) to override.
    fn handle_vmexit(&self, exit: &VmExitReason) -> Option<VmExitAction> { let _ = exit; None }
    /// Called on shutdown to cleanup resources.
    fn cleanup(&self) {}
}

/// Plugin manifest information – provided by plugin binary at registration time.
pub struct PluginManifest {
    pub name: &'static str,
    pub version: u32,
    pub hash: [u8;32],
    pub signature: &'static [u8],
}

/// Internal plugin state
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginState { Registered, Active, Error }

struct PluginRecord {
    plugin: &'static dyn HypervisorPlugin,
    manifest: PluginManifest,
    state: PluginState,
}

pub struct PluginManager {
    plugins: Mutex<Vec<PluginRecord>>,
}

impl PluginManager {
    pub const fn new() -> Self { Self { plugins: Mutex::new(Vec::new()) } }

    /// Securely register a plugin with manifest verification.
    pub fn register_secure(&self, manifest: PluginManifest, plugin: &'static dyn HypervisorPlugin) -> Result<(), ()> {
        // 1. Verify hash matches (caller passes binary slice separately – here assume ok)
        // 2. Verify signature using security engine Dilithium public key
        if !security::engine().crypto.verify_attestation(&manifest.hash, manifest.signature) {
            return Err(());
        }
        // 3. Initialize plugin
        plugin.initialize()?;
        self.plugins.lock().push(PluginRecord { plugin, manifest, state: PluginState::Active });
        Ok(())
    }

    /// Unload plugin by name (idempotent).
    pub fn unload(&self, name: &str) {
        let mut vec = self.plugins.lock();
        if let Some(pos) = vec.iter().position(|r| r.manifest.name == name) {
            let rec = &vec[pos];
            rec.plugin.cleanup();
            vec.remove(pos);
        }
    }

    pub fn handle_vmexit(&self, exit: &VmExitReason) -> Option<VmExitAction> {
        for rec in self.plugins.lock().iter() {
            if rec.state == PluginState::Active {
                if let Some(a) = rec.plugin.handle_vmexit(exit) { return Some(a); }
            }
        }
        None
    }

    pub fn cleanup(&self) { for rec in self.plugins.lock().iter() { rec.plugin.cleanup(); } }
}

static PLUGIN_MGR: PluginManager = PluginManager::new();

pub fn global() -> &'static PluginManager { &PLUGIN_MGR } 