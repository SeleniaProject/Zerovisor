#![allow(dead_code)]

use uefi::prelude::Boot;
use uefi::table::SystemTable;
use core::fmt::Write as _;

/// Configure a firmware watchdog timeout in seconds if supported by UEFI.
/// Returns true on success or false if not supported or failed.
pub fn arm(system_table: &SystemTable<Boot>, timeout_secs: usize) -> bool {
    // UEFI Spec defines SetWatchdogTimer via RuntimeServices in some firmwares and
    // via BootServices in others depending on crate version exposure. The `uefi`
    // crate 0.28 exposes it on BootServices.
    let bs = system_table.boot_services();
    match bs.set_watchdog_timer(timeout_secs, 0x0000, None) {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Disable firmware watchdog if possible.
pub fn disarm(system_table: &SystemTable<Boot>) -> bool {
    let bs = system_table.boot_services();
    match bs.set_watchdog_timer(0, 0x0000, None) {
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Print watchdog status line (best-effort; many firmwares do not expose getters).
pub fn report(system_table: &mut SystemTable<Boot>) {
    let stdout = system_table.stdout();
    let _ = stdout.write_str("watchdog: armed (best-effort)\r\n");
}


