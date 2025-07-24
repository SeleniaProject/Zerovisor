// Zerovisor CNI driver (Task: Kubernetes CNI integration)
#![allow(dead_code)]
use core::ffi::c_char;
use alloc::collections::BTreeMap;
use spin::Mutex;
use crate::log;
use crate::nic_manager;
use crate::security::{self, SecurityEvent};

/// Global in-memory map: interface name → NIC handle assigned
static ASSIGNMENTS: Mutex<BTreeMap<(u32, alloc::string::String), nic_manager::NicHandle>> = Mutex::new(BTreeMap::new());

#[repr(C)]
pub enum CniStatus { Success = 0, Failure = 1 }

unsafe fn cstr(ptr: *const c_char) -> &'static str {
    if ptr.is_null() { return "<null>"; }
    let mut len = 0; while *ptr.add(len) != 0 { len += 1; }
    core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr as *const u8, len))
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cni_add(iface: *const c_char, vm_id: u32) -> CniStatus {
    log!("[CNI] add {} to VM {}", cstr(iface), vm_id);
    let name = cstr(iface).to_string();
    match nic_manager::assign_nic(vm_id) {
        Ok(handle) => {
            ASSIGNMENTS.lock().insert((vm_id, name.clone()), handle);
            security::record_event(SecurityEvent::IoMapping { guest_pa: 0x1000_8000, size: 0x200 });
            CniStatus::Success
        }
        Err(_) => CniStatus::Failure,
    }
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cni_del(iface: *const c_char, vm_id: u32) -> CniStatus {
    log!("[CNI] del {} from VM {}", cstr(iface), vm_id);
    let name = cstr(iface).to_string();
    if let Some(handle) = ASSIGNMENTS.lock().remove(&(vm_id, name)) {
        let _ = nic_manager::release_nic(vm_id, handle);
    }
    CniStatus::Success
} 