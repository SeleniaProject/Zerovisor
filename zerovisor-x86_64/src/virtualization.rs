//! x86_64 virtualization implementation (VMX and SVM)
extern crate alloc;
use alloc::collections::BTreeMap;

use zerovisor_hal::virtualization::{VirtualizationEngine, VmHandle, VcpuHandle, VmConfig, VcpuConfig, VmExitReason, VmExitAction};
use zerovisor_hal::cpu::CpuState;
use zerovisor_hal::memory::{PhysicalAddress, MemoryFlags};
use zerovisor_hal::arch::x86_64::ept_manager::EptHierarchy;
use zerovisor_hal::arch::x86_64::ept::EptFlags;
use crate::X86Error;
use crate::memory::{X86MemoryManager, phys_to_virt};
use crate::cpu::VirtualizationType;
use crate::svm_engine::SvmEngine;
use spin::Once;

static MEMORY: Once<spin::Mutex<X86MemoryManager>> = Once::new();

/// x86_64 VMX virtualization engine
pub struct VmxEngine {
    vmxon_region: Option<PhysicalAddress>,
    next_vm_id: VmHandle,
    next_vcpu_id: VcpuHandle,
    epts: BTreeMap<VmHandle, &'static mut EptHierarchy>,
}

impl VirtualizationEngine for VmxEngine {
    type Error = X86Error;
    
    fn init() -> Result<Self, Self::Error> {
        Ok(Self {
            vmxon_region: None,
            next_vm_id: 1,
            next_vcpu_id: 1,
            epts: BTreeMap::new(),
        })
    }
    
    fn is_supported() -> bool {
        // Check for VMX support in CPUID
        let cpuid = raw_cpuid::CpuId::new();
        if let Some(feature_info) = cpuid.get_feature_info() {
            feature_info.has_vmx()
        } else {
            false
        }
    }
    
    fn enable(&mut self) -> Result<(), Self::Error> {
        if !Self::is_supported() {
            return Err(X86Error::VmxNotSupported);
        }
        
        // Allocate VMXON region
        let vmxon_region = allocate_vmxon_region()?;
        
        // Execute VMXON instruction
        unsafe {
            vmxon(vmxon_region)?;
        }
        
        self.vmxon_region = Some(vmxon_region);
        Ok(())
    }
    
    fn disable(&mut self) -> Result<(), Self::Error> {
        if self.vmxon_region.is_some() {
            unsafe {
                vmxoff()?;
            }
            self.vmxon_region = None;
        }
        Ok(())
    }
    
    fn create_vm(&mut self, _config: &VmConfig) -> Result<VmHandle, Self::Error> {
        let vm_id = self.next_vm_id;
        self.next_vm_id += 1;

        // Allocate 32 MiB contiguous landing region for the VM
        let mgr = MEMORY.call_once(|| spin::Mutex::new(X86MemoryManager::init().map_err(|_| X86Error::MemoryInitFailed).unwrap()));
        let mut guard = mgr.lock();
        let phys = guard.allocate_physical(32*1024*1024, 2*1024*1024)?;
        let virt = phys_to_virt(phys);

        crate::log!("[x86_64] VM{} landing region phys={:#x} virt={:#x}", vm_id, phys, virt);

        // Create EPT hierarchy and identity-map landing region (32 MiB, RWX)
        let mut ept = EptHierarchy::new().map_err(|_| X86Error::MemoryError)?;
        ept.map(0, phys, 32 * 1024 * 1024, EptFlags::READ | EptFlags::WRITE | EptFlags::EXEC)
            .map_err(|_| X86Error::MemoryError)?;

        // Leak the hierarchy to attain 'static lifetime and store it for later use
        let boxed = Box::new(ept);
        self.epts.insert(vm_id, Box::leak(boxed));

        Ok(vm_id)
    }
    
    fn destroy_vm(&mut self, _vm: VmHandle) -> Result<(), Self::Error> {
        // Would clean up VM-specific structures
        Ok(())
    }
    
    fn create_vcpu(&mut self, _vm: VmHandle, _config: &VcpuConfig) -> Result<VcpuHandle, Self::Error> {
        let vcpu_id = self.next_vcpu_id;
        self.next_vcpu_id += 1;
        
        // Would allocate and initialize VMCS
        
        Ok(vcpu_id)
    }
    
