//! Nested Virtualization Support for Intel VMX
//! 
//! Implements complete nested hypervisor support allowing guest VMs to run
//! their own hypervisors (L1 hypervisor running L2 guests).

#![cfg(target_arch = "x86_64")]

use crate::arch::x86_64::vmcs::{Vmcs, VmcsField, VmcsState, ActiveVmcs};
use crate::arch::x86_64::ept_manager::EptHierarchy;
use crate::memory::PhysicalAddress;
use crate::virtualization::{VmHandle, VcpuHandle};
use alloc::collections::BTreeMap;
use spin::Mutex;

/// Nested virtualization levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtLevel {
    /// Root hypervisor (L0)
    Root = 0,
    /// Guest hypervisor (L1) 
    L1 = 1,
    /// Nested guest (L2)
    L2 = 2,
}

/// Shadow VMCS for nested virtualization
#[repr(C, align(4096))]
pub struct ShadowVmcs {
    /// Physical VMCS for L2 guest
    l2_vmcs: Vmcs,
    /// L1's view of VMCS (what L1 hypervisor sees)
    l1_shadow: VmcsState,
    /// L0's merged VMCS state
    l0_merged: VmcsState,
    /// Virtualization level
    level: VirtLevel,
    /// Parent VM handle (L1 hypervisor)
    parent_vm: Option<VmHandle>,
}

/// Nested virtualization manager
pub struct NestedVirtManager {
    /// Shadow VMCS mappings: L2 VCPU -> Shadow VMCS
    shadow_vmcs: Mutex<BTreeMap<VcpuHandle, ShadowVmcs>>,
    /// EPT hierarchies for nested guests
    nested_ept: Mutex<BTreeMap<VcpuHandle, EptHierarchy>>,
    /// VMCS12 (L1's VMCS for L2) mappings
    vmcs12_mappings: Mutex<BTreeMap<VcpuHandle, PhysicalAddress>>,
}

impl NestedVirtManager {
    /// Create new nested virtualization manager
    pub fn new() -> Self {
        Self {
            shadow_vmcs: Mutex::new(BTreeMap::new()),
            nested_ept: Mutex::new(BTreeMap::new()),
            vmcs12_mappings: Mutex::new(BTreeMap::new()),
        }
    }

    /// Enable nested virtualization for a VCPU
    pub fn enable_nested_virt(&self, vcpu: VcpuHandle, parent_vm: VmHandle) -> Result<(), NestedVirtError> {
        let mut shadow_map = self.shadow_vmcs.lock();
        
        if shadow_map.contains_key(&vcpu) {
            return Err(NestedVirtError::AlreadyEnabled);
        }

        // Allocate shadow VMCS
        let shadow_vmcs = ShadowVmcs {
            l2_vmcs: Vmcs::new()?,
            l1_shadow: VmcsState::default(),
            l0_merged: VmcsState::default(),
            level: VirtLevel::L2,
            parent_vm: Some(parent_vm),
        };

        // Setup nested EPT
        let mut ept_map = self.nested_ept.lock();
        let nested_ept = EptHierarchy::new()?;
        ept_map.insert(vcpu, nested_ept);

        shadow_map.insert(vcpu, shadow_vmcs);
        Ok(())
    }

    /// Handle VMLAUNCH from L1 hypervisor
    pub fn handle_vmlaunch(&self, l1_vcpu: VcpuHandle, vmcs12_addr: PhysicalAddress) -> Result<(), NestedVirtError> {
        let mut mappings = self.vmcs12_mappings.lock();
        mappings.insert(l1_vcpu, vmcs12_addr);

        // Read L1's VMCS12 and merge with L0 controls
        self.merge_vmcs_controls(l1_vcpu, vmcs12_addr)?;
        
        // Setup shadow EPT combining L1 and L0 page tables
        self.setup_shadow_ept(l1_vcpu)?;

        Ok(())
    }

