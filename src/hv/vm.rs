#![allow(dead_code)]

use core::sync::atomic::{AtomicU64, Ordering};
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
        let _ = self;
    }
}


