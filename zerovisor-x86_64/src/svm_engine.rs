//! AMD SVM virtualization engine implementation
extern crate alloc;
use alloc::collections::BTreeMap;

use zerovisor_hal::virtualization::{VirtualizationEngine, VmHandle, VcpuHandle, VmConfig, VcpuConfig, VmExitReason, VmExitAction};
use zerovisor_hal::cpu::CpuState;
use zerovisor_hal::memory::{PhysicalAddress, MemoryFlags};
use zerovisor_hal::arch::x86_64::svm::{Vmcb, VmcbState, VmcbError, SvmExitCode, vmrun, vmload, vmsave};
use crate::X86Error;
use crate::memory::{X86MemoryManager, phys_to_virt};
use spin::Once;

static MEMORY: Once<spin::Mutex<X86MemoryManager>> = Once::new();

/// AMD SVM virtualization engine
pub struct SvmEngine {
    host_save_area: Option<PhysicalAddress>,
    next_vm_id: VmHandle,
    next_vcpu_id: VcpuHandle,
    vmcbs: BTreeMap<VcpuHandle, Vmcb>,
    nested_page_tables: BTreeMap<VmHandle, PhysicalAddress>,
}

impl VirtualizationEngine for SvmEngine {
    type Error = X86Error;
    
    fn init() -> Result<Self, Self::Error> {
        Ok(Self {
            host_save_area: None,
            next_vm_id: 1,
            next_vcpu_id: 1,
            vmcbs: BTreeMap::new(),
            nested_page_tables: BTreeMap::new(),
        })
    }
    
    fn is_supported() -> bool {
        // Check for SVM support in CPUID
        let cpuid = raw_cpuid::CpuId::new();
        if let Some(extended_info) = cpuid.get_extended_function_info() {
            extended_info.has_svm()
        } else {
            false
        }
    }
    
    fn enable(&mut self) -> Result<(), Self::Error> {
        if !Self::is_supported() {
            return Err(X86Error::SvmNotSupported);
        }
        
        // Allocate host save area (4KB aligned)
        let host_save_area = allocate_host_save_area()?;
        
        // Set VM_HSAVE_PA MSR to point to host save area
        unsafe {
            set_vm_hsave_pa(host_save_area)?;
        }
        
        self.host_save_area = Some(host_save_area);
        Ok(())
    }
    
