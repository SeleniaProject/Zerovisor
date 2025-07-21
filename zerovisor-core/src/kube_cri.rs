//! Kubernetes CRI shim for Zerovisor (Task: Kubernetes CRI ランタイム統合)
//!
//! This lightweight module exposes a subset of the Kubernetes Container
//! Runtime Interface sufficient to start/stop micro-VM based pods.  A full
//! gRPC server is out of scope for a `no_std` hypervisor, so instead we expose
//! a thin C ABI that can be wrapped by an external user-mode shim (similar to
//! containerd-shim-v2).  The shim forwards CRI requests via virtio-serial or
//! MMIO mailbox to these handlers.
//!
//! NOTE: All functions are placeholders and simply log the request for now.

#![allow(dead_code)]

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::ffi::c_char;

use crate::api::{create_vm, start_vm, stop_vm, destroy_vm};
use crate::log;

/// C ABI compatible response codes.
#[repr(C)]
pub enum CriStatus { Success = 0, Failure = 1 }

/// Convert C string pointer to Rust &str (unsafe helper).
unsafe fn cstr(ptr: *const c_char) -> &'static str {
    use core::slice;
    if ptr.is_null() { return "<null>"; }
    let mut len = 0;
    while *ptr.add(len) != 0 { len += 1; }
    let bytes = slice::from_raw_parts(ptr as *const u8, len);
    core::str::from_utf8_unchecked(bytes)
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cri_create_pod_sandbox(pod_uid: *const c_char) -> CriStatus {
    let uid = cstr(pod_uid);
    log!("[CRI] create_pod_sandbox {}", uid);
    // For simplicity we allocate a VM per pod.
    let cfg = crate::api::VmConfig::default();
    match create_vm(cfg) { Ok(_) => CriStatus::Success, Err(_) => CriStatus::Failure }
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cri_create_container(container_id: *const c_char) -> CriStatus {
    log!("[CRI] create_container {}", cstr(container_id));
    CriStatus::Success
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cri_start_container(container_id: *const c_char) -> CriStatus {
    log!("[CRI] start_container {}", cstr(container_id));
    // Placeholder: map to micro-VM start.
    let _ = start_vm(0); // VM id 0 stub
    CriStatus::Success
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cri_stop_container(container_id: *const c_char) -> CriStatus {
    log!("[CRI] stop_container {}", cstr(container_id));
    let _ = stop_vm(0);
    CriStatus::Success
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cri_remove_container(container_id: *const c_char) -> CriStatus {
    log!("[CRI] remove_container {}", cstr(container_id));
    let _ = destroy_vm(0);
    CriStatus::Success
} 