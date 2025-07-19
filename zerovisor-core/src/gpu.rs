//! GPU manager (Task 7.1)
//!
//! ハイパーバイザ上で SR-IOV / MIG 仮想 GPU を VM へ割り当てる高水準 API。
//! アーキ固有処理は HAL の `GpuVirtualization` 実装に委譲し、本モジュールは
//! リソース追跡と VM との関連付けのみを行う。

#![allow(dead_code)]

extern crate alloc;
use alloc::collections::BTreeMap;
use spin::Mutex;

use zerovisor_hal::gpu::*;
use zerovisor_hal::virtualization::VmHandle;
use crate::security::{self, SecurityEvent};
use crate::ZerovisorError;

/// グローバル GPU マネージャ
pub struct GpuManager<E: GpuVirtualization + Send + Sync + 'static> {
    engine: Mutex<E>,
    // VM→(GPU handle list) のマッピング
    allocs: Mutex<BTreeMap<VmHandle, alloc::vec::Vec<GpuHandle>>>,
}

static mut GPU_MANAGER: Option<GpuManager<zerovisor_hal::arch::x86_64::gpu::SrIovGpuEngine>> = None;

/// サブシステム初期化
pub fn init() -> Result<(), ZerovisorError> {
    if unsafe { GPU_MANAGER.is_some() } { return Ok(()); }

    // 現状は x86_64 SR-IOV エンジンのみサポート
    let engine = zerovisor_hal::arch::x86_64::gpu::SrIovGpuEngine::init()
        .map_err(|_| ZerovisorError::InitializationFailed)?;

    unsafe { GPU_MANAGER = Some(GpuManager { engine: Mutex::new(engine), allocs: Mutex::new(BTreeMap::new()) }); }
    Ok(())
}

/// GPU VF を VM に割り当て
pub fn assign_gpu(vm: VmHandle, cfg: &GpuConfig) -> Result<GpuHandle, GpuError> {
    let mgr = unsafe { GPU_MANAGER.as_ref().ok_or(GpuError::InitializationFailed)? };
    let mut eng = mgr.engine.lock();
    let handle = eng.create_vf(cfg)?;

    mgr.allocs.lock().entry(vm).or_default().push(handle);

    security::record_event(SecurityEvent::PerfWarning { avg_latency_ns: 0, wcet_ns: None }); // placeholder for audit
    Ok(handle)
}

/// Map shared DMA buffer into guest and record shared page count.
pub fn map_guest_dma(vm: VmHandle, gpu: GpuHandle, guest_pa: u64, size: usize) -> Result<(), GpuError> {
    let mgr = unsafe { GPU_MANAGER.as_ref().ok_or(GpuError::InitializationFailed)? };
    let mut eng = mgr.engine.lock();
    eng.map_guest_memory(gpu, guest_pa, size)?;

    crate::monitor::add_shared_pages((size as u64 + 0xFFF) / 0x1000);
    Ok(())
}

pub fn unmap_guest_dma(vm: VmHandle, gpu: GpuHandle, guest_pa: u64, size: usize) -> Result<(), GpuError> {
    let mgr = unsafe { GPU_MANAGER.as_ref().ok_or(GpuError::InitializationFailed)? };
    let mut eng = mgr.engine.lock();
    eng.unmap_guest_memory(gpu, guest_pa, size)?;
    crate::monitor::remove_shared_pages((size as u64 + 0xFFF) / 0x1000);
    Ok(())
}

/// GPU VF を解放
pub fn release_gpu(vm: VmHandle, handle: GpuHandle) -> Result<(), GpuError> {
    let mgr = unsafe { GPU_MANAGER.as_ref().ok_or(GpuError::InitializationFailed)? };
    let mut eng = mgr.engine.lock();
    eng.destroy_vf(handle)?;

    if let Some(list) = mgr.allocs.lock().get_mut(&vm) {
        list.retain(|&h| h != handle);
    }
    Ok(())
}

/// VM に割り当てられた GPU 一覧
pub fn list_assigned(vm: VmHandle) -> alloc::vec::Vec<GpuHandle> {
    unsafe {
        GPU_MANAGER
            .as_ref()
            .and_then(|m| m.allocs.lock().get(&vm).cloned())
            .unwrap_or_default()
    }
} 