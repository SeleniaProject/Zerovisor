#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use uefi::prelude::Boot;
use uefi::table::SystemTable;

/// Global incremental VM identifier allocator.
static NEXT_VM_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug)]
pub struct VmId(pub u64);

#[derive(Debug, Default)]
pub struct VmConfig {
    pub memory_bytes: u64,
    pub vcpu_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HvVendor { Intel, Amd, Unknown }

#[derive(Debug)]
pub struct Vm {
    pub id: VmId,
    pub config: VmConfig,
    pub vendor: HvVendor,
    pub pml4_phys: u64,
}

impl Vm {
    pub fn create(system_table: &SystemTable<Boot>, config: VmConfig) -> Vm {
        let id = VmId(NEXT_VM_ID.fetch_add(1, Ordering::Relaxed));
        crate::obs::metrics::Counter::new(&crate::obs::metrics::VM_CREATED).inc();
        crate::obs::trace::emit(crate::obs::trace::Event::VmCreate(id.0));
        crate::diag::audit::record(crate::diag::audit::AuditKind::VmCreate(id.0));
        // Detect vendor
        let vendor = match crate::arch::x86::vm::detect_vendor() {
            crate::arch::x86::vm::Vendor::Intel => HvVendor::Intel,
            crate::arch::x86::vm::Vendor::Amd => HvVendor::Amd,
            crate::arch::x86::vm::Vendor::Unknown => HvVendor::Unknown,
        };
        // Build identity mapping up to requested memory (clamp to at least 1 GiB)
        let limit = if config.memory_bytes == 0 { 1u64 << 30 } else { config.memory_bytes };
        let pml4 = match vendor {
            HvVendor::Intel => {
                let caps = crate::mm::ept::EptCaps { large_page_2m: true, large_page_1g: true };
                crate::mm::ept::build_identity_best(system_table, limit, caps).unwrap_or(core::ptr::null_mut())
            }
            HvVendor::Amd => {
                crate::mm::npt::build_identity_2m(system_table, limit).unwrap_or(core::ptr::null_mut())
            }
            HvVendor::Unknown => core::ptr::null_mut(),
        } as u64;
        Vm { id, config, vendor, pml4_phys: pml4 }
    }

    pub fn start(&self, system_table: &mut SystemTable<Boot>) {
        crate::obs::metrics::Counter::new(&crate::obs::metrics::VM_STARTED).inc();
        crate::obs::trace::emit(crate::obs::trace::Event::VmStart(self.id.0));
        crate::diag::audit::record(crate::diag::audit::AuditKind::VmStart(self.id.0));
        match self.vendor {
            HvVendor::Intel => {
                if crate::arch::x86::vm::vmx::vmx_preflight_available() {
                    let _ = crate::arch::x86::vm::vmx::vmx_smoke_test(system_table);
                    let _ = crate::arch::x86::vm::vmx::vmx_ept_smoke_test(system_table);
                }
            }
            HvVendor::Amd => {
                if crate::arch::x86::vm::svm::svm_preflight_available() {
                    let _ = crate::arch::x86::vm::svm::svm_try_enable();
                    let _ = crate::arch::x86::vm::svm::svm_prepare_npt(system_table, self.config.memory_bytes.max(1u64 << 30));
                }
            }
            HvVendor::Unknown => {}
        }
    }

    pub fn stop(&self) { /* no-op for prototype */ }

    pub fn destroy(self) {
        crate::obs::trace::emit(crate::obs::trace::Event::VmStop(self.id.0));
        crate::obs::trace::emit(crate::obs::trace::Event::VmDestroy(self.id.0));
        crate::diag::audit::record(crate::diag::audit::AuditKind::VmStop(self.id.0));
        crate::diag::audit::record(crate::diag::audit::AuditKind::VmDestroy(self.id.0));
        let _ = self;
    }

    pub fn pause(&self) {
        crate::obs::trace::emit(crate::obs::trace::Event::VmStop(self.id.0));
    }

    pub fn resume(&self) {
        crate::obs::trace::emit(crate::obs::trace::Event::VmStart(self.id.0));
    }
}

// ---- Minimal VM registry for control-plane operations ----

#[derive(Clone, Copy, Debug)]
pub struct VmInfo {
    pub id: u64,
    pub vendor: HvVendor,
    pub pml4_phys: u64,
    pub memory_bytes: u64,
}

const VM_REG_CAP: usize = 16;
static VM_REG_LEN: AtomicUsize = AtomicUsize::new(0);
static mut VM_REG: [VmInfo; VM_REG_CAP] = [VmInfo { id: 0, vendor: HvVendor::Unknown, pml4_phys: 0, memory_bytes: 0 }; VM_REG_CAP];

/// Register a VM for later lookup by id. Returns true on success.
pub fn register_vm(vm: &Vm) -> bool {
    let idx = VM_REG_LEN.load(Ordering::Relaxed);
    if idx >= VM_REG_CAP { return false; }
    let info = VmInfo { id: vm.id.0, vendor: vm.vendor, pml4_phys: vm.pml4_phys, memory_bytes: vm.config.memory_bytes.max(1u64 << 30) };
    unsafe { VM_REG[idx] = info; }
    VM_REG_LEN.store(idx + 1, Ordering::Relaxed);
    true
}

/// Find a VM by id and return its snapshot info.
pub fn find_vm(id: u64) -> Option<VmInfo> {
    let len = VM_REG_LEN.load(Ordering::Relaxed);
    for i in 0..len {
        let info = unsafe { VM_REG[i] };
        if info.id == id { return Some(info); }
    }
    None
}

/// Iterate registered VMs.
pub fn list_vms(mut f: impl FnMut(VmInfo)) {
    let len = VM_REG_LEN.load(Ordering::Relaxed);
    for i in 0..len {
        let info = unsafe { VM_REG[i] };
        if info.id != 0 { f(info); }
    }
}