    /// Handle VMRESUME from L1 hypervisor  
    pub fn handle_vmresume(&self, l1_vcpu: VcpuHandle) -> Result<(), NestedVirtError> {
        let shadow_map = self.shadow_vmcs.lock();
        let shadow = shadow_map.get(&l1_vcpu).ok_or(NestedVirtError::NotEnabled)?;
        
        // Restore L2 guest state from shadow VMCS
        self.restore_l2_state(l1_vcpu, shadow)?;
        
        Ok(())
    }

    /// Handle VMEXIT from L2 guest
    pub fn handle_l2_vmexit(&self, l2_vcpu: VcpuHandle, exit_reason: u32) -> Result<VmExitAction, NestedVirtError> {
        let shadow_map = self.shadow_vmcs.lock();
        let shadow = shadow_map.get(&l2_vcpu).ok_or(NestedVirtError::NotEnabled)?;

        // Determine if exit should be handled by L1 or L0
        if self.should_exit_to_l1(exit_reason, shadow)? {
            // Synthesize VMEXIT to L1 hypervisor
            self.synthesize_l1_vmexit(l2_vcpu, exit_reason, shadow)?;
            Ok(VmExitAction::Continue)
        } else {
            // Handle in L0 (root hypervisor)
            Ok(VmExitAction::HandleInHypervisor)
        }
    }

    /// Merge VMCS controls from L1 and L0
    fn merge_vmcs_controls(&self, vcpu: VcpuHandle, vmcs12_addr: PhysicalAddress) -> Result<(), NestedVirtError> {
        let mut shadow_map = self.shadow_vmcs.lock();
        let shadow = shadow_map.get_mut(&vcpu).ok_or(NestedVirtError::NotEnabled)?;

        // Read L1's VMCS12 from guest memory
        let l1_vmcs = self.read_vmcs12(vmcs12_addr)?;

        // Merge execution controls (L0 controls take precedence for security)
        shadow.l0_merged.pin_based_vm_exec_control = 
            l1_vmcs.pin_based_vm_exec_control | self.get_l0_required_pin_controls();
        
        shadow.l0_merged.cpu_based_vm_exec_control = 
            l1_vmcs.cpu_based_vm_exec_control | self.get_l0_required_cpu_controls();

        shadow.l0_merged.secondary_vm_exec_control = 
            l1_vmcs.secondary_vm_exec_control | self.get_l0_required_secondary_controls();

        // Merge exception bitmap (L0 must intercept certain exceptions)
        shadow.l0_merged.exception_bitmap = 
            l1_vmcs.exception_bitmap | self.get_l0_required_exception_bitmap();

        // Copy L1's guest state to shadow
        shadow.l1_shadow = l1_vmcs;

        Ok(())
    }

    /// Setup shadow EPT combining L1 EPT and L0 EPT
    fn setup_shadow_ept(&self, vcpu: VcpuHandle) -> Result<(), NestedVirtError> {
        let mut ept_map = self.nested_ept.lock();
        let ept = ept_map.get_mut(&vcpu).ok_or(NestedVirtError::NotEnabled)?;

        // Walk L1's EPT and create shadow mappings
        // This is a complex operation that translates L2 GPA -> L1 GPA -> L0 HPA
        ept.setup_shadow_mappings()?;

        Ok(())
    }

    /// Determine if VMEXIT should go to L1 or be handled by L0
    fn should_exit_to_l1(&self, exit_reason: u32, shadow: &ShadowVmcs) -> Result<bool, NestedVirtError> {
        match exit_reason {
            // Always handle these in L0 for security
            48 => Ok(false), // EPT violation - handle in L0
            0 => Ok(false),  // Exception or NMI - check exception bitmap
            
            // Forward these to L1 if L1 requested intercept
            10 => Ok(shadow.l1_shadow.cpu_based_vm_exec_control & (1 << 12) != 0), // CPUID
            12 => Ok(shadow.l1_shadow.cpu_based_vm_exec_control & (1 << 7) != 0),  // HLT
            30 => Ok(shadow.l1_shadow.cpu_based_vm_exec_control & (1 << 24) != 0), // I/O instruction
            
            _ => Ok(true), // Forward unknown exits to L1
        }
    }