    fn disable(&mut self) -> Result<(), Self::Error> {
        if self.host_save_area.is_some() {
            // Clear VM_HSAVE_PA MSR
            unsafe {
                set_vm_hsave_pa(0)?;
            }
            self.host_save_area = None;
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

        crate::log!("[SVM] VM{} landing region phys={:#x} virt={:#x}", vm_id, phys, virt);

        // Create nested page table for this VM
        let npt_root = allocate_nested_page_table()?;
        self.nested_page_tables.insert(vm_id, npt_root);

        // Identity-map the landing region in nested page table
        setup_identity_mapping(npt_root, 0, phys, 32 * 1024 * 1024)?;

        Ok(vm_id)
    }
    
    fn destroy_vm(&mut self, vm: VmHandle) -> Result<(), Self::Error> {
        // Clean up nested page table
        if let Some(_npt_root) = self.nested_page_tables.remove(&vm) {
            // Would free nested page table structures
        }
        Ok(())
    }
    
    fn create_vcpu(&mut self, vm: VmHandle, _config: &VcpuConfig) -> Result<VcpuHandle, Self::Error> {
        let vcpu_id = self.next_vcpu_id;
        self.next_vcpu_id += 1;
        
        // Allocate VMCB (4KB aligned)
        let vmcb_pa = allocate_vmcb()?;
        let vmcb = Vmcb::new(vmcb_pa);
        
        // Initialize VMCB with default state
        let mut active_vmcb = vmcb.load().map_err(|_| X86Error::MemoryError)?;
        let mut vmcb_state = VmcbState::default();
        
        // Set up nested paging if available
        if let Some(&npt_root) = self.nested_page_tables.get(&vm) {
            vmcb_state.np_enable = 1;
            vmcb_state.n_cr3 = npt_root;
        }
        
        // Configure basic intercepts
        vmcb_state.intercept_instr1 = 
            (1 << 18) | // HLT
            (1 << 19) | // INVLPG
            (1 << 20) | // INVLPGA
            (1 << 21) | // IOIO
            (1 << 22) | // MSR
            (1 << 23) | // TASK_SWITCH
            (1 << 27) | // CPUID
            (1 << 28);  // RSM
            
        active_vmcb.load_state(&vmcb_state);
        
        self.vmcbs.insert(vcpu_id, vmcb);
        
        Ok(vcpu_id)
    }
    
    fn run_vcpu(&mut self, vcpu: VcpuHandle) -> Result<VmExitReason, Self::Error> {
        if let Some(vmcb) = self.vmcbs.get(&vcpu) {
            unsafe {
                // Execute VMRUN
                match vmrun(vmcb.phys_addr()) {
                    Ok(()) => {
                        // VM exited, read exit code from VMCB
                        let active_vmcb = vmcb.load().map_err(|_| X86Error::MemoryError)?;
                        let exit_code = active_vmcb.read_u64(zerovisor_hal::arch::x86_64::svm::vmcb_offsets::EXITCODE);
                        let exit_info1 = active_vmcb.read_u64(zerovisor_hal::arch::x86_64::svm::vmcb_offsets::EXITINFO1);
                        let exit_info2 = active_vmcb.read_u64(zerovisor_hal::arch::x86_64::svm::vmcb_offsets::EXITINFO2);
                        
                        // Convert SVM exit code to generic VM exit reason
                        match SvmExitCode::from(exit_code) {
                            SvmExitCode::HLT => Ok(VmExitReason::Hlt),
                            SvmExitCode::IOIO => Ok(VmExitReason::IoInstruction { 
                                port: (exit_info1 & 0xFFFF) as u16,
                                size: ((exit_info1 >> 4) & 0x7) as u8,
                                is_write: (exit_info1 & 1) != 0,
                            }),
                            SvmExitCode::CPUID => Ok(VmExitReason::Cpuid { 
                                leaf: exit_info1 as u32,
                                subleaf: exit_info2 as u32,
                            }),
                            SvmExitCode::MSR => Ok(VmExitReason::MsrAccess { 
                                msr: exit_info1 as u32,
                                is_write: (exit_info1 & 1) != 0,
                            }),
                            SvmExitCode::NPF => Ok(VmExitReason::MemoryAccess { 
                                guest_physical: exit_info2,
                                is_write: (exit_info1 & 2) != 0,
                                is_execute: (exit_info1 & 4) != 0,
                            }),
                            _ => Ok(VmExitReason::Unknown { exit_code }),
                        }
                    },
                    Err(_) => Err(X86Error::SvmNotSupported),
                }
            }
        } else {
            Err(X86Error::InvalidCpuid)
        }
    }   
 fn get_vcpu_state(&self, vcpu: VcpuHandle) -> Result<CpuState, Self::Error> {
        if let Some(vmcb) = self.vmcbs.get(&vcpu) {
            let active_vmcb = vmcb.load().map_err(|_| X86Error::MemoryError)?;
            let mut vmcb_state = VmcbState::default();
            active_vmcb.save_state(&mut vmcb_state);
            
            // Convert VMCB state to generic CPU state
            let mut cpu_state = CpuState::default();
            cpu_state.control_registers[0] = vmcb_state.guest_cr0;
            cpu_state.control_registers[2] = vmcb_state.guest_cr2;
            cpu_state.control_registers[3] = vmcb_state.guest_cr3;
            cpu_state.control_registers[4] = vmcb_state.guest_cr4;
            
            // Set general registers (simplified)
            cpu_state.general_registers[0] = vmcb_state.guest_rax; // RAX
            
            Ok(cpu_state)
        } else {
            Err(X86Error::InvalidCpuid)
        }
    }
    
    fn set_vcpu_state(&mut self, vcpu: VcpuHandle, state: &CpuState) -> Result<(), Self::Error> {
        if let Some(vmcb) = self.vmcbs.get(&vcpu) {
            let mut active_vmcb = vmcb.load().map_err(|_| X86Error::MemoryError)?;
            let mut vmcb_state = VmcbState::default();
            active_vmcb.save_state(&mut vmcb_state);
            
            // Update VMCB state from generic CPU state
            vmcb_state.guest_cr0 = state.control_registers[0];
            vmcb_state.guest_cr2 = state.control_registers[2];
            vmcb_state.guest_cr3 = state.control_registers[3];
            vmcb_state.guest_cr4 = state.control_registers[4];
            
            // Set general registers (simplified)
            vmcb_state.guest_rax = state.general_registers[0]; // RAX
            
            active_vmcb.load_state(&vmcb_state);
            Ok(())
        } else {
            Err(X86Error::InvalidCpuid)
        }
    }
    
    fn handle_vm_exit(&mut self, _vcpu: VcpuHandle, reason: VmExitReason) -> Result<VmExitAction, Self::Error> {
        match reason {
            VmExitReason::Hlt => Ok(VmExitAction::Continue),
            VmExitReason::IoInstruction { .. } => Ok(VmExitAction::Emulate),
            VmExitReason::Cpuid { .. } => Ok(VmExitAction::Emulate),
            VmExitReason::MsrAccess { .. } => Ok(VmExitAction::Emulate),
            VmExitReason::MemoryAccess { .. } => Ok(VmExitAction::Continue),
            _ => Ok(VmExitAction::Continue),
        }
    }
    
    fn setup_nested_paging(&mut self, vm: VmHandle) -> Result<(), Self::Error> {
        // Nested paging is set up during VM creation for SVM
        if self.nested_page_tables.contains_key(&vm) {
            Ok(())
        } else {
            Err(X86Error::MemoryError)
        }
    }
    
    fn map_guest_memory(&mut self, vm: VmHandle, guest_phys: PhysicalAddress, 
                       host_phys: PhysicalAddress, size: usize, _flags: MemoryFlags) -> Result<(), Self::Error> {
        if let Some(&npt_root) = self.nested_page_tables.get(&vm) {
            // Map memory in nested page table
            setup_identity_mapping(npt_root, guest_phys, host_phys, size)
        } else {
            Err(X86Error::MemoryError)
        }
    }
    
    fn unmap_guest_memory(&mut self, vm: VmHandle, guest_phys: PhysicalAddress, size: usize) -> Result<(), Self::Error> {
        if let Some(&npt_root) = self.nested_page_tables.get(&vm) {
            // Unmap memory from nested page table
            unmap_nested_pages(npt_root, guest_phys, size)
        } else {
            Err(X86Error::MemoryError)
        }
    }
}/// Alloca
te host save area for SVM
fn allocate_host_save_area() -> Result<PhysicalAddress, X86Error> {
    // Would allocate 4KB aligned physical memory
    Ok(0x300000) // Placeholder
}

/// Allocate VMCB for SVM
fn allocate_vmcb() -> Result<PhysicalAddress, X86Error> {
    // Would allocate 4KB aligned physical memory
    Ok(0x400000) // Placeholder
}

/// Allocate nested page table root
fn allocate_nested_page_table() -> Result<PhysicalAddress, X86Error> {
    // Would allocate 4KB aligned physical memory for NPT root
    Ok(0x500000) // Placeholder
}

/// Set VM_HSAVE_PA MSR
unsafe fn set_vm_hsave_pa(pa: PhysicalAddress) -> Result<(), X86Error> {
    const VM_HSAVE_PA: u32 = 0xC0010117;
    
    let msr = x86_64::registers::model_specific::Msr::new(VM_HSAVE_PA);
    msr.write(pa);
    Ok(())
}

/// Set up identity mapping in nested page table
fn setup_identity_mapping(npt_root: PhysicalAddress, guest_phys: PhysicalAddress, 
                         host_phys: PhysicalAddress, size: usize) -> Result<(), X86Error> {
    // Would set up actual nested page table entries
    // This is a simplified placeholder
    let _ = (npt_root, guest_phys, host_phys, size);
    Ok(())
}

/// Unmap pages from nested page table
fn unmap_nested_pages(npt_root: PhysicalAddress, guest_phys: PhysicalAddress, 
                     size: usize) -> Result<(), X86Error> {
    // Would remove nested page table entries
    // This is a simplified placeholder
    let _ = (npt_root, guest_phys, size);
    Ok(())
}

/// Initialize SVM engine
pub fn init() -> Result<(), X86Error> {
    Ok(())
}