    fn run_vcpu(&mut self, _vcpu: VcpuHandle) -> Result<VmExitReason, Self::Error> {
        // Would execute VMLAUNCH/VMRESUME and handle VM exit
        Ok(VmExitReason::Hlt)
    }
    
    fn get_vcpu_state(&self, _vcpu: VcpuHandle) -> Result<CpuState, Self::Error> {
        // Would read VMCS fields to construct CPU state
        Ok(CpuState::default())
    }
    
    fn set_vcpu_state(&mut self, _vcpu: VcpuHandle, _state: &CpuState) -> Result<(), Self::Error> {
        // Would write CPU state to VMCS fields
        Ok(())
    }
    
    fn handle_vm_exit(&mut self, _vcpu: VcpuHandle, reason: VmExitReason) -> Result<VmExitAction, Self::Error> {
        match reason {
            VmExitReason::Hlt => Ok(VmExitAction::Continue),
            VmExitReason::IoInstruction { .. } => Ok(VmExitAction::Emulate),
            VmExitReason::Cpuid { .. } => Ok(VmExitAction::Emulate),
            _ => Ok(VmExitAction::Continue),
        }
    }
    
    fn setup_nested_paging(&mut self, _vm: VmHandle) -> Result<(), Self::Error> {
        // Would set up EPT (Extended Page Tables)
        Ok(())
    }
    
    fn map_guest_memory(&mut self, vm: VmHandle, guest_phys: PhysicalAddress, 
                       host_phys: PhysicalAddress, size: usize, _flags: MemoryFlags) -> Result<(), Self::Error> {
        if let Some(ept) = self.epts.get_mut(&vm) {
            use zerovisor_hal::arch::x86_64::ept::EptFlags as F;
            ept.map(guest_phys as u64, host_phys as u64, size as u64, F::READ | F::WRITE | F::EXEC)
                .map_err(|_| X86Error::MemoryError)
        } else {
            Err(X86Error::MemoryError)
        }
    }
    
    fn unmap_guest_memory(&mut self, vm: VmHandle, guest_phys: PhysicalAddress, size: usize) -> Result<(), Self::Error> {
        if let Some(ept) = self.epts.get_mut(&vm) {
            ept.unmap(guest_phys as u64, size as u64).map_err(|_| X86Error::MemoryError)
        } else {
            Err(X86Error::MemoryError)
        }
    }
}

/// Allocate VMXON region
fn allocate_vmxon_region() -> Result<PhysicalAddress, X86Error> {
    // Would allocate 4KB aligned physical memory
    Ok(0x200000) // Placeholder
}

/// Execute VMXON instruction
unsafe fn vmxon(_vmxon_region: PhysicalAddress) -> Result<(), X86Error> {
    // Would execute actual VMXON instruction
    Ok(())
}

/// Execute VMXOFF instruction
unsafe fn vmxoff() -> Result<(), X86Error> {
    // Would execute actual VMXOFF instruction
    Ok(())
}

/// Initialize x86_64 virtualization
pub fn init() -> Result<(), X86Error> {
    // Initialize the appropriate virtualization engine
    let _engine = X86VirtualizationEngine::init()?;
    Ok(())
}/// Unif
ied x86_64 virtualization engine that supports both VMX and SVM
pub enum X86VirtualizationEngine {
    Vmx(VmxEngine),
    Svm(SvmEngine),
}

impl VirtualizationEngine for X86VirtualizationEngine {
    type Error = X86Error;
    
    fn init() -> Result<Self, Self::Error> {
        // Detect which virtualization technology is available
        let cpuid = raw_cpuid::CpuId::new();
        
        // Check for Intel VMX first
        if let Some(feature_info) = cpuid.get_feature_info() {
            if feature_info.has_vmx() {
                return Ok(Self::Vmx(VmxEngine::init()?));
            }
        }
        
        // Check for AMD SVM
        if let Some(extended_info) = cpuid.get_extended_function_info() {
            if extended_info.has_svm() {
                return Ok(Self::Svm(SvmEngine::init()?));
            }
        }
        
        Err(X86Error::UnsupportedCpu)
    }
    
