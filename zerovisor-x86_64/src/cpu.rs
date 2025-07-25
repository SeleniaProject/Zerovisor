//! x86_64 CPU implementation

use zerovisor_hal::cpu::{Cpu, CpuFeatures, CpuState, CpuRegister, RegisterValue, ArchSpecificState};
use crate::X86Error;
use x86_64::registers::control::{Cr0, Cr4};
use x86_64::registers::model_specific::Msr;

/// x86_64 CPU implementation
pub struct X86Cpu {
    features: CpuFeatures,
    cpu_id: u32,
    vmx_enabled: bool,
    svm_enabled: bool,
    virtualization_type: VirtualizationType,
}

/// Supported virtualization technologies
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtualizationType {
    None,
    Vmx,
    Svm,
}

impl Cpu for X86Cpu {
    type Error = X86Error;
    
    fn init() -> Result<Self, Self::Error> {
        let (features, virt_type) = detect_cpu_features();
        let cpu_id = get_cpu_id();
        
        Ok(Self {
            features,
            cpu_id,
            vmx_enabled: false,
            svm_enabled: false,
            virtualization_type: virt_type,
        })
    }
    
    fn has_virtualization_support(&self) -> bool {
        self.features.contains(CpuFeatures::VIRTUALIZATION)
    }
    
    fn enable_virtualization(&mut self) -> Result<(), Self::Error> {
        if !self.has_virtualization_support() {
            match self.virtualization_type {
                VirtualizationType::Vmx => return Err(X86Error::VmxNotSupported),
                VirtualizationType::Svm => return Err(X86Error::SvmNotSupported),
                VirtualizationType::None => return Err(X86Error::UnsupportedCpu),
            }
        }
        
        match self.virtualization_type {
            VirtualizationType::Vmx => {
                unsafe {
                    // Enable VMX in CR4
                    let mut cr4 = Cr4::read();
                    cr4.insert(Cr4::VIRTUAL_MACHINE_EXTENSIONS);
                    Cr4::write(cr4);
                    
                    // Set VMXE bit in IA32_FEATURE_CONTROL MSR if needed
                    enable_vmx_in_feature_control()?;
                }
                self.vmx_enabled = true;
            },
            VirtualizationType::Svm => {
                unsafe {
                    // Enable SVM in EFER MSR
                    enable_svm_in_efer()?;
                }
                self.svm_enabled = true;
            },
            VirtualizationType::None => {
                return Err(X86Error::UnsupportedCpu);
            }
        }
        
        Ok(())
    }
    
    fn disable_virtualization(&mut self) -> Result<(), Self::Error> {
        match self.virtualization_type {
            VirtualizationType::Vmx => {
                if self.vmx_enabled {
                    unsafe {
                        let mut cr4 = Cr4::read();
                        cr4.remove(Cr4::VIRTUAL_MACHINE_EXTENSIONS);
                        Cr4::write(cr4);
                    }
                    self.vmx_enabled = false;
                }
            },
            VirtualizationType::Svm => {
                if self.svm_enabled {
                    unsafe {
                        // Disable SVM in EFER MSR
                        disable_svm_in_efer()?;
                    }
                    self.svm_enabled = false;
                }
            },
            VirtualizationType::None => {
                // Nothing to disable
            }
        }
        Ok(())
    }
    
    fn features(&self) -> CpuFeatures {
        self.features
    }
    
    fn save_state(&self) -> CpuState {
        unsafe {
            let mut state = CpuState::default();
            
            // Save general purpose registers
            save_general_registers(&mut state.general_registers);
            
            // Save control registers
            state.control_registers[0] = Cr0::read().bits();
            state.control_registers[4] = Cr4::read().bits();
            
            // Save architecture-specific state
            state.arch_specific = ArchSpecificState::X86_64 {
                msr_values: save_msr_values(),
                segment_registers: save_segment_registers(),
                descriptor_tables: save_descriptor_tables(),
            };
            
            state
        }
    }
    
    fn restore_state(&mut self, state: &CpuState) -> Result<(), Self::Error> {
        unsafe {
            // Restore general purpose registers
            restore_general_registers(&state.general_registers);
            
            // Restore control registers
            Cr0::write_raw(state.control_registers[0]);
            Cr4::write_raw(state.control_registers[4]);
            
            // Restore architecture-specific state
            if let ArchSpecificState::X86_64 { msr_values, segment_registers, descriptor_tables } = &state.arch_specific {
                restore_msr_values(msr_values);
                restore_segment_registers(segment_registers);
                restore_descriptor_tables(descriptor_tables);
            }
        }
        
        Ok(())
    }
    
    fn read_register(&self, reg: CpuRegister) -> RegisterValue {
        match reg {
            CpuRegister::ControlRegister(0) => unsafe { Cr0::read().bits() },
            CpuRegister::ControlRegister(4) => unsafe { Cr4::read().bits() },
            _ => 0, // Simplified implementation
        }
    }
    
    fn write_register(&mut self, reg: CpuRegister, value: RegisterValue) -> Result<(), Self::Error> {
        match reg {
            CpuRegister::ControlRegister(0) => unsafe { Cr0::write_raw(value) },
            CpuRegister::ControlRegister(4) => unsafe { Cr4::write_raw(value) },
            _ => return Err(X86Error::InvalidCpuid),
        }
        Ok(())
    }
    
    fn flush_tlb(&self) {
        unsafe {
            x86_64::instructions::tlb::flush_all();
        }
    }
    
    fn invalidate_icache(&self) {
        // x86_64 has coherent instruction cache
        core::sync::atomic::fence(core::sync::atomic::Ordering::SeqCst);
    }
    
