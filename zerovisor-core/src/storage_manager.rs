//! Storage passthrough manager (Task: SR-IOV NIC & Storage device passthrough)
//!
//! This module offers a high-level API to create/destroy SR-IOV NVMe virtual
//! functions and map DMA buffers for guest VMs.  Architecture-specific
//! functionality is delegated to `zerovisor-hal`.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use zerovisor_hal::storage::*;
use zerovisor_hal::virtualization::VmHandle;
use crate::security::{self, SecurityEvent};
use crate::monitor;
use crate::ZerovisorError;

#[cfg(target_arch = "x86_64")]
use zerovisor_hal::arch::x86_64::NvmeSrioVEngine as ArchStorageEngine;

/// Global storage manager instance.
pub struct StorageManager<E: StorageVirtualization + Send + Sync + 'static> {
    engine: Mutex<E>,
    allocs: Mutex<BTreeMap<VmHandle, Vec<StorageHandle>>>,
}

static mut STORAGE_MANAGER: Option<StorageManager<ArchStorageEngine>> = None;

/// Initialize storage passthrough subsystem.
pub fn init() -> Result<(), ZerovisorError> {
    if unsafe { STORAGE_MANAGER.is_some() } { return Ok(()); }
    let engine = ArchStorageEngine::init().map_err(|_| ZerovisorError::InitializationFailed)?;
    unsafe { STORAGE_MANAGER = Some(StorageManager { engine: Mutex::new(engine), allocs: Mutex::new(BTreeMap::new()) }); }
    Ok(())
}

/// Assign SR-IOV VF to VM.
pub fn assign_storage(vm: VmHandle, cfg: &StorageConfig) -> Result<StorageHandle, StorageError> {
    let mgr = unsafe { STORAGE_MANAGER.as_ref().ok_or(StorageError::InitFailed)? };
    let mut eng = mgr.engine.lock();
    let handle = eng.create_vf(cfg)?;
    mgr.allocs.lock().entry(vm).or_default().push(handle);
    Ok(handle)
}

/// Release VF.
pub fn release_storage(vm: VmHandle, handle: StorageHandle) -> Result<(), StorageError> {
    let mgr = unsafe { STORAGE_MANAGER.as_ref().ok_or(StorageError::InitFailed)? };
    let mut eng = mgr.engine.lock();
    eng.destroy_vf(handle)?;
    if let Some(list) = mgr.allocs.lock().get_mut(&vm) { list.retain(|&h| h != handle); }
    Ok(())
}

/// Map guest DMA buffer.
pub fn map_guest_dma(vm: VmHandle, handle: StorageHandle, guest_pa: u64, size: usize) -> Result<(), StorageError> {
    let mgr = unsafe { STORAGE_MANAGER.as_ref().ok_or(StorageError::InitFailed)? };
    let mut eng = mgr.engine.lock();
    eng.map_guest_memory(handle, guest_pa, size)?;
    monitor::add_shared_pages((size as u64 + 0xFFF) / 0x1000);
    security::record_event(SecurityEvent::IoMapping { guest_pa, size });
    Ok(())
}

pub fn unmap_guest_dma(vm: VmHandle, handle: StorageHandle, guest_pa: u64, size: usize) -> Result<(), StorageError> {
    let mgr = unsafe { STORAGE_MANAGER.as_ref().ok_or(StorageError::InitFailed)? };
    let mut eng = mgr.engine.lock();
    eng.unmap_guest_memory(handle, guest_pa, size)?;
    monitor::remove_shared_pages((size as u64 + 0xFFF) / 0x1000);
    Ok(())
} 