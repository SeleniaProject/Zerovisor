//! Minimal PCI configuration space access (x86_64 I/O-port mechanism)
//! Used by GPU SR-IOV engine (Task 7.1).

#![cfg(target_arch = "x86_64")]

#[inline(always)]
fn pci_config_addr(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    0x8000_0000 | ((bus as u32) << 16) | ((device as u32) << 11) | ((function as u32) << 8) | ((offset as u32) & 0xFC)
}

#[inline(always)]
unsafe fn io_outl(port: u16, val: u32) {
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack, preserves_flags));
    }
}
#[inline(always)]
unsafe fn io_inl(port: u16) -> u32 {
    unsafe {
        let val: u32;
        core::arch::asm!("in eax, dx", in("dx") port, out("eax") val, options(nomem, nostack, preserves_flags));
        val
    }
}

pub unsafe fn read_config_dword(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    unsafe {
        io_outl(0xCF8, pci_config_addr(bus, device, function, offset));
        io_inl(0xCFC)
    }
}

pub unsafe fn write_config_dword(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    unsafe {
        io_outl(0xCF8, pci_config_addr(bus, device, function, offset));
        io_outl(0xCFC, value);
    }
} 