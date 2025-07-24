#![cfg(test)]
//! Device virtualization integration test – ensures virtio-net VF assignment succeeds.

use zerovisor_core::{cni, nic_manager};

#[test]
fn virtio_net_assignment() {
    unsafe { nic_manager::init().unwrap(); }
    let status = unsafe { cni::zerovisor_cni_add(b"eth0\0".as_ptr() as *const i8, 1) };
    assert_eq!(status as u32, cni::CniStatus::Success as u32);
} 