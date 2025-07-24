//! NIC passthrough manager (Task: SR-IOV NIC & Storage device passthrough)
//!
//! This module exposes high-level APIs to assign an SR-IOV capable NIC VF to a
//! guest VM and handle DMA buffer mappings.  The heavy lifting is delegated to
//! architecture-specific back-ends residing in `zerovisor-hal`.

#![allow(dead_code)]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

use zerovisor_hal::nic::*;
use zerovisor_hal::memory::PhysicalAddress;
use zerovisor_hal::virtualization::VmHandle;
use crate::monitor;
use crate::security::{self, SecurityEvent};
use crate::ZerovisorError;

#[cfg(target_arch = "x86_64")]
use zerovisor_hal::arch::x86_64::{global as hal_nic_global, SriovNicEngine};

/// Handle type alias for clarity (maps to work-queue context in real impl).
pub type NicHandle = u32;

pub struct NicManager<N: HpcNic + Send + Sync + 'static> {
    nic: N,
    allocs: Mutex<BTreeMap<VmHandle, Vec<NicHandle>>>,
    next_handle: Mutex<NicHandle>,
}

static mut NIC_MANAGER: Option<NicManager<&'static dyn HpcNic>> = None;

pub fn init() -> Result<(), ZerovisorError> {
    if unsafe { NIC_MANAGER.is_some() } { return Ok(()); }
    #[cfg(target_arch = "x86_64")]
    {
        zerovisor_hal::arch::x86_64::init_global();
        let nic = hal_nic_global().ok_or(ZerovisorError::HardwareNotSupported)?;
        unsafe {
            NIC_MANAGER = Some(NicManager { nic, allocs: Mutex::new(BTreeMap::new()), next_handle: Mutex::new(1) });
        }
    }
    Ok(())
}

/// For demo the config is implicit; returns opaque handle.
pub fn assign_nic(vm: VmHandle) -> Result<NicHandle, NicError> {
    let mgr = unsafe { NIC_MANAGER.as_ref().ok_or(NicError::NotSupported)? };
    let mut hlock = mgr.next_handle.lock();
    let handle = *hlock;
    *hlock += 1;
    mgr.allocs.lock().entry(vm).or_default().push(handle);
    Ok(handle)
}

pub fn release_nic(vm: VmHandle, handle: NicHandle) -> Result<(), NicError> {
    let mgr = unsafe { NIC_MANAGER.as_ref().ok_or(NicError::NotSupported)? };
    if let Some(list) = mgr.allocs.lock().get_mut(&vm) {
        list.retain(|&h| h != handle);
    }
    Ok(())
}

/// Map guest buffer for RDMA (placeholder implementation).
pub fn map_guest_dma(vm: VmHandle, handle: NicHandle, guest_pa: PhysicalAddress, size: usize) -> Result<(), NicError> {
    let mgr = unsafe { NIC_MANAGER.as_ref().ok_or(NicError::NotSupported)? };
    // no actual NIC mapping in stub; just record
    monitor::add_shared_pages((size as u64 + 0xFFF) / 0x1000);
    security::record_event(SecurityEvent::IoMapping { guest_pa, size });
    Ok(())
}

pub fn unmap_guest_dma(vm: VmHandle, handle: NicHandle, guest_pa: PhysicalAddress, size: usize) -> Result<(), NicError> {
    let mgr = unsafe { NIC_MANAGER.as_ref().ok_or(NicError::NotSupported)? };
    monitor::remove_shared_pages((size as u64 + 0xFFF) / 0x1000);
    Ok(())
} 