    fn is_supported() -> bool {
        VmxEngine::is_supported() || SvmEngine::is_supported()
    }
    
    fn enable(&mut self) -> Result<(), Self::Error> {
        match self {
            Self::Vmx(engine) => engine.enable(),
            Self::Svm(engine) => engine.enable(),
        }
    }
    
    fn disable(&mut self) -> Result<(), Self::Error> {
        match self {
            Self::Vmx(engine) => engine.disable(),
            Self::Svm(engine) => engine.disable(),
        }
    }
    
    fn create_vm(&mut self, config: &VmConfig) -> Result<VmHandle, Self::Error> {
        match self {
            Self::Vmx(engine) => engine.create_vm(config),
            Self::Svm(engine) => engine.create_vm(config),
        }
    }
    
    fn destroy_vm(&mut self, vm: VmHandle) -> Result<(), Self::Error> {
        match self {
            Self::Vmx(engine) => engine.destroy_vm(vm),
            Self::Svm(engine) => engine.destroy_vm(vm),
        }
    }
    
    fn create_vcpu(&mut self, vm: VmHandle, config: &VcpuConfig) -> Result<VcpuHandle, Self::Error> {
        match self {
            Self::Vmx(engine) => engine.create_vcpu(vm, config),
            Self::Svm(engine) => engine.create_vcpu(vm, config),
        }
    }
    
    fn run_vcpu(&mut self, vcpu: VcpuHandle) -> Result<VmExitReason, Self::Error> {
        match self {
            Self::Vmx(engine) => engine.run_vcpu(vcpu),
            Self::Svm(engine) => engine.run_vcpu(vcpu),
        }
    }
    
    fn get_vcpu_state(&self, vcpu: VcpuHandle) -> Result<CpuState, Self::Error> {
        match self {
            Self::Vmx(engine) => engine.get_vcpu_state(vcpu),
            Self::Svm(engine) => engine.get_vcpu_state(vcpu),
        }
    }
    
    fn set_vcpu_state(&mut self, vcpu: VcpuHandle, state: &CpuState) -> Result<(), Self::Error> {
        match self {
            Self::Vmx(engine) => engine.set_vcpu_state(vcpu, state),
            Self::Svm(engine) => engine.set_vcpu_state(vcpu, state),
        }
    }
    
    fn handle_vm_exit(&mut self, vcpu: VcpuHandle, reason: VmExitReason) -> Result<VmExitAction, Self::Error> {
        match self {
            Self::Vmx(engine) => engine.handle_vm_exit(vcpu, reason),
            Self::Svm(engine) => engine.handle_vm_exit(vcpu, reason),
        }
    }
    
    fn setup_nested_paging(&mut self, vm: VmHandle) -> Result<(), Self::Error> {
        match self {
            Self::Vmx(engine) => engine.setup_nested_paging(vm),
            Self::Svm(engine) => engine.setup_nested_paging(vm),
        }
    }
    
    fn map_guest_memory(&mut self, vm: VmHandle, guest_phys: PhysicalAddress, 
                       host_phys: PhysicalAddress, size: usize, flags: MemoryFlags) -> Result<(), Self::Error> {
        match self {
            Self::Vmx(engine) => engine.map_guest_memory(vm, guest_phys, host_phys, size, flags),
            Self::Svm(engine) => engine.map_guest_memory(vm, guest_phys, host_phys, size, flags),
        }
    }
    
    fn unmap_guest_memory(&mut self, vm: VmHandle, guest_phys: PhysicalAddress, size: usize) -> Result<(), Self::Error> {
        match self {
            Self::Vmx(engine) => engine.unmap_guest_memory(vm, guest_phys, size),
            Self::Svm(engine) => engine.unmap_guest_memory(vm, guest_phys, size),
        }
    }
}

impl X86VirtualizationEngine {
    /// Get the virtualization technology being used
    pub fn virtualization_type(&self) -> VirtualizationType {
        match self {
            Self::Vmx(_) => VirtualizationType::Vmx,
            Self::Svm(_) => VirtualizationType::Svm,
        }
    }
}