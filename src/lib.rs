#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]

//! Zerovisor - World-class Type-1 Hypervisor written in Rust
//! 
//! This is the main entry point for the Zerovisor hypervisor, providing
//! a unified interface across different architectures (x86_64, ARM64, RISC-V).

pub use zerovisor_core::*;
pub use zerovisor_hal as hal;

/// Re-export architecture-specific modules
#[cfg(target_arch = "x86_64")]
pub use zerovisor_hal as arch; // x86_64 specific functionality exposed via HAL

#[cfg(target_arch = "aarch64")]
pub use zerovisor_hal as arch; // ARM64 placeholder

#[cfg(target_arch = "riscv64")]
pub use zerovisor_hal as arch; // RISC-V placeholder

/// Initialize Zerovisor hypervisor
pub fn init() -> Result<(), ZerovisorError> {
    zerovisor_core::init()
}

// --------------------------------------------------------------------------
// UEFI Bootloader Entry Point
// --------------------------------------------------------------------------

/// Zerovisor firmware entry. The UEFI bootloader jumps here after exiting
/// boot services, passing the physical memory map pointer and number of
/// entries.  The function never returns.
#[no_mangle]
pub extern "C" fn zerovisor_entry(memory_ptr: *const hal::memory::MemoryRegion, entries: usize) -> ! {
    // SAFETY: The bootloader guarantees that the pointer is valid and the
    // memory map has static lifetime after boot services are exited.
    let memory_map = unsafe { core::slice::from_raw_parts(memory_ptr, entries) };

    // Initialise BootManager – verifies hardware & enables VMX.
    let _bm = zerovisor_core::boot_manager::BootManager::initialize(memory_ptr, entries)
        .expect("BootManager init failed");

    // Initialise the rest of the hypervisor stack using the real memory map.
    zerovisor_core::init_with_memory_map(memory_map).expect("Zerovisor init failed");

    // Enter idle loop – the scheduler will take over once VMs are created.
    loop {
        unsafe { core::arch::asm!("hlt", options(nomem, nostack, preserves_flags)); }
    }
}