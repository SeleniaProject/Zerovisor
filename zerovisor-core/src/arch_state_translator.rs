//! Cross-Architecture CPU state translation traits and stubs
//! Used by live migration layer to convert guest CPU/VMCS/VMSA state between
//! different ISAs (x86_64 ↔ ARM64 ↔ RISC-V).  The real implementation must
//! enumerate and map each register field; here we provide compile-time stubs
//! so that higher-level code can be integrated incrementally.

#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

/// Abstract serialisable CPU state blob (architecture-specific encoding).
#[derive(Debug, Clone)]
pub struct CpuStateBlob(pub Vec<u8>);

/// ID of architecture for migration negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchId { X86_64, Arm64, Riscv64 }

/// Translate CPU state from source ISA to destination ISA.
pub trait ArchStateTranslator {
    /// Return true if this translator supports given source/destination pair.
    fn supports(src: ArchId, dst: ArchId) -> bool where Self: Sized;

    /// Convert `input` blob (encoded in `src` format) into `dst` format.
    fn translate(src: ArchId, dst: ArchId, input: &CpuStateBlob) -> CpuStateBlob where Self: Sized;
}

/// Fallback identity translator – used when src == dst.
pub struct IdentityTranslator;

impl ArchStateTranslator for IdentityTranslator {
    fn supports(src: ArchId, dst: ArchId) -> bool { src == dst }
    fn translate(_s: ArchId, _d: ArchId, blob: &CpuStateBlob) -> CpuStateBlob { blob.clone() }
}

/// Simple placeholder translator that returns CPU state unchanged but marks it as converted.
pub struct DummyCrossTranslator;

impl ArchStateTranslator for DummyCrossTranslator {
    fn supports(_src: ArchId, _dst: ArchId) -> bool { true }

    fn translate(_s: ArchId, _d: ArchId, blob: &CpuStateBlob) -> CpuStateBlob {
        let mut out = blob.clone();
        // Prepend 4-byte tag to indicate dummy translation (0xCAFEBABE).
        let mut new = vec![0xCA, 0xFE, 0xBA, 0xBE];
        new.extend_from_slice(&out.0);
        CpuStateBlob(new)
    }
}

/// Cross translator between specific architectures
pub struct X86ToArmTranslator;
impl ArchStateTranslator for X86ToArmTranslator {
    fn supports(src: ArchId, dst: ArchId) -> bool {
        matches!((src, dst), (ArchId::X86_64, ArchId::Arm64) | (ArchId::Arm64, ArchId::X86_64))
    }
    fn translate(src: ArchId, dst: ArchId, input: &CpuStateBlob) -> CpuStateBlob {
        use zerovisor_hal::cpu::{CpuState, ArchSpecificState};

        // Deserialize input blob into CpuState structure.
        if input.0.len() != core::mem::size_of::<CpuState>() {
            return input.clone();
        }

        let mut in_state: CpuState = CpuState::default();
        unsafe {
            core::ptr::copy_nonoverlapping(
                input.0.as_ptr(),
                &mut in_state as *mut _ as *mut u8,
                input.0.len(),
            );
        }

        // Prepare output state – start from zero so we don’t leak architecture-specific fields.
        let mut out_state: CpuState = CpuState::default();

        // ----- General purpose register mapping -----
        // For simplicity we transfer the first 16 GPRs 1:1.  Both ISAs use 64-bit GPRs so no width conversion is required.
        // X86-64: RAX, RBX, RCX, RDX, RSI, RDI, RBP, RSP, R8-R15 → placed in X0-X15 for AArch64 .
        for i in 0..16 {
            out_state.general_registers[i] = in_state.general_registers[i];
        }

        // Map RIP to PC and RSP to SP.
        out_state.program_counter = in_state.program_counter;
        out_state.stack_pointer   = in_state.stack_pointer;

        // Preserve flags in system register slot 0 for demonstration.
        out_state.system_registers[0] = in_state.flags;

        // Architecture-specific payload.
        out_state.arch_specific = if dst == ArchId::Arm64 {
            ArchSpecificState::Arm64 {
                system_registers: [0; 128],
                vector_registers: [0; 32],
                exception_level: 1, // EL1 guest context by default
            }
        } else {
            // Reverse translation (Arm → X86) – populate minimal x86 fields.
            ArchSpecificState::X86_64 {
                msr_values: [0; 256],
                segment_registers: [0; 6],
                descriptor_tables: [0; 4],
            }
        };

        // Serialize back into a blob.
        let mut out = CpuStateBlob(vec![0u8; core::mem::size_of::<CpuState>()]);
        unsafe {
            core::ptr::copy_nonoverlapping(
                &out_state as *const _ as *const u8,
                out.0.as_mut_ptr(),
                out.0.len(),
            );
        }
        out
    }
}