    fn cpu_id(&self) -> u32 {
        self.cpu_id
    }
}

/// Check if virtualization is supported
pub fn has_virtualization_support() -> bool {
    let (features, _) = detect_cpu_features();
    features.contains(CpuFeatures::VIRTUALIZATION)
}

/// Initialize x86_64 CPU
pub fn init() -> Result<(), crate::X86Error> {
    // Basic CPU initialization
    Ok(())
}

/// Detect CPU features and virtualization type
fn detect_cpu_features() -> (CpuFeatures, VirtualizationType) {
    let mut features = CpuFeatures::empty();
    let mut virt_type = VirtualizationType::None;
    
    // Check CPUID for virtualization support
    let cpuid = raw_cpuid::CpuId::new();
    
    // Check for Intel VMX support
    if let Some(feature_info) = cpuid.get_feature_info() {
        if feature_info.has_vmx() {
            features |= CpuFeatures::VIRTUALIZATION;
            features |= CpuFeatures::HARDWARE_ASSIST;
            virt_type = VirtualizationType::Vmx;
        }
    }
    
    // Check for AMD SVM support
    if let Some(extended_info) = cpuid.get_extended_function_info() {
        if extended_info.has_svm() {
            features |= CpuFeatures::VIRTUALIZATION;
            features |= CpuFeatures::HARDWARE_ASSIST;
            virt_type = VirtualizationType::Svm;
        }
    }
    
    if let Some(extended_features) = cpuid.get_extended_feature_info() {
        if extended_features.has_smep() {
            features |= CpuFeatures::MEMORY_ENCRYPTION;
        }
    }
    
    features |= CpuFeatures::LARGE_PAGES;
    features |= CpuFeatures::CACHE_COHERENCY;
    features |= CpuFeatures::PRECISE_TIMERS;
    
    (features, virt_type)
}

/// Get CPU ID
fn get_cpu_id() -> u32 {
    // Simplified - would use APIC ID in real implementation
    0
}

/// Enable VMX in IA32_FEATURE_CONTROL MSR
unsafe fn enable_vmx_in_feature_control() -> Result<(), X86Error> {
    const IA32_FEATURE_CONTROL: u32 = 0x3A;
    const FEATURE_CONTROL_LOCKED: u64 = 1 << 0;
    const FEATURE_CONTROL_VMX_ENABLED_INSIDE_SMX: u64 = 1 << 1;
    const FEATURE_CONTROL_VMX_ENABLED_OUTSIDE_SMX: u64 = 1 << 2;
    
    let msr = Msr::new(IA32_FEATURE_CONTROL);
    let mut value = msr.read();
    
    if (value & FEATURE_CONTROL_LOCKED) == 0 {
        value |= FEATURE_CONTROL_LOCKED | FEATURE_CONTROL_VMX_ENABLED_OUTSIDE_SMX;
        msr.write(value);
    } else if (value & FEATURE_CONTROL_VMX_ENABLED_OUTSIDE_SMX) == 0 {
        return Err(X86Error::VmxNotSupported);
    }
    
    Ok(())
}

// Placeholder implementations for register save/restore
unsafe fn save_general_registers(_regs: &mut [RegisterValue; 32]) {
    // Would save actual registers in real implementation
}

unsafe fn restore_general_registers(_regs: &[RegisterValue; 32]) {
    // Would restore actual registers in real implementation
}

fn save_msr_values() -> [RegisterValue; 256] {
    // Would save important MSRs in real implementation
    [0; 256]
}

unsafe fn restore_msr_values(_msr_values: &[RegisterValue; 256]) {
    // Would restore MSRs in real implementation
}

fn save_segment_registers() -> [RegisterValue; 6] {
    // Would save segment registers in real implementation
    [0; 6]
}

unsafe fn restore_segment_registers(_seg_regs: &[RegisterValue; 6]) {
    // Would restore segment registers in real implementation
}

fn save_descriptor_tables() -> [RegisterValue; 4] {
    // Would save GDT, IDT, etc. in real implementation
    [0; 4]
}

unsafe fn restore_descriptor_tables(_desc_tables: &[RegisterValue; 4]) {
    // Would restore descriptor tables in real implementation
}/// En
able SVM in EFER MSR
unsafe fn enable_svm_in_efer() -> Result<(), X86Error> {
    const IA32_EFER: u32 = 0xC0000080;
    const EFER_SVME: u64 = 1 << 12;
    
    let msr = Msr::new(IA32_EFER);
    let mut value = msr.read();
    
    if (value & EFER_SVME) == 0 {
        value |= EFER_SVME;
        msr.write(value);
    }
    
    Ok(())
}

/// Disable SVM in EFER MSR
unsafe fn disable_svm_in_efer() -> Result<(), X86Error> {
    const IA32_EFER: u32 = 0xC0000080;
    const EFER_SVME: u64 = 1 << 12;
    
    let msr = Msr::new(IA32_EFER);
    let mut value = msr.read();
    
    if (value & EFER_SVME) != 0 {
        value &= !EFER_SVME;
        msr.write(value);
    }
    
    Ok(())
}

impl X86Cpu {
    /// Get the virtualization type supported by this CPU
    pub fn virtualization_type(&self) -> VirtualizationType {
        self.virtualization_type
    }
    
    /// Check if VMX is enabled
    pub fn is_vmx_enabled(&self) -> bool {
        self.vmx_enabled
    }
    
    /// Check if SVM is enabled
    pub fn is_svm_enabled(&self) -> bool {
        self.svm_enabled
    }
}