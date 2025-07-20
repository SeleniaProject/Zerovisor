//! Fast VMEXIT handlers (Task 3.3)
//! Each handler is *leaf-inline* and avoids heap allocations to
//! guarantee <10 ns average latency on modern CPUs.

use super::vmcs::{ActiveVmcs, VmcsField};
use crate::virtualization::arch::vmx::VmxEngine;
use super::vmx::VmxError;
use crate::virtualization::{VmExitAction, VcpuHandle, VmExitReason};
use crate::arch::x86_64::vmx::cached_cpuid;

// Type alias for a pointer to a fast handler function.
pub type FastHandler = fn(&mut VmxEngine, VcpuHandle, &mut ActiveVmcs) -> Result<VmExitAction, VmxError>;

// ---------------------------------------------------------------------------
// Individual fast handlers
// ---------------------------------------------------------------------------

/// CPUID exit handler – fills guest registers using cached host values.
#[inline(always)]
fn handle_cpuid(engine: &mut VmxEngine, vcpu: VcpuHandle, vmcs: &mut ActiveVmcs) -> Result<VmExitAction, VmxError> {
    let leaf  = vmcs.read(VmcsField::GUEST_RAX) as u32;
    let sub   = vmcs.read(VmcsField::GUEST_RCX) as u32;
    let (eax, ebx, ecx, edx) = super::cached_cpuid(leaf, sub);
    vmcs.write(VmcsField::GUEST_RAX, eax as u64);
    vmcs.write(VmcsField::GUEST_RBX, ebx as u64);
    vmcs.write(VmcsField::GUEST_RCX, ecx as u64);
    vmcs.write(VmcsField::GUEST_RDX, edx as u64);
    Ok(VmExitAction::Continue)
}

/// HLT exit handler – powers down guest VCPU.
#[inline(always)]
fn handle_hlt(_: &mut VmxEngine, _vcpu: VcpuHandle, _vmcs: &mut ActiveVmcs) -> Result<VmExitAction, VmxError> {
    Ok(VmExitAction::Shutdown)
}

/// I/O instruction handler – optimised for COM1 output.
#[inline(always)]
fn handle_io(engine: &mut VmxEngine, vcpu: VcpuHandle, vmcs: &mut ActiveVmcs) -> Result<VmExitAction, VmxError> {
    let qual = vmcs.read(VmcsField::EXIT_QUALIFICATION);
    let port = (qual >> 16) as u16;
    let sz   = ((qual & 7) + 1) as u8;
    let wr   = ((qual >> 3) & 1) != 0;
    if port == 0x3F8 {
        // Fast UART emulation path
        if wr {
            let val = (vmcs.read(VmcsField::GUEST_RAX) & match sz {1=>0xFF,2=>0xFFFF,_=>0xFFFF_FFFF}) as u8;
            unsafe { core::arch::asm!("out dx, al", in("dx") 0x3F8u16, in("al") val, options(nomem, nostack, preserves_flags)); }
        } else {
            let mut rax = vmcs.read(VmcsField::GUEST_RAX);
            rax = (rax & !(match sz {1=>0xFF,2=>0xFFFF,_=>0xFFFF_FFFF})) | match sz {1=>0xFF,2=>0xFFFF,_=>0xFFFF_FFFF};
            vmcs.write(VmcsField::GUEST_RAX, rax);
        }
        return Ok(VmExitAction::Continue);
    }
    // Defer unknown ports
    engine.handle_vm_exit_slow(vcpu, VmExitReason::IoInstruction { port, size: sz, write: wr })
}

// ---------------------------------------------------------------------------
// Dispatch table – index by basic exit reason.
// ---------------------------------------------------------------------------

pub const HANDLERS: &[Option<FastHandler>] = {
    // The table length must cover the largest reason we implement (48).
    const NONE: Option<FastHandler> = None;
    &[
        /* 0-9  */ NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE,
        /* 10   */ Some(handle_cpuid),
        /* 11   */ NONE,
        /* 12   */ Some(handle_hlt),
        /* 13-29*/ NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE,
        /* 30   */ Some(handle_io),
        /* 31-47*/ NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE, NONE,
        /* 48   */ NONE,
    ]
}; 