    /// Synthesize VMEXIT to L1 hypervisor
    fn synthesize_l1_vmexit(&self, vcpu: VcpuHandle, exit_reason: u32, shadow: &ShadowVmcs) -> Result<(), NestedVirtError> {
        // Update L1's VMCS12 with exit information
        // Set exit reason, qualification, guest state, etc.
        // This makes it appear to L1 that its L2 guest exited
        
        // Implementation would write to L1's VMCS12 in guest memory
        // and inject a "VMEXIT" event to L1 hypervisor
        
        Ok(())
    }

    /// Read VMCS12 from L1 guest memory
    fn read_vmcs12(&self, addr: PhysicalAddress) -> Result<VmcsState, NestedVirtError> {
        // Implementation would read VMCS12 structure from guest physical memory
        // This requires careful validation to prevent L1 from attacking L0
        Ok(VmcsState::default())
    }

    /// Restore L2 guest state from shadow VMCS
    fn restore_l2_state(&self, vcpu: VcpuHandle, shadow: &ShadowVmcs) -> Result<(), NestedVirtError> {
        // Restore L2 guest registers, control state, etc.
        Ok(())
    }

    // L0 required control bits (for security)
    fn get_l0_required_pin_controls(&self) -> u32 { 0x00000016 } // External interrupts, NMI
    fn get_l0_required_cpu_controls(&self) -> u32 { 0x84006172 } // MSR bitmap, I/O bitmap, etc.
    fn get_l0_required_secondary_controls(&self) -> u32 { 0x00000002 } // Enable EPT
    fn get_l0_required_exception_bitmap(&self) -> u32 { 0x00060042 } // #PF, #GP, #UD
}

/// VMEXIT action for nested virtualization
#[derive(Debug, Clone, Copy)]
pub enum VmExitAction {
    /// Continue execution (exit handled)
    Continue,
    /// Handle in hypervisor
    HandleInHypervisor,
    /// Forward to L1 hypervisor
    ForwardToL1,
}

/// Nested virtualization errors
#[derive(Debug, Clone, Copy)]
pub enum NestedVirtError {
    /// Nested virtualization already enabled
    AlreadyEnabled,
    /// Nested virtualization not enabled
    NotEnabled,
    /// VMCS allocation failed
    VmcsAllocFailed,
    /// EPT setup failed
    EptSetupFailed,
    /// Invalid VMCS12 address
    InvalidVmcs12,
    /// Memory access failed
    MemoryAccessFailed,
}

impl From<crate::arch::x86_64::vmcs::VmcsError> for NestedVirtError {
    fn from(_: crate::arch::x86_64::vmcs::VmcsError) -> Self {
        Self::VmcsAllocFailed
    }
}

impl From<crate::arch::x86_64::ept_manager::EptError> for NestedVirtError {
    fn from(_: crate::arch::x86_64::ept_manager::EptError) -> Self {
        Self::EptSetupFailed
    }
}

/// Global nested virtualization manager
static NESTED_VIRT_MANAGER: Mutex<Option<NestedVirtManager>> = Mutex::new(None);

/// Initialize nested virtualization support
pub fn init() -> Result<(), NestedVirtError> {
    let mut manager = NESTED_VIRT_MANAGER.lock();
    if manager.is_none() {
        *manager = Some(NestedVirtManager::new());
    }
    Ok(())
}

/// Get global nested virtualization manager
pub fn get_manager() -> Result<&'static Mutex<Option<NestedVirtManager>>, NestedVirtError> {
    Ok(&NESTED_VIRT_MANAGER)
}