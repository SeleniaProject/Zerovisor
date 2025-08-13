#![allow(dead_code)]

use core::fmt::Write as _;

#[inline(always)]
fn read_cr0() -> u64 { let v: u64; unsafe { core::arch::asm!("mov {}, cr0", out(reg) v, options(nostack, preserves_flags)); } v }
#[inline(always)]
fn read_cr4() -> u64 { let v: u64; unsafe { core::arch::asm!("mov {}, cr4", out(reg) v, options(nostack, preserves_flags)); } v }
#[inline(always)]
fn read_rflags() -> u64 { let v: u64; unsafe { core::arch::asm!("pushfq; pop {}", out(reg) v, options(nostack, preserves_flags)); } v }

#[inline(always)]
fn rdmsr(idx: u32) -> u64 { unsafe { crate::arch::x86::msr::rdmsr(idx) } }

/// Report security-relevant CPU control bits (W^X hints, SMEP/SMAP, NXE) to UEFI console.
pub fn report_security(system_table: &mut uefi::table::SystemTable<uefi::prelude::Boot>) {
    let lang = crate::i18n::detect_lang(system_table);
    let stdout = system_table.stdout();

    // CR0.WP
    let cr0 = read_cr0();
    let wp = (cr0 & (1 << 16)) != 0;
    let _ = stdout.write_str(if wp { crate::i18n::t(lang, crate::i18n::key::SEC_WP_ON) } else { crate::i18n::t(lang, crate::i18n::key::SEC_WP_OFF) });

    // CR4.SMEP (bit 20), CR4.SMAP (bit 21)
    let cr4 = read_cr4();
    let smep = (cr4 & (1 << 20)) != 0;
    let smap = (cr4 & (1 << 21)) != 0;
    let _ = stdout.write_str(if smep { crate::i18n::t(lang, crate::i18n::key::SEC_SMEP_ON) } else { crate::i18n::t(lang, crate::i18n::key::SEC_SMEP_OFF) });
    let _ = stdout.write_str(if smap { crate::i18n::t(lang, crate::i18n::key::SEC_SMAP_ON) } else { crate::i18n::t(lang, crate::i18n::key::SEC_SMAP_OFF) });

    // EFER.NXE (bit 11)
    let efer = rdmsr(0xC000_0080);
    let nxe = (efer & (1 << 11)) != 0;
    let _ = stdout.write_str(if nxe { crate::i18n::t(lang, crate::i18n::key::SEC_NXE_ON) } else { crate::i18n::t(lang, crate::i18n::key::SEC_NXE_OFF) });

    // RFLAGS (informational)
    let _rflags = read_rflags();

    // Summary line
    let ok = wp && smep && smap && nxe;
    let _ = stdout.write_str(if ok { crate::i18n::t(lang, crate::i18n::key::SEC_SUMMARY_OK) } else { crate::i18n::t(lang, crate::i18n::key::SEC_SUMMARY_NG) });
}


