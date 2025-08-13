#![allow(dead_code)]

//! SMP bring-up scaffolding (enumeration via ACPI MADT).
//!
//! This module provides a read-only enumeration of present processors using the
//! ACPI MADT and formats a simple summary. Actual AP startup (trampoline, SIPI)
//! will be implemented in later steps once low-level memory and IDT setup are
//! ready.

use uefi::prelude::Boot;
use uefi::table::SystemTable;

/// Enumerate CPUs using MADT and print a brief list with counts.
pub fn enumerate_and_report(system_table: &SystemTable<Boot>) {
    if let Some(madt_hdr) = crate::firmware::acpi::find_madt(system_table) {
        let stdout = system_table.stdout();
        let _ = stdout.write_str("SMP: MADT present\r\n");
        crate::firmware::acpi::madt_list_cpus_from(madt_hdr, |s| { let _ = stdout.write_str(s); });
    } else {
        let stdout = system_table.stdout();
        let _ = stdout.write_str("SMP: MADT not found\r\n");
    }
}

/// Minimal AP startup sequence (INIT + two SIPIs) targeting all APs except BSP.
///
/// Note: This function prepares only the delivery; it assumes a real-mode
/// trampoline is present at `trampoline_phys_page` (4KiB aligned) and that the
/// path to transition to long mode will be implemented later.
pub fn start_aps_init_sipi(system_table: &SystemTable<Boot>, lapic_base: usize, trampoline_phys_page: u64) {
    // The startup vector is the physical page number (bits 12..19)
    let vec = ((trampoline_phys_page >> 12) & 0xFF) as u8;
    // Gather APIC IDs via MADT
    if let Some(madt_hdr) = crate::firmware::acpi::find_madt(system_table) {
        let base = madt_hdr as *const crate::firmware::acpi::SdtHeader as usize;
        let total = unsafe { (*(madt_hdr as *const crate::firmware::acpi::MadtHeader)).header.length as usize };
        let mut off = core::mem::size_of::<crate::firmware::acpi::MadtHeader>();
        // Identify BSP APIC ID by reading our own LAPIC ID register.
        let bsp_apic = crate::arch::x86::lapic::read_lapic_id(lapic_base);
        // Send INIT then two SIPIs to each AP
        while off + 2 <= total {
            let p = (base + off) as *const u8;
            let etype = unsafe { p.read() };
            let elen = unsafe { p.add(1).read() } as usize;
            if elen < 2 || off + elen > total { break; }
            if etype == 0 && elen >= 8 {
                let apic_id = unsafe { p.add(3).read() } as u32;
                if apic_id != bsp_apic {
                    crate::arch::x86::lapic::send_init(lapic_base, apic_id);
                    crate::arch::x86::lapic::wait_icr_delivery(lapic_base);
                    // Small wait (~10ms) via UEFI Stall
                    let _ = system_table.boot_services().stall(10_000);
                    crate::arch::x86::lapic::send_sipi(lapic_base, apic_id, vec);
                    crate::arch::x86::lapic::wait_icr_delivery(lapic_base);
                    let _ = system_table.boot_services().stall(200);
                    crate::arch::x86::lapic::send_sipi(lapic_base, apic_id, vec);
                    crate::arch::x86::lapic::wait_icr_delivery(lapic_base);
                }
            }
            off += elen;
        }
    }
}


