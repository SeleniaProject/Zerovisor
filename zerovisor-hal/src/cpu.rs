//! CPU abstraction layer for multi-architecture support

use bitflags::bitflags;

/// Physical address type
pub type PhysicalAddress = u64;

/// Virtual address type  
pub type VirtualAddress = u64;

/// CPU register value type
pub type RegisterValue = u64;

/// CPU abstraction trait for different architectures
pub trait Cpu {
    /// CPU-specific error type
    type Error;
    
    /// Initialize the CPU for hypervisor operation
    fn init() -> Result<Self, Self::Error> where Self: Sized;
    
    /// Check if virtualization extensions are available
    fn has_virtualization_support(&self) -> bool;
    
    /// Enable virtualization extensions (VMX/SVM/ARMv8-A/RISC-V H-ext)
    fn enable_virtualization(&mut self) -> Result<(), Self::Error>;
    
    /// Disable virtualization extensions
    fn disable_virtualization(&mut self) -> Result<(), Self::Error>;
    
    /// Get current CPU features
    fn features(&self) -> CpuFeatures;
    
    /// Save CPU state
    fn save_state(&self) -> CpuState;
    
    /// Restore CPU state
    fn restore_state(&mut self, state: &CpuState) -> Result<(), Self::Error>;
    
    /// Read a CPU register
    fn read_register(&self, reg: CpuRegister) -> RegisterValue;
    
    /// Write a CPU register
    fn write_register(&mut self, reg: CpuRegister, value: RegisterValue) -> Result<(), Self::Error>;
    
    /// Flush TLB entries
    fn flush_tlb(&self);
    
    /// Invalidate instruction cache
    fn invalidate_icache(&self);
    
    /// Get CPU ID/core number
    fn cpu_id(&self) -> u32;
}

bitflags! {
    /// CPU feature flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CpuFeatures: u64 {
        // Virtualization features
        const VIRTUALIZATION = 1 << 0;
        const NESTED_PAGING = 1 << 1;
        const HARDWARE_ASSIST = 1 << 2;
        
        // Security features
        const MEMORY_ENCRYPTION = 1 << 8;
        const SECURE_BOOT = 1 << 9;
        const TRUSTED_EXECUTION = 1 << 10;
        
        // Performance features
        const LARGE_PAGES = 1 << 16;
        const NUMA_SUPPORT = 1 << 17;
        const CACHE_COHERENCY = 1 << 18;
        
        // Real-time features
        const PRECISE_TIMERS = 1 << 24;
        const INTERRUPT_PRIORITIES = 1 << 25;
        const DETERMINISTIC_EXECUTION = 1 << 26;
    }
}

/// CPU register enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuRegister {
    // General purpose registers (architecture-specific mapping)
    GeneralPurpose(u8),
    
    // Control registers
    ControlRegister(u8),
    
    // System registers
    SystemRegister(u8),
    
    // Virtualization-specific registers
    VirtualizationRegister(u8),
}

/// CPU state for context switching
#[derive(Debug, Clone)]
pub struct CpuState {
    /// General purpose registers
    pub general_registers: [RegisterValue; 32],
    
    /// Control registers
    pub control_registers: [RegisterValue; 16],
    
    /// System registers
    pub system_registers: [RegisterValue; 64],
    
    /// Program counter / instruction pointer
    pub program_counter: VirtualAddress,
    
    /// Stack pointer
    pub stack_pointer: VirtualAddress,
    
    /// Processor flags/status
    pub flags: RegisterValue,
    
    /// Architecture-specific state
    pub arch_specific: ArchSpecificState,
}

/// Architecture-specific CPU state
#[derive(Debug, Clone)]
pub enum ArchSpecificState {
    X86_64 {
        /// x86_64 specific registers and state
        msr_values: [RegisterValue; 256],
        segment_registers: [RegisterValue; 6],
        descriptor_tables: [RegisterValue; 4],
    },
    Arm64 {
        /// ARM64 specific registers and state
        system_registers: [RegisterValue; 128],
        vector_registers: [RegisterValue; 32],
        exception_level: u8,
    },
    RiscV {
        /// RISC-V specific registers and state
        csr_registers: [RegisterValue; 4096],
        privilege_level: u8,
        extension_state: [RegisterValue; 32],
    },
}

impl Default for CpuState {
    fn default() -> Self {
        Self {
            general_registers: [0; 32],
            control_registers: [0; 16],
            system_registers: [0; 64],
            program_counter: 0,
            stack_pointer: 0,
            flags: 0,
            arch_specific: ArchSpecificState::X86_64 {
                msr_values: [0; 256],
                segment_registers: [0; 6],
                descriptor_tables: [0; 4],
            },
        }
    }
}