pub struct X86ToRiscvTranslator;
impl ArchStateTranslator for X86ToRiscvTranslator {
    fn supports(src: ArchId, dst: ArchId) -> bool {
        matches!((src, dst), (ArchId::X86_64, ArchId::Riscv64) | (ArchId::Riscv64, ArchId::X86_64))
    }
    fn translate(src: ArchId, dst: ArchId, input: &CpuStateBlob) -> CpuStateBlob {
        use zerovisor_hal::cpu::{CpuState, ArchSpecificState};
        if input.0.len() != core::mem::size_of::<CpuState>() {
            return input.clone();
        }

        let mut in_state: CpuState = CpuState::default();
        unsafe {
            core::ptr::copy_nonoverlapping(
                input.0.as_ptr(),
                &mut in_state as *mut _ as *mut u8,
                input.0.len(),
            );
        }

        // Create output state with zero initialisation.
        let mut out_state: CpuState = CpuState::default();

        // Copy first 32 general registers directly; RISC-V x0 is hard-wired zero so we overwrite afterwards.
        out_state.general_registers.copy_from_slice(&in_state.general_registers);

        if dst == ArchId::Riscv64 {
            // Ensure x0 = 0 according to the ISA spec.
            out_state.general_registers[0] = 0;
        }

        out_state.program_counter = in_state.program_counter;
        out_state.stack_pointer   = in_state.stack_pointer;

        out_state.arch_specific = if dst == ArchId::Riscv64 {
            ArchSpecificState::RiscV {
                csr_registers: [0; 4096],
                privilege_level: 1,
                extension_state: [0; 32],
            }
        } else {
            ArchSpecificState::X86_64 {
                msr_values: [0; 256],
                segment_registers: [0; 6],
                descriptor_tables: [0; 4],
            }
        };

        let mut out = CpuStateBlob(vec![0u8; core::mem::size_of::<CpuState>()]);
        unsafe {
            core::ptr::copy_nonoverlapping(
                &out_state as *const _ as *const u8,
                out.0.as_mut_ptr(),
                out.0.len(),
            );
        }
        out
    }
}

pub struct ArmToRiscvTranslator;
impl ArchStateTranslator for ArmToRiscvTranslator {
    fn supports(src: ArchId, dst: ArchId) -> bool {
        matches!((src, dst), (ArchId::Arm64, ArchId::Riscv64) | (ArchId::Riscv64, ArchId::Arm64))
    }
    fn translate(src: ArchId, dst: ArchId, input: &CpuStateBlob) -> CpuStateBlob {
        use zerovisor_hal::cpu::{CpuState, ArchSpecificState};
        if input.0.len() != core::mem::size_of::<CpuState>() {
            return input.clone();
        }

        let mut in_state: CpuState = CpuState::default();
        unsafe {
            core::ptr::copy_nonoverlapping(
                input.0.as_ptr(),
                &mut in_state as *mut _ as *mut u8,
                input.0.len(),
            );
        }

        // Create output state
        let mut out_state: CpuState = CpuState::default();
        out_state.general_registers.copy_from_slice(&in_state.general_registers);
        out_state.program_counter = in_state.program_counter;
        out_state.stack_pointer   = in_state.stack_pointer;

        out_state.arch_specific = if dst == ArchId::Riscv64 {
            ArchSpecificState::RiscV {
                csr_registers: [0; 4096],
                privilege_level: 1,
                extension_state: [0; 32],
            }
        } else {
            ArchSpecificState::Arm64 {
                system_registers: [0; 128],
                vector_registers: [0; 32],
                exception_level: 1,
            }
        };

        let mut out = CpuStateBlob(vec![0u8; core::mem::size_of::<CpuState>()]);
        unsafe {
            core::ptr::copy_nonoverlapping(
                &out_state as *const _ as *const u8,
                out.0.as_mut_ptr(),
                out.0.len(),
            );
        }
        out
    }
}

/// Attempt translation using available translators.
pub fn translate_arch(src: ArchId, dst: ArchId, input: &CpuStateBlob) -> CpuStateBlob {
    if IdentityTranslator::supports(src, dst) {
        return IdentityTranslator::translate(src, dst, input);
    }
    if X86ToArmTranslator::supports(src, dst) {
        return X86ToArmTranslator::translate(src, dst, input);
    }
    if X86ToRiscvTranslator::supports(src, dst) {
        return X86ToRiscvTranslator::translate(src, dst, input);
    }
    if ArmToRiscvTranslator::supports(src, dst) {
        return ArmToRiscvTranslator::translate(src, dst, input);
    }
    DummyCrossTranslator::translate(src, dst, input)
} 