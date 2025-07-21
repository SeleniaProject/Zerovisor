//! IsolationEngine – guest memory and device isolation enforcement
//!
//! 本モジュールはゲスト同士およびゲストとホストのメモリ／デバイス分離を保証します。
//! グローバルに 1 つの `IsolationEngine` インスタンスが存在し、各 VM の
//! 1. ゲスト物理メモリ領域
//! 2. 割り当て済みデバイス (PCI BDF 等)
//! をトラッキングします。重複や不正共有が検出された場合は `security::record_event`
//! を呼出してセキュリティイベントとして保存します。

#![allow(dead_code)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use core::ops::Range;
use spin::{Mutex, Once};

use crate::security::{record_event, SecurityEvent};

/// VM identifier alias (matches `VmHandle`)
pub type VmId = u32;

/// Device identifier – PCI BDF などを `u32` で符号化 (bus<<16 | dev<<8 | func)
pub type DeviceId = u32;

/// Isolation policy violation errors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationError {
    /// Requested memory range overlaps existing VM range
    MemoryOverlap { owner: VmId },
    /// Device already assigned to another VM
    DeviceInUse { owner: VmId },
    /// VM not registered
    UnknownVm,
}

/// Internal representation of per-VM resources
#[derive(Default)]
struct VmResources {
    /// Guest physical memory regions (sorted, non-overlapping)
    memory: BTreeMap<Range<u64>, ()>,
    /// Devices owned by the VM
    devices: alloc::collections::BTreeMap<DeviceId, ()>,
}

/// Isolation engine state
pub struct IsolationEngine {
    /// Map VM → resources
    vms: Mutex<BTreeMap<VmId, VmResources>>, // protected by spinlock
}

impl IsolationEngine {
    /// Construct empty engine
    const fn new() -> Self {
        Self { vms: Mutex::new(BTreeMap::new()) }
    }

    /// Register or update a VM memory mapping
    pub fn register_memory(&self, vm: VmId, guest_phys: u64, size: u64) -> Result<(), IsolationError> {
        if size == 0 { return Ok(()); }
        let range = guest_phys..guest_phys.saturating_add(size);
        let mut map = self.vms.lock();
        let entry = map.entry(vm).or_default();

        // Check overlap against *all* other VMs first
        for (&other_vm, res) in map.iter() {
            if other_vm == vm { continue; }
            for (r, _) in &res.memory {
                if ranges_overlap(&range, r) {
                    record_event(SecurityEvent::MemoryIntegrityViolation {
                        phys_addr: guest_phys,
                        expected_hash: [0u8; 32],
                        actual_hash: [0u8; 32],
                    });
                    return Err(IsolationError::MemoryOverlap { owner: other_vm });
                }
            }
        }

        // Ensure no self-overlap (should not happen with EPT manager)
        for (r, _) in &entry.memory {
            if ranges_overlap(&range, r) {
                return Err(IsolationError::MemoryOverlap { owner: vm });
            }
        }
        entry.memory.insert(range, ());
        Ok(())
    }

    /// Deregister a VM memory range (e.g., on unmap)
    pub fn unregister_memory(&self, vm: VmId, guest_phys: u64, size: u64) -> Result<(), IsolationError> {
        let range = guest_phys..guest_phys.saturating_add(size);
        let mut map = self.vms.lock();
        let entry = map.get_mut(&vm).ok_or(IsolationError::UnknownVm)?;
        let key = entry.memory.keys().find(|r| **r == range).cloned();
        if let Some(k) = key {
            entry.memory.remove(&k);
        }
        Ok(())
    }

    /// Assign a device exclusively to a VM
    pub fn assign_device(&self, vm: VmId, dev: DeviceId) -> Result<(), IsolationError> {
        let mut map = self.vms.lock();
        // Verify device unused
        for (&owner, res) in map.iter() {
            if res.devices.contains_key(&dev) && owner != vm {
                return Err(IsolationError::DeviceInUse { owner });
            }
        }
        let entry = map.entry(vm).or_default();
        entry.devices.insert(dev, ());
        Ok(())
    }

    /// Release device from a VM
    pub fn release_device(&self, vm: VmId, dev: DeviceId) -> Result<(), IsolationError> {
        let mut map = self.vms.lock();
        let res = map.get_mut(&vm).ok_or(IsolationError::UnknownVm)?;
        res.devices.remove(&dev);
        Ok(())
    }

    /// Remove all resources for VM (on destroy)
    pub fn cleanup_vm(&self, vm: VmId) {
        let mut map = self.vms.lock();
        map.remove(&vm);
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Global singleton helpers
// ──────────────────────────────────────────────────────────────────────────────

static ISOLATION: Once<IsolationEngine> = Once::new();

/// Initialise global IsolationEngine (idempotent)
pub fn init() {
    ISOLATION.call_once(|| IsolationEngine::new());
}

/// Obtain reference to global engine. Panics if `init()` not called.
pub fn engine() -> &'static IsolationEngine {
    ISOLATION.get().expect("IsolationEngine not initialised")
}

// ──────────────────────────────────────────────────────────────────────────────
// Helper – range overlap test
// ──────────────────────────────────────────────────────────────────────────────
fn ranges_overlap(a: &Range<u64>, b: &Range<u64>) -> bool {
    a.start < b.end && b.start < a.end
} 