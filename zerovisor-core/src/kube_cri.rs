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
use crate::kube_runtime;

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
    // Create PodSandbox via kube_runtime
    let runtime = kube_runtime::global();
    let pod_cfg = kube_runtime::PodConfig { name: uid.into(), namespace: "default".into() };
    match runtime.create_pod(pod_cfg) { Ok(_) => CriStatus::Success, Err(_) => CriStatus::Failure }
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cri_create_container(container_id: *const c_char) -> CriStatus {
    let cid = cstr(container_id);
    let runtime = kube_runtime::global();
    // Here we pass dummy config; real shim will encode JSON.
    let cfg = kube_runtime::ContainerConfig { name: cid.into(), image: "scratch".into(), cmd: Vec::new() };
    match runtime.create_container(&"default-foo".into(), cfg) { Ok(_) => CriStatus::Success, Err(_) => CriStatus::Failure }
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cri_start_container(container_id: *const c_char) -> CriStatus {
    log!("[CRI] start_container {}", cstr(container_id));
    let runtime = kube_runtime::global();
    match runtime.start_container(&cstr(container_id).into()) { Ok(_) => CriStatus::Success, Err(_) => CriStatus::Failure }
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cri_stop_container(container_id: *const c_char) -> CriStatus {
    log!("[CRI] stop_container {}", cstr(container_id));
    let runtime = kube_runtime::global();
    match runtime.stop_container(&cstr(container_id).into()) { Ok(_) => CriStatus::Success, Err(_) => CriStatus::Failure }
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cri_remove_container(container_id: *const c_char) -> CriStatus {
    log!("[CRI] remove_container {}", cstr(container_id));
    let runtime = kube_runtime::global();
    match runtime.remove_container(&cstr(container_id).into()) { Ok(_) => CriStatus::Success, Err(_) => CriStatus::Failure }
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cri_container_stats(container_id: *const c_char, cpu_usage_ns: *mut u64, mem_bytes: *mut u64, uptime_ns: *mut u64) -> CriStatus {
    let runtime = kube_runtime::global();
    match runtime.container_stats(&cstr(container_id).into()) {
        Ok(stats) => {
            if !cpu_usage_ns.is_null() { *cpu_usage_ns = stats.cpu_usage_ns; }
            if !mem_bytes.is_null() { *mem_bytes = stats.mem_usage_bytes; }
            if !uptime_ns.is_null() { *uptime_ns = stats.uptime.as_nanos() as u64; }
            CriStatus::Success
        }
        Err(_) => CriStatus::Failure
    }
}

#[no_mangle]
pub unsafe extern "C" fn zerovisor_cri_container_logs(container_id: *const c_char, last_n: u32, buf: *mut *const u8, len: *mut usize) -> CriStatus {
    let runtime = kube_runtime::global();
    match runtime.container_logs(&cstr(container_id).into(), last_n as usize) {
        Ok(vec) => {
            // Concatenate lines with \n for simplicity
            let joined = vec.join("\n");
            let slice = joined.as_bytes();
            *buf = slice.as_ptr();
            *len = slice.len();
            core::mem::forget(joined); // leak to caller; they must copy
            CriStatus::Success
        }
        Err(_) => CriStatus::Failure
    }
} 