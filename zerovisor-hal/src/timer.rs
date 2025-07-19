//! Timer abstraction layer for precise timing and real-time guarantees

/// Timer callback function type
pub type TimerCallback = fn(timer_id: TimerId);

/// Timer identifier
pub type TimerId = u32;

/// Timer trait for different architectures
pub trait Timer {
    /// Timer specific error type
    type Error;
    
    /// Initialize the timer subsystem
    fn init() -> Result<Self, Self::Error> where Self: Sized;
    
    /// Get current timestamp in nanoseconds
    fn current_time_ns(&self) -> u64;
    
    /// Get timer frequency in Hz
    fn frequency(&self) -> u64;
    
    /// Set a one-shot timer
    fn set_oneshot(&mut self, delay_ns: u64, callback: TimerCallback) -> Result<TimerId, Self::Error>;
    
    /// Set a periodic timer
    fn set_periodic(&mut self, period_ns: u64, callback: TimerCallback) -> Result<TimerId, Self::Error>;
    
    /// Cancel a timer
    fn cancel_timer(&mut self, timer_id: TimerId) -> Result<(), Self::Error>;
    
    /// Sleep for specified nanoseconds (blocking)
    fn sleep_ns(&self, duration_ns: u64);
    
    /// Busy wait for specified nanoseconds (precise, non-blocking)
    fn busy_wait_ns(&self, duration_ns: u64);
    
    /// Get timer resolution in nanoseconds
    fn resolution_ns(&self) -> u64;
    
    /// Check if high-precision timer is available
    fn has_high_precision(&self) -> bool;
    
    /// Calibrate timer against reference
    fn calibrate(&mut self) -> Result<(), Self::Error>;
}

/// High-precision timer for real-time operations
pub trait PrecisionTimer: Timer {
    /// Get timestamp with sub-nanosecond precision
    fn precise_time_ps(&self) -> u64;
    
    /// Set timer with sub-nanosecond precision
    fn set_precise_oneshot(&mut self, delay_ps: u64, callback: TimerCallback) -> Result<TimerId, Self::Error>;
    
    /// Get maximum achievable precision in picoseconds
    fn max_precision_ps(&self) -> u64;
    
    /// Measure actual timer jitter
    fn measure_jitter(&self) -> TimerJitterStats;
}

/// Timer statistics for real-time analysis
#[derive(Debug, Clone, Copy)]
pub struct TimerJitterStats {
    /// Average jitter in nanoseconds
    pub avg_jitter_ns: u64,
    
    /// Maximum observed jitter in nanoseconds
    pub max_jitter_ns: u64,
    
    /// Minimum observed jitter in nanoseconds
    pub min_jitter_ns: u64,
    
    /// Standard deviation of jitter
    pub jitter_stddev_ns: u64,
    
    /// Number of samples used for statistics
    pub sample_count: u64,
}

/// Timer configuration for different use cases
#[derive(Debug, Clone, Copy)]
pub struct TimerConfig {
    /// Required precision in nanoseconds
    pub precision_ns: u64,
    
    /// Maximum acceptable jitter in nanoseconds
    pub max_jitter_ns: u64,
    
    /// Whether timer should be real-time priority
    pub real_time: bool,
    
    /// Power management mode
    pub power_mode: PowerMode,
}

/// Power management modes for timers
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerMode {
    /// Maximum performance, highest power consumption
    HighPerformance,
    
    /// Balanced performance and power
    Balanced,
    
    /// Power saving mode, may reduce precision
    PowerSaver,
    
    /// Ultra-low power mode for battery operation
    UltraLowPower,
}

/// Real-time timer constraints
#[derive(Debug, Clone, Copy)]
pub struct RealTimeTimerConstraints {
    /// Maximum acceptable latency in nanoseconds
    pub max_latency_ns: u64,
    
    /// Required timer resolution in nanoseconds
    pub resolution_ns: u64,
    
    /// Whether timer must be deterministic
    pub deterministic: bool,
    
    /// Priority level for timer interrupts
    pub interrupt_priority: u8,
}

/// Timer event types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerEvent {
    /// Timer expired (one-shot)
    Expired(TimerId),
    
    /// Periodic timer tick
    Tick(TimerId),
    
    /// Timer was cancelled
    Cancelled(TimerId),
    
    /// Timer error occurred
    Error(TimerId),
}

/// Architecture-specific timer implementations
pub mod arch {
    
    /// x86_64 specific timer features
    #[cfg(target_arch = "x86_64")]
    pub mod x86_64 {
        use super::*;
        
        /// TSC (Time Stamp Counter) based timer
        pub struct TscTimer {
            frequency: u64,
            calibrated: bool,
        }
        
        /// HPET (High Precision Event Timer) based timer
        pub struct HpetTimer {
            base_address: u64,
            frequency: u64,
        }
        
        /// APIC timer for per-CPU timing
        pub struct ApicTimer {
            frequency: u64,
            divisor: u32,
        }
    }
    
    /// ARM64 specific timer features
    #[cfg(target_arch = "aarch64")]
    pub mod arm64 {
        use super::*;
        
        /// Generic Timer based implementation
        pub struct GenericTimer {
            frequency: u64,
            virtual_offset: u64,
        }
        
        /// System Timer (SP804) implementation
        pub struct SystemTimer {
            base_address: u64,
            frequency: u64,
        }
    }
    
    /// RISC-V specific timer features
    #[cfg(target_arch = "riscv64")]
    pub mod riscv {
        use super::*;
        
        /// RISC-V timer implementation using mtime/mtimecmp
        pub struct RiscVTimer {
            mtime_address: u64,
            mtimecmp_address: u64,
            frequency: u64,
        }
        
        /// SiFive CLINT timer implementation
        pub struct ClintTimer {
            base_address: u64,
            frequency: u64,
        }
    }
}