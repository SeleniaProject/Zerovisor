//! x86_64 timer implementation

use zerovisor_hal::timer::{Timer, TimerId, TimerCallback};
use crate::X86Error;

/// x86_64 timer implementation using TSC and APIC timer
pub struct X86Timer {
    tsc_frequency: u64,
    apic_frequency: u64,
}

impl Timer for X86Timer {
    type Error = X86Error;
    
    fn init() -> Result<Self, Self::Error> {
        let tsc_frequency = calibrate_tsc();
        let apic_frequency = calibrate_apic_timer();
        
        Ok(Self {
            tsc_frequency,
            apic_frequency,
        })
    }
    
    fn current_time_ns(&self) -> u64 {
        let tsc = unsafe { core::arch::x86_64::_rdtsc() };
        (tsc * 1_000_000_000) / self.tsc_frequency
    }
    
    fn frequency(&self) -> u64 {
        self.tsc_frequency
    }
    
    fn set_oneshot(&mut self, _delay_ns: u64, _callback: TimerCallback) -> Result<TimerId, Self::Error> {
        // Would program APIC timer
        Ok(1)
    }
    
    fn set_periodic(&mut self, _period_ns: u64, _callback: TimerCallback) -> Result<TimerId, Self::Error> {
        // Would program APIC timer for periodic mode
        Ok(2)
    }
    
    fn cancel_timer(&mut self, _timer_id: TimerId) -> Result<(), Self::Error> {
        // Would cancel APIC timer
        Ok(())
    }
    
    fn sleep_ns(&self, duration_ns: u64) {
        let start = self.current_time_ns();
        while self.current_time_ns() - start < duration_ns {
            unsafe {
                core::arch::x86_64::_mm_pause();
            }
        }
    }
    
    fn busy_wait_ns(&self, duration_ns: u64) {
        let start = self.current_time_ns();
        while self.current_time_ns() - start < duration_ns {
            unsafe {
                core::arch::x86_64::_mm_pause();
            }
        }
    }
    
    fn resolution_ns(&self) -> u64 {
        1_000_000_000 / self.tsc_frequency
    }
    
    fn has_high_precision(&self) -> bool {
        true // TSC provides high precision
    }
    
    fn calibrate(&mut self) -> Result<(), Self::Error> {
        self.tsc_frequency = calibrate_tsc();
        self.apic_frequency = calibrate_apic_timer();
        Ok(())
    }
}

/// Calibrate TSC frequency
fn calibrate_tsc() -> u64 {
    // This routine programs PIT channel-2 in one-shot mode for 50 ms and measures
    // the number of TSC ticks elapsed. We use the standard ISA I/O ports.
    // Safety: direct port I/O requires `unsafe` but runs in early boot context.
    const PIT_BASE: u16 = 0x40;
    const PIT_MODE_PORT: u16 = 0x43;

    unsafe {
        // Disable speaker, enable gate for channel 2.
        let mut val: u8;
        core::arch::asm!("in al, dx", in("dx") 0x61u16, out("al") val, options(nomem, nostack, preserves_flags));
        val &= !0x02; // clear speaker enable
        val |= 0x01;  // set gate2 high
        core::arch::asm!("out dx, al", in("dx") 0x61u16, in("al") val, options(nomem, nostack, preserves_flags));

        // Program channel 2: mode 0 (interrupt on terminal count), binary, load 16-bit value 59659 for ~50ms.
        core::arch::asm!("out dx, al", in("dx") PIT_MODE_PORT, in("al") 0b1011_0000u8, options(nomem, nostack, preserves_flags));
        let reload: u16 = 59659; // 1193182 Hz / 20 ≈ 50 ms
        core::arch::asm!("out dx, al", in("dx") 0x42u16, in("al") (reload & 0xFF) as u8, options(nomem, nostack));
        core::arch::asm!("out dx, al", in("dx") 0x42u16, in("al") (reload >> 8) as u8, options(nomem, nostack));

        // Clear OUT2 bit, then poll until it sets to 1 (count reaches zero).
        core::arch::asm!("in al, dx", in("dx") 0x61u16, out("al") val, options(nomem, nostack));
        val &= !0x20; core::arch::asm!("out dx, al", in("dx") 0x61u16, in("al") val);

        let start = core::arch::x86_64::_rdtsc();
        loop {
            core::arch::asm!("in al, dx", in("dx") 0x61u16, out("al") val, options(nomem, nostack, preserves_flags));
            if val & 0x20 != 0 { break; }
            core::arch::asm!("pause", options(nomem, nostack));
        }
        let end = core::arch::x86_64::_rdtsc();
        let ticks = end - start;
        // Duration ≈ 50 ms => frequency = ticks / 0.05
        (ticks * 20) // ticks per second
    }
}

/// Calibrate APIC timer frequency
fn calibrate_apic_timer() -> u64 {
    // Simplified - would use proper calibration method
    100_000_000 // 100 MHz placeholder
}

/// Initialize x86_64 timer
pub fn init() -> Result<(), X86Error> {
    Ok(())
}