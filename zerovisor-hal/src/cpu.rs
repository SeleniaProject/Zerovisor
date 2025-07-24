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
    // Control registers
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64,
    pub cr4: u64,
    pub dr7: u64,
    
    // General purpose registers
    pub rax: u64,
    pub rbx: u64,
    pub rcx: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub rbp: u64,
    pub rsp: u64,
    pub r8: u64,
    pub r9: u64,
    pub r10: u64,
    pub r11: u64,
    pub r12: u64,
    pub r13: u64,
    pub r14: u64,
    pub r15: u64,
    
    // Instruction pointer and flags
    pub rip: u64,
    pub rflags: u64,
    
    // Segment registers
    pub es: SegmentRegister,
    pub cs: SegmentRegister,
    pub ss: SegmentRegister,
    pub ds: SegmentRegister,
    pub fs: SegmentRegister,
    pub gs: SegmentRegister,
    
    // System registers
    pub gdtr: DescriptorTableRegister,
    pub idtr: DescriptorTableRegister,
    pub tr: SegmentRegister,
    pub ldtr: SegmentRegister,
    
    // MSRs
    pub ia32_debugctl: u64,
    pub ia32_pat: u64,
    pub ia32_efer: u64,
    pub ia32_perf_global_ctrl: u64,
    pub ia32_sysenter_cs: u64,
    pub ia32_sysenter_esp: u64,
    pub ia32_sysenter_eip: u64,
}

/// Segment register structure
#[derive(Debug, Clone, Copy)]
pub struct SegmentRegister {
    pub selector: u16,
    pub base: u64,
    pub limit: u32,
    pub access_rights: u32,
}

/// Descriptor table register structure
#[derive(Debug, Clone, Copy)]
pub struct DescriptorTableRegister {
    pub base: u64,
    pub limit: u16,
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

/// Additional CPU state for legacy compatibility
#[derive(Debug, Clone)]
pub struct LegacyCpuState {
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

impl Default for CpuState {
    fn default() -> Self {
        Self {
            cr0: 0,
            cr2: 0,
            cr3: 0,
            cr4: 0,
            dr7: 0,
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            rsp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rip: 0,
            rflags: 0,
            es: SegmentRegister::default(),
            cs: SegmentRegister::default(),
            ss: SegmentRegister::default(),
            ds: SegmentRegister::default(),
            fs: SegmentRegister::default(),
            gs: SegmentRegister::default(),
            gdtr: DescriptorTableRegister::default(),
            idtr: DescriptorTableRegister::default(),
            tr: SegmentRegister::default(),
            ldtr: SegmentRegister::default(),
            ia32_debugctl: 0,
            ia32_pat: 0,
            ia32_efer: 0,
            ia32_perf_global_ctrl: 0,
            ia32_sysenter_cs: 0,
            ia32_sysenter_esp: 0,
            ia32_sysenter_eip: 0,
        }
    }
}

impl Default for SegmentRegister {
    fn default() -> Self {
        Self {
            selector: 0,
            base: 0,
            limit: 0,
            access_rights: 0,
        }
    }
}

impl Default for DescriptorTableRegister {
    fn default() -> Self {
        Self {
            base: 0,
            limit: 0,
        }
    }
}