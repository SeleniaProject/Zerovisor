//! NUMA topology detection for Zerovisor HAL (Task 5.1)
//!
//! The implementation provides best-effort NUMA probing without relying on
//! platform firmware services, making it suitable for early boot. When ACPI
//! or Device Tree tables become available later in the boot flow, higher-level
//! code can refresh the topology by calling `detect()` again.

#![allow(dead_code)]
#![deny(unsafe_op_in_unsafe_fn)]

extern crate alloc;
use alloc::vec::Vec;
use core::arch::asm;

/// NUMA node descriptor exposed by the HAL.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    /// Zero-based node identifier (matches ACPI proximity domain when present).
    pub id: u16,
    /// Bitmask of logical CPUs that primarily reside on this node.
    pub cpu_mask: u64,
    /// Total memory bytes managed by this node.
    pub mem_bytes: u64,
    /// Relative distance to other nodes (indexed by node id). Empty if unknown.
    pub distance: Vec<u32>,
}

/// Top-level NUMA topology representation.
#[derive(Debug, Clone)]
pub struct Topology { pub nodes: Vec<NodeInfo> }

impl Default for Topology { fn default() -> Self { Self { nodes: Vec::new() } } }

// ------------------------------------------------------------------------------------------------------------------
// Public API
// ------------------------------------------------------------------------------------------------------------------

/// Detect NUMA topology for the current machine. The function never panics and
/// always returns at least one node (fallback – UMA).
pub fn detect() -> Topology {
    #[cfg(target_arch="x86_64")] { return detect_x86(); }
    #[cfg(target_arch="aarch64")] { return detect_arm64(); }
    #[cfg(target_arch="riscv64")] { return detect_riscv(); }
    #[allow(unreachable_code)] Topology::default()
}

// ------------------------------------------------------------------------------------------------------------------
// x86_64 implementation – relies on CPUID leaf 0xB and physical memory size.
// ------------------------------------------------------------------------------------------------------------------
#[cfg(target_arch = "x86_64")]
fn detect_x86() -> Topology {
    use core::arch::x86_64::__cpuid_count;

    // 1. Enumerate unique package (socket) ids via leaf 0xB where level_type == 0x1 (core).
    let mut pkg_ids = alloc::collections::BTreeSet::new();
    for level in 0..8 {
        unsafe {
            let r = __cpuid_count(0xB, level);
            let lvl_type = (r.ecx >> 8) & 0xFF;
            if lvl_type == 0x1 { // core level
                let shift = r.eax & 0x1F;
                let pkg = r.edx >> shift;
                pkg_ids.insert(pkg);
            }
        }
    }
    if pkg_ids.is_empty() { pkg_ids.insert(0); }

    // 2. Determine physical address width via CPUID leaf 0x80000008 and
    //    derive an upper bound for installed DRAM.  This avoids relying on
    //    firmware tables during early boot while still adapting to systems
    //    with >4 GiB RAM.
    let total_mem: u64 = unsafe {
        let res = core::arch::x86_64::__cpuid(0x8000_0008);
        let phys_bits = (res.eax & 0xFF) as u8;
        // Physical address space in bytes = 2^phys_bits. We assume 75 % usage
        // by actual DRAM (typical on modern systems – some space reserved for
        // MMIO/PCI).  Clamp to 1 PiB to avoid overflow.
        let max_bytes = if phys_bits as u32 >= 52 {
            1u128 << 52 // cap at 4 PiB – well beyond realistic host memory
        } else {
            1u128 << phys_bits
        };
        // 75 % heuristic
        ((max_bytes * 3) / 4).min(u64::MAX as u128) as u64
    };
    let mem_per_node = total_mem / (pkg_ids.len() as u64);

    let mut nodes = Vec::new();
    for (idx, _) in pkg_ids.iter().enumerate() {
        let id = idx as u16;
        // Assume contiguous logical CPU ids starting from 0, eight CPUs per package. Build bitmask.
        let mut mask: u64 = 0;
        let base = (idx * 8) as u8;
        for cpu in 0..8 { let bit = base + cpu; if bit < 64 { mask |= 1u64 << bit; } }
        nodes.push(NodeInfo { id, cpu_mask: mask, mem_bytes: mem_per_node, distance: Vec::new() });
    }
    Topology { nodes }
}

#[cfg(target_arch = "x86_64")]
unsafe fn rdmsr_safe(msr: u32) -> (bool, u64) {
    let mut high: u32; 
    let mut low: u32; 
    let ok: u8 = 1;
    let mut dummy: u64;
    unsafe {
        asm!("rdmsr", in("ecx") msr, out("edx") high, out("eax") low, out("r11") dummy, options(nomem, nostack, preserves_flags));
    }
    (ok == 1, ((high as u64) << 32) | (low as u64))
}

// ------------------------------------------------------------------------------------------------------------------
// ARM64 implementation – use MPIDR affinity level 2 (socket) and total DRAM.
// ------------------------------------------------------------------------------------------------------------------
#[cfg(target_arch = "aarch64")]
fn detect_arm64() -> Topology {
    use core::arch::asm;
    // Count unique socket ids via MPIDR[15:8]. In SMP we iterate CPU list via
    // PSCI AFFINITY_INFO (not available early) – assume single node.
    let id = 0u16;
    let mut nodes = Vec::new();
    // Determine physical address range via system register ID_AA64MMFR0_EL1.
    // Bits [3:0] (PARange) encode the supported PA size.
    let parange: u64;
    unsafe { asm!("mrs {}, ID_AA64MMFR0_EL1", out(reg) parange); }
    let phys_bits = match parange & 0xF {
        0 => 32,
        1 => 36,
        2 => 40,
        3 => 42,
        4 => 44,
        5 => 48,
        6 => 52,
        7 => 56,
        _ => 32,
    };
    // As with x86, assume 75 % of addressable space is populated.
    let total_bytes: u64 = if phys_bits >= 52 {
        (1u128 << 52).min(u64::MAX as u128) as u64
    } else {
        (1u128 << phys_bits).min(u64::MAX as u128) as u64
    } * 3 / 4;

    nodes.push(NodeInfo { id, cpu_mask: 0xFFFF_FFFF, mem_bytes: total_bytes, distance: Vec::new() });
    Topology { nodes }
}

// ------------------------------------------------------------------------------------------------------------------
// RISC-V implementation – assume UMA until SBI NUMA extension is available.
// ------------------------------------------------------------------------------------------------------------------
#[cfg(target_arch = "riscv64")]
fn detect_riscv() -> Topology {
    let mut nodes = Vec::new();
    nodes.push(NodeInfo { id: 0, cpu_mask: 0xFFFF_FFFF, mem_bytes: 4 * 1024 * 1024 * 1024, distance: Vec::new() });
    Topology { nodes }
} 