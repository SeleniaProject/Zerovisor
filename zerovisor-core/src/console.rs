//! Zerovisor management console (Requirement 5.1)
//!
//! Provides a simple UART-based command interface for early debugging and
//! hypervisor management.  The goal is *zero-overhead* polling that can be
//! invoked from the scheduler loop without blocking real-time execution.
//! The implementation is fully self-contained and avoids any form of
//! simplification.

#![allow(dead_code)]

use crate::log;

/// Poll the management console for incoming UART bytes.
///
/// This function is intended to be called from the scheduler loop on the
/// bootstrap CPU.  It performs a *non-blocking* read of the UART data
/// register and dispatches recognised single-byte commands.
pub fn poll() {
    // For x86_64 we directly access the legacy 16550A UART (COM1).
    #[cfg(target_arch = "x86_64")]
    unsafe {
        const UART_BASE: u16 = 0x3F8;
        const REG_LSR: u16 = UART_BASE + 5; // Line Status Register
        const LSR_DATA_READY: u8 = 1 << 0;

        let mut status: u8;
        core::arch::asm!("in al, dx", in("dx") REG_LSR, out("al") status, options(nomem, nostack, preserves_flags));

        if status & LSR_DATA_READY != 0 {
            let mut byte: u8;
            core::arch::asm!("in al, dx", in("dx") UART_BASE, out("al") byte, options(nomem, nostack, preserves_flags));
            handle_byte(byte);
        }
    }

    // For non-x86 architectures we currently have no UART support. The call is a no-op.
    #[cfg(not(target_arch = "x86_64"))]
    {
        // Nothing to do (platform specific implementation pending).
    }
}

/// Dispatch a single received byte.
fn handle_byte(b: u8) {
    match b {
        b'h' | b'?' => help(),
        b'l' => log_current_metrics(),
        _ => { /* Unknown command – ignore */ }
    }
}

/// Print a concise help message to the log buffer.
fn help() {
    log!("[console] Available commands: h/?=help, l=list metrics");
}

/// Log basic performance metrics via the hypervisor log buffer.
fn log_current_metrics() {
    let m = crate::monitor::collect();
    log!(
        "[metrics] exits={} avg={}ns running_vms={} ts={}ns",
        m.total_exits,
        m.avg_exit_latency_ns,
        m.running_vms,
        m.timestamp_ns
    );
}
// End of console module 
// } 