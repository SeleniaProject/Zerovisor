//! Interrupt handling abstraction layer

use bitflags::bitflags;

/// Interrupt vector number
pub type InterruptVector = u8;

/// Interrupt priority level
pub type InterruptPriority = u8;

/// Interrupt controller trait for different architectures
pub trait InterruptController {
    /// Interrupt controller specific error type
    type Error;
    
    /// Initialize the interrupt controller
    fn init() -> Result<Self, Self::Error> where Self: Sized;
    
    /// Enable interrupts globally
    fn enable_interrupts(&mut self);
    
    /// Disable interrupts globally
    fn disable_interrupts(&mut self);
    
    /// Check if interrupts are enabled
    fn interrupts_enabled(&self) -> bool;
    
    /// Register an interrupt handler
    fn register_handler(&mut self, vector: InterruptVector, handler: InterruptHandler) -> Result<(), Self::Error>;
    
    /// Unregister an interrupt handler
    fn unregister_handler(&mut self, vector: InterruptVector) -> Result<(), Self::Error>;
    
    /// Enable specific interrupt
    fn enable_interrupt(&mut self, vector: InterruptVector) -> Result<(), Self::Error>;
    
    /// Disable specific interrupt
    fn disable_interrupt(&mut self, vector: InterruptVector) -> Result<(), Self::Error>;
    
    /// Set interrupt priority
    fn set_priority(&mut self, vector: InterruptVector, priority: InterruptPriority) -> Result<(), Self::Error>;
    
    /// Send inter-processor interrupt
    fn send_ipi(&self, target_cpu: u32, vector: InterruptVector) -> Result<(), Self::Error>;
    
    /// Acknowledge interrupt
    fn acknowledge(&mut self, vector: InterruptVector);
    
    /// Get pending interrupts
    fn pending_interrupts(&self) -> InterruptMask;
}

/// Interrupt handler function type
pub type InterruptHandler = fn(vector: InterruptVector, context: &InterruptContext);

/// Interrupt context passed to handlers
#[derive(Debug, Clone)]
pub struct InterruptContext {
    /// Interrupt vector that triggered
    pub vector: InterruptVector,
    
    /// CPU state at time of interrupt
    pub cpu_state: crate::cpu::CpuState,
    
    /// Error code (if applicable)
    pub error_code: Option<u64>,
    
    /// Interrupt flags
    pub flags: InterruptFlags,
}

bitflags! {
    /// Interrupt flags
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct InterruptFlags: u32 {
        const MASKABLE = 1 << 0;
        const NON_MASKABLE = 1 << 1;
        const EXCEPTION = 1 << 2;
        const EXTERNAL = 1 << 3;
        const TIMER = 1 << 4;
        const IPI = 1 << 5;
        const FAULT = 1 << 6;
        const TRAP = 1 << 7;
    }
}

bitflags! {
    /// Interrupt mask for tracking pending interrupts
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct InterruptMask: u64 {
        const TIMER = 1 << 0;
        const KEYBOARD = 1 << 1;
        const SERIAL = 1 << 2;
        const NETWORK = 1 << 3;
        const STORAGE = 1 << 4;
        const USB = 1 << 5;
        const PCI = 1 << 6;
        const ACPI = 1 << 7;
        const IPI = 1 << 8;
        const FAULT = 1 << 9;
    }
}

/// Standard interrupt vectors (architecture may override)
pub mod vectors {
    use super::InterruptVector;
    
    pub const DIVIDE_ERROR: InterruptVector = 0;
    pub const DEBUG: InterruptVector = 1;
    pub const NMI: InterruptVector = 2;
    pub const BREAKPOINT: InterruptVector = 3;
    pub const OVERFLOW: InterruptVector = 4;
    pub const BOUND_RANGE: InterruptVector = 5;
    pub const INVALID_OPCODE: InterruptVector = 6;
    pub const DEVICE_NOT_AVAILABLE: InterruptVector = 7;
    pub const DOUBLE_FAULT: InterruptVector = 8;
    pub const INVALID_TSS: InterruptVector = 10;
    pub const SEGMENT_NOT_PRESENT: InterruptVector = 11;
    pub const STACK_FAULT: InterruptVector = 12;
    pub const GENERAL_PROTECTION: InterruptVector = 13;
    pub const PAGE_FAULT: InterruptVector = 14;
    pub const X87_FLOATING_POINT: InterruptVector = 16;
    pub const ALIGNMENT_CHECK: InterruptVector = 17;
    pub const MACHINE_CHECK: InterruptVector = 18;
    pub const SIMD_FLOATING_POINT: InterruptVector = 19;
    pub const VIRTUALIZATION: InterruptVector = 20;
    
    // External interrupts start at 32
    pub const TIMER: InterruptVector = 32;
    pub const KEYBOARD: InterruptVector = 33;
    pub const SERIAL: InterruptVector = 36;
    pub const SPURIOUS: InterruptVector = 255;
}

/// Real-time interrupt constraints
#[derive(Debug, Clone, Copy)]
pub struct RealTimeConstraints {
    /// Maximum interrupt latency in nanoseconds
    pub max_latency_ns: u64,
    
    /// Maximum interrupt jitter in nanoseconds
    pub max_jitter_ns: u64,
    
    /// Required priority level
    pub priority: InterruptPriority,
    
    /// Whether interrupt is critical for real-time operation
    pub critical: bool,
}